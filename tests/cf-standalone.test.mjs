import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from 'node:fs'
import { createServer } from 'node:http'
import { delimiter, join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { PassThrough } from 'node:stream'
import { after, before, describe, it } from 'node:test'
import { promisify } from 'node:util'
import { workspaceKey } from '../hosts/lib/state.js'
import { Bridge } from '../src/bridge.js'
import { launchConfiguration } from '../src/channels.js'
import { leadEnv } from '../src/launch.js'
import { addAgent } from '../src/roster.js'
import { startUiServer } from '../src/ui.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

/**
 * The `cf` side of the pane protocol (Phase 2, TEST-PANE-19).
 *
 * `tests/ui-panes.test.mjs` proves the routes; this proves the caller. The
 * server is the same real one, started the same way, with this test on the
 * pipe as Rust — and `cf` is spawned as the child process it really is,
 * with exactly the environment `leadEnv` and `controllerEnv` hand out. What
 * a lead types goes in one end; what lands in the store and on the bridge
 * comes out the other.
 *
 * No harness CLI is ever run: `claude` and `codex` are five-line stand-ins
 * on a fake PATH that record their argv, and (for codex) write the rollout
 * a real one would write. No network: everything is loopback or a pipe.
 */

const run = promisify(execFile)
const CF = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
const BUNDLE_BIN = join(import.meta.dirname, '..', 'bin')
const CONSULT_DEADLINE_MS = 250
/** Nothing listens on port 1, and reaching it needs no DNS and no network. */
const NO_APP = 'http://127.0.0.1:1'
/** The session id of the real kimi wire log the fixture shapes come from. */
const KIMI_SESSION = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'

async function cf(args, env, cwd) {
  try {
    const { stdout, stderr } = await run(process.execPath, [CF, ...args], {
      env,
      cwd,
      timeout: 30_000,
    })
    return { code: 0, stdout, stderr }
  } catch (cause) {
    return { code: cause.code ?? 1, stdout: cause.stdout ?? '', stderr: cause.stderr ?? '' }
  }
}

/** Runs a command with EXACTLY the given environment — nothing inherited. */
async function cf2(command, args, env) {
  try {
    const { stdout, stderr } = await run(command, args, { env, timeout: 30_000 })
    return { code: 0, stdout, stderr }
  } catch (cause) {
    return { code: cause.code ?? 1, stdout: cause.stdout ?? '', stderr: cause.stderr ?? '' }
  }
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

/** An executable that is not a harness: it records its argv and exits. */
function fakeHarness(dir, name, body = '') {
  mkdirSync(dir, { recursive: true })
  const path = join(dir, name)
  writeFileSync(
    path,
    `#!${process.execPath}\n` +
      "import { appendFileSync } from 'node:fs'\n" +
      `appendFileSync(${JSON.stringify(`${path}.argv`)}, JSON.stringify(process.argv.slice(2)) + '\\n')\n` +
      body,
  )
  chmodSync(path, 0o755)
  return {
    path,
    calls: () =>
      readFileSync(`${path}.argv`, 'utf8')
        .trimEnd()
        .split('\n')
        .map((line) => JSON.parse(line)),
  }
}

/**
 * A stand-in codex: it records the prompt it was launched with as an
 * ordinary user turn in its own rollout store, the way codex does, and
 * exits straight away when asked to `resume` (there is nothing to write).
 */
function codexRolloutBody(env, sessionId) {
  return `
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
const argv = process.argv.slice(2)
if (argv[0] !== 'resume') {
  const dir = join(${JSON.stringify(env.CODEX_HOME)}, 'sessions')
  mkdirSync(dir, { recursive: true })
  const seed = argv.at(-1)
  const meta = { type: 'session_meta', payload: { cwd: process.cwd(), timestamp: new Date().toISOString(), cli_version: '0.153.4' } }
  const turn = { type: 'response_item', payload: { id: 'msg_seed', type: 'message', role: 'user', content: [{ type: 'input_text', text: seed }] } }
  writeFileSync(join(dir, 'rollout-2026-09-07T00-00-00-${sessionId}.jsonl'), [meta, turn].map((r) => JSON.stringify(r)).join('\\n') + '\\n')
}
`
}

/**
 * A stand-in kimi. The record shapes and field names are copied from the
 * real protocol-1.5 wire logs in
 * `tests/engine/fixtures/completion/kimi/tool-result.jsonl` and
 * `superseded-tool.jsonl` (their provenance is in the Kimi section of
 * `tests/engine/fixtures/completion/README.md`): `turn.prompt` and the
 * `context.append_message` that carries the native message id, a
 * step.begin / content.part / step.end answer, and the native
 * `turn.ended {reason:completed}` boundary.
 */
function kimiWireBody(env, sessionId) {
  return `
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
const argv = process.argv.slice(2)
const prompt = argv.includes('-p') ? argv[argv.indexOf('-p') + 1] : undefined
if (prompt !== undefined) {
  const dir = join(${JSON.stringify(join(env.HOME, '.kimi-code'))}, 'sessions', 'wd_fixture', ${JSON.stringify(sessionId)}, 'agents', 'main')
  mkdirSync(dir, { recursive: true })
  const at = Date.now()
  const step = 'd26c913f-c98b-4262-8340-06a147aa7937'
  const records = [
    { type: 'metadata', protocol_version: '1.5', created_at: at },
    { type: 'turn.prompt', agentId: 'main', input: [{ type: 'text', text: prompt }], origin: { kind: 'user' }, time: at },
    { type: 'context.append_message', agentId: 'main', message: { role: 'user', content: [{ type: 'text', text: prompt }], toolCalls: [], origin: { kind: 'user' }, id: 'msg_01M0TA752ZAJKC8PRMYSJE95WY' }, time: at },
    { type: 'context.append_loop_event', agentId: 'main', event: { type: 'step.begin', uuid: step, turnId: '1', step: 1 }, time: at },
    { type: 'context.append_loop_event', agentId: 'main', event: { type: 'content.part', uuid: '94831cd3-f477-42b9-bb79-936e6c3c4ea9', turnId: '1', step: 1, stepUuid: step, part: { type: 'text', text: 'kimi answered' } }, time: at },
    { type: 'context.append_loop_event', agentId: 'main', event: { type: 'step.end', uuid: step, turnId: '1', step: 1, finishReason: 'end_turn', usage: { inputOther: 1, output: 1, inputCacheRead: 0, inputCacheCreation: 0 } }, time: at },
    { type: 'turn.ended', agentId: 'main', turnId: 1, reason: 'completed', durationMs: 10, time: at },
  ]
  writeFileSync(join(dir, 'wire.jsonl'), records.map((r) => JSON.stringify(r)).join('\\n') + '\\n')
  process.stdout.write(JSON.stringify({ session_id: ${JSON.stringify(sessionId)} }) + '\\n')
  process.stdout.write(JSON.stringify({ role: 'assistant', content: 'kimi answered' }) + '\\n')
}
`
}

/**
 * An OpenCode session in its real store, ready for a launch to bind to.
 *
 * The session row, the assistant message, its parts and its events are the
 * real captured rows from `tests/engine/fixtures/completion/opencode`, merged
 * with the real `native-events.json` exactly as `tests/engine/completion.test.mjs`
 * merges them; only the session's directory and creation time are rewritten,
 * so discovery (which searches by directory and time) can find it.
 *
 * What is NOT here is the user turn. Those captured fixtures hold only the
 * assistant side, and a launch binds on the marker in a user turn — so the
 * stand-in harness writes that row itself, from the prompt it was actually
 * given. Those four rows are SYNTHETIC: their ids, timestamps and event
 * positions are invented (the message id is the one the captured assistant
 * row already names as its `parentID`, and the positions are free slots below
 * the captured ones, which start at 7). They are shaped like the real ones,
 * but they are not captured, and nothing here should be read as evidence of
 * OpenCode's user-turn wire format.
 */
function stageOpencodeSession(env) {
  const fixtures = join(import.meta.dirname, 'engine', 'fixtures', 'completion', 'opencode')
  const fixture = JSON.parse(readFileSync(join(fixtures, 'completion-window.json'), 'utf8'))
  const native = JSON.parse(readFileSync(join(fixtures, 'native-events.json'), 'utf8')).event
  const dir = join(env.XDG_DATA_HOME ?? join(env.HOME, '.local', 'share'), 'opencode')
  mkdirSync(dir, { recursive: true })
  const db = new DatabaseSync(join(dir, 'opencode.db'))
  db.exec(`
    create table session (id text primary key, project_id text, parent_id text, slug text,
      directory text, title text, version text, share_url text, summary_additions integer,
      summary_deletions integer, summary_files integer, summary_diffs text, revert text,
      permission text, time_created integer, time_updated integer, time_compacting integer,
      time_archived integer, workspace_id text, path text, agent text, model text,
      cost real default 0, tokens_input integer default 0, tokens_output integer default 0,
      tokens_reasoning integer default 0, tokens_cache_read integer default 0,
      tokens_cache_write integer default 0, metadata text);
    create table message (id text primary key, session_id text, time_created integer,
      time_updated integer, data text);
    create table part (id text primary key, message_id text, session_id text,
      time_created integer, time_updated integer, data text);
    create table event (id text primary key, aggregate_id text, seq integer, type text, data text);
  `)

  // The session row is NOT written here. OpenCode creates it when it starts,
  // and discovery searches for exactly that row — staging it up front would
  // let discovery win before the harness had written anything, and a launch
  // would bind against a transcript with no turns in it.
  const session = fixture.session[0]
  const sid = session.id
  for (const row of fixture.message) {
    db.prepare(
      'insert into message (id,session_id,time_created,time_updated,data) values (?,?,?,?,?)',
    ).run(row.id, row.session_id, row.time_created, row.time_updated, row.data)
  }
  for (const row of fixture.part) {
    db.prepare(
      'insert into part (id,message_id,session_id,time_created,time_updated,data) values (?,?,?,?,?,?)',
    ).run(row.id, row.message_id, row.session_id, row.time_created, row.time_updated, row.data)
  }
  const objectIds = new Set([
    ...fixture.message.map((row) => row.id),
    ...fixture.part.map((row) => row.id),
  ])
  const events = [
    ...new Map(
      [...(fixture.event ?? []), ...native]
        .filter((row) => row.aggregate_id === sid)
        .filter((row) => {
          const data = JSON.parse(row.data)
          return row.type === 'session.updated.1' || objectIds.has(data.info?.id ?? data.part?.id)
        })
        .map((row) => [row.id, row]),
    ).values(),
  ]
  for (const row of events) {
    db.prepare('insert into event (id,aggregate_id,seq,type,data) values (?,?,?,?,?)').run(
      row.id,
      row.aggregate_id,
      row.seq,
      row.type,
      row.data,
    )
  }
  db.close()
  return { session, sessionId: sid, userMessage: JSON.parse(fixture.message[0].data).parentID }
}

/**
 * A stand-in opencode that records the prompt it was given.
 *
 * It writes the user turn into the session's own store, which is the only
 * reason a launch can bind: `persist` decides what text lands there, so a
 * test can send a prompt whose marker never arrives and watch the binding
 * refuse. Without that, the marker would be manufactured beside the harness
 * and the test would pass whatever the pane was actually sent.
 */
function opencodeBody(env, staged, { persist = 'prompt' } = {}) {
  const dir = join(env.XDG_DATA_HOME ?? join(env.HOME, '.local', 'share'), 'opencode')
  const columns = Object.keys(staged.session)
  return `
import { DatabaseSync } from 'node:sqlite'
const argv = process.argv.slice(2)
if (argv.includes('--prompt')) {
  const prompt = argv[argv.indexOf('--prompt') + 1]
  const text = ${persist === 'prompt' ? 'prompt' : "prompt.split('\\n').slice(1).join('\\n')"}
  const db = new DatabaseSync(${JSON.stringify(join(dir, 'opencode.db'))})
  const sid = ${JSON.stringify(staged.sessionId)}
  const message = ${JSON.stringify(staged.userMessage)}
  // A resume appends to a session that already exists; only a first turn
  // creates one, and only a first turn is what a launch binds against.
  if (db.prepare('select id from session where id = ?').get(sid) === undefined) {
  // opencode makes its session when it starts, in the directory it was
  // started in — which is what discovery looks for.
  const session = { ...${JSON.stringify(staged.session)}, directory: process.cwd(), time_created: Date.now() }
  db.prepare('insert into session (${columns.join(',')}) values (${columns.map(() => '?').join(',')})')
    .run(...${JSON.stringify(columns)}.map((column) => session[column]))
  const part = 'prt_0773f3849001UserTurnPartA'
  const at = 1788707026000
  db.prepare('insert into message (id,session_id,time_created,time_updated,data) values (?,?,?,?,?)')
    .run(message, sid, at, at, JSON.stringify({ role: 'user', sessionID: sid, time: { created: at } }))
  db.prepare('insert into part (id,message_id,session_id,time_created,time_updated,data) values (?,?,?,?,?,?)')
    .run(part, message, sid, at, at, JSON.stringify({ type: 'text', text, time: { start: at, end: at } }))
  db.prepare('insert into event (id,aggregate_id,seq,type,data) values (?,?,?,?,?)')
    .run('evt_user_message', sid, 3, 'message.updated.1', JSON.stringify({ sessionID: sid, info: { id: message, sessionID: sid, role: 'user', time: { created: at } } }))
  db.prepare('insert into event (id,aggregate_id,seq,type,data) values (?,?,?,?,?)')
    .run('evt_user_part', sid, 5, 'message.part.updated.1', JSON.stringify({ sessionID: sid, part: { id: part, sessionID: sid, messageID: message, type: 'text', text } }))
  }
  db.close()
}
`
}

/** The real server, with this test playing Rust — as `ui-panes` starts it. */
async function paneServer({ paneOpenDeadlineMs = CONSULT_DEADLINE_MS } = {}) {
  const t = tempEnv()
  chooseCmuxMode(t)
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  addAgent({ name: 'nyx', harness: 'claude', model: 'opus' }, t.env)
  addAgent({ name: 'ilmarinen', harness: 'kimi', model: 'moonshot-ai/kimi-k3' }, t.env)
  addAgent({ name: 'pygmalion', harness: 'image', model: 'gpt-image-2' }, t.env)
  addAgent({ name: 'clio', harness: 'pi', model: 'pi-core' }, t.env)
  addAgent({ name: 'gefjon', harness: 'opencode', model: 'grok-code' }, t.env)
  const workspace = join(t.root, 'workspace')
  mkdirSync(workspace, { recursive: true })
  // The app refuses to open a tab for a harness this machine does not have,
  // and detection is "the CLI resolves on PATH". A throwaway PATH has none,
  // so the throwaway machine is given them; a test that wants one missing
  // hands the pane a PATH of its own.
  for (const harness of ['claude', 'codex', 'pi', 'opencode', 'kimi']) {
    fakeHarness(t.env.PATH, harness)
  }

  const server = await startUiServer(t.env, { paneOpenDeadlineMs })
  const nodeToRust = new PassThrough()
  const rustToNode = new PassThrough()
  let rust
  try {
    server.attachBridge(
      new Bridge({ input: rustToNode, output: nodeToRust, idPrefix: 'n-', peerIdPrefix: 'r-' }),
    )
    rust = new Bridge({ input: nodeToRust, output: rustToNode, idPrefix: 'r-', peerIdPrefix: 'n-' })
  } catch (cause) {
    await server.close()
    t.cleanup()
    throw cause
  }

  const seen = { open: [], snapshot: [], paste: [] }
  const state = {
    open: (request) => ({ ok: true, id: request.id, generation: request.generation }),
    epoch: 7,
  }
  rust.on('pane.open', (request) => {
    seen.open.push(request)
    return state.open(request)
  })
  rust.on('pane.snapshot', (request) => ({
    ok: true,
    generation: request.generation,
    inputEpoch: state.epoch,
    draftLatched: false,
    pasteInFlight: false,
    inputFailed: false,
    queuedHumanBytes: 0,
  }))
  rust.on('pane.write_paste', (request) => {
    seen.paste.push(request)
    return { ok: true }
  })

  const url = server.url.replace(/\/$/, '')
  const created = await fetch(`${url}/api/tabs`, {
    method: 'POST',
    headers: { authorization: `Bearer ${server.token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ dir: workspace, harness: 'claude-code' }),
  })
  const tab = await created.json()
  if (created.status !== 201) {
    throw new Error(`the app refused to open a tab (${created.status}): ${JSON.stringify(tab)}`)
  }
  // Opening a tab now launches its LEAD pane too. That is setup, not the
  // subject: what these tests count is the panes a consult opens.
  seen.open.length = 0

  return {
    t,
    url,
    workspace,
    tab,
    seen,
    state,
    /** Exactly what a lead pane's `cf` is given, and nothing more. */
    lead: { ...t.env, ...tab.leadEnv },
    /** The pane's process ended — the only honest signal that it is gone. */
    async endPane(pane) {
      rust.event('pane.exit', { id: pane.id, generation: pane.generation })
      await waitFor(async () => {
        const listed = await fetch(`${url}/api/panes`, {
          headers: { authorization: `Bearer ${tab.leadEnv.CONSENSFLOW_APP_TOKEN}` },
        })
        const body = await listed.json()
        return !body.panes.some((candidate) => candidate.id === pane.id)
      })
    },
    threads() {
      const file = join(
        t.env.CONSENSFLOW_HOME,
        'workspaces',
        workspaceKey(workspace),
        'threads.json',
      )
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

/**
 * The real server, with every call written down on the way through.
 *
 * A store keeps the LATEST progress, not the sequence of them, so a route
 * that must be called twice in order cannot be proved from the store alone.
 * This is not a stand-in: every request is forwarded and every answer comes
 * back from the real server.
 */
async function recordingProxy(target, { refuse = [] } = {}) {
  const calls = []
  const refused = new Set(refuse.map((op) => `/api/panes/${op}`))
  const server = createServer(async (request, reply) => {
    let raw = ''
    for await (const chunk of request) raw += chunk
    calls.push({ path: request.url, body: raw.length === 0 ? null : JSON.parse(raw) })
    if (refused.has(request.url)) {
      // The shape `src/ui.js` sends for a PaneError, so the client meets the
      // refusal it would really meet.
      reply.writeHead(409, { 'content-type': 'application/json' })
      return reply.end(
        JSON.stringify({ error: 'elsewhere', reason: 'that session is running in another pane' }),
      )
    }
    let answer
    try {
      answer = await fetch(`${target}${request.url}`, {
        method: request.method,
        headers: {
          'content-type': 'application/json',
          ...(request.headers.authorization === undefined
            ? {}
            : { authorization: request.headers.authorization }),
        },
        ...(raw.length === 0 ? {} : { body: raw }),
      })
    } catch (cause) {
      reply.writeHead(502, { 'content-type': 'application/json' })
      return reply.end(JSON.stringify({ error: String(cause) }))
    }
    const text = await answer.text()
    reply.writeHead(answer.status, { 'content-type': 'application/json' })
    reply.end(text)
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    calls,
    /** Every body sent to one pane route, in the order it was sent. */
    to: (op) => calls.filter((call) => call.path === `/api/panes/${op}`).map((call) => call.body),
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

/** A stand-in app for the two routes slice B has not built yet. */
async function stubApp(routes) {
  const seen = []
  const server = createServer((request, reply) => {
    let raw = ''
    request.on('data', (chunk) => {
      raw += chunk
    })
    request.on('end', () => {
      const body = raw.length === 0 ? null : JSON.parse(raw)
      seen.push({ path: request.url, body, authorization: request.headers.authorization ?? null })
      const route = routes[request.url]
      const answer = typeof route === 'function' ? route(body) : route
      reply.writeHead(answer === undefined ? 404 : (answer.status ?? 200), {
        'content-type': 'application/json',
      })
      reply.end(JSON.stringify(answer === undefined ? { error: 'not found' } : answer.body))
    })
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    seen,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

describe('cf run in a lead pane asks the app', () => {
  let s
  let first

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('opens one pane and names the conversation and the pane', async () => {
    const result = await cf(['run', '@zeus', 'review the arbiter'], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)

    const line = /^conversation: (\S+) \(opened\) — pane (\S+)$/m.exec(result.stdout)
    assert.ok(line, `no conversation line in:\n${result.stdout}${result.stderr}`)
    first = { conversation: line[1], pane: line[2] }
    assert.match(first.conversation, /^zeus-[a-z]+-[a-z]+$/)

    assert.equal(s.seen.open.length, 1, 'one consult is one pane.open')
    const open = s.seen.open[0]
    assert.equal(open.id, first.pane)
    assert.equal(open.cwd, s.workspace)
    assert.deepEqual(open.argv.slice(2, 5), ['run', '@zeus', 'review the arbiter'])
    assert.ok(open.argv.includes('--in-pane'))
    assert.equal(open.argv[open.argv.indexOf('--session') + 1], first.conversation)

    const row = s.threads()[first.conversation]
    assert.equal(row.lead, s.lead.CONSENSFLOW_LEAD_ID, 'the app owns the lead identity')
  })

  it('continues that conversation on the next bare run — no `fresh` was ever sent', async () => {
    const pastes = s.seen.paste.length
    const result = await cf(['run', '@zeus', 'now the epochs'], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)
    assert.match(
      result.stdout,
      new RegExp(
        `^conversation: ${first.conversation} \\(continuing\\) — pane ${first.pane}$`,
        'm',
      ),
    )
    assert.equal(s.seen.open.length, 1, 'a live conversation never opens a second pane')
    assert.equal(s.seen.paste.length, pastes + 1)
    assert.match(s.seen.paste.at(-1).body, /now the epochs/)
  })

  it('--new mints a second conversation and says it is new', async () => {
    const result = await cf(['run', '@zeus', 'a separate question', '--new'], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)

    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(result.stdout)
    assert.ok(line, `no (new) line in:\n${result.stdout}${result.stderr}`)
    assert.notEqual(line[1], first.conversation)
    assert.equal(s.seen.open.length, 2)
    assert.ok(s.seen.open.at(-1).argv.includes('--new'))
  })

  it('--json prints the app’s own answer and no prose', async () => {
    const result = await cf(['run', '@zeus', 'as data', '--new', '--json'], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)
    const answer = JSON.parse(result.stdout)
    assert.equal(answer.outcome, 'opened')
    assert.equal(answer.agent, 'zeus')
    assert.equal(answer.tab, s.tab.tab.id)
    assert.equal(typeof answer.pane.id, 'string')
    assert.doesNotMatch(result.stdout, /conversation:/)
  })

  it('refuses --in-pane without a launch ticket, and opens nothing', async () => {
    const opens = s.seen.open.length
    const result = await cf(
      [
        'run',
        '@zeus',
        'sneaking in',
        '--in-pane',
        '--session',
        first.conversation,
        '--launch',
        'a-nonce',
      ],
      s.lead,
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /ticket/)
    assert.equal(s.seen.open.length, opens, 'a refusal launches nothing')
  })

  it('teaches its way out of --new --session, which the app cannot obey', async () => {
    const opens = s.seen.open.length
    const result = await cf(
      ['run', '@zeus', 'both at once', '--new', '--session', 'zeus-quiet-fern'],
      s.lead,
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /--new/, 'it names the flag that mints the name')
    assert.match(result.stderr, /drop --session/, 'it says which flag to drop')
    assert.match(result.stderr, /zeus-quiet-fern/, 'it names the conversation it would have used')
    assert.equal(s.seen.open.length, opens, 'a refusal launches nothing')
  })

  it('--notify records the lead’s delivery preference on the conversation', async () => {
    const result = await cf(
      ['run', '@zeus', 'quietly please', '--new', '--notify', 'manual'],
      s.lead,
      s.workspace,
    )
    assert.equal(result.code, 0, result.stderr)
    const line = /^conversation: (\S+) \(new\) — pane \S+$/m.exec(result.stdout)
    assert.ok(line, `${result.stdout}${result.stderr}`)
    assert.equal(s.threads()[line[1]].notifyPreference, 'manual')
  })

  it('still refuses an unknown agent locally, before it asks the app', async () => {
    const opens = s.seen.open.length
    const result = await cf(['run', '@nobody', 'q'], s.lead, s.workspace)
    assert.equal(result.code, 1)
    assert.equal(
      result.stderr,
      'cf: no agent named "nobody"; you have: zeus, nyx, ilmarinen, pygmalion, clio, gefjon\n',
    )
    assert.equal(s.seen.open.length, opens)
  })
})

describe('cf run reports a launch that never came back', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('says the outcome is unknown and names the launch', async () => {
    s.state.open = () => new Promise(() => {})
    const result = await cf(['run', '@zeus', 'hold here', '--new'], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stdout, /^conversation: \S+ \(unknown\) — launch \S+$/m)
    assert.match(result.stdout, /never came back|no retry|unresolved/i)
  })
})

describe('cf say and cf attach drive a pane through the app', () => {
  let s
  let conversation
  let pane

  before(async () => {
    s = await paneServer()
    const opened = await cf(['run', '@zeus', 'first turn', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    conversation = line[1]
    pane = line[2]
  })
  after(async () => {
    await s.close()
  })

  it('cf say pastes on the live pane and records what was sent', async () => {
    const pastes = s.seen.paste.length
    const result = await cf(['say', conversation, 'one more thing'], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stdout, new RegExp(`${conversation}`))
    assert.equal(s.seen.paste.length, pastes + 1)
    assert.equal(s.seen.paste.at(-1).body, 'one more thing')
    assert.equal(s.seen.paste.at(-1).epoch, 7, 'the paste carries the pane’s input epoch')

    const sent = s.threads()[conversation].sent.at(-1)
    assert.equal(sent.kind, 'say')
    assert.equal(typeof sent.opId, 'string')
  })

  it('cf attach answers a live conversation with its pane, opening nothing', async () => {
    const opens = s.seen.open.length
    const result = await cf(['attach', conversation], s.lead, s.workspace)
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stdout, new RegExp(`${conversation}.*${pane}`))
    assert.equal(s.seen.open.length, opens, 'a second window on one session is never opened')
  })

  it('cf say refuses once the pane is gone, in the app’s own words', async () => {
    await s.endPane({ id: pane, generation: 1 })
    const result = await cf(['say', conversation, 'anyone there'], s.lead, s.workspace)
    assert.equal(result.code, 1)
    assert.match(result.stderr, /no live pane|attach/)
  })

  it('relays the app’s refusal to attach a conversation that never bound', async () => {
    // Nothing ever ran in this pane, so the conversation holds no native
    // session and there is nothing to reopen. The app refuses it at
    // admission and says what to do instead; reopening a BOUND conversation
    // is proved where one exists — see the --in-pane controller suite.
    const opens = s.seen.open.length
    const result = await cf(['attach', conversation], s.lead, s.workspace)
    assert.equal(result.code, 1, result.stdout)
    assert.match(result.stderr, /never bound|nothing to attach/i)
    assert.match(result.stderr, /consult/i, 'the refusal says what to do instead')
    assert.equal(s.seen.open.length, opens, 'a doomed pane is never opened')
  })
})

describe('the --in-pane controller takes its ownership from the redemption', () => {
  let s
  let bound

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('binds claude on the preallocated id and never on leadId(env)', async () => {
    const opened = await cf(['run', '@nyx', 'the claude question', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const conversation = line[1]
    const open = s.seen.open.at(-1)
    const nativeSession = open.argv[open.argv.indexOf('--native-session') + 1]
    assert.match(nativeSession, /^[0-9a-f-]{36}$/, 'claude binds on an id the app minted')

    const binDir = join(s.t.root, 'harness-bin')
    const claude = fakeHarness(binDir, 'claude')
    // Everything the app hands the pane, plus a lead identity that is a lie:
    // the controller must read its conversation from the ticket, not here.
    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        CONSENSFLOW_LEAD_ID: 'tab:999:1',
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(result.code, 0, `${result.stdout}${result.stderr}`)

    assert.deepEqual(claude.calls().at(-1).slice(0, 2), ['--session-id', nativeSession])

    const row = s.threads()[conversation]
    assert.equal(row.sessionId, nativeSession)
    assert.equal(row.binding.evidence, 'preallocated')
    assert.equal(row.binding.launchId, open.launch)
    assert.equal(row.lead, s.lead.CONSENSFLOW_LEAD_ID, 'the poisoned lead id changed nothing')
    const sent = row.sent.at(-1)
    assert.equal(sent.kind, 'seed', 'the controller records what it put in the pane')
  })

  it('burns its ticket: a second run under the same launch is refused', async () => {
    const open = s.seen.open.at(-1)
    const binDir = join(s.t.root, 'harness-bin')
    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /ticket|launch/i)
  })

  it('binds codex on the nonce its seed carried into the rollout', async () => {
    const opened = await cf(['run', '@zeus', 'the codex question', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const conversation = line[1]
    const open = s.seen.open.at(-1)
    const nonce = open.argv[open.argv.indexOf('--launch') + 1]
    assert.equal(nonce, open.launch, 'the nonce IS the launch identity')

    // A stand-in codex that does what codex does: record the prompt it was
    // launched with, as an ordinary user turn, in its own rollout store.
    const binDir = join(s.t.root, 'codex-bin')
    const sessionId = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
    const codex = fakeHarness(
      binDir,
      'codex',
      "import { mkdirSync, writeFileSync } from 'node:fs'\n" +
        "import { join } from 'node:path'\n" +
        `const dir = join(${JSON.stringify(s.t.env.CODEX_HOME)}, 'sessions')\n` +
        'mkdirSync(dir, { recursive: true })\n' +
        'const seed = process.argv.at(-1)\n' +
        'const meta = { type: "session_meta", payload: { cwd: process.cwd(), timestamp: new Date().toISOString(), cli_version: "0.153.4" } }\n' +
        'const turn = { type: "response_item", payload: { id: "msg_seed", type: "message", role: "user", content: [{ type: "input_text", text: seed }] } }\n' +
        `writeFileSync(join(dir, 'rollout-2026-09-07T00-00-00-${sessionId}.jsonl'), [meta, turn].map((r) => JSON.stringify(r)).join('\\n') + '\\n')\n`,
    )

    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(result.code, 0, `${result.stdout}${result.stderr}`)

    const seed = codex.calls().at(-1).at(-1)
    assert.equal(
      seed.split('\n')[0],
      `[consensflow launch ${nonce}]`,
      'the nonce is the seed’s first line, where bindEvidence looks for it',
    )

    const row = s.threads()[conversation]
    assert.equal(row.sessionId, sessionId)
    assert.equal(row.binding.evidence, 'nonce')

    bound = { conversation, sessionId, pane: open.id, bin: binDir }
  })

  it('reports kimi’s first turn through the capability, and stays honestly unbound', async () => {
    const opened = await cf(
      ['run', '@ilmarinen', 'the kimi question', '--new'],
      s.lead,
      s.workspace,
    )
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const conversation = line[1]
    const open = s.seen.open.at(-1)

    // kimi can neither be handed a task nor typed into, so the pane streams
    // its first turn. This stand-in answers in kimi's own stream shape.
    const binDir = join(s.t.root, 'kimi-bin')
    fakeHarness(
      binDir,
      'kimi',
      'if (process.argv.includes("-p")) {\n' +
        '  process.stdout.write(JSON.stringify({ session_id: "k-77" }) + "\\n")\n' +
        '  process.stdout.write(JSON.stringify({ role: "assistant", content: "kimi answered" }) + "\\n")\n' +
        '}\n',
    )

    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        CONSENSFLOW_LEAD_ID: 'tab:999:1',
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )

    const row = s.threads()[conversation]
    assert.ok(
      row.progress,
      `no progress recorded; the pane said:\n${result.stdout}${result.stderr}`,
    )
    assert.equal(
      row.progress.state,
      'first-turn-done',
      'progress for this launch is written under the controller capability',
    )
    assert.equal(row.progress.exitCode, 0)
    assert.equal(row.sent.at(-1).kind, 'seed')
    assert.equal(row.lead, s.lead.CONSENSFLOW_LEAD_ID, 'the poisoned lead id changed nothing')

    // No kimi store was written here, so there is no transcript to read the
    // launch marker out of: unbound, and said so — never bound on a guess.
    // The window still opens (it is the human's), so the pane exits on the
    // window and the refusal is on stderr, with the row left unbound.
    assert.match(result.stderr, /could not be read, so it stays unbound/)
    assert.equal(row.sessionId ?? null, null)
    assert.equal(
      result.stdout,
      `kimi answered\n${conversation} · @ilmarinen — handing this terminal to kimi\n`,
      'an unbound session still becomes the human’s window',
    )
    assert.equal(result.code, 0, 'the window ran, so the pane exits on the window')
  })

  it('binds kimi on the nonce its packet carried into the wire log', async () => {
    const opened = await cf(
      ['run', '@ilmarinen', 'the bound kimi question', '--new'],
      s.lead,
      s.workspace,
    )
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const conversation = line[1]
    const open = s.seen.open.at(-1)
    const nonce = open.argv[open.argv.indexOf('--launch') + 1]

    // A stand-in kimi that writes what kimi writes: the record shapes and
    // field names are copied from the real protocol-1.5 wire logs in
    // `tests/engine/fixtures/completion/kimi/tool-result.jsonl` and
    // `superseded-tool.jsonl` — `turn.prompt` and the
    // `context.append_message` that carries the native message id, a
    // `step.begin`/`content.part`/`step.end` answer, and the native
    // `turn.ended {reason:completed}` boundary.
    const binDir = join(s.t.root, 'kimi-bound-bin')
    const kimiHome = join(s.t.env.HOME, '.kimi-code')
    const kimi = fakeHarness(
      binDir,
      'kimi',
      `
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
const argv = process.argv.slice(2)
const prompt = argv[argv.indexOf('-p') + 1]
const dir = join(${JSON.stringify(kimiHome)}, 'sessions', 'wd_fixture', ${JSON.stringify(KIMI_SESSION)}, 'agents', 'main')
mkdirSync(dir, { recursive: true })
const at = Date.now()
const step = 'd26c913f-c98b-4262-8340-06a147aa7937'
const records = [
  { type: 'metadata', protocol_version: '1.5', created_at: at },
  { type: 'turn.prompt', agentId: 'main', input: [{ type: 'text', text: prompt }], origin: { kind: 'user' }, time: at },
  { type: 'context.append_message', agentId: 'main', message: { role: 'user', content: [{ type: 'text', text: prompt }], toolCalls: [], origin: { kind: 'user' }, id: 'msg_01M0TA752ZAJKC8PRMYSJE95WY' }, time: at },
  { type: 'context.append_loop_event', agentId: 'main', event: { type: 'step.begin', uuid: step, turnId: '1', step: 1 }, time: at },
  { type: 'context.append_loop_event', agentId: 'main', event: { type: 'content.part', uuid: '94831cd3-f477-42b9-bb79-936e6c3c4ea9', turnId: '1', step: 1, stepUuid: step, part: { type: 'text', text: 'kimi answered' } }, time: at },
  { type: 'context.append_loop_event', agentId: 'main', event: { type: 'step.end', uuid: step, turnId: '1', step: 1, finishReason: 'end_turn', usage: { inputOther: 1, output: 1, inputCacheRead: 0, inputCacheCreation: 0 } }, time: at },
  { type: 'turn.ended', agentId: 'main', turnId: 1, reason: 'completed', durationMs: 10, time: at },
]
writeFileSync(join(dir, 'wire.jsonl'), records.map((r) => JSON.stringify(r)).join('\\n') + '\\n')
process.stdout.write(JSON.stringify({ session_id: ${JSON.stringify(KIMI_SESSION)} }) + '\\n')
process.stdout.write(JSON.stringify({ role: 'assistant', content: 'kimi answered' }) + '\\n')
`,
    )

    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        CONSENSFLOW_LEAD_ID: 'tab:999:1',
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(result.code, 0, `${result.stdout}${result.stderr}`)

    const streamed = kimi.calls().find((call) => call.includes('-p'))
    const packet = streamed[streamed.indexOf('-p') + 1]
    assert.equal(
      packet.split('\n')[0],
      `[consensflow launch ${nonce}]`,
      'kimi takes its prompt in argv, so the marker rides on the packet’s first line',
    )

    const row = s.threads()[conversation]
    assert.equal(row.sessionId, KIMI_SESSION)
    assert.equal(row.binding.evidence, 'nonce')
    assert.equal(row.binding.launchId, open.launch)
    assert.equal(row.progress.state, 'first-turn-done')
    assert.equal(row.lead, s.lead.CONSENSFLOW_LEAD_ID, 'the poisoned lead id changed nothing')
  })

  it('reports kimi’s first turn as two ordered states and streams it to the pane', async () => {
    const proxy = await recordingProxy(s.url)
    try {
      const opened = await cf(
        ['run', '@ilmarinen', 'the streamed question', '--new'],
        s.lead,
        s.workspace,
      )
      const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
      assert.ok(line, `${opened.stdout}${opened.stderr}`)
      const conversation = line[1]
      const open = s.seen.open.at(-1)

      const binDir = join(s.t.root, 'kimi-stream-bin')
      fakeHarness(binDir, 'kimi', kimiWireBody(s.t.env, 'session_streamed_0001'))
      const result = await cf(
        open.argv.slice(2),
        {
          ...s.t.env,
          ...open.env,
          CONSENSFLOW_APP: proxy.url,
          PATH: [binDir, s.t.env.PATH].join(delimiter),
        },
        s.workspace,
      )
      assert.equal(result.code, 0, `${result.stdout}${result.stderr}`)

      // The store keeps only the LATEST progress, so the pair can only be
      // proved on the wire: a lead watching a kimi pane has to be told it
      // started before it is told it finished.
      assert.deepEqual(
        proxy.to('progress.set').map((body) => body.progress),
        [{ state: 'first-turn' }, { state: 'first-turn-done', exitCode: 0 }],
        'both states, in that order',
      )

      // kimi cannot be seeded interactively, so its first turn is streamed
      // into the pane: the lead watches it work, and this is that output.
      assert.equal(
        result.stdout,
        `kimi answered\n${conversation} · @ilmarinen — handing this terminal to kimi\n`,
      )
    } finally {
      await proxy.close()
    }
  })

  it('reopens a bound conversation on the session it already has', async () => {
    await s.endPane({ id: bound.pane, generation: 1 })
    const reopened = await cf(['attach', bound.conversation], s.lead, s.workspace)
    assert.equal(reopened.code, 0, reopened.stderr)

    const open = s.seen.open.at(-1)
    assert.deepEqual(open.argv.slice(2, 5), ['attach', bound.conversation, '--in-pane'])

    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [bound.bin, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(result.code, 0, `${result.stdout}${result.stderr}`)

    const codex = readFileSync(join(bound.bin, 'codex.argv'), 'utf8').trimEnd().split('\n')
    assert.deepEqual(
      JSON.parse(codex.at(-1)),
      ['resume', bound.sessionId],
      'a reopened pane resumes the bound session — it never starts a second one',
    )
  })
})

describe('cf refuses when it cannot reach the app', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  const env = {
    ...t.env,
    CONSENSFLOW_APP: NO_APP,
    CONSENSFLOW_APP_TOKEN: 'lead-token',
    CONSENSFLOW_TAB: 'tab-1',
    CONSENSFLOW_LEAD_ID: 'tab:1:1',
    CONSENSFLOW_PANE_ID: 'p-1',
  }
  after(() => t.cleanup())

  for (const [label, args] of [
    ['run', ['run', '@zeus', 'q']],
    ['say', ['say', 'zeus-quiet-fern', 'words']],
    ['read', ['read', 'd-1']],
    ['results', ['results']],
    ['attach', ['attach', 'zeus-quiet-fern']],
    ['chat', ['chat', 'zeus-quiet-fern']],
  ]) {
    it(`cf ${label} names the app it could not reach, and what to do`, async () => {
      const result = await cf(args, env, t.root)
      assert.equal(result.code, 1, result.stdout)
      assert.match(result.stderr, /127\.0\.0\.1:1/, `cf ${label} must name the app`)
      assert.match(
        result.stderr,
        label === 'chat' ? /cf say [^\n]*cf attach/ : /Is the app still running\?/,
        `cf ${label} must say what to do about it`,
      )
    })
  }
})

describe('an agent never spawns agents', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  after(() => t.cleanup())

  for (const [label, args] of [
    ['run', ['run', '@zeus', 'q']],
    ['say', ['say', 'zeus-quiet-fern', 'words']],
    ['read', ['read', 'd-1']],
    ['results', ['results']],
    ['attach', ['attach', 'zeus-quiet-fern']],
    ['chat', ['chat', 'zeus-quiet-fern']],
  ]) {
    it(`CONSENSFLOW_CHILD=1 refuses cf ${label}`, async () => {
      const result = await cf(
        args,
        {
          ...t.env,
          CONSENSFLOW_CHILD: '1',
          CONSENSFLOW_APP: NO_APP,
          CONSENSFLOW_APP_TOKEN: 'lead-token',
          CONSENSFLOW_TAB: 'tab-1',
          CONSENSFLOW_PANE_ID: 'p-1',
        },
        t.root,
      )
      assert.equal(result.code, 1)
      assert.equal(
        result.stderr,
        'cf: this is already an agent run — an agent does not spawn agents\n',
      )
    })
  }
})

describe('--notify is the app’s, and says so outside it', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  after(() => t.cleanup())

  it('refuses --notify when there is no app to deliver anything', async () => {
    const result = await cf(['run', '@zeus', 'q', '--notify', 'auto'], t.env, t.root)
    assert.equal(result.code, 1)
    assert.match(result.stderr, /--notify/)
    assert.match(result.stderr, /app/)
  })
})

describe('the standalone CLI contract without an app pane', () => {
  const t = tempEnv()
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, t.env)
  const workspace = join(t.root, 'cmux-workspace')
  mkdirSync(workspace, { recursive: true })
  after(() => t.cleanup())

  // The standalone app owns panes now. Keep the no-app contract pinned so
  // this suite cannot silently reintroduce the removed cmux fallback.
  const BASELINE = [
    [
      'run',
      ['run', '@zeus', 'a question'],
      1,
      '',
      "cf: cf run is how you reach a pane, and panes live in ConsensFlow's app — run it from a pane the app opened\n",
    ],
    [
      'attach',
      ['attach', 'ghost'],
      1,
      '',
      "cf: cf attach is how you reach a pane, and panes live in ConsensFlow's app — run it from a pane the app opened\n",
    ],
    [
      'chat',
      ['chat', '@ghost'],
      1,
      '',
      'cf: Conversations live in ConsensFlow app panes. Use cf say <conversation> "<words>" or cf attach <conversation>.\n',
    ],
    [
      'results',
      ['results'],
      1,
      '',
      "cf: cf results is how you reach a pane, and panes live in ConsensFlow's app — run it from a pane the app opened\n",
    ],
    [
      'catchup',
      ['catchup', 'ghost'],
      1,
      '',
      'cf: cf catchup is retired — discover completed results with `cf results [conversation|@agent]`, then read one whole with `cf read <conversation> [--answer <id>] [--part <k>]`\n',
    ],
    [
      'sessions',
      ['sessions'],
      0,
      'no conversations here yet — `cf run @name "<task>"` starts one\n',
      '',
    ],
  ]

  it('cf help lists exactly the standalone verbs', async () => {
    const result = await cf(['help'], t.env, workspace)
    assert.equal(result.code, 0, result.stderr)
    const verbs = [
      ...new Set(
        result.stdout
          .split('\n')
          .map((line) => /^ {2}([a-z][a-z-]*)(?: |$)/.exec(line)?.[1])
          .filter((verb) => verb !== undefined),
      ),
    ]
    assert.deepEqual(verbs, [
      'setup',
      'run',
      'say',
      'attach',
      'read',
      'results',
      'sessions',
      'last',
      'catalog',
      'agent',
      'skills',
      'ui',
      'doctor',
      'off',
      'reset',
    ])
  })

  for (const [label, args, code, stdout, stderr] of BASELINE) {
    it(`cf ${label} answers exactly what it answered before`, async () => {
      const result = await cf(args, t.env, workspace)
      assert.equal(result.stderr, stderr)
      assert.equal(result.stdout, stdout)
      assert.equal(result.code, code)
    })
  }
})

describe('the bundle’s cf shadows a stale one on PATH', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('resolves the lead pane’s `cf` to the bundle, not to the old install', async () => {
    const stale = join(t.root, 'usr-local-bin')
    mkdirSync(stale, { recursive: true })
    writeFileSync(join(stale, 'cf'), '#!/bin/sh\necho "consensflow 0.0.1-stale"\n')
    chmodSync(join(stale, 'cf'), 0o755)

    const env = leadEnv({
      tab: 'tab-1',
      pane: 'p-1',
      leadId: 'tab:1:1',
      app: { url: NO_APP },
      path: stale,
      node: process.execPath,
    })
    assert.equal(env.PATH.split(delimiter)[0], BUNDLE_BIN)
    assert.equal(
      env.CONSENSFLOW_NODE,
      process.execPath,
      'the shim runs the app’s own runtime, never one it finds on PATH',
    )

    // Nothing supplied by the test: this is the environment a lead pane is
    // given, with a stale `cf` sitting on the PATH behind the bundle's.
    const version = readFileSync(join(import.meta.dirname, '..', 'package.json'), 'utf8')
    const { stdout } = await run('cf', ['--version'], {
      env: { ...t.env, ...env },
      timeout: 30_000,
    })
    assert.equal(stdout.trim(), JSON.parse(version).version)
  })
})

describe('a closed conversation is resumed, never started again', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  /** Run the controller argv the app just produced, on a fake PATH. */
  const inPane = (open, binDir) =>
    cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )

  /**
   * One conversation per harness, taken all the way: opened fresh, bound to
   * a native session, its pane ended, then consulted again. The second
   * consult is the one under test — the app answers it with the session the
   * row already holds, and the pane must REOPEN that session rather than
   * start a new one on top of it.
   */
  const closedRun = async ({ agent, kind, bin, stage, firstArgv, resumeArgv }) => {
    const binDir = join(s.t.root, `${bin}-resume-bin`)
    const harness = stage(binDir)

    const opened = await cf(['run', `@${agent}`, 'the first turn', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const conversation = line[1]
    const first = s.seen.open.at(-1)
    await harness.before?.(first, conversation)
    const started = await inPane(first, binDir)
    assert.equal(started.code, 0, `first turn: ${started.stdout}${started.stderr}`)

    const row = s.threads()[conversation]
    assert.equal(
      typeof row.sessionId,
      'string',
      `${kind} never bound.\nrow: ${JSON.stringify(row)}\npane said: ${started.stdout}${started.stderr}`,
    )
    assert.deepEqual(
      harness.calls().at(-1).slice(0, firstArgv.length),
      firstArgv.map((part) => (part === '<session>' ? row.sessionId : part)),
      'the first turn opens the session the app named',
    )

    await s.endPane({ id: first.id, generation: first.generation })

    const again = await cf(['run', `@${agent}`, 'the follow-up'], s.lead, s.workspace)
    assert.equal(again.code, 0, again.stderr)
    assert.match(again.stdout, new RegExp(`conversation: ${conversation} \\(opened\\)`))

    const resumed = s.seen.open.at(-1)
    assert.equal(
      resumed.argv[resumed.argv.indexOf('--native-session') + 1],
      row.sessionId,
      'the app answers a closed conversation with the session it already holds',
    )
    assert.ok(!resumed.argv.includes('--new'), 'a reopened conversation is not a new one')

    const back = await inPane(resumed, binDir)
    assert.equal(back.code, 0, `resume: ${back.stdout}${back.stderr}`)

    const calls = harness.calls()
    assert.deepEqual(
      calls.at(-1).slice(0, resumeArgv.length),
      resumeArgv.map((part) => (part === '<session>' ? row.sessionId : part)),
      `${kind} must reopen its own session, not start a new one`,
    )
    return { conversation, sessionId: row.sessionId, calls }
  }

  it('claude reopens with --resume, not --session-id', async () => {
    await closedRun({
      agent: 'nyx',
      kind: 'claude-code',
      bin: 'claude',
      stage: (dir) => fakeHarness(dir, 'claude'),
      firstArgv: ['--session-id', '<session>'],
      resumeArgv: ['--resume', '<session>', '--model', 'opus'],
    })
  })

  it('pi reopens the conversation’s own session id, extension and all', async () => {
    const { calls, sessionId } = await closedRun({
      agent: 'clio',
      kind: 'pi',
      bin: 'pi',
      stage: (dir) => fakeHarness(dir, 'pi'),
      // pi's start and resume argv are the same shape — its session id IS
      // the conversation name — so this pins the shape rather than telling
      // the two apart; the delivery extension leads both.
      firstArgv: ['--extension'],
      resumeArgv: ['--extension'],
    })
    const reopened = calls.at(-1)
    assert.equal(reopened[reopened.indexOf('--session-id') + 1], sessionId)
    assert.match(reopened[1], /consensflow-delivery\.mjs$/)
  })

  it('codex reopens with `codex resume <id>`, not a cold window', async () => {
    const sessionId = 'aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff'
    await closedRun({
      agent: 'zeus',
      kind: 'codex',
      bin: 'codex',
      stage: (dir) => fakeHarness(dir, 'codex', codexRolloutBody(s.t.env, sessionId)),
      firstArgv: ['--model', 'gpt-5-codex'],
      resumeArgv: ['resume', '<session>'],
    })
  })

  it('opencode reopens with --session, not a cold window', async () => {
    await closedRun({
      agent: 'gefjon',
      kind: 'opencode',
      bin: 'opencode',
      stage: (dir) => {
        const staged = stageOpencodeSession(s.t.env)
        return fakeHarness(dir, 'opencode', opencodeBody(s.t.env, staged))
      },
      firstArgv: ['--model', 'grok-code'],
      resumeArgv: ['--session', '<session>'],
    })
  })

  it('kimi continues on -S <id> and then hands over its TUI', async () => {
    const { calls, sessionId } = await closedRun({
      agent: 'ilmarinen',
      kind: 'kimi',
      bin: 'kimi',
      stage: (dir) => fakeHarness(dir, 'kimi', kimiWireBody(s.t.env, KIMI_SESSION)),
      firstArgv: ['-S', '<session>'],
      resumeArgv: ['-S', '<session>'],
    })
    // kimi cannot be seeded interactively, so the follow-up streams first on
    // the SAME session and only then becomes the window.
    const streamed = calls.at(-2)
    assert.equal(streamed[streamed.indexOf('-S') + 1], sessionId)
    const packet = streamed[streamed.indexOf('-p') + 1]
    assert.match(packet, /the follow-up/, 'the question survives')
    // A follow-up in a live conversation is not a first turn: the agent is
    // already in this workspace and has already been told how to work.
    assert.doesNotMatch(packet, /# ConsensFlow Packet/, 'no ceremony for a follow-up')
    assert.doesNotMatch(packet, /## How to work/, 'it already knows how to work')
    assert.doesNotMatch(packet, /Workspace:/, 'it is already in the workspace')
  })
})

describe('a marker that never reaches the harness never binds', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('does not bind when the marker never reaches the harness', async () => {
    // The mutation this exists for: strip the marker line from the prompt
    // opencode is actually given, and the binding must refuse. It used to
    // pass, because the fixture manufactured the marker beside the harness
    // instead of taking it from what the pane was sent.
    const binDir = join(s.t.root, 'opencode-nomarker-bin')
    const staged = stageOpencodeSession(s.t.env)
    fakeHarness(binDir, 'opencode', opencodeBody(s.t.env, staged, { persist: 'without-marker' }))

    const opened = await cf(['run', '@gefjon', 'a question', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\)/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const open = s.seen.open.at(-1)
    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )

    assert.match(result.stderr, /carries no launch marker/)
    assert.equal(s.threads()[line[1]].sessionId ?? null, null, 'no marker, no binding')
  })
})

describe('cf results lists completed results through the app', () => {
  const t = tempEnv()
  let app
  after(async () => {
    await app?.close()
    t.cleanup()
  })

  const LIST = {
    workers: [
      {
        conversation: 'nyx-coral-lane',
        agent: 'nyx',
        running: false,
        reason: null,
        results: [
          { id: 'a-11', bytes: 41, preview: 'the first answer, whole', status: 'unread' },
          { id: 'a-12', bytes: 18, preview: 'the second answer', status: 'read' },
        ],
      },
      {
        conversation: 'zeus-quiet-fern',
        agent: 'zeus',
        running: true,
        reason: 'thinking',
        results: [
          {
            id: 'a-20',
            bytes: 9,
            preview: 'thinking out',
            status: 'reading',
            deliveryId: 'd-4',
            parts: 2,
          },
        ],
      },
    ],
  }

  before(async () => {
    app = await stubApp({ '/api/panes/results.list': { status: 200, body: LIST } })
  })

  const env = () => ({
    ...t.env,
    CONSENSFLOW_APP: app.url,
    CONSENSFLOW_APP_TOKEN: 'lead-token',
    CONSENSFLOW_TAB: 'tab-1',
    CONSENSFLOW_LEAD_ID: 'tab:1:1',
    CONSENSFLOW_PANE_ID: 'p-1',
  })

  it('lists every completed result id, status and preview under the tab it was given', async () => {
    const result = await cf(['results'], env(), t.root)
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stdout, /nyx-coral-lane · @nyx/)
    assert.match(result.stdout, /a-11 unread/)
    assert.match(result.stdout, /the first answer, whole/)
    assert.match(result.stdout, /a-12 read/)
    assert.match(result.stdout, /zeus-quiet-fern · @zeus/)
    assert.match(result.stdout, /a-20 reading/)
    assert.match(result.stdout, /cf read d-4/, 'a result being read names its delivery')

    const call = app.seen.at(-1)
    assert.equal(call.path, '/api/panes/results.list')
    assert.equal(call.authorization, 'Bearer lead-token')
    assert.equal(call.body.tab, 'tab-1')
    assert.equal(typeof call.body.opId, 'string')
  })

  it('filters to one conversation by exact name', async () => {
    const result = await cf(['results', 'nyx-coral-lane'], env(), t.root)
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stdout, /nyx-coral-lane/)
    assert.doesNotMatch(result.stdout, /zeus-quiet-fern/)
  })

  it('filters to a roster agent with @', async () => {
    const result = await cf(['results', '@zeus'], env(), t.root)
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stdout, /zeus-quiet-fern/)
    assert.doesNotMatch(result.stdout, /nyx-coral-lane/)
  })

  it('says what it has when the filter matches nothing', async () => {
    const result = await cf(['results', 'ghost'], env(), t.root)
    assert.equal(result.code, 1)
    assert.match(result.stderr, /ghost/)
    assert.match(result.stderr, /nyx-coral-lane/)
  })

  it('--json prints the workers the app listed', async () => {
    const result = await cf(['results', '--json'], env(), t.root)
    assert.equal(result.code, 0, result.stderr)
    assert.deepEqual(JSON.parse(result.stdout), LIST)
  })

  it('discovery marks nothing as seen', async () => {
    const before = app.seen.length
    const result = await cf(['results', 'nyx-coral-lane'], env(), t.root)
    assert.equal(result.code, 0, result.stderr)
    for (const call of app.seen.slice(before)) {
      assert.notEqual(call.path, '/api/panes/seen', 'discovery never writes a read mark')
    }
  })
})

describe('cf read reads one whole completed result', () => {
  const t = tempEnv()
  let app
  after(async () => {
    await app?.close()
    t.cleanup()
  })

  const PART = {
    outcome: 'read',
    deliveryId: 'd-3',
    conversation: 'nyx-coral-lane',
    agent: 'nyx',
    k: 1,
    of: 2,
    text: '[part 1 of 2 — 11 bytes]\nthe beginning\n[end of part 1 of 2 — delivery d-3]\nnext: cf read d-3 --part 2\n',
  }

  before(async () => {
    app = await stubApp({ '/api/panes/results.read': { status: 200, body: PART } })
  })

  const env = () => ({
    ...t.env,
    CONSENSFLOW_APP: app.url,
    CONSENSFLOW_APP_TOKEN: 'lead-token',
    CONSENSFLOW_TAB: 'tab-1',
    CONSENSFLOW_LEAD_ID: 'tab:1:1',
    CONSENSFLOW_PANE_ID: 'p-1',
  })
  const read = (args) => cf(['read', ...args], env(), t.root)

  it('prints the first framed part verbatim and teaches the next one on stderr', async () => {
    const result = await read(['nyx-coral-lane'])
    assert.equal(result.code, 0, result.stderr)
    assert.equal(result.stdout, PART.text, 'a part is printed exactly as the app framed it')
    assert.match(result.stderr, /part 1 of 2/)
    assert.match(
      result.stderr,
      /cf read d-3 --part 2/,
      'follow-up parts use the immutable delivery id',
    )

    const call = app.seen.at(-1)
    assert.equal(call.path, '/api/panes/results.read')
    assert.equal(call.authorization, 'Bearer lead-token')
    assert.equal(call.body.tab, 'tab-1')
    assert.equal(call.body.session, 'nyx-coral-lane')
    assert.equal(call.body.part, 1)
    assert.equal(typeof call.body.opId, 'string')
    assert.ok(!('answerId' in call.body), 'no answer id unless one was asked for')
  })

  it('--answer selects one completed result', async () => {
    const result = await read(['nyx-coral-lane', '--answer', 'a-11'])
    assert.equal(result.code, 0, result.stderr)
    assert.equal(app.seen.at(-1).body.answerId, 'a-11')
  })

  it('--part asks for that part', async () => {
    const result = await read(['nyx-coral-lane', '--answer', 'a-11', '--part', '2'])
    assert.equal(result.code, 0, result.stderr)
    assert.equal(app.seen.at(-1).body.part, 2)
  })

  it('refuses a delivery id with --answer: the delivery already names its answer', async () => {
    const before = app.seen.length
    const result = await read(['d-3', '--answer', 'a-11'])
    assert.equal(result.code, 1)
    assert.match(result.stderr, /--answer/)
    assert.equal(app.seen.length, before, 'a refused read asks the app for nothing')
  })

  it('refuses a bad part number before asking the app', async () => {
    const before = app.seen.length
    const result = await read(['nyx-coral-lane', '--part', '2garbage'])
    assert.equal(result.code, 1)
    assert.match(result.stderr, /part/)
    assert.equal(app.seen.length, before, 'a refused part number asks the app for nothing')
  })

  it('reading marks nothing as seen', async () => {
    const before = app.seen.length
    const result = await read(['nyx-coral-lane'])
    assert.equal(result.code, 0, result.stderr)
    for (const call of app.seen.slice(before)) {
      assert.notEqual(call.path, '/api/panes/seen', 'reading never writes a read mark')
    }
  })
})

describe('app-only flags are checked before anything acts on them', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  for (const [label, args] of [
    ['--launch', ['run', '@zeus', 'q', '--launch', 'some-nonce']],
    ['--native-session', ['run', '@zeus', 'q', '--native-session', 'some-id']],
  ]) {
    it(`refuses ${label} without --in-pane instead of falling through`, async () => {
      const opens = s.seen.open.length
      const result = await cf(args, s.lead, s.workspace)
      assert.equal(result.code, 1)
      assert.match(result.stderr, /--in-pane/)
      assert.equal(s.seen.open.length, opens, 'nothing was launched on a flag we did not honour')
    })
  }

  it('refuses --in-pane carrying both pieces of launch evidence', async () => {
    const result = await cf(
      ['run', '@zeus', 'q', '--in-pane', '--launch', 'n', '--native-session', 'i'],
      { ...s.lead, CONSENSFLOW_LAUNCH: 'not-a-real-ticket' },
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /one/)
    assert.doesNotMatch(result.stderr, /ticket/, 'the argv is wrong before the ticket matters')
  })

  it('refuses --in-pane carrying no launch evidence at all', async () => {
    const result = await cf(
      ['run', '@zeus', 'q', '--in-pane'],
      { ...s.lead, CONSENSFLOW_LAUNCH: 'not-a-real-ticket' },
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /--launch|--native-session/)
  })

  it('refuses a --launch that is not the launch the ticket redeemed', async () => {
    const opened = await cf(['run', '@zeus', 'a codex question', '--new'], s.lead, s.workspace)
    assert.match(opened.stdout, /conversation:/, opened.stderr)
    const open = s.seen.open.at(-1)
    const argv = open.argv.slice(2)
    argv[argv.indexOf('--launch') + 1] = 'somebody-elses-nonce'

    const result = await cf(argv, { ...s.t.env, ...open.env }, s.workspace)
    assert.equal(result.code, 1, `${result.stdout}${result.stderr}`)
    assert.match(
      result.stderr,
      /somebody-elses-nonce/,
      `the refusal must name the nonce it was given; got:\n${result.stderr}`,
    )
    assert.equal(
      (s.threads()[open.argv[open.argv.indexOf('--session') + 1]].sent ?? []).length,
      0,
      'a launch we cannot trust records nothing',
    )
  })

  it('refuses --notify inside a pane, before the ticket is spent', async () => {
    const opened = await cf(['run', '@nyx', 'a question', '--new'], s.lead, s.workspace)
    assert.match(opened.stdout, /conversation:/, opened.stderr)
    const open = s.seen.open.at(-1)
    const session = open.argv[open.argv.indexOf('--session') + 1]
    const binDir = join(s.t.root, 'notify-in-pane-bin')
    fakeHarness(binDir, 'claude')
    const inPane = (extra) =>
      cf(
        [...open.argv.slice(2), ...extra],
        {
          ...s.t.env,
          ...open.env,
          PATH: [binDir, s.t.env.PATH].join(delimiter),
        },
        s.workspace,
      )

    const refused = await inPane(['--notify', 'manual'])
    assert.equal(refused.code, 1, `${refused.stdout}${refused.stderr}`)
    assert.match(refused.stderr, /--notify/)
    assert.equal(existsSync(join(binDir, 'claude.argv')), false, 'no window was handed over')
    assert.equal((s.threads()[session].sent ?? []).length, 0, 'nothing was recorded')

    // And the ticket is untouched: the same launch still works without it.
    const accepted = await inPane([])
    assert.equal(accepted.code, 0, `${accepted.stdout}${accepted.stderr}`)
    assert.equal((s.threads()[session].sent ?? []).length, 1)
  })

  it('refuses an app-only flag for an image agent instead of ignoring it', async () => {
    for (const flag of [
      ['--in-pane'],
      ['--launch', 'n'],
      ['--native-session', 'i'],
      ['--notify', 'auto'],
    ]) {
      const result = await cf(['run', '@pygmalion', 'draw a cat', ...flag], s.lead, s.workspace)
      assert.equal(result.code, 1, `${flag[0]} was accepted for an image agent`)
      assert.match(result.stderr, new RegExp(flag[0].replace(/^--/, '--')))
    }
  })
})

describe('attach goes through the same evidence contract as run', () => {
  let s
  let conversation
  let bound
  let binDir

  before(async () => {
    s = await paneServer()
    binDir = join(s.t.root, 'attach-evidence-bin')
    fakeHarness(binDir, 'claude')
    const opened = await cf(['run', '@nyx', 'the first turn', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    conversation = line[1]
    const open = s.seen.open.at(-1)
    const ran = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(ran.code, 0, `${ran.stdout}${ran.stderr}`)
    bound = s.threads()[conversation].sessionId
    assert.equal(typeof bound, 'string')
    await s.endPane({ id: open.id, generation: open.generation })
  })
  after(async () => {
    await s.close()
  })

  for (const [label, flag] of [
    ['--launch', ['--launch', 'a-nonce']],
    ['--native-session', ['--native-session', 'some-id']],
  ]) {
    it(`refuses ${label} on attach outside a pane`, async () => {
      const opens = s.seen.open.length
      const result = await cf(['attach', conversation, ...flag], s.lead, s.workspace)
      assert.equal(result.code, 1, result.stdout)
      assert.match(result.stderr, /--in-pane/)
      assert.equal(s.seen.open.length, opens, 'a refused flag opens no pane')
    })
  }

  it('refuses an attach pane carrying both pieces of evidence', async () => {
    const result = await cf(
      ['attach', conversation, '--in-pane', '--launch', 'n', '--native-session', bound],
      { ...s.lead, CONSENSFLOW_LAUNCH: 'not-a-real-ticket' },
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /one/)
    assert.doesNotMatch(result.stderr, /ticket/, 'the argv is wrong before the ticket matters')
  })

  it('refuses an attach pane carrying none', async () => {
    const result = await cf(
      ['attach', conversation, '--in-pane'],
      { ...s.lead, CONSENSFLOW_LAUNCH: 'not-a-real-ticket' },
      s.workspace,
    )
    assert.equal(result.code, 1)
    assert.match(result.stderr, /--launch|--native-session/)
  })

  it('refuses a corrupted nonce on a valid ticket, after redeeming it', async () => {
    // A real nonce-only launch — one evidence flag, so the arity check has
    // nothing to say — with the nonce itself corrupted. The ticket is real,
    // and the point is that redemption alone authorises nothing.
    const fresh = await cf(['run', '@zeus', 'a codex question', '--new'], s.lead, s.workspace)
    assert.match(fresh.stdout, /conversation:/, fresh.stderr)
    const open = s.seen.open.at(-1)
    const session = open.argv[open.argv.indexOf('--session') + 1]
    const nonce = open.argv[open.argv.indexOf('--launch') + 1]
    assert.equal(open.argv.includes('--native-session'), false, 'a fresh codex is nonce-only')

    const codexBin = join(s.t.root, 'attach-nonce-bin')
    fakeHarness(codexBin, 'codex')
    const result = await cf(
      ['attach', session, '--in-pane', '--launch', `${nonce}-corrupted`],
      { ...s.t.env, ...open.env, PATH: [codexBin, s.t.env.PATH].join(delimiter) },
      s.workspace,
    )
    assert.equal(result.code, 1, `${result.stdout}${result.stderr}`)
    assert.match(result.stderr, /-corrupted/, 'the refusal names the marker it was handed')
    assert.equal(existsSync(join(codexBin, 'codex.argv')), false, 'nothing was handed a terminal')

    const row = s.threads()[session]
    assert.equal((row.sent ?? []).length, 0, 'and nothing was written')
    assert.equal(row.sessionId ?? null, null)

    // Redemption did happen: the ticket is spent, and says so on reuse.
    const again = await cf(
      ['attach', session, '--in-pane', '--launch', nonce],
      { ...s.t.env, ...open.env, PATH: [codexBin, s.t.env.PATH].join(delimiter) },
      s.workspace,
    )
    assert.equal(again.code, 1)
    assert.match(again.stderr, /spent|expired|revoked/i)
  })
})

describe('the standalone refusals that keep a lead out of the cmux path', () => {
  let s
  let conversation

  before(async () => {
    s = await paneServer()
    const opened = await cf(['run', '@zeus', 'a question', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\)/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    conversation = line[1]
  })
  after(async () => {
    await s.close()
  })

  it('refuses cf attach --print: the app opens the pane, not the caller', async () => {
    const result = await cf(['attach', conversation, '--print'], s.lead, s.workspace)
    assert.equal(result.code, 1, result.stdout)
    assert.match(result.stderr, /--print/)
    assert.doesNotMatch(result.stdout, /claude|codex|pi /, 'no command is printed to run by hand')
  })

  it('cf catchup is retired and names its replacements', async () => {
    const result = await cf(['catchup', conversation], s.lead, s.workspace)
    assert.equal(result.code, 1, result.stdout)
    assert.match(result.stderr, /retired/)
    assert.match(result.stderr, /cf results/)
    assert.match(result.stderr, /cf read/)
  })
})

describe('a first turn that captures no session fails where it can be seen', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  const firstTurn = async (binDir) => {
    const opened = await cf(['run', '@ilmarinen', 'a question', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const open = s.seen.open.at(-1)
    return {
      conversation: line[1],
      result: await cf(
        open.argv.slice(2),
        {
          ...s.t.env,
          ...open.env,
          PATH: [binDir, s.t.env.PATH].join(delimiter),
        },
        s.workspace,
      ),
    }
  }

  it('says so when the harness is not installed at all', async () => {
    const binDir = join(s.t.root, 'kimi-absent-bin')
    mkdirSync(binDir, { recursive: true })
    const { result } = await firstTurn(binDir)
    assert.equal(result.code, 1, `a pane with no window must not exit 0: ${result.stdout}`)
    assert.match(result.stderr, /no session/i)
  })

  it('says so when the harness runs and fails', async () => {
    const binDir = join(s.t.root, 'kimi-failing-bin')
    fakeHarness(binDir, 'kimi', 'process.exit(3)\n')
    const { result } = await firstTurn(binDir)
    assert.equal(result.code, 1, `a failed first turn must not exit 0: ${result.stdout}`)
    assert.match(result.stderr, /no session/i)
  })
})

describe('a binding that fails decides nothing about the exit status', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  /**
   * A codex whose rollout DOES carry our seed — so discovery finds it — but
   * under somebody else's marker, so the binding must refuse. `lingering`
   * decides the race: a window that stays up is found while it runs, one
   * that exits at once is found in the look afterwards.
   */
  const wrongMarker = async (label, { lingering }) => {
    const binDir = join(s.t.root, `codex-${label}-bin`)
    const uuid = `aaaaaaaa-bbbb-4ccc-8ddd-${label.padEnd(12, 'f').slice(0, 12)}`
    fakeHarness(
      binDir,
      'codex',
      `
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
const dir = join(${JSON.stringify(s.t.env.CODEX_HOME)}, 'sessions')
mkdirSync(dir, { recursive: true })
const seed = process.argv.slice(2).at(-1)
const meta = { type: 'session_meta', payload: { cwd: process.cwd(), timestamp: new Date().toISOString(), cli_version: '0.153.4' } }
const turn = { type: 'response_item', payload: { id: 'msg_seed', type: 'message', role: 'user', content: [{ type: 'input_text', text: '[consensflow launch a-different-launch]\\n' + seed }] } }
writeFileSync(join(dir, 'rollout-2026-09-07T00-00-00-${uuid}.jsonl'), [meta, turn].map((r) => JSON.stringify(r)).join('\\n') + '\\n')
${lingering ? 'await new Promise((resolve) => setTimeout(resolve, 1500))' : ''}
`,
    )

    const opened = await cf(['run', '@zeus', `a question ${label}`, '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\) — pane (\S+)$/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const open = s.seen.open.at(-1)
    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    return { result, conversation: line[1] }
  }

  it('exits on the window’s outcome whether discovery wins the race or loses it', async () => {
    const whileUp = await wrongMarker('whileup', { lingering: true })
    const afterExit = await wrongMarker('after', { lingering: false })

    for (const [label, { result, conversation }] of [
      ['discovery while the window is up', whileUp],
      ['discovery after it exits', afterExit],
    ]) {
      assert.equal(result.code, 0, `${label}: the window ran, so the pane exits on the window`)
      assert.match(
        result.stderr,
        /carries no launch marker/,
        `${label}: the refusal is reported, and only on stderr`,
      )
      assert.equal(s.threads()[conversation].sessionId ?? null, null, `${label}: still unbound`)
    }
  })
})

describe('a pi worker carries the delivery extension', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  it('puts the real launch configuration on pi’s command line and in its env', async () => {
    const binDir = join(s.t.root, 'pi-extension-bin')
    // A stand-in pi that reports what it was actually given.
    fakeHarness(
      binDir,
      'pi',
      `
import { writeFileSync } from 'node:fs'
writeFileSync(${JSON.stringify(join(s.t.root, 'pi-env.json'))}, JSON.stringify(process.env))
`,
    )
    const opened = await cf(['run', '@clio', 'a question', '--new'], s.lead, s.workspace)
    const line = /^conversation: (\S+) \(new\)/m.exec(opened.stdout)
    assert.ok(line, `${opened.stdout}${opened.stderr}`)
    const open = s.seen.open.at(-1)
    const result = await cf(
      open.argv.slice(2),
      {
        ...s.t.env,
        ...open.env,
        PATH: [binDir, s.t.env.PATH].join(delimiter),
      },
      s.workspace,
    )
    assert.equal(result.code, 0, `${result.stdout}${result.stderr}`)

    // The module is the authority: what it computes for this launch and this
    // workspace is what the pane must have handed pi.
    const expected = await launchConfiguration('pi', {
      launchId: open.launch,
      workspace: realpathSync(s.workspace),
    })
    const argv = readFileSync(join(binDir, 'pi.argv'), 'utf8')
      .trimEnd()
      .split('\n')
      .map(JSON.parse)
      .at(-1)
    assert.deepEqual(argv.slice(0, expected.args.length), expected.args)
    assert.match(argv[argv.indexOf('--extension') + 1], /consensflow-delivery\.mjs$/)
    assert.equal(
      argv[argv.indexOf('--session-id') + 1],
      s.threads()[line[1]].sessionId,
      'the extension rides alongside the session pi was told to open',
    )

    const childEnvironment = JSON.parse(readFileSync(join(s.t.root, 'pi-env.json'), 'utf8'))
    for (const [key, value] of Object.entries(expected.env)) {
      assert.equal(childEnvironment[key], value, `${key} must reach the pi process`)
    }
  })
})

describe('a controller refusal is a warning, never a crash', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  /** Launch a worker whose every `session.bind` the app turns down. */
  const refusedBind = async (agent, binDir, body) => {
    const proxy = await recordingProxy(s.url, { refuse: ['session.bind'] })
    try {
      fakeHarness(binDir, body.command, body.script)
      const opened = await cf(['run', `@${agent}`, 'a question', '--new'], s.lead, s.workspace)
      const line = /^conversation: (\S+) \(new\)/m.exec(opened.stdout)
      assert.ok(line, `${opened.stdout}${opened.stderr}`)
      const open = s.seen.open.at(-1)
      const result = await cf(
        open.argv.slice(2),
        {
          ...s.t.env,
          ...open.env,
          CONSENSFLOW_APP: proxy.url,
          PATH: [binDir, s.t.env.PATH].join(delimiter),
        },
        s.workspace,
      )
      return { result, conversation: line[1], proxy }
    } finally {
      await proxy.close()
    }
  }

  it('kimi still gets its window when the app turns the binding down', async () => {
    const { result, conversation } = await refusedBind(
      'ilmarinen',
      join(s.t.root, 'kimi-409-bin'),
      {
        command: 'kimi',
        script: kimiWireBody(s.t.env, 'session_refused_0001'),
      },
    )
    assert.match(
      result.stdout,
      new RegExp(`${conversation} · @ilmarinen — handing this terminal to kimi`),
      'the human keeps their window even when the binding is refused',
    )
    assert.equal(result.code, 0, `the window ran, so the pane exits on it: ${result.stderr}`)
    assert.match(result.stderr, /refus|elsewhere|another pane/i)
    assert.doesNotMatch(result.stderr, /AppRefused|at Object|node:internal/, 'no stack trace')
  })

  it('a lingering codex window survives a refused binding', async () => {
    const uuid = 'aaaaaaaa-bbbb-4ccc-8ddd-409409409409'
    const { result } = await refusedBind('zeus', join(s.t.root, 'codex-409-bin'), {
      command: 'codex',
      script: `
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
const dir = join(${JSON.stringify(s.t.env.CODEX_HOME)}, 'sessions')
mkdirSync(dir, { recursive: true })
const seed = process.argv.slice(2).at(-1)
const meta = { type: 'session_meta', payload: { cwd: process.cwd(), timestamp: new Date().toISOString(), cli_version: '0.153.4' } }
const turn = { type: 'response_item', payload: { id: 'msg_seed', type: 'message', role: 'user', content: [{ type: 'input_text', text: seed }] } }
writeFileSync(join(dir, 'rollout-2026-09-07T00-00-00-${uuid}.jsonl'), [meta, turn].map((r) => JSON.stringify(r)).join('\\n') + '\\n')
await new Promise((resolve) => setTimeout(resolve, 1200))
`,
    })
    assert.match(result.stdout, /handing this terminal to codex/)
    assert.equal(result.code, 0, `the window ran, so the pane exits on it: ${result.stderr}`)
    assert.match(result.stderr, /refus|elsewhere|another pane/i)
    assert.doesNotMatch(result.stderr, /AppRefused|at Object|node:internal/, 'no stack trace')
  })

  it('claude keeps its window when the preallocated binding is refused', async () => {
    const { result, conversation } = await refusedBind('nyx', join(s.t.root, 'claude-409-bin'), {
      command: 'claude',
      script: '',
    })
    assert.match(
      result.stdout,
      new RegExp(`${conversation} · @nyx — handing this terminal to claude`),
    )
    assert.equal(result.code, 0, result.stderr)
    assert.match(result.stderr, /refus|elsewhere|another pane/i)
    assert.doesNotMatch(result.stderr, /AppRefused|at Object|node:internal/, 'no stack trace')
  })
})

describe('a harness that is not installed is a refusal, not a stack', () => {
  let s

  before(async () => {
    s = await paneServer()
  })
  after(async () => {
    await s.close()
  })

  for (const [agent, binary] of [
    ['zeus', 'codex'],
    ['gefjon', 'opencode'],
  ]) {
    it(`names ${binary} when it cannot be started`, async () => {
      const binDir = join(s.t.root, `${binary}-absent-bin`)
      mkdirSync(binDir, { recursive: true })
      const opened = await cf(['run', `@${agent}`, 'a question', '--new'], s.lead, s.workspace)
      assert.match(opened.stdout, /conversation:/, opened.stderr)
      const open = s.seen.open.at(-1)
      const result = await cf(
        open.argv.slice(2),
        {
          ...s.t.env,
          ...open.env,
          PATH: binDir,
        },
        s.workspace,
      )

      assert.notEqual(result.code, 0, 'a pane with no window does not succeed')
      assert.match(result.stderr, new RegExp(`cf: .*${binary}`))
      assert.doesNotMatch(result.stderr, /at ChildProcess|node:internal|Error: spawn/, 'no stack')
    })
  }
})

describe('the shim runs ConsensFlow’s own runtime or nothing', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('refuses an unset CONSENSFLOW_NODE and never runs a node off PATH', async () => {
    const fakePath = join(t.root, 'fake-path')
    mkdirSync(fakePath, { recursive: true })
    const marker = join(t.root, 'node-was-run')
    writeFileSync(join(fakePath, 'node'), `#!/bin/sh\ntouch ${JSON.stringify(marker)}\nexit 0\n`)
    chmodSync(join(fakePath, 'node'), 0o755)

    const result = await cf2('cf', ['--version'], {
      PATH: [BUNDLE_BIN, fakePath].join(delimiter),
    })
    assert.equal(result.code, 1)
    assert.match(result.stderr, /CONSENSFLOW_NODE/)
    assert.equal(
      existsSync(marker),
      false,
      'a node found on PATH is exactly what the shim exists to not run',
    )
  })
})

describe('cf read prints one part of a delivery, verbatim', () => {
  const t = tempEnv()
  let app
  after(async () => {
    await app?.close()
    t.cleanup()
  })

  it('asks for the part it was told and adds nothing to what comes back', async () => {
    const part = {
      k: 2,
      of: 3,
      text: '[part 2 of 3 — 11 bytes]\nhello there\n[end part 2 of 3]\nnext: cf read d-7 --part 3\n',
    }
    app = await stubApp({ '/api/panes/read': { status: 200, body: part } })
    const result = await cf(
      ['read', 'd-7', '--part', '2'],
      {
        ...t.env,
        CONSENSFLOW_APP: app.url,
        CONSENSFLOW_APP_TOKEN: 'lead-token',
        CONSENSFLOW_TAB: 'tab-1',
        CONSENSFLOW_LEAD_ID: 'tab:1:1',
        CONSENSFLOW_PANE_ID: 'p-1',
      },
      t.root,
    )

    assert.equal(result.code, 0, result.stderr)
    assert.equal(result.stdout, part.text, 'a part is printed exactly as the app framed it')
    const call = app.seen.at(-1)
    assert.equal(call.path, '/api/panes/read')
    assert.equal(call.authorization, 'Bearer lead-token')
    assert.equal(call.body.tab, 'tab-1')
    assert.equal(call.body.deliveryId, 'd-7')
    assert.equal(call.body.part, 2)
    assert.equal(typeof call.body.opId, 'string')
  })
})

describe('cf read takes a part number, not something that starts like one', () => {
  const t = tempEnv()
  let app
  const env = () => ({
    ...t.env,
    CONSENSFLOW_APP: app.url,
    CONSENSFLOW_APP_TOKEN: 'lead-token',
    CONSENSFLOW_TAB: 'tab-1',
    CONSENSFLOW_LEAD_ID: 'tab:1:1',
    CONSENSFLOW_PANE_ID: 'p-1',
  })

  before(async () => {
    app = await stubApp({ '/api/panes/read': { status: 200, body: { text: 'never reached' } } })
  })
  after(async () => {
    await app?.close()
    t.cleanup()
  })

  for (const bad of ['2garbage', '1e3', '0x2', ' 2', '2.0', '-1', '0', '', '9007199254740993']) {
    it(`refuses --part ${JSON.stringify(bad)} rather than reading a different part`, async () => {
      const before = app.seen.length
      const result = await cf(['read', 'd-7', '--part', bad], env(), t.root)
      assert.equal(result.code, 1, `--part ${JSON.stringify(bad)} was accepted`)
      assert.match(result.stderr, /part/)
      assert.equal(app.seen.length, before, 'a refused part number asks the app for nothing')
    })
  }

  it('still takes a plain whole number', async () => {
    const result = await cf(['read', 'd-7', '--part', '12'], env(), t.root)
    assert.equal(result.code, 0, result.stderr)
    assert.equal(app.seen.at(-1).body.part, 12)
  })
})
