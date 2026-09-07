import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute, join } from 'node:path'
import { PassThrough } from 'node:stream'
import { after, before, describe, it } from 'node:test'
import { answers as harnessAnswers } from '../hosts/lib/completion.js'
import { plan } from '../hosts/lib/deliveries.js'
import { bindEvidence } from '../hosts/lib/session-binding.js'
import { workspaceKey } from '../hosts/lib/state.js'
import { Bridge } from '../src/bridge.js'
import { addAgent, removeAgent } from '../src/roster.js'
import { startUiServer } from '../src/ui.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

/**
 * The pane HTTP surface (Phase 2, TEST-PANE-17) against the REAL server,
 * with this test on the pipe as Rust.
 *
 * `startUiServer` is the server the app runs: real HTTP, the real store
 * under a throwaway root, real launch credentials, the real `Bridge`. The
 * only thing played here is the Rust side — a second `Bridge` over two
 * `PassThrough` streams, exactly the pairing `tests/bridge.test.mjs` uses,
 * answering `pane.open`, `pane.snapshot` and `pane.write_paste` and sending
 * `pane.exit` the way `app/src-tauri/src/commands.rs` does.
 *
 * In-process because the deadline matters: a `pane.open` that never comes
 * back must answer `{outcome:'unknown'}` inside a test's patience, and the
 * deadline is an argument, never an environment variable. One spawned case
 * at the end proves the same route through the real `cf ui --json` wiring:
 * handle line first, then frames.
 */

const CONSULT_DEADLINE_MS = 250

/** One read per response: the status message quotes the body it parsed. */
async function json(response, expected) {
  const text = await response.text()
  if (expected !== undefined) assert.equal(response.status, expected, text)
  return text.length === 0 ? null : JSON.parse(text)
}

function waitFor(predicate, timeoutMs = 4000) {
  const started = Date.now()
  return new Promise((resolve, reject) => {
    const tick = async () => {
      try {
        if (await predicate()) return resolve()
      } catch (cause) {
        return reject(cause)
      }
      if (Date.now() - started > timeoutMs) return reject(new Error('timed out waiting'))
      setTimeout(tick, 5)
    }
    tick()
  })
}

/** The real server, with a fake Rust on the other end of the bridge. */
async function paneServer({ paneOpenDeadlineMs = CONSULT_DEADLINE_MS, maxFrameBytes } = {}) {
  const t = tempEnv()
  chooseCmuxMode(t)
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  addAgent({ name: 'nyx', harness: 'claude', model: 'opus' }, t.env)
  addAgent({ name: 'clio', harness: 'pi', model: 'pi-core' }, t.env)
  const workspace = join(t.root, 'workspace')
  mkdirSync(workspace, { recursive: true })

  // A lead pane runs the harness's own CLI, and the pane host refuses an
  // argv[0] that is not absolute — so the launcher looks the binary up on
  // this machine. Shims put one where the lookup will find it WITHOUT ever
  // running: no test here spawns a harness, and the pane host is played on
  // the pipe. The path they resolve to is what the frames are checked
  // against, which is the whole point of giving them a real one.
  const shims = join(t.root, 'shims')
  mkdirSync(shims, { recursive: true })
  const shimmed = {}
  for (const command of ['claude', 'codex', 'pi', 'opencode', 'kimi']) {
    const file = join(shims, command)
    writeFileSync(file, '#!/bin/sh\nexit 0\n')
    chmodSync(file, 0o755)
    shimmed[command] = file
  }
  t.env.PATH = `${shims}:${t.env.PATH ?? ''}`

  const server = await startUiServer(t.env, { paneOpenDeadlineMs })

  const nodeToRust = new PassThrough()
  const rustToNode = new PassThrough()
  const nodeSide = () =>
    new Bridge({
      input: rustToNode,
      output: nodeToRust,
      idPrefix: 'n-',
      peerIdPrefix: 'r-',
      ...(maxFrameBytes === undefined ? {} : { maxFrameBytes }),
    })
  let rust
  try {
    server.attachBridge(nodeSide())
    rust = new Bridge({
      input: nodeToRust,
      output: rustToNode,
      idPrefix: 'r-',
      peerIdPrefix: 'n-',
    })
  } catch (cause) {
    // A server left listening keeps the runner alive long after the failure
    // that mattered has been reported.
    await server.close()
    t.cleanup()
    throw cause
  }

  // What Rust was asked to do, in order, and how it answers. The answers
  // are swapped per test: a pane, a deduplicated pane, silence, a refusal.
  const seen = { open: [], snapshot: [], paste: [], changed: [], list: [] }
  const state = {
    open: (request) => ({ ok: true, id: request.id, generation: request.generation }),
    epoch: 7,
    latched: false,
    diesAfterPaste: null,
    // What `pane.list` answers. Unset is a host that cannot answer at all,
    // which is the shape every test that never sets it has always had.
    list: null,
  }
  const threadsOf = () => {
    const file = join(t.env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(workspace), 'threads.json')
    try {
      return JSON.parse(readFileSync(file, 'utf8'))
    } catch {
      return {}
    }
  }
  // What Node tells Rust without being asked. The page listens for this and
  // re-reads the state when it arrives.
  rust.onEvent('state.changed', (body) => seen.changed.push(body))
  rust.on('pane.open', (request) => {
    seen.open.push(request)
    return state.open(request)
  })
  rust.on('pane.list', (body) => {
    seen.list.push(body)
    if (state.list === null) throw new Error('no pane list')
    return state.list()
  })
  rust.on('pane.snapshot', (request) => {
    seen.snapshot.push(request)
    return {
      ok: true,
      generation: request.generation,
      inputEpoch: state.epoch,
      draftLatched: state.latched,
      pasteInFlight: false,
      inputFailed: false,
      queuedHumanBytes: 0,
    }
  })
  rust.on('pane.write_paste', async (request) => {
    // The arbiter's own refusals, in the arbiter's own words: a latched
    // draft is Rust's call, never Node's.
    if (state.latched) throw new Error('Draft')
    if (request.epoch !== state.epoch) throw new Error('Stale')
    seen.paste.push(request)
    if (state.diesAfterPaste !== null) {
      // The pane accepted the bytes and then its process ended — the one
      // shape where an operation's effect happens and its bookkeeping
      // cannot. The exit is fully processed before the paste is
      // acknowledged, so what follows is the failure, not a race.
      const { name } = state.diesAfterPaste
      state.diesAfterPaste = null
      rust.event('pane.exit', { id: request.id, generation: request.generation })
      await waitFor(() => threadsOf()[name]?.reserved === undefined)
    }
    return { ok: true }
  })

  const url = server.url.replace(/\/$/, '')
  const api = (token, path, { method = 'GET', body } = {}) =>
    fetch(`${url}${path}`, {
      method,
      headers: {
        ...(token === null ? {} : { authorization: `Bearer ${token}` }),
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })

  return {
    t,
    env: t.env,
    workspace,
    shimmed,
    url,
    token: server.token,
    rust,
    seen,
    state,
    api,
    /**
     * The pane host dies with a frame already in its hands: Node's input
     * ends, so every request still waiting rejects with `eof`. The frame
     * WAS written, which is the whole difference from a refused one.
     */
    killTransport() {
      rustToNode.end()
    },
    /** The app with no pane host at all — Rust gone, or never started. */
    detachBridge() {
      server.attachBridge(null)
    },
    attachBridge() {
      server.attachBridge(nodeSide())
    },
    /** A tab with its lead pane, and the lead credential the app gives it. */
    async tab(harness = 'claude-code', dir = workspace) {
      const created = await api(server.token, '/api/tabs', {
        method: 'POST',
        body: { dir, harness },
      })
      const payload = await json(created, 201)
      // Creating a tab now opens its LEAD's window too, which is one
      // `pane.open` of its own. Tests below count the frames a consult or
      // an attach causes, so the lead's is cleared here — it is proved
      // where it belongs, in the `tab.open` and BO10 suites.
      seen.open.length = 0
      return {
        ...payload,
        lead: payload.leadEnv.CONSENSFLOW_APP_TOKEN,
        leadPane: payload.tab.panes.find((pane) => pane.kind === 'lead'),
      }
    },
    /**
     * The pane process ends — the only honest signal that it is gone.
     *
     * A pane that RAN bound a native session on its first turn, which is
     * what makes its conversation reopenable; only a pane that died before
     * ever starting leaves an unbound one. So this binds what it is about
     * to end, unless the test already said otherwise.
     */
    async endPane(pane, leadToken, { bound = true } = {}) {
      if (bound) {
        const listed = await json(await api(leadToken, '/api/panes'))
        const rows = threadsOf()
        // A pane whose launch never resolved is not in the listing, so the
        // reservation is the other place its conversation is named.
        const name =
          listed.panes?.find((candidate) => candidate.id === pane.id)?.conversation ??
          Object.keys(rows).find((key) => rows[key]?.reserved?.pane === pane.id)
        if (typeof name === 'string' && rows[name] !== undefined && !rows[name].sessionId) {
          const file = join(
            t.env.CONSENSFLOW_HOME,
            'workspaces',
            workspaceKey(workspace),
            'threads.json',
          )
          rows[name].sessionId = `native-${name}`
          writeFileSync(file, `${JSON.stringify(rows, null, 2)}\n`)
        }
      }
      rust.event('pane.exit', { id: pane.id, generation: pane.generation })
      await waitFor(async () => {
        const listed = await json(await api(leadToken, '/api/panes'))
        return !listed.panes.some((candidate) => candidate.id === pane.id)
      })
    },
    /**
     * Seeds a delivery record. Phase 3's watcher is what writes these in
     * the real app; the store re-reads the file on every mutation, so a
     * fixture written here is a record the server really finds.
     */
    seedDelivery(record, dir = workspace) {
      const file = join(t.env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(dir), 'deliveries.json')
      mkdirSync(dirname(file), { recursive: true })
      const existing = (() => {
        try {
          return JSON.parse(readFileSync(file, 'utf8'))
        } catch {
          return {}
        }
      })()
      existing[record.id] = record
      writeFileSync(file, `${JSON.stringify(existing, null, 2)}\n`)
    },
    /**
     * Suspends a tab the way a restart does — `Store.open()`'s recovery
     * reads every tab closed, and nothing on the bridge asks for it, so
     * this writes what that leaves. The store re-reads the file on every
     * mutation, so the next one really sees it.
     */
    suspendTab(id) {
      const file = join(t.env.CONSENSFLOW_HOME, 'app', 'tabs.json')
      const envelope = JSON.parse(readFileSync(file, 'utf8'))
      envelope.tabs.find((tab) => tab.id === id).closed = true
      writeFileSync(file, `${JSON.stringify(envelope, null, 2)}\n`)
    },
    /** The pane allocator's counter — a refusal must burn no identity. */
    nextPane() {
      const file = join(t.env.CONSENSFLOW_HOME, 'app', 'tabs.json')
      return JSON.parse(readFileSync(file, 'utf8')).nextPane
    },
    /** The delivery records on disk. */
    deliveries(dir = workspace) {
      const file = join(t.env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(dir), 'deliveries.json')
      try {
        return JSON.parse(readFileSync(file, 'utf8'))
      } catch {
        return {}
      }
    },
    /**
     * Binds a conversation to a native session, the way its controller
     * does through `session.bind` on its first turn. Attaching needs one:
     * a conversation that never bound has no transcript to reopen.
     */
    bindSession(name, sessionId = `native-${name}`) {
      const file = join(
        t.env.CONSENSFLOW_HOME,
        'workspaces',
        workspaceKey(workspace),
        'threads.json',
      )
      const threads = JSON.parse(readFileSync(file, 'utf8'))
      threads[name].sessionId = sessionId
      writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)
      return sessionId
    },
    /** The conversation rows on disk, which is where the truth lives. */
    threads(dir = workspace) {
      const file = join(t.env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(dir), 'threads.json')
      try {
        return JSON.parse(readFileSync(file, 'utf8'))
      } catch {
        return {}
      }
    },
    async close() {
      rust.close()
      await server.close()
      t.cleanup()
    },
  }
}

describe('POST /api/panes/consult applies the continuation rule', () => {
  let s
  let tab
  let first

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
  })
  after(async () => {
    await s.close()
  })

  it('creates a fresh conversation, mints its name and opens exactly one pane', async () => {
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        task: 'review the arbiter',
        fresh: true,
        opId: 'op-fresh-1',
      },
    })
    first = await json(response, 200)
    assert.equal(first.outcome, 'opened')
    assert.match(first.conversation, /^zeus-[a-z]+-[a-z]+$/)
    assert.equal(first.agent, 'zeus')
    assert.equal(first.tab, tab.tab.id)
    assert.equal(typeof first.pane.id, 'string')
    assert.equal(first.pane.generation, 1)
    assert.equal(typeof first.launch, 'string')

    assert.equal(s.seen.open.length, 1, 'one consult is one pane.open')
    const open = s.seen.open[0]
    assert.equal(open.id, first.pane.id)
    assert.equal(open.generation, 1)
    assert.equal(open.cwd, s.workspace)
    assert.equal(open.launch, first.launch, 'Rust deduplicates by the launch id the answer names')
    assert.equal(isAbsolute(open.argv[0]), true, 'argv[0] is the bundle’s absolute node')
    assert.match(open.argv[1], /bin\/cf\.mjs$/)
    assert.deepEqual(open.argv.slice(2, 5), ['run', '@zeus', 'review the arbiter'])
    assert.ok(open.argv.includes('--in-pane'))
    assert.equal(open.argv[open.argv.indexOf('--session') + 1], first.conversation)
    assert.ok(open.argv.includes('--new'), 'a fresh conversation is created under its own name')

    assert.deepEqual(Object.keys(open.env).sort(), [
      'CONSENSFLOW_APP',
      'CONSENSFLOW_LAUNCH',
      'CONSENSFLOW_PANE_ID',
      'PATH',
    ])
    assert.equal(open.env.PATH, s.env.PATH, 'worker tools use the editor’s discovered PATH')
    assert.equal(open.env.CONSENSFLOW_PANE_ID, first.pane.id)
    assert.notEqual(
      open.env.CONSENSFLOW_LAUNCH,
      first.launch,
      'the single-use ticket is a credential and is never told to the lead',
    )

    const row = s.threads()[first.conversation]
    assert.equal(row.agent, 'zeus')
    assert.equal(row.kind, 'codex')
    assert.equal(row.lead, tab.leadEnv.CONSENSFLOW_LEAD_ID)
    assert.equal(row.reserved.launchId, first.launch)
    assert.equal(row.reserved.pane, first.pane.id)
    assert.equal(row.reserved.opId, 'op-fresh-1', 'the reservation carries its operation identity')
    assert.equal(typeof row.reserved.resolvedAt, 'string', 'the launch resolved')
    assert.equal(row.reserved.nonce, first.launch, 'codex binds by the launch nonce')
    assert.equal(open.argv[open.argv.indexOf('--launch') + 1], row.reserved.nonce)
  })

  it('answers the same opId with the same result and opens nothing new', async () => {
    const replay = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        task: 'review the arbiter',
        fresh: true,
        opId: 'op-fresh-1',
      },
    })
    assert.deepEqual(await json(replay, 200), first)
    assert.equal(s.seen.open.length, 1, 'a replayed opId never launches again')
  })

  it('sends a follow-up to a live pane instead of a second launch', async () => {
    const pastes = s.seen.paste.length
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        task: 'now check the epochs',
        opId: 'op-follow-1',
      },
    })
    const answer = await json(response, 200)
    assert.equal(answer.outcome, 'said')
    assert.equal(answer.conversation, first.conversation, 'this lead’s own conversation continues')
    assert.equal(s.seen.open.length, 1, 'a live conversation never opens a second launch')
    assert.equal(s.seen.paste.length, pastes + 1)
    const paste = s.seen.paste.at(-1)
    assert.equal(paste.id, first.pane.id)
    assert.equal(paste.epoch, 7, 'the paste carries the pane’s current input epoch')
    assert.match(paste.body, /now check the epochs/)

    const sent = s.threads()[first.conversation].sent.at(-1)
    assert.equal(sent.opId, 'op-follow-1')
    assert.equal(sent.kind, 'consult')
  })

  it('answers unknown when the launch was sent and never came back', async () => {
    s.state.open = () => new Promise(() => {})
    const stuck = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'nyx', task: 'hold here', fresh: true, opId: 'op-stuck-1' },
    })
    const unknown = await json(stuck, 200)
    assert.equal(unknown.outcome, 'unknown')
    assert.equal(typeof unknown.launch, 'string')

    const row = s.threads()[unknown.conversation]
    assert.equal(row.reserved.launchId, unknown.launch)
    assert.equal(row.reserved.resolvedAt, undefined, 'an unknown launch stays unresolved')

    const retry = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'nyx',
        task: 'hold here',
        session: unknown.conversation,
        opId: 'op-stuck-2',
      },
    })
    assert.equal((await json(retry, 409)).error, 'reserved')

    const same = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'nyx', task: 'hold here', fresh: true, opId: 'op-stuck-1' },
    })
    assert.deepEqual(await json(same, 200), unknown, 'the same opId keeps answering the same thing')

    // A launch we cannot vouch for is not a live pane for anybody: `say`
    // and `attach` refuse it exactly as a second consult does.
    for (const [op, extra] of [
      ['say', { text: 'are you there' }],
      ['attach', {}],
    ]) {
      const response = await s.api(tab.lead, `/api/panes/${op}`, {
        method: 'POST',
        body: { tab: tab.tab.id, session: unknown.conversation, opId: `op-stuck-${op}`, ...extra },
      })
      assert.equal((await json(response, 409)).error, 'reserved', op)
    }
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
  })

  it('takes a deduplicated pane.open as the launch that opened', async () => {
    s.state.open = (request) => ({
      ok: true,
      id: request.id,
      generation: request.generation,
      deduplicated: true,
    })
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'clio', task: 'a dedup', fresh: true, opId: 'op-dedup-1' },
    })
    const answer = await json(response, 200)
    assert.equal(answer.outcome, 'opened')
    const row = s.threads()[answer.conversation]
    assert.equal(typeof row.reserved.resolvedAt, 'string')
    assert.equal(row.reserved.preallocatedId, answer.conversation, 'pi binds on its own name')
    assert.equal(row.reserved.nonce, undefined, 'a preallocated kind needs no nonce')
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
  })

  it('resumes a conversation whose pane has ended, under the same name', async () => {
    const opened = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', task: 'first', fresh: true, opId: 'op-resume-1' },
    })
    const before = await json(opened, 200)
    assert.equal(before.outcome, 'opened')

    await s.endPane(before.pane, tab.lead)
    assert.equal(
      s.threads()[before.conversation].reserved,
      undefined,
      'a pane that ended releases its launch',
    )

    const resumed = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        session: before.conversation,
        task: 'again',
        opId: 'op-resume-2',
      },
    })
    const answer = await json(resumed, 200)
    assert.equal(answer.outcome, 'opened')
    assert.equal(answer.conversation, before.conversation, 'the same conversation, a new pane')
    assert.notEqual(answer.pane.id, before.pane.id)
    assert.notEqual(answer.launch, before.launch, 'a resume is its own launch')
    const open = s.seen.open.at(-1)
    assert.ok(!open.argv.includes('--new'), 'a resume never creates a second conversation')
    assert.equal(open.argv[open.argv.indexOf('--session') + 1], before.conversation)
  })

  it('refuses a malformed consult before anything is launched', async () => {
    const opens = s.seen.open.length
    const cases = [
      [{ agent: 'zeus', task: 'x', session: 'zeus-not-here', opId: 'op-e1' }, 404],
      [{ agent: 'zeus', task: '   ', fresh: true, opId: 'op-e2' }, 400],
      [{ agent: 'nobody', task: 'x', fresh: true, opId: 'op-e3' }, 400],
      [{ agent: 'zeus', task: 'x', fresh: true }, 400],
      [{ agent: 'zeus', task: 'x', fresh: true, notify: 'sometimes', opId: 'op-e5' }, 400],
    ]
    for (const [payload, status] of cases) {
      const response = await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, ...payload },
      })
      assert.equal(response.status, status, JSON.stringify(payload))
    }
    assert.equal(s.seen.open.length, opens, 'a refusal launches nothing')
  })

  it('hands a preallocated harness its own session id on the command line', async () => {
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'nyx', task: 'bind by id', fresh: true, opId: 'op-prealloc' },
    })
    const answer = await json(response, 200)
    const row = s.threads()[answer.conversation]
    assert.match(row.reserved.preallocatedId, /^[0-9a-f-]{36}$/, 'claude binds on an id we mint')
    assert.equal(row.reserved.nonce, undefined)
    const open = s.seen.open.at(-1)
    assert.equal(
      open.argv[open.argv.indexOf('--native-session') + 1],
      row.reserved.preallocatedId,
      'the id on the reservation is the id the harness is told to take',
    )
  })

  it('leaves no pane behind when the pane host refuses the launch', async () => {
    const listed = await json(await s.api(tab.lead, '/api/panes'))
    s.state.open = () => {
      throw new Error('no pty available')
    }
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', task: 'refused', fresh: true, opId: 'op-refused-1' },
    })
    assert.equal((await json(response, 409)).error, 'pane-refused')
    const after = await json(await s.api(tab.lead, '/api/panes'))
    assert.equal(after.panes.length, listed.panes.length, 'a refused launch leaves no pane row')
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
  })

  it('records the lead’s notify preference on the conversation it names', async () => {
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        task: 'quiet please',
        fresh: true,
        notify: 'manual',
        opId: 'op-notify-1',
      },
    })
    const answer = await json(response, 200)
    assert.equal(s.threads()[answer.conversation].notifyPreference, 'manual')

    // And it DECIDES, because no human has said otherwise: the lead's
    // preference is the last word before the default.
    const listed = await json(await s.api(tab.lead, '/api/panes'))
    const pane = listed.panes.find((candidate) => candidate.conversation === answer.conversation)
    assert.deepEqual(pane.policy, { mode: 'manual', source: 'lead' })
    // What sits ABOVE a lead is a human's explicit tab or pane choice,
    // written only by `store.policySet` from the page — its route is
    // Phase 4's, and the precedence itself is proved in
    // `tests/engine/policy.test.mjs`.
  })
})

describe('POST /api/panes/say pastes on the live pane', () => {
  let s
  let tab
  let opened

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', task: 'start', fresh: true, opId: 'say-setup' },
    })
    opened = await json(response, 200)
  })
  after(async () => {
    await s.close()
  })

  it('pastes the text at the pane’s epoch and records what was sent', async () => {
    const response = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        session: opened.conversation,
        text: 'one more thing',
        opId: 'say-1',
      },
    })
    const answer = await json(response, 200)
    assert.equal(answer.outcome, 'said')
    assert.equal(answer.conversation, opened.conversation)

    assert.equal(s.seen.snapshot.at(-1).id, opened.pane.id)
    const paste = s.seen.paste.at(-1)
    assert.equal(paste.id, opened.pane.id)
    assert.equal(paste.generation, opened.pane.generation)
    assert.equal(paste.epoch, 7)
    assert.equal(paste.body, 'one more thing')

    const sent = s.threads()[opened.conversation].sent.at(-1)
    assert.equal(sent.opId, 'say-1')
    assert.equal(sent.kind, 'say')
    assert.equal(typeof sent.at, 'string')
  })

  it('replays one opId without pasting twice', async () => {
    const pastes = s.seen.paste.length
    const replay = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        session: opened.conversation,
        text: 'one more thing',
        opId: 'say-1',
      },
    })
    assert.equal((await json(replay, 200)).outcome, 'said')
    assert.equal(s.seen.paste.length, pastes, 'the same opId never pastes twice')
  })

  it('surfaces the arbiter’s refusal instead of deciding the latch itself', async () => {
    s.state.latched = true
    const response = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, text: 'x', opId: 'say-2' },
    })
    const answer = await json(response, 409)
    assert.equal(answer.error, 'pane-refused')
    assert.match(answer.reason, /Draft/)
    assert.equal(
      s.threads()[opened.conversation].sent.some((entry) => entry.opId === 'say-2'),
      false,
      'a refused paste records no sent line',
    )
    s.state.latched = false
  })

  it('refuses a conversation that has no live pane', async () => {
    const missing = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: 'zeus-not-here', text: 'x', opId: 'say-3' },
    })
    assert.equal(missing.status, 404)

    await s.endPane(opened.pane, tab.lead)
    const gone = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, text: 'x', opId: 'say-4' },
    })
    assert.equal((await json(gone, 409)).error, 'no-live-pane')
  })
})

describe('POST /api/panes/attach resumes under a fresh ticket', () => {
  let s
  let tab
  let opened

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', task: 'start', fresh: true, opId: 'attach-setup' },
    })
    opened = await json(response, 200)
  })
  after(async () => {
    await s.close()
  })

  it('answers a live conversation with its pane and no second launch', async () => {
    const opens = s.seen.open.length
    const response = await s.api(tab.lead, '/api/panes/attach', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, opId: 'attach-1' },
    })
    const answer = await json(response, 200)
    assert.equal(answer.outcome, 'live')
    assert.deepEqual(answer.pane, opened.pane)
    assert.equal(s.seen.open.length, opens, 'two windows on one session is the bug this prevents')
  })

  it('ends the ticket of a pane that died before anyone redeemed it', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'nyx', task: 'dies early', fresh: true, opId: 'attach-3' },
      }),
      200,
    )
    const ticket = s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH
    await s.endPane(opened.pane, tab.lead)

    // The process never got as far as redeeming. Without the launch ending
    // its ticket, that credential would stay good for the rest of its
    // sixty-second life, for a pane that no longer exists.
    const redeemed = await s.api(null, '/api/launch/redeem', { method: 'POST', body: { ticket } })
    assert.equal(redeemed.status, 401)
    assert.deepEqual(await redeemed.json(), { error: 'unauthorized' })
  })

  it('reopens an ended conversation with the attach verb and a new ticket', async () => {
    const firstTicket = s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH
    await s.endPane(opened.pane, tab.lead)

    const response = await s.api(tab.lead, '/api/panes/attach', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, opId: 'attach-2' },
    })
    const answer = await json(response, 200)
    assert.equal(answer.outcome, 'opened')
    assert.equal(answer.conversation, opened.conversation)
    assert.notEqual(answer.pane.id, opened.pane.id)

    const open = s.seen.open.at(-1)
    assert.equal(open.argv[2], 'attach')
    assert.equal(open.argv[3], opened.conversation)
    assert.ok(open.argv.includes('--in-pane'))
    assert.notEqual(open.env.CONSENSFLOW_LAUNCH, firstTicket, 'a fresh single-use ticket')
    assert.equal(s.threads()[opened.conversation].reserved.launchId, answer.launch)
  })
})

describe('the findings asteria reproduced, as standing tests', () => {
  let s
  let tab

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
  })
  after(async () => {
    await s.close()
  })

  it('H1: two attaches racing on one conversation produce ONE launch', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', task: 'first', fresh: true, opId: 'h1-open' },
      }),
      200,
    )
    await s.endPane(opened.pane, tab.lead)

    // Both attaches read a state in which the pane is gone. Admission is
    // one queued store operation, so the second one sees what the first
    // wrote and never opens a second window on the conversation.
    const opens = s.seen.open.length
    const [a, b] = await Promise.all([
      s.api(tab.lead, '/api/panes/attach', {
        method: 'POST',
        body: { tab: tab.tab.id, session: opened.conversation, opId: 'h1-a' },
      }),
      s.api(tab.lead, '/api/panes/attach', {
        method: 'POST',
        body: { tab: tab.tab.id, session: opened.conversation, opId: 'h1-b' },
      }),
    ])
    const answers = [await json(a), await json(b)]
    const launched = answers.filter((answer) => answer.outcome === 'opened')
    assert.equal(launched.length, 1, JSON.stringify(answers))
    assert.equal(s.seen.open.length, opens + 1, 'exactly one pane.open went out')
    const panes = await json(await s.api(tab.lead, '/api/panes'))
    assert.equal(
      panes.panes.filter((pane) => pane.conversation === opened.conversation).length,
      1,
      'and the conversation has exactly one pane',
    )
  })

  it('H1: a second attach arriving mid-launch is refused, not launched', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'nyx', task: 'first', fresh: true, opId: 'h1p-open' },
      }),
      200,
    )
    await s.endPane(opened.pane, tab.lead)

    // Asteria's interleaving, with the pause where she put it: A is
    // admitted and its `pane.open` is outstanding — the moment its own
    // reservation exists but has not resolved — and B arrives right there.
    let release
    const held = new Promise((resolve) => {
      release = resolve
    })
    const opens = s.seen.open.length
    s.state.open = async (request) => {
      s.state.open = (next) => ({ ok: true, id: next.id, generation: next.generation })
      await held
      return { ok: true, id: request.id, generation: request.generation }
    }
    const a = s.api(tab.lead, '/api/panes/attach', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, opId: 'h1p-a' },
    })
    await waitFor(() => s.seen.open.length === opens + 1)

    const b = await s.api(tab.lead, '/api/panes/attach', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, opId: 'h1p-b' },
    })
    assert.equal((await json(b, 409)).error, 'reserved', 'B never joins a launch still in flight')
    assert.equal(s.seen.open.length, opens + 1, 'and never opens a second pane')

    release()
    const answer = await json(await a, 200)
    assert.equal(answer.outcome, 'opened')
    const panes = await json(await s.api(tab.lead, '/api/panes'))
    assert.equal(
      panes.panes.filter((pane) => pane.conversation === opened.conversation).length,
      1,
      'one conversation, one pane',
    )
  })

  it('H2: reopening a bound conversation resumes THAT session, not a new one', async () => {
    for (const [agent, kind] of [
      ['nyx', 'claude-code'],
      ['clio', 'pi'],
      ['zeus', 'codex'],
    ]) {
      const opened = await json(
        await s.api(tab.lead, '/api/panes/consult', {
          method: 'POST',
          body: { tab: tab.tab.id, agent, task: 'work', fresh: true, opId: `h2-${agent}-open` },
        }),
        200,
      )
      // Bind it, the way the controller does once the harness is up.
      const redeemed = await json(
        await s.api(null, '/api/launch/redeem', {
          method: 'POST',
          body: { ticket: s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH },
        }),
        200,
      )
      const reserved = s.threads()[opened.conversation].reserved
      const native = `native-${agent}-session`
      const bound = await json(
        await s.api(redeemed.capability, '/api/panes/session.bind', {
          method: 'POST',
          body: {
            launch: redeemed.launch,
            generation: opened.pane.generation,
            candidate: {
              sessionId: reserved.preallocatedId ?? native,
              turn: `[consensflow launch ${reserved.nonce}]\nwork`,
            },
          },
        }),
        200,
      )
      assert.equal(bound.outcome, 'bound', agent)
      const sessionId = s.threads()[opened.conversation].sessionId

      await s.endPane(opened.pane, tab.lead)
      const again = await json(
        await s.api(tab.lead, '/api/panes/attach', {
          method: 'POST',
          body: { tab: tab.tab.id, session: opened.conversation, opId: `h2-${agent}-again` },
        }),
        200,
      )
      assert.equal(again.outcome, 'opened', agent)

      // The reopened pane is told to resume the session the row names, and
      // the launch record expects exactly that session back.
      const open = s.seen.open.at(-1)
      assert.equal(
        open.argv[open.argv.indexOf('--native-session') + 1],
        sessionId,
        `${agent}: the pane resumes the bound session`,
      )
      const row = s.threads()[opened.conversation]
      const resumed = row.reserved
      assert.equal(resumed.nonce, undefined, `${agent}: a resume mints no new nonce`)
      const evidenceField = kind === 'codex' ? 'reportedId' : 'preallocatedId'
      assert.equal(resumed[evidenceField], sessionId, `${agent}: bound by ${evidenceField}`)

      // And the binding on the row already belongs to the pane now
      // carrying it — a reader between the reopen and the controller's own
      // bind never sees a generation naming the pane that is gone.
      assert.equal(row.binding.launchId, again.launch, `${agent}: the binding names this launch`)
      assert.equal(
        row.binding.generation,
        again.pane.generation,
        `${agent}: and this pane's generation`,
      )

      // And the same session binds again under the new launch.
      const back = await json(
        await s.api(null, '/api/launch/redeem', {
          method: 'POST',
          body: { ticket: s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH },
        }),
        200,
      )
      const rebound = await json(
        await s.api(back.capability, '/api/panes/session.bind', {
          method: 'POST',
          body: {
            launch: back.launch,
            generation: again.pane.generation,
            candidate: { sessionId },
          },
        }),
        200,
      )
      assert.equal(rebound.outcome, 'bound', `${agent}: the resumed session binds`)
      assert.equal(s.threads()[opened.conversation].sessionId, sessionId, agent)
    }
  })

  it('H3: an operation that fails after its effect keeps that answer', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', task: 'go', fresh: true, opId: 'h3-open' },
      }),
      200,
    )

    // Its controller bound a session on the first turn, as every pane that
    // really runs does — without one there is nothing to attach back to.
    s.bindSession(opened.conversation)

    // The paste lands; recording it does not. The bytes are already in the
    // pane, so a replay must NOT send them again.
    s.state.diesAfterPaste = { name: opened.conversation }
    const failed = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, text: 'once only', opId: 'h3-say' },
    })
    const failure = await failed.text()
    assert.equal(failed.status >= 400, true, failure)
    assert.match(failure, /is over|not the current reservation/)
    const pastes = s.seen.paste.length
    assert.equal(s.seen.paste.at(-1).body, 'once only')

    const replay = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, text: 'once only', opId: 'h3-say' },
    })
    assert.equal(replay.status, failed.status, 'the same opId keeps its answer')
    assert.equal(s.seen.paste.length, pastes, 'and never pastes the same words twice')

    // A refusal is an answer too: the Draft replay returns it, unchanged.
    const reopened = await json(
      await s.api(tab.lead, '/api/panes/attach', {
        method: 'POST',
        body: { tab: tab.tab.id, session: opened.conversation, opId: 'h3-reopen' },
      }),
      200,
    )
    assert.equal(reopened.outcome, 'opened')
    s.state.latched = true
    const refused = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, text: 'blocked', opId: 'h3-draft' },
    })
    assert.equal((await json(refused, 409)).error, 'pane-refused')
    s.state.latched = false
    const again = await s.api(tab.lead, '/api/panes/say', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, text: 'blocked', opId: 'h3-draft' },
    })
    assert.equal((await json(again, 409)).error, 'pane-refused', 'decided is decided')
    assert.equal(s.seen.paste.length, pastes, 'the unlatched retry pasted nothing')
  })

  it('H6: a named consult must match the conversation’s own agent', async () => {
    const mine = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'nyx', task: 'hers', fresh: true, opId: 'h6-open' },
      }),
      200,
    )
    const pastes = s.seen.paste.length
    const opens = s.seen.open.length
    const crossed = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        task: 'his',
        session: mine.conversation,
        notify: 'manual',
        opId: 'h6-cross',
      },
    })
    const answer = await json(crossed, 400)
    assert.equal(answer.error, 'agent-mismatch')
    assert.equal(s.seen.paste.length, pastes, 'nothing was pasted into the other agent’s pane')
    assert.equal(s.seen.open.length, opens, 'and nothing was launched')
    assert.equal(
      s.threads()[mine.conversation].notifyPreference,
      undefined,
      'and no preference was changed on the way',
    )
  })

  it('H8: an unresolved launch is never advertised as live', async () => {
    s.state.open = () => new Promise(() => {})
    const unknown = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'clio', task: 'silence', fresh: true, opId: 'h8-open' },
      }),
      200,
    )
    assert.equal(unknown.outcome, 'unknown')
    const listed = await json(await s.api(tab.lead, '/api/panes'))
    const pane = listed.panes.find((candidate) => candidate.id === unknown.pane.id)
    assert.equal(pane.live, false, 'a launch we cannot vouch for is not live')
    assert.equal(pane.status, 'unresolved')
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
  })

  it('a timed-out launch is recovered by its pane exit, and reopens', async () => {
    const stuck = s.threads()
    const name = Object.keys(stuck).find((key) => stuck[key].reserved?.resolvedAt === undefined)
    assert.equal(typeof name, 'string', 'the H8 launch is still unresolved')
    const pane = stuck[name].reserved

    // Rust says the process ended. That is the answer the timeout never
    // got: the reservation goes, and the conversation can open again.
    await s.endPane({ id: pane.pane, generation: pane.generation }, tab.lead)
    assert.equal(s.threads()[name].reserved, undefined)
    const reopened = await json(
      await s.api(tab.lead, '/api/panes/attach', {
        method: 'POST',
        body: { tab: tab.tab.id, session: name, opId: 'recover-1' },
      }),
      200,
    )
    assert.equal(reopened.outcome, 'opened')
  })

  it('carries the optional consult fields through to the pane', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'zeus',
          task: 'the task',
          brief: 'why this run',
          context: 'a note',
          handoffFile: '/tmp/handoff.md',
          fresh: true,
          opId: 'optional-1',
        },
      }),
      200,
    )
    const argv = s.seen.open.at(-1).argv
    assert.equal(argv[argv.indexOf('--brief') + 1], 'why this run')
    assert.equal(argv[argv.indexOf('--context') + 1], 'a note')
    assert.equal(argv[argv.indexOf('--handoff-file') + 1], '/tmp/handoff.md')

    // On a follow-up there is no command line, so the same three ride in
    // the pasted text or they are lost.
    const said = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'zeus',
          task: 'next',
          brief: 'still why',
          context: 'still a note',
          handoffFile: '/tmp/second.md',
          session: opened.conversation,
          opId: 'optional-2',
        },
      }),
      200,
    )
    assert.equal(said.outcome, 'said')
    const body = s.seen.paste.at(-1).body
    assert.match(body, /next/)
    assert.match(body, /still why/)
    assert.match(body, /still a note/)
    assert.match(body, /\/tmp\/second\.md/)
  })
})

describe('a conversation running somewhere else is not a conversation to take', () => {
  let s
  let one
  let two
  let running

  before(async () => {
    s = await paneServer()
    // Two sessions on ONE directory: they share a workspace, so they see
    // the same conversations, and they share no lead.
    one = await s.tab('claude-code')
    two = await s.tab('claude-code')
    running = await json(
      await s.api(one.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: one.tab.id, agent: 'zeus', task: 'mine', fresh: true, opId: 'else-setup' },
      }),
      200,
    )
    assert.equal(running.outcome, 'opened')
  })
  after(async () => {
    await s.close()
  })

  it('tells a live pane elsewhere apart from a launch that has not come back', async () => {
    // Elsewhere: the pane is up, in a session this caller does not hold.
    // Every operation that would touch it says the same thing, because a
    // second window on one session is two processes writing one store.
    const opens = s.seen.open.length
    for (const [op, extra] of [
      ['consult', { agent: 'zeus', task: 'yours now', session: running.conversation }],
      ['say', { session: running.conversation, text: 'over here' }],
      ['attach', { session: running.conversation }],
    ]) {
      const response = await s.api(two.lead, `/api/panes/${op}`, {
        method: 'POST',
        body: { tab: two.tab.id, opId: `else-${op}`, ...extra },
      })
      const answer = await json(response, 409)
      assert.equal(answer.error, 'elsewhere', op)
      assert.match(answer.reason, /running in another session/, op)
      assert.equal(answer.session, one.tab.id, `${op} says which session holds it`)
    }
    assert.equal(s.seen.open.length, opens, 'nothing opened a second window on that session')
    assert.equal(
      s.threads()[running.conversation].reserved.tab,
      one.tab.id,
      'and the conversation still belongs to the session that is running it',
    )

    // Reserved: a launch of this session's own that never came back. A
    // different situation and a different word — one says "somebody else
    // has it", the other says "we do not know whether this one is up".
    s.state.open = () => new Promise(() => {})
    const stuck = await json(
      await s.api(one.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: one.tab.id, agent: 'nyx', task: 'no answer', fresh: true, opId: 'else-stuck' },
      }),
      200,
    )
    assert.equal(stuck.outcome, 'unknown')
    const again = await s.api(one.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: one.tab.id,
        agent: 'nyx',
        task: 'no answer',
        session: stuck.conversation,
        opId: 'else-stuck-2',
      },
    })
    assert.equal((await json(again, 409)).error, 'reserved')
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
  })
})

describe('GET /api/panes lists the caller’s tab', () => {
  let s
  let one
  let two

  before(async () => {
    s = await paneServer()
    one = await s.tab('claude-code')
    two = await s.tab('pi')
    assert.equal(
      (
        await s.api(one.lead, '/api/panes/consult', {
          method: 'POST',
          body: { tab: one.tab.id, agent: 'zeus', task: 'a', fresh: true, opId: 'panes-1' },
        })
      ).status,
      200,
    )
    assert.equal(
      (
        await s.api(two.lead, '/api/panes/consult', {
          method: 'POST',
          body: { tab: two.tab.id, agent: 'nyx', task: 'b', fresh: true, opId: 'panes-2' },
        })
      ).status,
      200,
    )
  })
  after(async () => {
    await s.close()
  })

  it('shows one tab’s panes and never another tab’s', async () => {
    const response = await s.api(one.lead, '/api/panes')
    const answer = await json(response, 200)
    assert.equal(answer.tab, one.tab.id)
    assert.equal(answer.directory, s.workspace)
    assert.equal(answer.closed, false)
    assert.deepEqual(
      answer.panes.map((pane) => pane.kind),
      ['lead', 'worker'],
    )
    const worker = answer.panes[1]
    assert.match(worker.conversation, /^zeus-/)
    assert.equal(worker.agent, 'zeus')
    assert.equal(worker.generation, 1)
    assert.equal(worker.policy.mode, 'auto')
    // Nobody has chosen anything: a new tab records no policy, so the
    // default answers and the source says so.
    assert.equal(worker.policy.source, 'default')
    assert.equal(worker.live, true)

    const theirs = await json(await s.api(two.lead, '/api/panes'))
    assert.equal(theirs.tab, two.tab.id)
    assert.equal(
      theirs.panes.some((pane) => String(pane.conversation ?? '').startsWith('zeus-')),
      false,
      'a tab-scoped token never sees another tab’s conversations',
    )
  })
})

describe('the controller ops act only for their own launch', () => {
  let s
  let tab
  let opened
  let capability
  let launch

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', task: 'work', fresh: true, opId: 'ctl-setup' },
    })
    opened = await json(response, 200)
    // The ticket reaches the controller the way it really does: in the
    // environment Rust was told to give the pane's process.
    const redeemed = await s.api(null, '/api/launch/redeem', {
      method: 'POST',
      body: { ticket: s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH },
    })
    const ownership = await json(redeemed, 200)
    capability = ownership.capability
    launch = ownership.launch
    assert.equal(ownership.conversation, opened.conversation)
  })
  after(async () => {
    await s.close()
  })

  it('binds a native session against the launch record, never a sent word', async () => {
    const nonce = s.threads()[opened.conversation].reserved.nonce
    const response = await s.api(capability, '/api/panes/session.bind', {
      method: 'POST',
      body: {
        launch,
        generation: opened.pane.generation,
        candidate: {
          sessionId: 'rollout-2026-09-07',
          turn: `[consensflow launch ${nonce}]\nwork\n`,
        },
      },
    })
    const answer = await json(response, 200)
    assert.equal(answer.outcome, 'bound')
    assert.equal(answer.evidence, 'nonce')
    assert.equal(answer.conversation, opened.conversation)
    const row = s.threads()[opened.conversation]
    assert.equal(row.sessionId, 'rollout-2026-09-07')
    assert.equal(row.binding.evidence, 'nonce')
    assert.equal(row.binding.generation, opened.pane.generation)
  })

  it('refuses a candidate carrying no evidence for this launch', async () => {
    const response = await s.api(capability, '/api/panes/session.bind', {
      method: 'POST',
      body: {
        launch,
        generation: opened.pane.generation,
        candidate: { sessionId: 'someone-elses', turn: 'work\n' },
      },
    })
    // The other half of the rule the serialisation test pins: a refusal the
    // store makes about the request stays a 400 and carries a code, so it
    // never reads as a fault on this side.
    const refused = await json(response, 400)
    assert.equal(refused.error, 'bind-refused')
    assert.match(refused.reason, /session\.bind refuses/)
    assert.equal(
      s.threads()[opened.conversation].sessionId,
      'rollout-2026-09-07',
      'a refused bind never replaces the binding that holds',
    )
  })

  it('records progress and sent lines on its own conversation only', async () => {
    const progress = await s.api(capability, '/api/panes/progress.set', {
      method: 'POST',
      body: {
        launch,
        generation: opened.pane.generation,
        progress: { state: 'running', detail: 'turn 1' },
      },
    })
    assert.equal(progress.status, 200, JSON.stringify(await json(progress)))
    const recorded = s.threads()[opened.conversation].progress
    assert.equal(recorded.state, 'running')
    assert.equal(recorded.detail, 'turn 1')
    assert.equal(typeof recorded.at, 'string')

    const sent = await s.api(capability, '/api/panes/sent.record', {
      method: 'POST',
      body: {
        launch,
        generation: opened.pane.generation,
        opId: 'ctl-sent-1',
        entry: { kind: 'seed', chars: 42 },
      },
    })
    const recordedSent = await json(sent, 200)
    assert.equal(s.threads()[opened.conversation].sent.at(-1).kind, 'seed')
    const lines = s.threads()[opened.conversation].sent.length

    // A controller that retries — a lost answer, a reconnected bridge —
    // must not append its line twice.
    const replay = await s.api(capability, '/api/panes/sent.record', {
      method: 'POST',
      body: {
        launch,
        generation: opened.pane.generation,
        opId: 'ctl-sent-1',
        entry: { kind: 'seed', chars: 42 },
      },
    })
    assert.deepEqual(await json(replay, 200), recordedSent, 'the same opId, the same answer')
    assert.equal(s.threads()[opened.conversation].sent.length, lines, 'and no second line')

    const noOpId = await s.api(capability, '/api/panes/sent.record', {
      method: 'POST',
      body: { launch, generation: opened.pane.generation, entry: { kind: 'seed' } },
    })
    assert.equal(noOpId.status, 400, 'a recorded line names the operation that wrote it')

    // The two ops that are idempotent by nature need none.
    for (const [op, extra] of [
      ['progress.set', { progress: { state: 'settled' } }],
      [
        'session.bind',
        {
          candidate: {
            sessionId: 'rollout-2026-09-07',
            turn: `[consensflow launch ${s.threads()[opened.conversation].reserved.nonce}]\nwork\n`,
          },
        },
      ],
    ]) {
      const response = await s.api(capability, `/api/panes/${op}`, {
        method: 'POST',
        body: { launch, generation: opened.pane.generation, ...extra },
      })
      assert.equal(response.status, 200, `${op} needs no opId`)
    }

    // Ownership comes from redemption: a controller naming another
    // conversation is naming something it was never given.
    const other = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'nyx', task: 'elsewhere', fresh: true, opId: 'ctl-other' },
    })
    const elsewhere = await json(other, 200)
    const redirected = await s.api(capability, '/api/panes/sent.record', {
      method: 'POST',
      body: {
        launch,
        generation: opened.pane.generation,
        opId: 'ctl-redirect',
        session: elsewhere.conversation,
        entry: { kind: 'stolen' },
      },
    })
    // Otherwise valid — an opId and an entry the handler would accept — so
    // the refusal can only be the one under test.
    const refusal = await json(redirected, 400)
    assert.equal(refusal.error, 'names-a-conversation')
    assert.match(refusal.reason, /not for a session it names/)
    assert.equal(
      s.threads()[elsewhere.conversation].sent.length,
      0,
      'nothing was written to the conversation it named',
    )
  })
})

describe('the pane routes work through the real cf ui wiring', () => {
  it('keeps the handle line first, then opens a pane over the pipe', async () => {
    const t = tempEnv()
    chooseCmuxMode(t)
    addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
    const workspace = join(t.root, 'workspace')
    mkdirSync(workspace, { recursive: true })
    // The lead pane runs the harness's own CLI, so the child needs one to
    // find. It is never executed — the pane host is played on this pipe.
    const shims = join(t.root, 'shims')
    mkdirSync(shims, { recursive: true })
    writeFileSync(join(shims, 'claude'), '#!/bin/sh\nexit 0\n')
    chmodSync(join(shims, 'claude'), 0o755)
    t.env.PATH = `${shims}:${t.env.PATH ?? ''}`
    const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
    const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
      env: t.env,
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    try {
      let buffer = ''
      child.stdout.on('data', (chunk) => {
        buffer += chunk
      })
      const nextLine = (timeoutMs = 10_000) =>
        new Promise((resolve, reject) => {
          const started = Date.now()
          const tick = () => {
            const end = buffer.indexOf('\n')
            if (end !== -1) {
              const line = buffer.slice(0, end)
              buffer = buffer.slice(end + 1)
              return resolve(line)
            }
            if (Date.now() - started > timeoutMs) return reject(new Error('no line arrived'))
            setTimeout(tick, 5)
          }
          tick()
        })

      const handle = JSON.parse(await nextLine())
      const url = handle.url.replace(/\/$/, '')
      const api = (token, path, init = {}) =>
        fetch(`${url}${path}`, {
          method: init.method ?? 'GET',
          headers: {
            ...(token === null ? {} : { authorization: `Bearer ${token}` }),
            ...(init.body === undefined ? {} : { 'content-type': 'application/json' }),
          },
          ...(init.body === undefined ? {} : { body: JSON.stringify(init.body) }),
        })

      // The frames the child sends, in order, skipping the announcements
      // the store makes after every mutation it completes.
      const announced = []
      const nextRequest = async () => {
        for (;;) {
          const frame = JSON.parse(await nextLine())
          if (frame.kind !== 'evt') return frame
          announced.push(frame.op)
        }
      }
      const answerOpen = (frame) =>
        child.stdin.write(
          `${JSON.stringify({
            v: 1,
            id: frame.id,
            kind: 'res',
            op: 'pane.open',
            body: { ok: true, id: frame.body.id, generation: frame.body.generation },
          })}\n`,
        )

      // Creating a tab opens its LEAD's window, so this request does not
      // answer until that frame is answered — the two have to overlap.
      const creating = api(handle.token, '/api/tabs', {
        method: 'POST',
        body: { dir: workspace, harness: 'claude-code' },
      })
      const leadFrame = await nextRequest()
      assert.equal(leadFrame.op, 'pane.open')
      assert.match(leadFrame.body.argv[0], /claude$/, 'the lead runs its own harness')
      answerOpen(leadFrame)
      const tab = await json(await creating, 201)

      const pending = api(tab.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'zeus',
          task: 'over the pipe',
          fresh: true,
          opId: 'wired-1',
        },
      })

      // The worker's frame, after the lead's. The pipe also carries the
      // store's `state.changed` announcements, and nothing else.
      const frame = await nextRequest()
      assert.deepEqual([...new Set(announced)], ['state.changed'], 'only announcements between')
      assert.equal(frame.v, 1)
      assert.equal(frame.kind, 'req')
      assert.equal(frame.op, 'pane.open')
      assert.match(frame.id, /^n-/)
      child.stdin.write(
        `${JSON.stringify({
          v: 1,
          id: frame.id,
          kind: 'res',
          op: 'pane.open',
          body: { ok: true, id: frame.body.id, generation: frame.body.generation },
        })}\n`,
      )

      const response = await pending
      const answer = await json(response, 200)
      assert.equal(answer.outcome, 'opened')
      assert.equal(answer.launch, frame.body.launch)
    } finally {
      child.kill()
      t.cleanup()
    }
  })
})

describe('POST /api/panes/read prints one part of a delivery', () => {
  let s
  let tab
  let conversation
  let record

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'zeus',
          task: 'a long answer',
          fresh: true,
          opId: 'r-open',
        },
      }),
      200,
    )
    conversation = opened.conversation

    // A real record, planned by the real planner, with the real allocator's
    // id — so the parts this route prints are the parts the delivery has.
    // A literal id here; the allocator that mints them is proved in
    // `tests/store.test.mjs`.
    const id = 'd-1'
    const answer = Array.from({ length: 400 }, (_, at) => `line ${at} of the worker's answer`).join(
      '\n',
    )
    const planned = plan({
      row: { agent: 'zeus' },
      items: [{ id: 'msg_7', role: 'assistant', complete: true, text: answer }],
      policy: { mode: 'auto' },
      conversation,
      agent: 'zeus',
      kind: 'claude-code',
      target: {
        leadId: tab.leadEnv.CONSENSFLOW_LEAD_ID,
        session: 'lead-session',
        tab: tab.tab.id,
        pane: tab.leadPane.id,
        generation: 1,
      },
      newId: () => id,
      now: Date.parse('2026-09-07T10:00:00.000Z'),
      workspace: s.workspace,
      partBudget: { 'claude-code': { bytes: 900, lines: 40 } },
    })
    assert.equal(planned.length, 1)
    record = planned[0]
    assert.equal(record.channel, 'cf-read')
    assert.ok(record.parts.length > 2, 'the answer is long enough to need several parts')
    s.seedDelivery(record)
  })
  after(async () => {
    await s.close()
  })

  it('answers the part it was asked for, verbatim, with its markers', async () => {
    const answer = await json(
      await s.api(tab.lead, '/api/panes/read', {
        method: 'POST',
        body: { tab: tab.tab.id, deliveryId: record.id, part: 2, opId: 'read-2' },
      }),
      200,
    )
    const part = record.parts[1]
    assert.equal(answer.text, part.text, 'the client prints this and adds nothing')
    assert.equal(answer.k, 2)
    assert.equal(answer.of, record.parts.length)
    assert.equal(answer.open, part.open)
    assert.equal(answer.close, part.close)
    assert.equal(answer.next, part.next)
    assert.equal(answer.digest, part.digest)
    assert.equal(answer.deliveryId, record.id)
    assert.equal(answer.conversation, conversation)
    assert.match(answer.text, /^\[part 2 of \d+ — \d+ bytes\]\n/)
    assert.match(answer.text, new RegExp(`\\[end of part 2 of \\d+ — delivery ${record.id}\\]`))
    assert.match(answer.next, /--part 3/)

    // The part is under the LEAD harness's budget, which is what the whole
    // part mechanism exists for: a lead that keeps only the tail of a long
    // tool result would keep the end marker and drop the answer.
    const budget = record.partBudget
    assert.ok(answer.bytes <= budget.bytes, `${answer.bytes} <= ${budget.bytes}`)
    assert.ok(answer.lines <= budget.lines)
  })

  it('defaults to the first part and says how many there are', async () => {
    const answer = await json(
      await s.api(tab.lead, '/api/panes/read', {
        method: 'POST',
        body: { tab: tab.tab.id, deliveryId: record.id, opId: 'read-1' },
      }),
      200,
    )
    assert.equal(answer.k, 1)
    assert.equal(answer.text, record.parts[0].text)
  })

  it('records an ATTEMPT for the part it printed, and never coverage', async () => {
    const stored = s.deliveries()[record.id]
    assert.equal(stored.partAttempts['1'], 1)
    assert.equal(stored.partAttempts['2'], 1)
    assert.equal(stored.lastRead.lead, tab.leadEnv.CONSENSFLOW_LEAD_ID)
    assert.equal(stored.partCoverage, undefined, 'reading covers nothing')
    assert.equal(stored.evidenceIds, undefined)
    assert.equal(stored.state, 'pending', 'and moves no record along')

    // Replaying one opId is one attempt, not two.
    await json(
      await s.api(tab.lead, '/api/panes/read', {
        method: 'POST',
        body: { tab: tab.tab.id, deliveryId: record.id, part: 2, opId: 'read-2' },
      }),
      200,
    )
    assert.equal(s.deliveries()[record.id].partAttempts['2'], 1, 'a replay prints, records once')
  })

  it('makes a refusal the final answer for its opId', async () => {
    const attempts = s.deliveries()[record.id].partAttempts['1']
    const bad = await s.api(tab.lead, '/api/panes/read', {
      method: 'POST',
      body: { tab: tab.tab.id, deliveryId: record.id, part: 0, opId: 'read-final' },
    })
    assert.equal(bad.status, 400)
    const corrected = await s.api(tab.lead, '/api/panes/read', {
      method: 'POST',
      body: { tab: tab.tab.id, deliveryId: record.id, part: 1, opId: 'read-final' },
    })
    assert.equal(corrected.status, 400, 'one opId is one operation, refusals included')
    assert.equal(
      s.deliveries()[record.id].partAttempts['1'],
      attempts,
      'and the corrected request printed nothing and recorded nothing',
    )
  })

  it('refuses a part that does not exist, and a delivery of another session', async () => {
    for (const [body, status] of [
      [{ deliveryId: record.id, part: record.parts.length + 1, opId: 'read-e1' }, 400],
      [{ deliveryId: record.id, part: 0, opId: 'read-e2' }, 400],
      [{ deliveryId: 'd-99999', part: 1, opId: 'read-e3' }, 404],
      [{ deliveryId: 'not-an-id', part: 1, opId: 'read-e4' }, 400],
      [{ deliveryId: record.id, part: 1 }, 400],
    ]) {
      const response = await s.api(tab.lead, '/api/panes/read', {
        method: 'POST',
        body: { tab: tab.tab.id, ...body },
      })
      assert.equal(response.status, status, JSON.stringify(body))
    }

    // Another session in the same directory shares the workspace, and its
    // lead may not read a delivery addressed to this one.
    const other = await s.tab('claude-code')
    const response = await s.api(other.lead, '/api/panes/read', {
      method: 'POST',
      body: { tab: other.tab.id, deliveryId: record.id, part: 1, opId: 'read-e5' },
    })
    assert.equal((await json(response, 404)).error, 'no-such-delivery')
  })
})

describe('POST /api/panes/seen marks what the lead has taken in', () => {
  let s
  let tab
  let conversation

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', task: 'work', fresh: true, opId: 's-open' },
      }),
      200,
    )
    conversation = opened.conversation
  })
  after(async () => {
    await s.close()
  })

  it('walks the printed items and stops at the first one nobody has read', async () => {
    const answer = await json(
      await s.api(tab.lead, '/api/panes/seen', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          session: conversation,
          items: [
            { id: 'a', role: 'assistant', printed: true },
            { id: 'b', role: 'assistant', printed: true },
            { id: 'c', role: 'assistant' },
            { id: 'd', role: 'assistant', printed: true },
          ],
          opId: 'seen-1',
        },
      }),
      200,
    )
    assert.equal(answer.outcome, 'seen')
    assert.equal(answer.conversation, conversation)
    assert.equal(answer.lead, tab.leadEnv.CONSENSFLOW_LEAD_ID)
    assert.deepEqual(answer.seen, ['a', 'b'], 'the frontier stops at the first unread item')

    const row = s.threads()[conversation]
    assert.deepEqual(row.seen[tab.leadEnv.CONSENSFLOW_LEAD_ID], ['a', 'b'])
  })

  it('never lets a tool item break the walk', async () => {
    // A tool item is never delivery-covered and never printed, so leaving
    // it in the set stops the walk at the first tool call the worker made
    // — and everything the lead really read after it stays unmarked.
    const answer = await json(
      await s.api(tab.lead, '/api/panes/seen', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          session: conversation,
          items: [
            { id: 'a', role: 'assistant', printed: true },
            { id: 'tool-1', role: 'tool' },
            { id: 'b', role: 'assistant', printed: true },
            { id: 'tool-2', role: 'tool' },
            { id: 'c', role: 'assistant', printed: true },
            { id: 'd', role: 'assistant' },
          ],
          opId: 'seen-2',
        },
      }),
      200,
    )
    assert.deepEqual(answer.seen, ['a', 'b', 'c'])
    assert.equal(
      answer.seen.some((id) => id.startsWith('tool-')),
      false,
      'and a tool item is never a mark of its own',
    )
  })

  it('is idempotent by opId and by union', async () => {
    const before = s.threads()[conversation].seen[tab.leadEnv.CONSENSFLOW_LEAD_ID]
    const replay = await json(
      await s.api(tab.lead, '/api/panes/seen', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          session: conversation,
          items: [{ id: 'a', role: 'assistant', printed: true }],
          opId: 'seen-2',
        },
      }),
      200,
    )
    assert.deepEqual(replay.seen, ['a', 'b', 'c'], 'the same opId, the same answer')
    assert.deepEqual(s.threads()[conversation].seen[tab.leadEnv.CONSENSFLOW_LEAD_ID], before)
  })

  it('refuses an unknown conversation and a malformed item set', async () => {
    for (const [body, status] of [
      [{ session: 'zeus-not-here', items: [], opId: 'seen-e1' }, 404],
      [{ session: conversation, items: 'nope', opId: 'seen-e2' }, 400],
      [{ session: conversation, items: [{ role: 'assistant' }], opId: 'seen-e3' }, 400],
      [{ session: conversation, items: [] }, 400],
    ]) {
      const response = await s.api(tab.lead, '/api/panes/seen', {
        method: 'POST',
        body: { tab: tab.tab.id, ...body },
      })
      assert.equal(response.status, status, JSON.stringify(body))
    }
  })

  it('names the item contract when a client sends bare ids', async () => {
    // The walk has one owner, and it is here: the server holds the store
    // row and the deliveries. The client sends what it READ, in order, and
    // says which of those it printed to the lead.
    const bare = await s.api(tab.lead, '/api/panes/seen', {
      method: 'POST',
      body: { tab: tab.tab.id, session: conversation, items: ['a', 'b'], opId: 'contract-1' },
    })
    const refusal = await json(bare, 400)
    assert.match(refusal.error, /\{id, role, printed\}/)

    // `printed` is part of the contract, not a hint: it is what the client
    // knows and the server cannot — which items it just put in front of
    // the lead.
    const badFlag = await s.api(tab.lead, '/api/panes/seen', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        session: conversation,
        items: [{ id: 'z', role: 'assistant', printed: 'yes' }],
        opId: 'contract-2',
      },
    })
    assert.match((await json(badFlag, 400)).error, /printed/)
  })

  it('advances only over items the client says it printed', async () => {
    const fresh = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'nyx', task: 'more', fresh: true, opId: 'printed-open' },
      }),
      200,
    )
    const answer = await json(
      await s.api(tab.lead, '/api/panes/seen', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          session: fresh.conversation,
          items: [
            { id: 'p1', role: 'assistant', printed: true },
            { id: 'p2', role: 'assistant', printed: false },
            { id: 'p3', role: 'assistant', printed: true },
          ],
          opId: 'printed-1',
        },
      }),
      200,
    )
    assert.deepEqual(answer.seen, ['p1'], 'an item it did not print stops the frontier')

    const absent = await json(
      await s.api(tab.lead, '/api/panes/seen', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          session: fresh.conversation,
          items: [
            { id: 'p1', role: 'assistant', printed: true },
            { id: 'p2', role: 'assistant' },
          ],
          opId: 'printed-2',
        },
      }),
      200,
    )
    assert.deepEqual(absent.seen, ['p1'], 'and so does one that says nothing about printing')
  })

  it('makes a refusal the final answer for its opId', async () => {
    const bad = await s.api(tab.lead, '/api/panes/seen', {
      method: 'POST',
      body: { tab: tab.tab.id, session: conversation, items: 'nope', opId: 'seen-final' },
    })
    assert.equal(bad.status, 400)
    const corrected = await s.api(tab.lead, '/api/panes/seen', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        session: conversation,
        items: [{ id: 'a', role: 'assistant', printed: true }],
        opId: 'seen-final',
      },
    })
    assert.equal(corrected.status, 400, 'one opId is one operation, refusals included')
  })
})

describe('asteria round 2: a launch is never left stranded, and never crossed', () => {
  let s
  let tab

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
  })
  after(async () => {
    await s.close()
  })

  it('refuses before admission when there is no pane host at all', async () => {
    const opens = s.seen.open.length
    const counter = s.nextPane()
    s.detachBridge()
    const response = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', task: 'nobody home', fresh: true, opId: 'strand-1' },
    })
    assert.equal((await json(response, 503)).error, 'no-pane-host')
    assert.equal(s.seen.open.length, opens, 'nothing was transmitted')

    // Checked BEFORE admission: no reservation, no pane, nothing for a
    // later attach to trip over — and no child exists to emit the exit
    // that would have been the only way out.
    const threads = s.threads()
    const stranded = Object.entries(threads).filter(
      ([, row]) => row.reserved !== null && typeof row.reserved === 'object',
    )
    assert.deepEqual(stranded, [], 'no conversation is left holding a launch')
    assert.equal(
      s.nextPane(),
      counter,
      'and no pane identity was burned: the check ran before admission, not after it',
    )
    s.attachBridge()
  })

  it('gives the launch back when the frame never reached the transport', async () => {
    // A real pre-transmission failure, from the real Bridge: the frame does
    // not fit `maxFrameBytes`, so it is refused and NOTHING is written. The
    // launch was admitted a moment earlier and must be given back — nothing
    // is running, and no exit will ever come for it.
    // Big enough for the lead's own frame (creating a tab opens its window
    // now), small enough that the worker launch below cannot fit.
    const maxFrameBytes = 2048
    const small = await paneServer({ maxFrameBytes })
    try {
      const its = await small.tab('claude-code')
      const counter = small.nextPane()
      const response = await small.api(its.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: its.tab.id,
          agent: 'zeus',
          task: `too big to send${'.'.repeat(maxFrameBytes)}`,
          fresh: true,
          opId: 'big-1',
        },
      })
      assert.equal((await json(response, 409)).error, 'pane-refused')
      assert.equal(small.seen.open.length, 0, 'Rust never saw a frame')

      const stranded = Object.entries(small.threads()).filter(
        ([, row]) => row.reserved !== null && typeof row.reserved === 'object',
      )
      assert.deepEqual(stranded, [], 'and no conversation is left holding a launch')
      assert.equal(small.nextPane(), counter + 1, 'the pane it minted was given back too')
      const listed = await json(await small.api(its.lead, '/api/panes'))
      assert.deepEqual(
        listed.panes.map((pane) => pane.kind),
        ['lead'],
        'no pane row was left behind',
      )
    } finally {
      await small.close()
    }
  })

  it('gives the launch back when the frame is serialised and the serialisation throws', async () => {
    // The frame is encoded TWICE: `#open` encodes the body to weigh it, and
    // a moment later the Bridge encodes the whole request frame around it
    // (`src/bridge.js:164`, no try/catch, so it throws synchronously out of
    // `request`). Two encodings means the first is not a proof the second
    // will succeed, and the only thing standing between a throw there and a
    // conversation stranded holding a launch nothing will ever end is WHERE
    // `transmitted` is set: after `request` returns, never before it.
    //
    // So the injection arms the real `JSON.stringify` to throw for exactly
    // one `pane.open` REQUEST FRAME. The body alone still encodes — it has
    // no `kind` — so the pre-transmission check passes exactly as it does in
    // production, and the throw lands inside the real `bridge.request`
    // before a byte is written. Nothing else is stubbed.
    const realStringify = JSON.stringify
    let injected = 0
    JSON.stringify = (value, ...rest) => {
      if (value?.kind === 'req' && value?.op === 'pane.open') {
        injected += 1
        throw new TypeError('injected: this pane.open frame cannot be serialised')
      }
      return realStringify(value, ...rest)
    }
    const opens = s.seen.open.length
    const counter = s.nextPane()
    const before = (await json(await s.api(tab.lead, '/api/panes'))).panes.length
    let response
    try {
      response = await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'zeus',
          task: 'the frame that cannot be encoded',
          fresh: true,
          opId: 'serialise-1',
        },
      })
    } finally {
      JSON.stringify = realStringify
    }

    assert.equal(injected, 1, 'the injection ran: the Bridge did try to serialise the frame')
    // An error the route does not model is a fault on this side, not a bad
    // request: 500, never 400. What the store is left holding matters more
    // still, and that is the rest of this test.
    assert.deepEqual(await json(response, 500), {
      error: 'internal_error',
      reason: 'injected: this pane.open frame cannot be serialised',
    })
    assert.equal(s.seen.open.length, opens, 'and Rust never saw a frame')

    const stranded = Object.entries(s.threads()).filter(
      ([, row]) => row.reserved !== null && typeof row.reserved === 'object',
    )
    assert.deepEqual(stranded, [], 'no conversation is left holding a launch')
    assert.equal(s.nextPane(), counter + 1, 'the identity is spent, never handed out twice')
    const listed = await json(await s.api(tab.lead, '/api/panes'))
    assert.equal(listed.panes.length, before, 'and no pane row was left behind')

    // The proof that the launch really came back: the same conversation can
    // be opened again. A retry carries a new opId — the ledger remembers a
    // failure under the old one, on purpose.
    const retry = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'zeus',
        task: 'the frame that cannot be encoded',
        fresh: true,
        opId: 'serialise-2',
      },
    })
    const opened = await json(retry, 200)
    assert.equal(opened.outcome, 'opened')
    assert.equal(s.seen.open.length, opens + 1, "exactly one frame, the retry's")
  })

  it('treats a malformed open answer as uncertain, never as a refusal', async () => {
    for (const [label, answer] of [
      ['null', null],
      ['{}', {}],
      ['a non-boolean ok', { ok: 'yes', error: 'nope' }],
      ['ok:false with no error', { ok: false }],
      ['ok:false with a non-string error', { ok: false, error: 7 }],
    ]) {
      s.state.open = () => answer
      const opens = s.seen.open.length
      const response = await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'zeus',
          task: label,
          fresh: true,
          opId: `malformed-${label}`,
        },
      })
      const unknown = await json(response, 200)
      assert.equal(unknown.outcome, 'unknown', label)

      // The frame went out and the answer says nothing we can act on, so
      // the launch is uncertain: the reservation stands, and no new
      // operation may open a second pane for that conversation.
      const row = s.threads()[unknown.conversation]
      assert.equal(row.reserved.launchId, unknown.launch, label)
      assert.equal(row.reserved.resolvedAt, undefined, label)
      const second = await s.api(tab.lead, '/api/panes/attach', {
        method: 'POST',
        body: { tab: tab.tab.id, session: unknown.conversation, opId: `malformed-retry-${label}` },
      })
      assert.equal((await json(second, 409)).error, 'reserved', label)
      assert.equal(s.seen.open.length, opens + 1, `${label}: exactly one frame went out`)
      await s.endPane(unknown.pane, tab.lead)
    }
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
  })

  it('keeps the reservation when the outcome after transmission is unknown', async () => {
    s.state.open = () => new Promise(() => {})
    const unknown = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'clio', task: 'silence', fresh: true, opId: 'strand-5' },
      }),
      200,
    )
    assert.equal(unknown.outcome, 'unknown')
    assert.equal(
      s.threads()[unknown.conversation].reserved.resolvedAt,
      undefined,
      'a frame that went out and was never answered keeps its launch',
    )
    s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
    await s.endPane(unknown.pane, tab.lead)
  })

  it('replays a consult from the ledger even after the agent is gone', async () => {
    const first = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab.id,
          agent: 'clio',
          task: 'while it exists',
          fresh: true,
          opId: 'gone-1',
        },
      }),
      200,
    )
    assert.equal(first.outcome, 'opened')

    // The roster is mutable state like any other, so it is read inside the
    // operation, not before it: a replay answers what the operation
    // decided, not what the roster says now.
    removeAgent('clio', s.env)
    const replay = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab.id,
        agent: 'clio',
        task: 'while it exists',
        fresh: true,
        opId: 'gone-1',
      },
    })
    assert.deepEqual(await json(replay, 200), first, 'the same opId, the same answer')

    // A NEW operation naturally sees the roster as it now is.
    const now = await s.api(tab.lead, '/api/panes/consult', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'clio', task: 'and now', fresh: true, opId: 'gone-2' },
    })
    assert.equal(now.status, 400)
    addAgent({ name: 'clio', harness: 'pi', model: 'pi-core' }, s.env)
  })

  it('refuses an attach whose roster name has become another harness', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', task: 'codex work', fresh: true, opId: 'cross-1' },
      }),
      200,
    )
    const redeemed = await json(
      await s.api(null, '/api/launch/redeem', {
        method: 'POST',
        body: { ticket: s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH },
      }),
      200,
    )
    const nonce = s.threads()[opened.conversation].reserved.nonce
    await json(
      await s.api(redeemed.capability, '/api/panes/session.bind', {
        method: 'POST',
        body: {
          launch: redeemed.launch,
          generation: opened.pane.generation,
          candidate: { sessionId: 'codex-rollout-1', turn: `[consensflow launch ${nonce}]\nwork` },
        },
      }),
      200,
    )
    await s.endPane(opened.pane, tab.lead)

    // The roster name is recreated on a different harness. Attaching now
    // would hand a codex conversation a claude-style preallocated id, keep
    // `kind: 'codex'` on the row, and drop the binding it already had.
    removeAgent('zeus', s.env)
    addAgent({ name: 'zeus', harness: 'claude', model: 'opus' }, s.env)
    const opens = s.seen.open.length
    const response = await s.api(tab.lead, '/api/panes/attach', {
      method: 'POST',
      body: { tab: tab.tab.id, session: opened.conversation, opId: 'cross-2' },
    })
    const refusal = await json(response, 400)
    assert.equal(refusal.error, 'agent-mismatch')
    assert.equal(s.seen.open.length, opens, 'and nothing was opened on the way to refusing')

    const row = s.threads()[opened.conversation]
    assert.equal(row.kind, 'codex')
    assert.equal(row.sessionId, 'codex-rollout-1', 'the session it had is still the session it has')
    assert.equal(row.binding.evidence, 'nonce', 'and its binding was not dropped')
    assert.equal(row.reserved, undefined, 'and no launch was admitted')

    removeAgent('zeus', s.env)
    addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, s.env)
  })
})

/**
 * TEST-PANE-35: every bridge request Rust sends, answered.
 *
 * `app/src-tauri/src/commands.rs` sends twelve operations over the bridge —
 * `grep request_node` names them all. An op with no handler answers
 * `{ok:false, error:'unknown-op'}` (`src/bridge.js:348`), which Rust
 * normalises to `not-available-yet` and the page renders as "not available
 * yet". So the first thing to prove is the negative: none of them can reach
 * that any more.
 */
const RUST_SENDS = [
  ['tab.open', () => ({ dir: null, harness: 'claude-code' })],
  ['tab.resume', (s) => ({ tab: s.tabId })],
  ['shell.open', (s) => ({ tab: s.tabId })],
  ['consult', (s) => ({ tab: s.tabId, agent: 'zeus', task: 'hello' })],
  ['attach', (s) => ({ tab: s.tabId, agent: 'zeus', session: s.session })],
  ['pane.close', (s) => ({ id: s.paneId, generation: 1 })],
  ['notify.set', (s) => ({ scope: 'tab', id: s.tabId, mode: 'manual' })],
  ['state.list', () => ({})],
  ['answers.list', (s) => ({ tab: s.tabId, pane: s.paneId, conversation: s.session })],
  [
    'deliver.now',
    (s) => ({
      delivery: null,
      tab: s.tabId,
      conversation: s.session,
      answerId: 'a-1',
      resend: false,
    }),
  ],
  ['deliver.cancel', () => ({ delivery: 'd-1' })],
  ['held.send', (s) => ({ tab: s.tabId })],
]

describe('every operation Rust sends over the bridge has a handler', () => {
  let s
  let context

  before(async () => {
    s = await paneServer()
    const its = await s.tab('claude-code')
    const opened = await json(
      await s.api(its.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: its.tab.id, agent: 'zeus', task: 'seed', fresh: true, opId: 'bridge-seed' },
      }),
      200,
    )
    context = {
      tabId: its.tab.id,
      paneId: opened.pane.id,
      session: opened.conversation,
      dir: s.workspace,
    }
  })
  after(async () => {
    await s.close()
  })

  for (const [op, body] of RUST_SENDS) {
    it(`answers ${op} with something other than "not available yet"`, async () => {
      const built = body(context)
      const answer = await s.rust.request(op, {
        ...built,
        ...(built.dir === null ? { dir: context.dir } : {}),
      })
      assert.notEqual(
        answer?.error,
        'unknown-op',
        `${op} has no handler: Rust would render this as "not available yet"`,
      )
      assert.equal(typeof answer, 'object', op)
      assert.notEqual(answer, null, op)
    })
  }
})

describe('tab.open launches the tab lead itself', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  const tabsOnDisk = () =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs

  it('mints the tab, opens its lead pane and resolves the reservation', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    assert.equal(opened.outcome, 'opened')

    const frame = s.seen.open.at(-1)
    assert.equal(frame.cwd, s.workspace)
    assert.equal(frame.argv[0], s.shimmed.claude, 'the lead runs the harness CLI, absolutely')
    assert.equal(frame.launch, opened.launch)
    assert.equal(frame.id, opened.pane.id)
    // The lead's window is the tab's own pane, not an extra one.
    const tab = tabsOnDisk().find((candidate) => candidate.id === opened.tab)
    assert.equal(tab.panes.find((pane) => pane.kind === 'lead').id, opened.pane.id)
    assert.equal(tab.panes.length, 1)

    // The credential the lead calls back with, and the launch the store now
    // holds — resolved, because the pane host said it opened.
    assert.equal(typeof frame.env.CONSENSFLOW_APP_TOKEN, 'string')
    assert.equal(frame.env.CONSENSFLOW_TAB, opened.tab)
    assert.equal(tab.lead.reserved.launchId, opened.launch)
    assert.equal(tab.lead.reserved.generation, tab.lead.generation)
    assert.equal(typeof tab.lead.reserved.resolvedAt, 'string')
  })

  it('carries the channel the lead harness was configured with', async () => {
    // The real `src/channels.js`: OpenCode gets a port, a hostname and a
    // password, and the channel object goes onto the reservation because
    // the delivery watcher reads it from there rather than rediscovering a
    // port nobody wrote down.
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'opencode' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const frame = s.seen.open.at(-1)
    assert.equal(frame.argv[0], s.shimmed.opencode)
    // The channel's arguments come first, then the harness's own session
    // shape — the two are appended in that order and neither truncates the
    // other.
    assert.deepEqual(frame.argv.slice(1, 4), ['--port', frame.argv[2], '--hostname'])
    assert.equal(frame.argv[4], '127.0.0.1')
    assert.match(frame.env.OPENCODE_SERVER_PASSWORD, /^[\w-]{8,}$/)

    const tab = tabsOnDisk().find((candidate) => candidate.id === opened.tab)
    assert.equal(tab.lead.reserved.channel.kind, 'opencode-server')
    assert.equal(
      tab.lead.reserved.channel.endpoint,
      `http://127.0.0.1:${frame.argv[2]}`,
      'the endpoint on the reservation is the port the lead was told to listen on',
    )
    assert.equal(tab.lead.reserved.channel.password, frame.env.OPENCODE_SERVER_PASSWORD)
  })

  it('adds nothing for a harness with no channel of its own', async () => {
    // Only OpenCode and Pi have one, and both were proved by a live probe.
    // `launchConfiguration` THROWS for the rest, so the launcher asks
    // `enabledChannels` first instead of guessing from the name.
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'codex' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const frame = s.seen.open.at(-1)
    assert.equal(frame.argv[0], s.shimmed.codex)
    // Nothing a channel would have added: no port, no hostname, no
    // extension, and no channel variable in the environment. What the argv
    // DOES carry is the harness's own session shape, which is task 21.
    assert.equal(
      frame.argv.some((arg) => ['--port', '--hostname', '--extension'].includes(arg)),
      false,
      JSON.stringify(frame.argv),
    )
    assert.equal(frame.env.OPENCODE_SERVER_PASSWORD, undefined)
    assert.equal(frame.env.CF_DELIVERY_INBOX, undefined)
    const tab = tabsOnDisk().find((candidate) => candidate.id === opened.tab)
    assert.equal(tab.lead.reserved.channel, undefined)
  })

  it('gives the tab and the reservation back when the pane host refuses', async () => {
    const before = tabsOnDisk().length
    s.state.open = () => ({ ok: false, error: 'no pty available' })
    try {
      const refused = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
      assert.equal(refused.ok, false)
      assert.equal(refused.error, 'pane-refused')
    } finally {
      s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
    }
    // The tab was minted for a lead that never opened: it is closed again,
    // and no reservation is left for a launch nothing will ever end.
    const tabs = tabsOnDisk()
    assert.equal(tabs.length, before + 1, 'the record stays, so the id is never reused')
    assert.equal(tabs.at(-1).closed, true)
    assert.equal(tabs.at(-1).lead.reserved, undefined)
  })

  it('refuses a second lead launch while the first has not come back', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    // A tab whose lead is launching is not a tab that launches again. The
    // reservation is resolved here, so it is `resume` that must refuse.
    const again = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(again.ok, false)
    assert.equal(again.error, 'resume-refused', JSON.stringify(again))
  })

  it('reopens a suspended tab at the next lead generation', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const was = tabsOnDisk().find((tab) => tab.id === opened.tab).lead.generation
    s.suspendTab(opened.tab)

    const resumed = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))
    const tab = tabsOnDisk().find((candidate) => candidate.id === opened.tab)
    assert.equal(tab.closed, false)
    assert.equal(tab.lead.generation, was + 1)
    assert.equal(tab.lead.reserved.generation, was + 1, 'and the reservation is for THIS one')
    assert.equal(s.seen.open.at(-1).generation, tab.panes[0].generation)
  })
})

describe('the rest of the page: panes, policy, state and deliveries', () => {
  let s
  let tab

  before(async () => {
    s = await paneServer()
    tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    assert.equal(tab.ok, true, JSON.stringify(tab))
  })
  after(async () => {
    await s.close()
  })

  it('opens a shell beside the agents, with no launch to fence', async () => {
    const opened = await s.rust.request('shell.open', { tab: tab.tab })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    assert.equal(opened.kind, 'shell')
    const frame = s.seen.open.at(-1)
    assert.equal(frame.cwd, s.workspace, 'a shell opens where the tab is')
    assert.equal(frame.argv[0], s.env.SHELL ?? '/bin/sh')
    // Nothing here is an agent, so nothing is reserved and no controller
    // credential is minted for it.
    assert.deepEqual(frame.env, {})
    assert.deepEqual(s.threads(), {}, 'and no conversation was invented for it')
  })

  it('closes a pane the human closed', async () => {
    const shell = await s.rust.request('shell.open', { tab: tab.tab })
    const closed = await s.rust.request('pane.close', {
      id: shell.pane.id,
      generation: shell.pane.generation,
    })
    assert.equal(closed.ok, true, JSON.stringify(closed))
    const listed = await s.rust.request('state.list', {})
    assert.equal(
      listed.tabs
        .find((candidate) => candidate.id === tab.tab)
        .panes.some((pane) => pane.id === shell.pane.id),
      false,
      'the pane row goes with the window',
    )

    // A pane the app already forgot is not an error: the human closed a
    // window, and it is closed.
    const gone = await s.rust.request('pane.close', { id: shell.pane.id, generation: 1 })
    assert.equal(gone.ok, true)
    assert.equal(gone.outcome, 'gone')
  })

  it('records the human policy at both scopes, and the page reads it back', async () => {
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'zeus',
      task: 'policy',
    })
    assert.equal(consulted.ok, true, JSON.stringify(consulted))

    const set = await s.rust.request('notify.set', {
      scope: 'pane',
      id: consulted.pane.id,
      mode: 'manual',
    })
    assert.equal(set.ok, true, JSON.stringify(set))
    assert.equal(set.tab, tab.tab, 'a pane names its tab in the answer, the body carries none')

    const listed = await s.rust.request('state.list', {})
    const pane = listed.tabs
      .find((candidate) => candidate.id === tab.tab)
      .panes.find((candidate) => candidate.id === consulted.pane.id)
    assert.equal(pane.policy, 'manual', 'the raw field the page recomputes from')
    assert.deepEqual(pane.effectivePolicy, { mode: 'manual', source: 'pane-human' })

    // The tab's veto sits ABOVE the pane's own setting, and `auto` below it.
    await s.rust.request('notify.set', { scope: 'tab', id: tab.tab, mode: 'manual' })
    const vetoed = await s.rust.request('state.list', {})
    const tabRow = vetoed.tabs.find((candidate) => candidate.id === tab.tab)
    assert.equal(tabRow.policy, 'manual')

    const refused = await s.rust.request('notify.set', {
      scope: 'window',
      id: tab.tab,
      mode: 'auto',
    })
    assert.equal(refused.ok, false)
    assert.equal(refused.error, 'bad-scope')
  })

  it('draws the whole page in one answer', async () => {
    const state = await s.rust.request('state.list', {})
    assert.equal(state.ok, true)
    assert.equal(state.available, true, 'the page shows itself as available, never "not yet"')
    assert.deepEqual(
      state.agents.map((agent) => agent.name).sort(),
      ['clio', 'nyx', 'zeus'],
      'the roster the page offers',
    )
    const drawn = state.tabs.find((candidate) => candidate.id === tab.tab)
    assert.equal(drawn.directory, s.workspace)
    assert.equal(drawn.name, 'workspace', 'a display name, derived from the directory')
    assert.equal(drawn.closed, false)
    assert.equal(drawn.lead.harness, 'claude-code')
    assert.equal(drawn.lead.generation, 1)

    const lead = drawn.panes.find((pane) => pane.kind === 'lead')
    assert.equal(lead.alive, true, 'its launch resolved, so its window is there')
    assert.equal(lead.id, tab.pane.id)
    const worker = drawn.panes.find((pane) => pane.kind === 'worker')
    assert.equal(worker.agent, 'zeus')
    assert.equal(worker.harness, 'codex')
    assert.equal(typeof worker.conversation, 'string')
    assert.ok(Array.isArray(state.deliveries))
    assert.ok(Array.isArray(state.held))
  })

  it('tells Rust that something changed, once per mutation', async () => {
    const before = s.seen.changed.length
    await s.rust.request('notify.set', { scope: 'tab', id: tab.tab, mode: 'auto' })
    await waitFor(() => s.seen.changed.length > before)
    assert.equal(
      s.seen.changed.length,
      before + 1,
      'one policy write, one announcement — not one per file it touched',
    )
    assert.equal(s.seen.changed.at(-1).op, 'policy.set')

    // A refused operation changed nothing, and says nothing.
    const quiet = s.seen.changed.length
    const refused = await s.rust.request('notify.set', { scope: 'tab', id: 't-nope', mode: 'auto' })
    assert.equal(refused.ok, false)
    assert.equal(s.seen.changed.length, quiet)
  })

  it('lists the answers behind a conversation, and says when it cannot', async () => {
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'zeus',
      task: 'answers',
    })
    const listed = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: consulted.pane.id,
      conversation: consulted.conversation,
    })
    // Nothing has bound a native session yet, so there is no transcript to
    // read. That is an empty list with a reason, never an error: the page
    // draws a conversation that has not spoken yet.
    assert.equal(listed.ok, true, JSON.stringify(listed))
    assert.deepEqual(listed.answers, [])
    assert.equal(listed.unknown, false)
    assert.equal(listed.reason, 'not bound yet')

    const missing = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: consulted.pane.id,
      conversation: 'nobody-here',
    })
    assert.equal(missing.ok, false)
    assert.equal(missing.error, 'no-conversation')
  })

  it('sends a cancelled answer as a new record, and never invents a transition', async () => {
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'zeus',
      task: 'deliveries',
    })
    // What the watcher leaves when a manual policy stops an answer: a
    // record naming the answer, cancelled, with its reason.
    s.seedDelivery({
      id: 'd-77',
      tab: tab.tab,
      pane: consulted.pane.id,
      conversation: consulted.conversation,
      answerId: 'a-9',
      state: 'cancelled',
      reason: 'automatic delivery cancelled by manual policy (pane-human)',
      lead: `tab:${tab.tab}:1`,
    })

    const sent = await s.rust.request('deliver.now', {
      delivery: 'd-77',
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: 'a-9',
      resend: false,
    })
    assert.equal(sent.ok, true, JSON.stringify(sent))
    // `cancelled` is terminal in `hosts/lib/deliveries.js` and its own
    // `transition` silently refuses an illegal move — so writing `pending`
    // onto this record would have reported a success that never happened.
    // A human sending a cancelled answer gets a NEW record, which is what
    // `resend` is for, and the cancelled one stays as history.
    assert.equal(sent.resent, true)
    assert.notEqual(sent.delivery, 'd-77')
    assert.match(sent.delivery, /^d-\d+$/, 'its id comes from the store allocator')
    const fresh = s.deliveries()[sent.delivery]
    assert.equal(fresh.state, 'pending')
    assert.equal(fresh.manual, true, 'the one flag the watcher honours against a manual policy')
    assert.equal(fresh.resendOf, 'd-77')
    assert.equal(fresh.answerId, 'a-9')
    assert.equal(s.deliveries()['d-77'].state, 'cancelled', 'and history is not rewritten')

    // A record already pending needs no new identity: it is the state the
    // watcher acts on, and only the override is missing.
    const marked = await s.rust.request('deliver.now', {
      delivery: sent.delivery,
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: 'a-9',
      resend: false,
    })
    assert.equal(marked.ok, true, JSON.stringify(marked))
    assert.equal(marked.resent, false)
    assert.equal(marked.delivery, sent.delivery)

    // ...unless the human explicitly asks to send it again.
    const again = await s.rust.request('deliver.now', {
      delivery: sent.delivery,
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: 'a-9',
      resend: true,
    })
    assert.equal(again.resent, true)
    assert.notEqual(again.delivery, sent.delivery)

    const cancelled = await s.rust.request('deliver.cancel', { delivery: 'd-77' })
    assert.equal(cancelled.ok, true, JSON.stringify(cancelled))
    assert.equal(cancelled.state, 'cancelled')
    assert.equal(cancelled.changed, false, 'it was already cancelled: nothing moved')
    const stopped = await s.rust.request('deliver.cancel', { delivery: again.delivery })
    assert.equal(stopped.changed, true, 'a pending one really is stopped')
    assert.equal(stopped.state, 'cancelled')

    const unknown = await s.rust.request('deliver.cancel', { delivery: 'd-404' })
    assert.equal(unknown.ok, false)
    assert.equal(unknown.error, 'no-delivery')

    // An answer id that is in no transcript names nothing to send. The
    // refusal is about the ANSWER, not about a missing delivery record — a
    // record is exactly what a manual policy never has.
    const early = await s.rust.request('deliver.now', {
      delivery: null,
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: 'a-never-seen',
      resend: false,
    })
    assert.equal(early.ok, false)
    assert.equal(early.error, 'no-answer')
  })
})

describe('a lead pane that ends leaves a tab the human can get back', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  const tabOnDisk = (id) =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs.find(
      (tab) => tab.id === id,
    )

  const leadOf = async (id) => {
    const state = await s.rust.request('state.list', {})
    return state.tabs.find((tab) => tab.id === id).panes.find((pane) => pane.kind === 'lead')
  }

  it('suspends the tab when the lead process exits, and resume brings it back', async () => {
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    assert.equal((await leadOf(tab.tab)).alive, true)

    // The lead's process ended — the only honest signal that it is gone.
    s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
    await waitFor(() => tabOnDisk(tab.tab).closed === true)

    // Without this the page would draw a live lead that is dead, and
    // `tab.resume` would answer "not suspended" — a tab with no way back.
    assert.equal((await leadOf(tab.tab)).alive, false)
    assert.equal(tabOnDisk(tab.tab).lead.reserved, undefined, 'and its launch is over')

    const resumed = await s.rust.request('tab.resume', { tab: tab.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))
    assert.equal(tabOnDisk(tab.tab).lead.generation, 2)
    assert.equal((await leadOf(tab.tab)).alive, true)
  })

  it('does the same when the human closes the lead window', async () => {
    // Rust kills the process and THEN sends `pane.close`, so this cannot
    // refuse — the window is gone whatever the answer says.
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const closed = await s.rust.request('pane.close', {
      id: tab.pane.id,
      generation: tab.pane.generation,
    })
    assert.equal(closed.ok, true, JSON.stringify(closed))
    assert.equal(closed.kind, 'lead')
    assert.equal(tabOnDisk(tab.tab).closed, true)
    assert.equal((await leadOf(tab.tab)).alive, false)
    assert.equal((await s.rust.request('tab.resume', { tab: tab.tab })).ok, true)
  })

  it('resumes over the worker panes the host still has, and releases the ones it lost', async () => {
    // A resume asks the host what it still holds. A row the host CONFIRMS
    // is alive must survive — ending its launch would kill a running
    // worker's capability and take its conversation off the page — and a
    // row the host no longer has gets exactly its `pane.exit` bookkeeping.
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const lead = s.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN
    const worker = await json(
      await s.api(lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab,
          agent: 'zeus',
          task: 'still running across the resume',
          fresh: true,
          opId: 'reconcile-keeps-live',
        },
      }),
      200,
    )
    const hasWorker = async () =>
      (await s.rust.request('state.list', {})).tabs
        .find((candidate) => candidate.id === tab.tab)
        .panes.some((pane) => pane.id === worker.pane.id)
    const suspend = async (generation) => {
      s.rust.event('pane.exit', { id: tab.pane.id, generation })
      await waitFor(() => tabOnDisk(tab.tab).closed === true)
    }

    // The host still has it, at the same id and generation.
    s.state.list = () => ({
      ok: true,
      panes: [{ id: worker.pane.id, generation: worker.pane.generation, alive: true, idleMs: 0 }],
    })
    await suspend(tab.pane.generation)
    assert.equal((await s.rust.request('tab.resume', { tab: tab.tab })).ok, true)
    assert.equal(await hasWorker(), true, 'a pane the host confirms alive is not reconciled away')

    // The same id at a generation the host no longer has is not that pane:
    // identity is `(id, generation)` here as everywhere else.
    s.state.list = () => ({
      ok: true,
      panes: [{ id: worker.pane.id, generation: worker.pane.generation, alive: false, idleMs: 0 }],
    })
    await suspend(tab.pane.generation + 1)
    assert.equal((await s.rust.request('tab.resume', { tab: tab.tab })).ok, true)
    assert.equal(await hasWorker(), false, 'a pane the host lost is released with its launch')
  })

  it('refuses the resume when the pane list cannot be read, and changes nothing', async () => {
    // Fail closed: reconciling against an answer we do not understand ends
    // launches that may be running. Refusing leaves the tab as the human
    // left it — closed, same generation, same rows — and they can ask
    // again. Every unreadable shape is one refusal, not a partial resume.
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const lead = s.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN
    const worker = await json(
      await s.api(lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab,
          agent: 'zeus',
          task: 'must not be released by a bad list',
          fresh: true,
          opId: 'reconcile-fails-closed',
        },
      }),
      200,
    )
    s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
    await waitFor(() => tabOnDisk(tab.tab).closed === true)
    const before = tabOnDisk(tab.tab)

    const unreadable = [
      ['a host that cannot answer at all', null],
      ['a refusal', () => ({ ok: false, error: 'host-list-unavailable' })],
      ['an answer with no list in it', () => ({ ok: true })],
      ['a list that is not a list', () => ({ ok: true, panes: 'none' })],
      ['an entry that is not a record', () => ({ ok: true, panes: ['p-1'] })],
      ['an entry with no generation', () => ({ ok: true, panes: [{ id: 'p-1', alive: true }] })],
      [
        'an entry whose liveness is not a boolean',
        () => ({ ok: true, panes: [{ id: 'p-1', generation: 1, alive: 'yes' }] }),
      ],
      [
        // Past 2^53 two generations can compare equal when they are not,
        // so a row could be matched against a pane that is not it.
        'an entry whose generation is past the safe integers',
        () => ({
          ok: true,
          panes: [{ id: 'p-1', generation: Number.MAX_SAFE_INTEGER + 1, alive: true }],
        }),
      ],
    ]
    for (const [subject, list] of unreadable) {
      s.state.list = list
      const refused = await s.rust.request('tab.resume', { tab: tab.tab })
      assert.equal(refused.ok, false, `${subject}: ${JSON.stringify(refused)}`)
      assert.equal(refused.error, 'pane-list-unavailable', subject)
      const after = tabOnDisk(tab.tab)
      assert.equal(after.closed, true, `${subject}: the tab is still suspended`)
      assert.equal(after.lead.generation, before.lead.generation, `${subject}: no new generation`)
      assert.equal(
        after.panes.some((pane) => pane.id === worker.pane.id),
        true,
        `${subject}: the worker row stands`,
      )
    }

    // And the same tab resumes once the host can answer — the refusals
    // above cost it nothing.
    s.state.list = () => ({
      ok: true,
      panes: [{ id: worker.pane.id, generation: worker.pane.generation, alive: true, idleMs: 0 }],
    })
    assert.equal((await s.rust.request('tab.resume', { tab: tab.tab })).ok, true)
  })

  it('reconciles nothing when the tab is not suspended and the resume is refused', async () => {
    // Resuming a tab that is already open is a mistake the store refuses.
    // Its panes are RUNNING, so answering that mistake by asking what the
    // host holds — and acting on it — would end the launches of a tab that
    // is working. The refusal must arrive with nothing touched.
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const lead = s.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN
    const worker = await json(
      await s.api(lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab,
          agent: 'zeus',
          task: 'running in a tab nobody suspended',
          fresh: true,
          opId: 'reconcile-open-tab',
        },
      }),
      200,
    )
    // A host that would report every pane gone: the reconcile must never
    // reach it, so what it would have said cannot matter.
    s.state.list = () => ({ ok: true, panes: [] })
    const asked = s.seen.list.length

    const refused = await s.rust.request('tab.resume', { tab: tab.tab })
    assert.equal(refused.ok, false, JSON.stringify(refused))
    assert.equal(refused.error, 'resume-refused')
    assert.equal(s.seen.list.length, asked, 'the pane host was never asked')
    const after = tabOnDisk(tab.tab)
    assert.equal(after.closed, false)
    assert.equal(after.lead.generation, 1, 'no new generation')
    assert.equal(
      after.panes.some((pane) => pane.id === worker.pane.id),
      true,
      'the running worker keeps its row',
    )
  })

  it('ignores an exit that names a lead generation already replaced', async () => {
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
    await waitFor(() => tabOnDisk(tab.tab).closed === true)
    await s.rust.request('tab.resume', { tab: tab.tab })

    // The old generation's exit arriving late must not close the tab its
    // successor is running in.
    s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
    await new Promise((resolve) => setTimeout(resolve, 50))
    assert.equal(tabOnDisk(tab.tab).closed, false, 'the resumed tab stays open')
    assert.equal((await leadOf(tab.tab)).alive, true)
  })
})

describe('answers.list reads the harness transcript, not the conversation record', () => {
  let s
  let tab

  before(async () => {
    s = await paneServer()
    tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
  })
  after(async () => {
    await s.close()
  })

  it('lists what the agent said, and nothing the lead said to it', async () => {
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'nyx',
      task: 'transcript',
    })
    assert.equal(consulted.ok, true, JSON.stringify(consulted))

    // A real Claude Code transcript, carrying user turns AND assistant
    // turns, where the adapter goes looking for one.
    const sessionId = '11111111-2222-3333-4444-555555555555'
    const projects = join(s.env.CLAUDE_CONFIG_DIR, 'projects', 'workspace')
    mkdirSync(projects, { recursive: true })
    writeFileSync(
      join(projects, `${sessionId}.jsonl`),
      readFileSync('tests/engine/fixtures/completion/claude-code/frontier-history.jsonl', 'utf8'),
    )
    // Binding is the controller's job in the real app; the file is where
    // the truth lives, and the store re-reads it on every mutation.
    const file = join(
      s.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(s.workspace),
      'threads.json',
    )
    const threads = JSON.parse(readFileSync(file, 'utf8'))
    threads[consulted.conversation].sessionId = sessionId
    writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)

    const listed = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: consulted.pane.id,
      conversation: consulted.conversation,
    })
    assert.equal(listed.ok, true, JSON.stringify(listed))
    assert.equal(listed.unknown, false)
    // The fixture holds two user turns and one assistant turn. Listing the
    // lead's own prompts back to it as answers to deliver is a list nobody
    // can use, so only the assistant's turn is one.
    assert.equal(listed.answers.length, 1, JSON.stringify(listed.answers))
    const answer = listed.answers[0]
    assert.equal(typeof answer.id, 'string')
    assert.equal(answer.delivered, false, 'nothing has been delivered for this conversation')
    // This transcript's assistant turn is still streaming, and the adapter
    // says so with `complete: false`. It is offered WITH that said rather
    // than hidden: a half-written answer the human can see is better than
    // an empty list that looks like silence.
    assert.equal(answer.uncertain, true)
    assert.equal(answer.ready, false, 'an incomplete answer is visible but not send-ready')

    // A finished transcript previews, so the page can show the answer
    // without opening it.
    const settledId = '66666666-7777-8888-9999-000000000000'
    writeFileSync(
      join(projects, `${settledId}.jsonl`),
      readFileSync('tests/engine/fixtures/completion/claude-code/fragments.jsonl', 'utf8'),
    )
    const rebound = JSON.parse(readFileSync(file, 'utf8'))
    rebound[consulted.conversation].sessionId = settledId
    writeFileSync(file, `${JSON.stringify(rebound, null, 2)}\n`)

    const settled = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: consulted.pane.id,
      conversation: consulted.conversation,
    })
    assert.equal(settled.unknown, false, JSON.stringify(settled))
    assert.ok(settled.answers.length > 0)
    assert.ok(
      settled.answers.every((candidate) => candidate.preview.length > 0),
      'every answer carries text the page can show',
    )
    assert.ok(
      settled.answers.every((candidate) => candidate.preview.length <= 160),
      'and none of them is a whole transcript pasted into a list',
    )
    assert.ok(
      settled.answers.some((candidate) => candidate.ready === true),
      'a complete and settled answer is explicitly send-ready',
    )
    const settledAnswer = settled.answers.find((candidate) => candidate.ready)
    const attempt = {
      id: 'd-8002',
      conversation: consulted.conversation,
      answerId: settledAnswer.id,
      state: 'uncertain',
      channel: 'cf-read',
      parts: [{ k: 1 }, { k: 2 }, { k: 3 }],
      partCoverage: [[], ['receipt-2'], []],
    }
    s.seedDelivery(attempt)
    s.seedDelivery({ ...attempt, id: 'd-8001', partCoverage: [] })
    const progress = await s.rust.request('answers.list', {
      tab: tab.tab,
      conversation: consulted.conversation,
    })
    assert.deepEqual(
      progress.answers.find((item) => item.id === settledAnswer.id).partProgress,
      {
        delivery: 'd-8002',
        total: 3,
        uncovered: [1, 3],
      },
      'the latest attempt exposes exactly the unconfirmed parts without leaking answer bodies',
    )
    s.seedDelivery({ ...attempt, id: 'd-8003', state: 'accepted' })
    const accepted = await s.rust.request('answers.list', {
      tab: tab.tab,
      conversation: consulted.conversation,
    })
    assert.equal(
      accepted.answers.find((item) => item.id === settledAnswer.id).partProgress,
      undefined,
    )
  })

  it('says so plainly when the transcript cannot be read', async () => {
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'nyx',
      task: 'no transcript',
    })
    const file = join(
      s.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(s.workspace),
      'threads.json',
    )
    const threads = JSON.parse(readFileSync(file, 'utf8'))
    threads[consulted.conversation].sessionId = 'a-session-that-is-not-on-disk'
    writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)

    const listed = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: consulted.pane.id,
      conversation: consulted.conversation,
    })
    // `unknown` is the adapter's word for "I could not read this", and it
    // is not the same as "there are no answers": the page must be able to
    // tell an empty conversation from one it cannot see.
    assert.equal(listed.ok, true, JSON.stringify(listed))
    assert.equal(listed.unknown, true)
    assert.match(listed.reason, /no claude session/)
    assert.deepEqual(listed.answers, [])
  })
})

/**
 * TEST-PANE-35, task 21: a lead is a session like a worker.
 *
 * "Sessions resumable like herdr" (Decision Log) is not a worker-only
 * promise. A lead that cannot be resumed loses the whole PM conversation
 * every time its tab is suspended, which is the one thing a tab is FOR. So
 * the lead takes the same three shapes a worker does — an id we preallocate
 * and pass, an id the harness mints and reports, or the launch nonce
 * carried in the seed — and `tab.resume` reopens the session it bound.
 */
describe('the tab lead binds a native session, and resume returns to it', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  const tabOnDisk = (id) =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs.find(
      (tab) => tab.id === id,
    )

  it('gives claude-code an id we mint, and binds on it at open', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const argv = s.seen.open.at(-1).argv

    // `hosts/lib/runners.js` owns which flag each harness takes; claude's
    // is `--session-id`, and the id is a uuid the caller mints.
    const flag = argv.indexOf('--session-id')
    assert.notEqual(flag, -1, `no --session-id in ${JSON.stringify(argv)}`)
    const minted = argv[flag + 1]
    assert.match(minted, /^[0-9a-f]{8}-[0-9a-f]{4}-/)

    const lead = tabOnDisk(opened.tab).lead
    assert.equal(lead.nativeSession, minted, 'bound at open, on the id we passed')
    assert.equal(lead.binding.evidence, 'preallocated')
    assert.equal(lead.binding.launchId, opened.launch)
  })

  it('gives pi a tab-scoped name, which is its own id', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'pi' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const argv = s.seen.open.at(-1).argv
    const flag = argv.indexOf('--session-id')
    assert.notEqual(flag, -1, `no --session-id in ${JSON.stringify(argv)}`)
    assert.match(argv[flag + 1], new RegExp(`${opened.tab}`), 'scoped to the tab it leads')
    assert.equal(tabOnDisk(opened.tab).lead.nativeSession, argv[flag + 1])
  })

  it('carries the launch nonce in the seed when the harness mints its own id', async () => {
    // codex has no way to pre-set an interactive session id, so it opens
    // cold and the nonce travels in the first line of the seed — the same
    // evidence a worker leaves, and the only one that survives a harness
    // choosing its own identity.
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'codex' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const tab = tabOnDisk(opened.tab)
    const nonce = tab.lead.reserved.nonce
    assert.equal(typeof nonce, 'string')
    const seed = s.seen.open.at(-1).argv.at(-1)
    assert.equal(
      seed.split('\n')[0],
      `[consensflow launch ${nonce}]`,
      'the marker opens the seed — a bare nonce is not evidence',
    )

    // Nothing has observed the session yet, so nothing is bound. That is
    // not a failure: it is the honest state until discovery reports one.
    assert.equal(tab.lead.nativeSession, null)
    assert.equal(tab.lead.binding, undefined)
  })

  it('resumes the session the lead bound, with the harness resume argument', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const bound = tabOnDisk(opened.tab).lead.nativeSession

    s.rust.event('pane.exit', { id: opened.pane.id, generation: opened.pane.generation })
    await waitFor(() => tabOnDisk(opened.tab).closed === true)
    const resumed = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))

    // The whole point: the PM conversation survives the suspend.
    const argv = s.seen.open.at(-1).argv
    assert.deepEqual(argv.slice(1), ['--resume', bound], JSON.stringify(argv))
    assert.equal(resumed.resumedSession, bound)
    assert.equal(tabOnDisk(opened.tab).lead.nativeSession, bound, 'and it is still bound')
  })

  it('resumes cold when the lead never bound, and says why', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'codex' })
    s.rust.event('pane.exit', { id: opened.pane.id, generation: opened.pane.generation })
    await waitFor(() => tabOnDisk(opened.tab).closed === true)

    const resumed = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))
    assert.equal(resumed.resumedSession, null)
    assert.equal(resumed.cold, true)
    assert.match(resumed.reason, /never bound/i)
    // A cold resume is a NEW launch with its own nonce, not the old one.
    const tab = tabOnDisk(opened.tab)
    assert.notEqual(tab.lead.reserved.nonce, undefined)
    assert.equal(
      s.seen.open.at(-1).argv.at(-1).split('\n')[0],
      `[consensflow launch ${tab.lead.reserved.nonce}]`,
    )
  })

  it('tells the page whether the lead is bound', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const state = await s.rust.request('state.list', {})
    const lead = state.tabs.find((tab) => tab.id === opened.tab).lead
    assert.equal(lead.bound, true)
    assert.equal(lead.nativeSession, tabOnDisk(opened.tab).lead.nativeSession)
  })

  it('carries the harness billing guard on the frame, at open and at resume', async () => {
    // A worker is spawned through `cf`, which strips these names itself. A
    // lead is spawned by Rust from this frame alone, so the guard only
    // reaches the child if `dropEnv` does — otherwise a subscription login
    // silently switches to API-key billing. The list is the invocation's
    // own (`hosts/lib/runners.js`), never a second copy of the rule.
    for (const [harness, dropEnv] of [
      ['claude-code', ['ANTHROPIC_API_KEY']],
      ['codex', ['OPENAI_API_KEY']],
      ['pi', []],
      ['opencode', []],
    ]) {
      const opened = await s.rust.request('tab.open', { dir: s.workspace, harness })
      assert.equal(opened.ok, true, JSON.stringify(opened))
      assert.deepEqual(s.seen.open.at(-1).dropEnv, dropEnv, `${harness} at open`)
    }

    // And on the resume frame, which is built from `interactiveResume`
    // rather than `interactiveStart` — a different branch, same guard.
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    s.rust.event('pane.exit', { id: opened.pane.id, generation: opened.pane.generation })
    await waitFor(() => tabOnDisk(opened.tab).closed === true)
    const resumed = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))
    const frame = s.seen.open.at(-1)
    assert.equal(frame.argv.includes('--resume'), true, JSON.stringify(frame.argv))
    assert.deepEqual(frame.dropEnv, ['ANTHROPIC_API_KEY'], 'claude-code at resume')
  })
})

describe('BO11: only a typed refusal is the caller’s fault', () => {
  it('answers 500 with the reason when the store itself is broken', async () => {
    const s = await paneServer()
    try {
      const tab = await s.tab('claude-code')
      const opened = await json(
        await s.api(tab.lead, '/api/panes/consult', {
          method: 'POST',
          body: { tab: tab.tab.id, agent: 'zeus', task: 'corrupt', fresh: true, opId: 'bo11-1' },
        }),
        200,
      )
      const ownership = await json(
        await s.api(null, '/api/launch/redeem', {
          method: 'POST',
          body: { ticket: s.seen.open.at(-1).env.CONSENSFLOW_LAUNCH },
        }),
        200,
      )

      // The file the controller's write has to read is unreadable. That is
      // this machine failing, not the controller sending something wrong —
      // and a 400 would tell it to change a request that was fine.
      const file = join(
        s.env.CONSENSFLOW_HOME,
        'workspaces',
        workspaceKey(s.workspace),
        'threads.json',
      )
      writeFileSync(file, 'not json at all')

      const response = await s.api(ownership.capability, '/api/panes/progress.set', {
        method: 'POST',
        body: {
          launch: opened.launch,
          generation: opened.pane.generation,
          progress: { state: 'working' },
        },
      })
      const body = await json(response, 500)
      assert.equal(body.error, 'internal_error')
      assert.match(body.reason, /threads\.json/, 'and it still says what broke')
    } finally {
      await s.close()
    }
  })
})

/**
 * BO1: a lead that mints its own session id binds by the SAME marker a
 * worker does. A raw nonce in the seed is not evidence — `bindEvidence`
 * wants `[consensflow launch <nonce>]` opening one of the first five user
 * turns — so a seed carrying the bare uuid could never bind, and the lead
 * stayed cold through every resume.
 */
describe('BO1: a lead binds by the launch marker, and resumes on what it bound', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  const tabOnDisk = (id) =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs.find(
      (tab) => tab.id === id,
    )

  /** A codex rollout the way codex writes one, carrying the marker. */
  const seedCodexRollout = (sessionId, turn) => {
    const root = join(s.env.CODEX_HOME ?? join(s.env.HOME, '.codex'), 'sessions')
    mkdirSync(root, { recursive: true })
    const at = new Date().toISOString()
    writeFileSync(
      join(root, `rollout-${Date.now()}-${sessionId}.jsonl`),
      `${[
        JSON.stringify({
          type: 'session_meta',
          payload: { cwd: s.workspace, timestamp: at },
        }),
        JSON.stringify({ type: 'event_msg', payload: { type: 'user_message', message: turn } }),
      ].join('\n')}\n`,
    )
  }

  it('opens codex with the marker as the seed’s first line', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'codex' })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const nonce = tabOnDisk(opened.tab).lead.reserved.nonce
    const seed = s.seen.open.at(-1).argv.at(-1)
    assert.equal(
      seed.split('\n')[0],
      `[consensflow launch ${nonce}]`,
      'the seed opens with the marker the store checks for, not a bare id',
    )
    // The proof it is evidence and not just a string that looks like one:
    // the module that decides binding accepts this turn for this launch.
    assert.deepEqual(
      bindEvidence('codex', { sessionId: 'c-1', turn: seed }, { nonce, generation: 1 }),
      { bound: true, evidence: 'nonce', generation: 1 },
    )
  })

  it('binds a discovered codex transcript and resumes on that session', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'codex' })
    const nonce = tabOnDisk(opened.tab).lead.reserved.nonce
    assert.equal(tabOnDisk(opened.tab).lead.nativeSession, null, 'nothing observed yet')

    // codex has now written its rollout, carrying our marker.
    const sessionId = '00000000-1111-2222-3333-444444444444'
    seedCodexRollout(sessionId, `[consensflow launch ${nonce}]\nplease begin`)

    s.rust.event('pane.exit', { id: opened.pane.id, generation: opened.pane.generation })
    await waitFor(() => tabOnDisk(opened.tab).closed === true)
    const resumed = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))

    // Discovery ran, the store accepted the evidence, and the window came
    // back on the conversation instead of cold.
    const lead = tabOnDisk(opened.tab).lead
    assert.equal(lead.nativeSession, sessionId)
    assert.equal(lead.binding.evidence, 'nonce')
    assert.equal(resumed.cold, false)
    assert.equal(resumed.resumedSession, sessionId)
    assert.deepEqual(s.seen.open.at(-1).argv.slice(1), ['resume', sessionId])
  })

  it('leaves a lead unbound when the transcript carries someone else’s marker', async () => {
    const opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'codex' })
    seedCodexRollout(
      '55555555-6666-7777-8888-999999999999',
      '[consensflow launch not-our-nonce]\nhi',
    )
    s.rust.event('pane.exit', { id: opened.pane.id, generation: opened.pane.generation })
    await waitFor(() => tabOnDisk(opened.tab).closed === true)

    const resumed = await s.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))
    assert.equal(
      tabOnDisk(opened.tab).lead.nativeSession,
      null,
      'another launch’s session is not ours',
    )
    assert.equal(resumed.cold, true)
  })
})

/**
 * BO2: a lead exit that arrives late must not close the tab its successor
 * is running in. Releasing the launch and suspending the tab were two
 * separate writes with the guard read before both, so a resume landing
 * between them closed generation 2 while it kept its own reservation — a
 * tab the human had just asked for, shut by an event about a dead window.
 */
describe('BO2: a stale lead exit changes nothing', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  const tabOnDisk = (id) =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs.find(
      (tab) => tab.id === id,
    )

  it('never closes the generation that replaced it, however the two overlap', async () => {
    for (let attempt = 0; attempt < 12; attempt += 1) {
      const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })

      // Several deliveries of the SAME exit, all in one tick. Every handler
      // reads the tab while it is still generation 1, so every guard passes
      // — and the ones that lose the race find the reservation already
      // gone. Their release settles nothing; the suspend that used to
      // follow it regardless is what closed the generation that replaced it.
      for (let copy = 0; copy < 4; copy += 1) {
        s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
      }
      await waitFor(() => tabOnDisk(tab.tab).closed === true)
      const resumed = await s.rust.request('tab.resume', { tab: tab.tab })
      assert.equal(resumed.ok, true, JSON.stringify(resumed))
      await new Promise((resolve) => setTimeout(resolve, 25))

      const after = tabOnDisk(tab.tab)
      assert.equal(after.closed, false, `attempt ${attempt}: the resumed tab was closed`)
      assert.equal(after.lead.generation, 2)
      assert.notEqual(
        after.lead.reserved,
        undefined,
        `attempt ${attempt}: the new generation lost its reservation`,
      )
      assert.equal(after.lead.reserved.generation, 2)
    }
  })
})

describe('BO4, BO6, BO7: what a lead launch leaves behind when it does not simply work', () => {
  let s

  before(async () => {
    s = await paneServer({ paneOpenDeadlineMs: 120 })
  })
  after(async () => {
    await s.close()
  })

  const tabOnDisk = (id) =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs.find(
      (tab) => tab.id === id,
    )

  it('BO4: keeps an uncertain lead launch and its tab, waiting for an exit', async () => {
    // The frame went out and no answer came. The lead may well be running:
    // releasing here frees a launch nothing will ever end, and closing the
    // tab hides a window that is on the screen.
    const silent = new Promise(() => {})
    s.state.open = () => silent
    let opened
    try {
      opened = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    } finally {
      s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
    }
    assert.equal(opened.ok, true, JSON.stringify(opened))
    assert.equal(opened.outcome, 'unknown')

    const tab = tabOnDisk(opened.tab)
    assert.equal(tab.closed, false, 'the tab stays: its window may be up')
    assert.notEqual(tab.lead.reserved, undefined, 'and the launch is still held')
    assert.equal(tab.lead.reserved.resolvedAt, undefined, 'unresolved, which refuses a second one')

    // Only the pane's exit settles it — the same rule a worker follows.
    s.rust.event('pane.exit', { id: opened.pane.id, generation: opened.pane.generation })
    await waitFor(() => tabOnDisk(opened.tab).lead.reserved === undefined)
    assert.equal(tabOnDisk(opened.tab).closed, true)
  })

  it('BO4: keeps a shell pane whose open never came back', async () => {
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const before = (await s.rust.request('state.list', {})).tabs.find((t) => t.id === tab.tab).panes
      .length
    const silent = new Promise(() => {})
    s.state.open = () => silent
    let shell
    try {
      shell = await s.rust.request('shell.open', { tab: tab.tab })
    } finally {
      s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
    }
    assert.equal(shell.ok, true, JSON.stringify(shell))
    assert.equal(shell.outcome, 'unknown')

    // A shell we cannot account for is still a shell that may be running.
    // Removing its row loses the only handle anyone has on it.
    const after = (await s.rust.request('state.list', {})).tabs.find((t) => t.id === tab.tab)
    assert.equal(after.panes.length, before + 1, 'the pane row stays until its exit')
    s.rust.event('pane.exit', { id: shell.pane.id, generation: shell.pane.generation })
    await waitFor(async () => {
      const state = await s.rust.request('state.list', {})
      return state.tabs.find((t) => t.id === tab.tab).panes.length === before
    })
  })

  it('BO6: a refused resume puts the tab back to suspended, and the retry works', async () => {
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
    await waitFor(() => tabOnDisk(tab.tab).closed === true)

    s.state.open = () => ({ ok: false, error: 'no pty available' })
    let refused
    try {
      refused = await s.rust.request('tab.resume', { tab: tab.tab })
    } finally {
      s.state.open = (request) => ({ ok: true, id: request.id, generation: request.generation })
    }
    assert.equal(refused.ok, false)
    assert.equal(refused.error, 'pane-refused')

    // Left open with no reservation, the retry answered "not suspended" and
    // the tab could never be recovered at all.
    const after = tabOnDisk(tab.tab)
    assert.equal(after.closed, true, 'the generation that failed to open is suspended again')
    assert.equal(after.lead.reserved, undefined)
    const retried = await s.rust.request('tab.resume', { tab: tab.tab })
    assert.equal(retried.ok, true, JSON.stringify(retried))
  })

  it('BO7: a successful resume re-stamps the binding for the launch that opened it', async () => {
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const first = tabOnDisk(tab.tab).lead
    assert.equal(first.binding.generation, 1)

    s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
    await waitFor(() => tabOnDisk(tab.tab).closed === true)
    const resumed = await s.rust.request('tab.resume', { tab: tab.tab })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))

    // The session is the same one; the LAUNCH holding it is not. A binding
    // still naming the dead launch cannot be invalidated by generation,
    // which is the whole mechanism Phase 3 leans on.
    const after = tabOnDisk(tab.tab).lead
    assert.equal(after.nativeSession, first.nativeSession, 'the conversation survives')
    assert.equal(after.binding.generation, 2)
    assert.equal(after.binding.launchId, resumed.launch)
    assert.notEqual(after.binding.launchId, first.binding.launchId)
  })
})

describe('BO8, BO9: what the page is told about a delivery', () => {
  let s
  let tab
  let conversation

  before(async () => {
    s = await paneServer()
    tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'nyx',
      task: 'projected',
    })
    conversation = consulted.conversation
    // A record the way `plan()` writes one: the tab and pane live on the
    // TARGET, because a delivery is aimed at a lead, not at the pane that
    // produced the answer.
    s.seedDelivery({
      id: 'd-501',
      conversation,
      agent: 'nyx',
      answerId: 'a-1',
      answer: 'planned',
      state: 'pending',
      kind: 'claude-code',
      target: {
        leadId: `tab:${tab.tab}:1`,
        session: 'lead',
        tab: tab.tab,
        pane: tab.pane.id,
        generation: 1,
      },
    })
  })
  after(async () => {
    await s.close()
  })

  it('BO8: projects the tab and pane a planned record actually names', async () => {
    const state = await s.rust.request('state.list', {})
    const record = state.deliveries.find((candidate) => candidate.id === 'd-501')
    assert.notEqual(record, undefined, JSON.stringify(state.deliveries))
    // Reading `record.tab` and `record.pane` off a planned record finds
    // nothing at all, and the page's own pane filter then hides the badge
    // for a delivery that is perfectly valid.
    assert.equal(record.tab, tab.tab)
    assert.equal(record.pane, tab.pane.id)
    assert.equal(record.state, 'pending')
  })

  it('BO9: an answer is uncertain when its delivery is, not only when it is unfinished', async () => {
    // A finished answer whose delivery ended `uncertain`: the bytes may or
    // may not have reached the lead. Reporting that as `delivered: false,
    // uncertain: false` tells the human it plainly did not arrive, and the
    // one thing worse than a missing answer is a second copy of one.
    s.seedDelivery({
      id: 'd-502',
      conversation,
      agent: 'nyx',
      answerId: 'answer-uncertain',
      state: 'uncertain',
      reason: 'the write outcome is unknown',
      target: {
        leadId: `tab:${tab.tab}:1`,
        session: 'lead',
        tab: tab.tab,
        pane: tab.pane.id,
        generation: 1,
      },
    })
    const sessionId = '77777777-8888-9999-aaaa-bbbbbbbbbbbb'
    const projects = join(s.env.CLAUDE_CONFIG_DIR, 'projects', 'ws')
    mkdirSync(projects, { recursive: true })
    writeFileSync(
      join(projects, `${sessionId}.jsonl`),
      readFileSync('tests/engine/fixtures/completion/claude-code/fragments.jsonl', 'utf8'),
    )
    const file = join(
      s.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(s.workspace),
      'threads.json',
    )
    const threads = JSON.parse(readFileSync(file, 'utf8'))
    threads[conversation].sessionId = sessionId
    writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)

    const listed = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: tab.pane.id,
      conversation,
    })
    assert.equal(listed.ok, true, JSON.stringify(listed))
    const answers = listed.answers
    assert.ok(answers.length > 0)
    assert.ok(
      answers.every((answer) => answer.uncertain === false),
      'a complete answer with no delivery is not uncertain',
    )

    // Now aim the uncertain delivery at one of them.
    const target = answers[0]
    s.seedDelivery({
      id: 'd-502',
      conversation,
      agent: 'nyx',
      answerId: target.id,
      state: 'uncertain',
      reason: 'the write outcome is unknown',
      target: {
        leadId: `tab:${tab.tab}:1`,
        session: 'lead',
        tab: tab.tab,
        pane: tab.pane.id,
        generation: 1,
      },
    })
    const again = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: tab.pane.id,
      conversation,
    })
    const marked = again.answers.find((answer) => answer.id === target.id)
    assert.equal(marked.uncertain, true, 'its delivery may or may not have landed')
    assert.equal(marked.delivered, false, 'and it was never accepted')
  })
})

describe('BO3, BO5: the human sending and stopping a delivery', () => {
  let s
  let tab
  let conversation
  let answerId

  before(async () => {
    s = await paneServer()
    tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'nyx',
      task: 'manual policy',
    })
    conversation = consulted.conversation
    // The pane is on manual, which is the case that matters: under manual
    // the watcher plans NOTHING, so there is never a record to mark.
    await s.rust.request('notify.set', { scope: 'pane', id: consulted.pane.id, mode: 'manual' })

    const sessionId = 'cccccccc-dddd-eeee-ffff-000000000000'
    const projects = join(s.env.CLAUDE_CONFIG_DIR, 'projects', 'manual')
    mkdirSync(projects, { recursive: true })
    writeFileSync(
      join(projects, `${sessionId}.jsonl`),
      readFileSync('tests/engine/fixtures/completion/claude-code/fragments.jsonl', 'utf8'),
    )
    const file = join(
      s.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(s.workspace),
      'threads.json',
    )
    const threads = JSON.parse(readFileSync(file, 'utf8'))
    threads[conversation].sessionId = sessionId
    writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)
    const listed = await s.rust.request('answers.list', {
      tab: tab.tab,
      pane: tab.pane.id,
      conversation,
    })
    answerId = listed.answers[0].id
  })
  after(async () => {
    await s.close()
  })

  it('BO5: sends an answer the watcher never planned, because the policy is manual', async () => {
    const sent = await s.rust.request('deliver.now', {
      delivery: null,
      tab: tab.tab,
      conversation,
      answerId,
      resend: false,
    })
    // "not-seen-yet" was the whole bug: under a manual policy no record is
    // ever planned, so the one button that exists to override manual could
    // never do anything at all.
    assert.equal(sent.ok, true, JSON.stringify(sent))
    assert.match(sent.delivery, /^d-\d+$/)
    const record = s.deliveries()[sent.delivery]
    assert.equal(record.manual, true, 'the override the watcher honours')
    assert.equal(record.state, 'pending')
    assert.equal(record.answerId, answerId)
    assert.equal(record.conversation, conversation)
    assert.equal(record.target.tab, tab.tab, 'aimed at the lead that is live now')
    assert.equal(record.target.generation, 1)
  })

  it('BO3: never overwrites a delivery that was accepted while the click was in flight', async () => {
    const planned = await s.rust.request('deliver.now', {
      delivery: null,
      tab: tab.tab,
      conversation,
      answerId,
      resend: true,
    })
    assert.equal(planned.ok, true, JSON.stringify(planned))

    // The watcher accepts it — which is what happens while a human's finger
    // is still on the button.
    const accepted = {
      ...s.deliveries()[planned.delivery],
      state: 'accepted',
      acceptedAt: '2026-09-07T00:00:00.000Z',
    }
    s.seedDelivery(accepted)

    // Cancelling now must not produce a cancelled record still carrying
    // acceptedAt. `accepted` is terminal; the decision is made against the
    // record as it IS, inside the queue, not against the snapshot the page
    // was looking at.
    const cancelled = await s.rust.request('deliver.cancel', { delivery: planned.delivery })
    assert.equal(cancelled.ok, true, JSON.stringify(cancelled))
    assert.equal(cancelled.changed, false)
    assert.equal(cancelled.state, 'accepted')
    assert.equal(s.deliveries()[planned.delivery].state, 'accepted')

    // And sending it again makes a NEW record rather than dragging an
    // accepted one back to pending.
    const again = await s.rust.request('deliver.now', {
      delivery: planned.delivery,
      tab: tab.tab,
      conversation,
      answerId,
      resend: false,
    })
    assert.equal(again.ok, true, JSON.stringify(again))
    assert.equal(again.resent, true)
    assert.notEqual(again.delivery, planned.delivery)
    assert.equal(s.deliveries()[planned.delivery].state, 'accepted', 'history is not rewritten')
    assert.equal(s.deliveries()[again.delivery].acceptedAt, undefined, 'and the new one is fresh')
  })
})

describe('BO10: creating a tab over HTTP opens its lead too', () => {
  it('is the same operation tab.open is, with the same response', async () => {
    const s = await paneServer()
    try {
      const opens = s.seen.open.length
      const created = await s.api(s.token, '/api/tabs', {
        method: 'POST',
        body: { dir: s.workspace, harness: 'claude-code' },
      })
      const payload = await json(created, 201)

      // The response contract the app already depends on.
      assert.equal(typeof payload.tab.id, 'string')
      assert.equal(payload.tab.closed, false)
      assert.equal(typeof payload.leadEnv.CONSENSFLOW_APP_TOKEN, 'string')
      assert.equal(payload.leadEnv.CONSENSFLOW_TAB, payload.tab.id)

      // ...and the thing it never did: a tab whose lead is only metadata is
      // a window nobody opened, and the app had no second call to make.
      assert.equal(s.seen.open.length, opens + 1, 'a pane.open frame really went out')
      assert.equal(
        s.seen.open.at(-1).env.PATH,
        payload.leadEnv.PATH,
        'the real lead receives the same inherited tool PATH promised by the HTTP response',
      )
      assert.ok(
        s.seen.open.at(-1).env.PATH.endsWith(s.env.PATH),
        'the bundled cf shadows older copies while normal harness and system tools stay available',
      )
      const tabs = JSON.parse(
        readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8'),
      ).tabs
      const record = tabs.find((tab) => tab.id === payload.tab.id)
      assert.notEqual(record.lead.reserved, undefined, 'the launch is held')
      assert.equal(typeof record.lead.reserved.resolvedAt, 'string', 'and resolved')
      assert.equal(typeof record.lead.nativeSession, 'string', 'and bound')
    } finally {
      await s.close()
    }
  })
})

describe('page delivery actions enforce settled transcript identity', () => {
  let s
  let tab
  let consulted
  let incomplete
  let nonAssistant

  before(async () => {
    s = await paneServer()
    tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    consulted = await s.rust.request('consult', {
      tab: tab.tab,
      agent: 'nyx',
      task: 'page guards',
    })
    await s.rust.request('notify.set', { scope: 'pane', id: consulted.pane.id, mode: 'manual' })

    const sessionId = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    const projects = join(s.env.CLAUDE_CONFIG_DIR, 'projects', 'page-guards')
    mkdirSync(projects, { recursive: true })
    writeFileSync(
      join(projects, `${sessionId}.jsonl`),
      readFileSync('tests/engine/fixtures/completion/claude-code/frontier-history.jsonl', 'utf8'),
    )
    const file = join(
      s.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(s.workspace),
      'threads.json',
    )
    const threads = JSON.parse(readFileSync(file, 'utf8'))
    threads[consulted.conversation].sessionId = sessionId
    writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)

    const found = await harnessAnswers('claude-code', sessionId, s.env)
    incomplete = found.items.find((item) => item.role === 'assistant' && item.complete !== true)
    nonAssistant = found.items.find((item) => item.role !== 'assistant')
    assert.ok(incomplete, 'fixture must expose an unsettled assistant item')
    assert.ok(nonAssistant, 'fixture must expose a non-assistant item')
  })

  after(async () => {
    await s.close()
  })

  it('refuses incomplete and non-assistant manual answers without creating a delivery', async () => {
    const before = s.deliveries()
    const incompleteResponse = await s.rust.request('deliver.now', {
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: incomplete.id,
      resend: false,
    })
    assert.equal(incompleteResponse.ok, false, JSON.stringify(incompleteResponse))
    assert.equal(incompleteResponse.error, 'answer-incomplete')

    const nonAssistantResponse = await s.rust.request('deliver.now', {
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: nonAssistant.id,
      resend: false,
    })
    assert.equal(nonAssistantResponse.ok, false, JSON.stringify(nonAssistantResponse))
    assert.equal(nonAssistantResponse.error, 'answer-not-assistant')
    assert.deepEqual(s.deliveries(), before)
  })

  it('rejects an unknown explicit delivery instead of falling back to answer planning', async () => {
    const response = await s.rust.request('deliver.now', {
      delivery: 'd-does-not-exist',
      tab: tab.tab,
      conversation: consulted.conversation,
      answerId: incomplete.id,
      resend: false,
    })
    assert.equal(response.ok, false, JSON.stringify(response))
    assert.equal(response.error, 'no-delivery')
  })

  it('rejects every supplied identity dimension that disagrees with an explicit delivery', async () => {
    const id = 'd-page-identity'
    s.seedDelivery({
      id,
      conversation: consulted.conversation,
      answerId: incomplete.id,
      state: 'pending',
      target: {
        leadId: `tab:${tab.tab}:1`,
        session: 'lead-session',
        tab: tab.tab,
        pane: tab.pane.id,
        generation: 1,
      },
    })
    for (const body of [
      { tab: 't-wrong' },
      { conversation: 'wrong-conversation' },
      { answerId: 'wrong-answer' },
    ]) {
      const response = await s.rust.request('deliver.now', {
        delivery: id,
        tab: tab.tab,
        conversation: consulted.conversation,
        answerId: incomplete.id,
        resend: false,
        ...body,
      })
      assert.equal(response.ok, false, JSON.stringify(response))
      assert.equal(response.error, 'delivery-mismatch')
    }
    assert.equal(s.deliveries()[id].state, 'pending')
  })
})

describe('page delivery lookups stay within the requested tab', () => {
  let s
  let first
  let second
  let conversation
  let answerId

  before(async () => {
    s = await paneServer()
    first = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    second = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const consulted = await s.rust.request('consult', {
      tab: first.tab,
      agent: 'nyx',
      task: 'same workspace ownership',
    })
    conversation = consulted.conversation

    const sessionId = 'bbbbbbbb-cccc-dddd-eeee-ffffffffffff'
    const projects = join(s.env.CLAUDE_CONFIG_DIR, 'projects', 'workspace')
    mkdirSync(projects, { recursive: true })
    writeFileSync(
      join(projects, `${sessionId}.jsonl`),
      readFileSync('tests/engine/fixtures/completion/claude-code/fragments.jsonl', 'utf8'),
    )
    s.bindSession(conversation, sessionId)
    const found = await harnessAnswers('claude-code', sessionId, s.env)
    answerId = found.items.find((item) => item.role === 'assistant' && item.complete === true).id
  })

  after(async () => {
    await s.close()
  })

  it('does not let tab B deliver tab A answer A or mutate A record', async () => {
    const id = 'd-cross-tab'
    s.seedDelivery({
      id,
      conversation,
      answerId,
      state: 'pending',
      target: {
        leadId: `tab:${first.tab}:1`,
        session: 'lead-a',
        tab: first.tab,
        pane: first.pane.id,
        generation: 1,
      },
    })

    const response = await s.rust.request('deliver.now', {
      delivery: null,
      tab: second.tab,
      conversation,
      answerId,
      resend: false,
    })
    assert.equal(response.ok, false, JSON.stringify(response))
    assert.equal(response.error, 'conversation-mismatch')
    assert.equal(s.deliveries()[id].manual, undefined, 'tab A record was not selected or changed')
    assert.equal(s.deliveries()[id].target.tab, first.tab)
  })

  it('does not let tab B read tab A conversation answers from the shared workspace', async () => {
    const listed = await s.rust.request('answers.list', {
      tab: second.tab,
      pane: second.pane.id,
      conversation,
    })
    assert.equal(listed.ok, false, JSON.stringify(listed))
    assert.equal(listed.error, 'conversation-mismatch')
  })
})

describe('closed tabs keep held answers visible but not sendable', () => {
  let s
  let tab

  before(async () => {
    s = await paneServer()
    tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    s.seedDelivery({
      id: 'd-held-closed',
      conversation: 'worker-closed',
      answerId: 'answer-closed',
      state: 'pending',
      target: {
        leadId: `tab:${tab.tab}:1`,
        session: 'lead-session-closed',
        tab: tab.tab,
        pane: tab.pane.id,
        generation: 1,
      },
    })
    s.suspendTab(tab.tab)
  })

  after(async () => {
    await s.close()
  })

  it('lists the held answer while closed and refuses send until resume', async () => {
    const state = await s.rust.request('state.list', {})
    assert.equal(state.ok, true, JSON.stringify(state))
    assert.deepEqual(state.held, [{ id: 'd-held-closed', tab: tab.tab, answerId: 'answer-closed' }])

    const sent = await s.rust.request('held.send', { tab: tab.tab })
    assert.equal(sent.ok, false, JSON.stringify(sent))
    assert.equal(sent.error, 'held-refused')
  })
})

describe('the lead harnesses offered are exactly the ones that work', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('offers one list, and every value on it opens a tab', async () => {
    const state = await s.rust.request('state.list', {})
    assert.ok(Array.isArray(state.leadHarnesses), 'the page picks from a list Node owns')
    assert.ok(state.leadHarnesses.length > 0)
    for (const harness of state.leadHarnesses) {
      const opened = await s.rust.request('tab.open', { dir: s.workspace, harness })
      assert.equal(opened.ok, true, `${harness}: ${JSON.stringify(opened)}`)
    }
  })

  it('does not offer kimi, which cannot be seeded with its launch marker', async () => {
    // `interactiveStart` returns null for kimi by design — `-p` is
    // non-interactive and it has no positional prompt — so a kimi lead
    // opens with no way to carry `[consensflow launch <nonce>]` and can
    // never bind. Offering it is offering a tab that silently loses its
    // conversation on every resume. Withdrawn until it can be seeded.
    assert.equal(
      (await s.rust.request('state.list', {})).leadHarnesses.includes('kimi'),
      false,
      'the picker does not offer it',
    )
    const refused = await s.rust.request('tab.open', { dir: s.workspace, harness: 'kimi' })
    assert.equal(refused.ok, false, 'and the store does not accept it')
    assert.equal(refused.error, 'tab-refused')
    assert.match(refused.reason, /harness/)
  })
})

describe('attach refuses a conversation that never bound a session', () => {
  let s
  let tab

  before(async () => {
    s = await paneServer()
    tab = await s.tab('claude-code')
  })
  after(async () => {
    await s.close()
  })

  it('refuses before admission and before any pane is opened', async () => {
    // A conversation whose pane has ended and which never bound a native
    // session has nothing to reopen. Admitting it emits nonce-only
    // evidence, `cf attach` rightly refuses because there is no session to
    // attach to, and the human is left looking at a pane whose outer
    // command exited 0, whose controller exited 1, and which never ran a
    // harness at all. Cold recovery is not specified and is not built.
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', task: 'never binds', fresh: true, opId: 'ub-1' },
      }),
      200,
    )
    assert.equal(s.threads()[opened.conversation].sessionId, null, 'nothing bound it')
    await s.endPane(opened.pane, tab.lead, { bound: false })

    const opens = s.seen.open.length
    const counter = s.nextPane()
    const refused = await s.api(tab.lead, '/api/panes/attach', {
      method: 'POST',
      body: { tab: tab.tab.id, agent: 'zeus', session: opened.conversation, opId: 'ub-2' },
    })
    const body = await json(refused, 409)
    assert.equal(body.error, 'unbound-conversation')
    assert.match(body.reason, /consult/, 'and it says what to do instead')

    // Before admission and before the frame: no pane, no reservation, and
    // not even an identity spent on a launch that could not work.
    assert.equal(s.seen.open.length, opens, 'no pane was opened')
    assert.equal(s.nextPane(), counter, 'and no pane identity was burned')
    assert.equal(s.threads()[opened.conversation].reserved, undefined)
  })

  it('still attaches a conversation that did bind one', async () => {
    const opened = await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', task: 'binds', fresh: true, opId: 'ub-3' },
      }),
      200,
    )
    const file = join(
      s.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(s.workspace),
      'threads.json',
    )
    const threads = JSON.parse(readFileSync(file, 'utf8'))
    threads[opened.conversation].sessionId = 'rollout-bound'
    writeFileSync(file, `${JSON.stringify(threads, null, 2)}\n`)
    await s.endPane(opened.pane, tab.lead)

    const attached = await json(
      await s.api(tab.lead, '/api/panes/attach', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'zeus', session: opened.conversation, opId: 'ub-4' },
      }),
      200,
    )
    assert.equal(attached.outcome, 'opened')
    assert.equal(s.seen.open.at(-1).argv.includes('--native-session'), true)
  })
})

describe('BO4: a transport that dies after the frame went out keeps the launch', () => {
  it('does not release a lead whose window may already be up', async () => {
    const s = await paneServer()
    try {
      // The pane host takes the frame and then dies. The bytes left, so the
      // lead may be on the screen right now — releasing its reservation
      // frees a launch nothing will ever end, and closing the tab hides a
      // window the human can see. Only a matching exit settles it, and no
      // exit can come from a host that is gone.
      s.state.open = () => {
        s.killTransport()
        return new Promise(() => {})
      }
      const refused = await s.rust
        .request('tab.open', { dir: s.workspace, harness: 'claude-code' })
        .catch((cause) => ({ ok: false, error: String(cause?.message ?? cause) }))
      assert.equal(refused.ok, false, JSON.stringify(refused))

      const tabs = JSON.parse(
        readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8'),
      ).tabs
      const tab = tabs.at(-1)
      assert.notEqual(tab, undefined)
      assert.equal(tab.closed, false, 'the tab stays: its window may be up')
      assert.notEqual(tab.lead.reserved, undefined, 'and its launch is still held')
      assert.equal(tab.lead.reserved.resolvedAt, undefined)
    } finally {
      await s.close()
    }
  })
})

describe('a worker pane carries no billing guard of its own', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('leaves the variable in place, because cf is what spawns its harness', async () => {
    const tab = await s.tab('claude-code')
    const before = s.seen.open.length
    await json(
      await s.api(tab.lead, '/api/panes/consult', {
        method: 'POST',
        body: { tab: tab.tab.id, agent: 'nyx', task: 'guards', fresh: true, opId: 'guard-1' },
      }),
      200,
    )
    // A worker pane runs `cf`, which applies `childEnv` itself when it
    // spawns the harness underneath. Asking Rust to drop the name here
    // would take it from `cf` too, before `cf` could decide anything with
    // it — the guard belongs at the spawn that is actually guarded.
    assert.equal(s.seen.open[before].dropEnv, undefined)
  })
})

/**
 * The same app, restartable: one throwaway home, and a server that can be
 * closed and started again over it. `paneServer` cleans its root away on
 * close, so proving what a FRESH `Panes` instance does needs its own
 * harness — the store on disk survives, everything in memory does not.
 */
async function restartableApp() {
  const t = tempEnv()
  chooseCmuxMode(t)
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  const workspace = join(t.root, 'workspace')
  const shims = join(t.root, 'shims')
  mkdirSync(workspace, { recursive: true })
  mkdirSync(shims, { recursive: true })
  for (const command of ['claude', 'codex']) {
    const file = join(shims, command)
    writeFileSync(file, '#!/bin/sh\nexit 0\n')
    chmodSync(file, 0o755)
  }
  t.env.PATH = `${shims}:${t.env.PATH ?? ''}`

  const seen = { open: [] }
  const host = new Map()
  const state = {
    open: (request) => {
      host.set(`${request.id}:${request.generation}`, {
        id: request.id,
        generation: request.generation,
        alive: true,
        idleMs: 0,
      })
      return { ok: true, id: request.id, generation: request.generation }
    },
    list: () => ({ ok: true, panes: [...host.values()] }),
  }
  let server = null
  let rust = null
  let node = null
  let nodeToRust = null
  let rustToNode = null
  const nodeSide = () =>
    new Bridge({ input: rustToNode, output: nodeToRust, idPrefix: 'n-', peerIdPrefix: 'r-' })
  const boot = async () => {
    server = await startUiServer(t.env, { paneOpenDeadlineMs: CONSULT_DEADLINE_MS })
    nodeToRust = new PassThrough()
    rustToNode = new PassThrough()
    node = nodeSide()
    server.attachBridge(node)
    rust = new Bridge({
      input: nodeToRust,
      output: rustToNode,
      idPrefix: 'r-',
      peerIdPrefix: 'n-',
    })
    rust.on('pane.open', (request) => {
      seen.open.push(request)
      return state.open(request)
    })
    rust.on('pane.list', () => state.list())
  }
  await boot()
  const read = (...parts) =>
    JSON.parse(readFileSync(join(t.env.CONSENSFLOW_HOME, ...parts), 'utf8'))
  return {
    workspace,
    seen,
    state,
    host,
    get rust() {
      return rust
    },
    api: (token, path, { method = 'GET', body } = {}) =>
      fetch(`${server.url.replace(/\/$/, '')}${path}`, {
        method,
        headers: {
          ...(token === null ? {} : { authorization: `Bearer ${token}` }),
          ...(body === undefined ? {} : { 'content-type': 'application/json' }),
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }),
    tab: (id) => read('app', 'tabs.json').tabs.find((candidate) => candidate.id === id),
    threads: () => read('workspaces', workspaceKey(workspace), 'threads.json'),
    /**
     * The pane host's side of the pipe dies with a frame in its hands: every
     * request still waiting REJECTS, while the handler it is waiting on runs
     * on — the streams are untouched, so it can still answer later.
     */
    dropHost() {
      node.close()
    },
    /** A pane host again, over the same pipe. The app never restarted. */
    reattach() {
      node = nodeSide()
      server.attachBridge(node)
    },
    /** A new process over the same store: nothing in memory carries over. */
    async restart() {
      rust.close()
      await server.close()
      await boot()
    },
    async close() {
      rust.close()
      await server.close()
      t.cleanup()
    },
  }
}

/**
 * A resume reaps panes the pane host no longer has. A launch whose
 * `pane.open` is still in flight has no pane there YET — the host has not
 * answered for it — so a `pane.list` that overtakes it reads "not there"
 * about a window that is about to exist, and reaps a live launch: its row,
 * its reservation, its controller capability and its ticket, all while the
 * frame is still being answered.
 *
 * The deadline does NOT end that. `Bridge.request`'s deadline stops NODE
 * waiting; the pane host's handler runs on, and the window it is opening
 * will exist. So a transmitted open protects its pane identity for as long
 * as this process lives — until the launch ends for real (resolved, or
 * refused and cleaned up) or its pane exits — while a reconcile still
 * waits out the operations that are genuinely still running. Only a FRESH
 * app, which never sent those frames, reaps what its predecessor left.
 */
describe('a resume drains the launches in flight before it reaps panes', () => {
  let s
  // What the pane host actually holds, filled as it answers each open.
  const host = new Map()
  const answer = (request) => {
    host.set(`${request.id}:${request.generation}`, {
      id: request.id,
      generation: request.generation,
      alive: true,
      idleMs: 0,
    })
    return { ok: true, id: request.id, generation: request.generation }
  }

  before(async () => {
    s = await paneServer()
    s.state.open = answer
    s.state.list = () => ({ ok: true, panes: [...host.values()] })
  })
  after(async () => {
    await s.close()
  })

  const tabOnDisk = (id) =>
    JSON.parse(readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8')).tabs.find(
      (tab) => tab.id === id,
    )

  const suspend = async (tab, pane) => {
    host.delete(`${pane.id}:${pane.generation}`)
    s.rust.event('pane.exit', { id: pane.id, generation: pane.generation })
    await waitFor(() => tabOnDisk(tab).closed === true)
  }

  it('never reaps the pane of an open it has not been answered for', async () => {
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const lead = s.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN

    // The gate: the FIRST open from here is held unanswered, every later
    // one (the resumed lead's own window) answers normally.
    let gated = null
    let release = null
    s.state.open = (request) => {
      if (gated !== null) return answer(request)
      gated = request
      return new Promise((resolve) => {
        release = () => resolve(answer(request))
      })
    }
    const consult = s.api(lead, '/api/panes/consult', {
      method: 'POST',
      body: {
        tab: tab.tab,
        agent: 'zeus',
        task: 'admitted, transmitted, unanswered',
        fresh: true,
        opId: 'drain-in-flight',
      },
    })
    await waitFor(() => gated !== null)
    const ticket = gated.env.CONSENSFLOW_LAUNCH

    await suspend(tab.tab, tab.pane)
    const asked = s.seen.list.length
    const resume = s.rust.request('tab.resume', { tab: tab.tab })
    await new Promise((resolve) => setTimeout(resolve, 50))

    // The resume is waiting on the open, not racing it.
    assert.equal(s.seen.list.length, asked, 'pane.list did not overtake the admitted open')
    assert.equal(
      tabOnDisk(tab.tab).panes.some((pane) => pane.id === gated.id),
      true,
      'and the pane of the launch in flight still stands',
    )

    release()
    const opened = await json(await consult, 200)
    assert.equal(opened.outcome, 'opened')
    assert.equal((await resume).ok, true, 'the resume completes once the open is released')
    assert.equal(s.seen.list.length, asked + 1, 'and it asked once, after the open settled')

    // Nothing the launch owns was reaped: the row, the reservation, the
    // ticket nobody had redeemed yet, and the capability it hands out.
    assert.equal(
      tabOnDisk(tab.tab).panes.some((pane) => pane.id === opened.pane.id),
      true,
      'the pane row',
    )
    assert.equal(
      typeof s.threads()[opened.conversation].reserved.resolvedAt,
      'string',
      'the reservation, now resolved',
    )
    const controller = await json(
      await s.api(null, '/api/launch/redeem', { method: 'POST', body: { ticket } }),
      200,
    )
    assert.equal(
      (
        await json(
          await s.api(controller.capability, '/api/panes/progress.set', {
            method: 'POST',
            body: {
              launch: controller.launch,
              generation: opened.pane.generation,
              progress: { state: 'running', detail: 'survived the resume' },
            },
          }),
          200,
        )
      ).outcome,
      'recorded',
      'the capability the ticket bought still acts for its launch',
    )
  })

  it('holds the pane of an open the host is still running after Node stopped waiting', async () => {
    // The deadline is Node's patience, not the host's cancellation: the
    // consult answers `unknown` while the handler runs on and the window it
    // is opening will exist. Reaping that row would release a launch whose
    // pane is about to appear.
    const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
    const lead = s.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN
    let held = null
    let release = null
    s.state.open = (request) => {
      if (held !== null) return answer(request)
      held = request
      return new Promise((resolve) => {
        release = () => resolve(answer(request))
      })
    }
    const unknown = await json(
      await s.api(lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab,
          agent: 'zeus',
          task: 'held past the deadline',
          fresh: true,
          opId: 'held-past-deadline',
        },
      }),
      200,
    )
    assert.equal(unknown.outcome, 'unknown', 'Node stopped waiting')
    assert.equal(
      s.threads()[unknown.conversation].reserved.resolvedAt,
      undefined,
      'and the launch stands, because the pane may well be coming',
    )
    const ticket = held.env.CONSENSFLOW_LAUNCH

    // The host has not reported this pane — it is still opening it.
    await suspend(tab.tab, tab.pane)
    assert.equal((await s.rust.request('tab.resume', { tab: tab.tab })).ok, true)

    assert.equal(
      tabOnDisk(tab.tab).panes.some((pane) => pane.id === unknown.pane.id),
      true,
      'the row of a launch the host is still running stands',
    )
    assert.notEqual(s.threads()[unknown.conversation].reserved, undefined, 'and its reservation')
    const controller = await json(
      await s.api(null, '/api/launch/redeem', { method: 'POST', body: { ticket } }),
      200,
    )
    assert.equal(
      (
        await json(
          await s.api(controller.capability, '/api/panes/progress.set', {
            method: 'POST',
            body: {
              launch: controller.launch,
              generation: unknown.pane.generation,
              progress: { state: 'running', detail: 'opened late' },
            },
          }),
          200,
        )
      ).outcome,
      'recorded',
      'and the ticket and the capability it buys',
    )

    // The handler finishes: the host now has the pane, and it stays.
    release()
    await waitFor(() => host.has(`${unknown.pane.id}:${unknown.pane.generation}`))
    await suspend(tab.tab, { id: tab.pane.id, generation: tab.pane.generation + 1 })
    assert.equal((await s.rust.request('tab.resume', { tab: tab.tab })).ok, true)
    assert.equal(
      tabOnDisk(tab.tab).panes.some((pane) => pane.id === unknown.pane.id),
      true,
      "the pane the host now tracks is still the tab's",
    )
  })
})

/**
 * The other side of that protection: it is IN MEMORY, and a restarted app
 * has none of it. Rows its predecessor left unresolved name windows nobody
 * is opening any more, so a fresh instance must reap them — which is what
 * makes the protection above safe to keep for a whole process lifetime.
 */
describe('a resume keeps the panes of opens that were transmitted and then failed', () => {
  it('holds the row when the request rejects after the frame went out', async () => {
    // A deadline is not the only way an open ends without an answer: the
    // transport can die with the frame already in the host's hands. The
    // request REJECTS, and because nothing said the pane was not opened,
    // the reservation stands — so the identity must stay protected too,
    // exactly as it does for a deadline. The host handler is still running.
    const app = await restartableApp()
    try {
      const tab = await app.rust.request('tab.open', { dir: app.workspace, harness: 'claude-code' })
      const lead = app.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN
      let held = null
      let release = null
      const opened = app.state.open
      app.state.open = (request) => {
        if (held !== null) return opened(request)
        held = request
        return new Promise((resolve) => {
          release = () => resolve(opened(request))
        })
      }
      const consult = app.api(lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: tab.tab,
          agent: 'zeus',
          task: 'transmitted, then the pipe died',
          fresh: true,
          opId: 'transmitted-rejection',
        },
      })
      await waitFor(() => held !== null)

      // The frame is written and unanswered when the host's side goes.
      app.dropHost()
      assert.equal((await consult).status, 500, 'the consult fails, as it always has')
      const worker = app.tab(tab.tab).panes.find((pane) => pane.kind === 'worker')
      assert.notEqual(worker, undefined, 'the row stands')
      const conversation = Object.keys(app.threads()).find(
        (name) => app.threads()[name].reserved?.pane === worker.id,
      )
      assert.notEqual(conversation, undefined, 'and its reservation')
      const ticket = held.env.CONSENSFLOW_LAUNCH

      // A pane host again — one that has never heard of that pane.
      app.reattach()
      app.state.list = () => ({ ok: true, panes: [] })
      app.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
      await waitFor(() => app.tab(tab.tab).closed === true)
      assert.equal((await app.rust.request('tab.resume', { tab: tab.tab })).ok, true)

      assert.equal(
        app.tab(tab.tab).panes.some((pane) => pane.id === worker.id),
        true,
        'the row of a launch nothing ever refused stands',
      )
      assert.notEqual(app.threads()[conversation].reserved, undefined, 'and its reservation')
      const controller = await json(
        await app.api(null, '/api/launch/redeem', { method: 'POST', body: { ticket } }),
        200,
      )
      assert.equal(
        (
          await json(
            await app.api(controller.capability, '/api/panes/progress.set', {
              method: 'POST',
              body: {
                launch: controller.launch,
                generation: worker.generation,
                progress: { state: 'running', detail: 'never refused' },
              },
            }),
            200,
          )
        ).outcome,
        'recorded',
        'and the ticket and the capability it buys',
      )

      // The handler finishes into a pipe nobody is reading: Node learns
      // nothing, which is the whole reason the protection has to stand.
      release()
      assert.equal(
        app.tab(tab.tab).panes.some((pane) => pane.id === worker.id),
        true,
        'and still stands afterwards',
      )
    } finally {
      await app.close()
    }
  })
})

describe('a shell frame that never left strands nothing', () => {
  it('leaves no open for a later resume to wait on forever', async () => {
    // `#transmit` remembers the open BEFORE `bridge.request`, because the
    // frame is written inside that call and an open nobody remembers is
    // one a reconcile can reap. But `request` re-serialises the whole
    // frame and can throw synchronously out of it (`src/bridge.js:164`),
    // so the memory outlives a frame that never left — and a drain waits
    // on it for as long as the app runs. Workers and leads clear it in
    // their pre-transmission cleanup; the shell must too.
    const s = await paneServer()
    try {
      const tab = await s.rust.request('tab.open', { dir: s.workspace, harness: 'claude-code' })
      const tabOnDisk = () =>
        JSON.parse(
          readFileSync(join(s.env.CONSENSFLOW_HOME, 'app', 'tabs.json'), 'utf8'),
        ).tabs.find((candidate) => candidate.id === tab.tab)

      // Exactly one `pane.open` request frame fails to serialise — the
      // body itself still encodes, so the pre-transmission check passes as
      // it does in production and the throw lands inside `request`.
      const realStringify = JSON.stringify
      let injected = 0
      JSON.stringify = (value, ...rest) => {
        if (value?.kind === 'req' && value?.op === 'pane.open') {
          injected += 1
          throw new TypeError('injected: this shell frame cannot be serialised')
        }
        return realStringify(value, ...rest)
      }
      let refused
      try {
        refused = await s.rust.request('shell.open', { tab: tab.tab })
      } finally {
        JSON.stringify = realStringify
      }
      assert.equal(injected, 1, 'the Bridge did try to serialise the frame')
      assert.equal(refused.ok, false, JSON.stringify(refused))
      assert.equal(
        tabOnDisk().panes.some((pane) => pane.kind === 'shell'),
        false,
        'the shell row was given back',
      )

      s.rust.event('pane.exit', { id: tab.pane.id, generation: tab.pane.generation })
      await waitFor(() => tabOnDisk().closed === true)
      const outcome = await Promise.race([
        s.rust.request('tab.resume', { tab: tab.tab }).then((answer) => answer.ok),
        new Promise((resolve) => setTimeout(() => resolve('waited forever'), 1_000)),
      ])
      assert.equal(outcome, true, 'the resume drains an open that never left, and finishes')
    } finally {
      await s.close()
    }
  })
})

describe('a fresh app reaps the unresolved rows its predecessor left', () => {
  it('reaps a row no live process is protecting, on the first resume', async () => {
    const app = await restartableApp()
    try {
      const tab = await app.rust.request('tab.open', { dir: app.workspace, harness: 'claude-code' })
      const lead = app.seen.open.at(-1).env.CONSENSFLOW_APP_TOKEN
      app.state.open = () => new Promise(() => {})
      const unknown = await json(
        await app.api(lead, '/api/panes/consult', {
          method: 'POST',
          body: {
            tab: tab.tab,
            agent: 'zeus',
            task: 'left behind by the old process',
            fresh: true,
            opId: 'stale-across-restart',
          },
        }),
        200,
      )
      assert.equal(unknown.outcome, 'unknown')
      assert.equal(app.threads()[unknown.conversation].reserved.resolvedAt, undefined)

      // A new Panes instance over the same store: it sent no frames, so it
      // protects nothing, and `Store.open()` reads every tab closed.
      await app.restart()
      assert.equal(app.tab(tab.tab).closed, true, 'a restart reads its tabs closed')
      assert.equal(
        app.tab(tab.tab).panes.some((pane) => pane.id === unknown.pane.id),
        true,
        'with the stale row still on it',
      )

      assert.equal((await app.rust.request('tab.resume', { tab: tab.tab })).ok, true)
      assert.equal(
        app.tab(tab.tab).panes.some((pane) => pane.id === unknown.pane.id),
        false,
        'the stale row is reaped',
      )
      assert.equal(
        app.threads()[unknown.conversation].reserved,
        undefined,
        'and its launch is released',
      )
    } finally {
      await app.close()
    }
  })
})
