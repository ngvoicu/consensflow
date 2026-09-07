import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute, join } from 'node:path'
import { PassThrough } from 'node:stream'
import { after, before, describe, it } from 'node:test'
import { plan } from '../hosts/lib/deliveries.js'
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
  const seen = { open: [], snapshot: [], paste: [] }
  const state = {
    open: (request) => ({ ok: true, id: request.id, generation: request.generation }),
    epoch: 7,
    latched: false,
    diesAfterPaste: null,
  }
  const threadsOf = () => {
    const file = join(t.env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(workspace), 'threads.json')
    try {
      return JSON.parse(readFileSync(file, 'utf8'))
    } catch {
      return {}
    }
  }
  rust.on('pane.open', (request) => {
    seen.open.push(request)
    return state.open(request)
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
    url,
    token: server.token,
    rust,
    seen,
    state,
    api,
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
      return {
        ...payload,
        lead: payload.leadEnv.CONSENSFLOW_APP_TOKEN,
        leadPane: payload.tab.panes.find((pane) => pane.kind === 'lead'),
      }
    },
    /** The pane process ends — the only honest signal that it is gone. */
    async endPane(pane, leadToken) {
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
    ])
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
    assert.equal(response.status, 400)
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

      const created = await api(handle.token, '/api/tabs', {
        method: 'POST',
        body: { dir: workspace, harness: 'claude-code' },
      })
      const tab = await json(created, 201)

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

      // The frame arrives on stdout, after the handle line and nothing else.
      const frame = JSON.parse(await nextLine())
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
    const small = await paneServer({ maxFrameBytes: 400 })
    try {
      const its = await small.tab('claude-code')
      const counter = small.nextPane()
      const response = await small.api(its.lead, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: its.tab.id,
          agent: 'zeus',
          task: 'too big to send',
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
    // 400 is this route's fallback for an error it does not model; what the
    // status is matters far less here than what the store is left holding.
    await json(response, 400)
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
