import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { once } from 'node:events'
import { existsSync, readdirSync } from 'node:fs'
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises'
import { createServer as createNetServer } from 'node:net'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import WebSocket, { WebSocketServer } from 'ws'
import * as codexSession from '../hosts/codex-session.mjs'
import {
  codexProcessArguments,
  consensflowShellEnvironment,
  startBroker,
} from '../hosts/codex-session.mjs'
import { fakeNodeExecutable } from './helpers.mjs'

const A = '01a09094-938f-7fd1-a2d3-315cf92b4559'
const B = '01a09094-a559-7db0-bf50-e2309856c3c0'
const TOKEN = 'private-launch-token-1234567890'

async function fixture(t, options = {}) {
  const upstream = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  await once(upstream, 'listening')
  const requests = []
  const pending = []
  const sockets = []
  upstream.on('connection', (socket) => {
    sockets.push(socket)
    socket.on('message', (raw) => {
      const message = JSON.parse(raw)
      requests.push(message)
      if (message.method === 'initialize')
        socket.send(JSON.stringify({ id: message.id, result: {} }))
      else if (message.id !== undefined) pending.push({ socket, message })
    })
  })
  const native = `ws://127.0.0.1:${upstream.address().port}`
  const broker = await startBroker({
    ...options,
    port: 0,
    token: TOKEN,
    launchId: 'launch-1',
    upstream: native,
  })
  const clients = []
  t.after(async () => {
    for (const client of clients) client.terminate()
    await broker.close()
    for (const client of upstream.clients) client.terminate()
    await new Promise((resolve) => upstream.close(resolve))
  })
  const wait = async (predicate) => {
    for (let i = 0; i < 200; i++) {
      if (predicate()) return
      await new Promise((resolve) => setTimeout(resolve, 5))
    }
    assert.fail('native boundary did not complete')
  }
  const connect = async () => {
    const socket = new WebSocket(broker.endpoint.replace('http:', 'ws:'), {
      headers: { authorization: `Bearer ${TOKEN}` },
    })
    clients.push(socket)
    await once(socket, 'open')
    socket.send(
      JSON.stringify({
        id: 'init',
        method: 'initialize',
        params: { clientInfo: { name: 'codex-tui', version: 'test' } },
      }),
    )
    await once(socket, 'message')
    return socket
  }
  const read = async () =>
    (
      await fetch(`${broker.endpoint}/session`, { headers: { authorization: `Bearer ${TOKEN}` } })
    ).json()
  const deliver = async (sessionId, overrides = {}) =>
    (
      await fetch(`${broker.endpoint}/deliver`, {
        method: 'POST',
        headers: { authorization: `Bearer ${TOKEN}`, 'content-type': 'application/json' },
        body: JSON.stringify({
          launchId: 'launch-1',
          sessionId,
          text: 'complete\nworker result',
          expiresAt: Date.now() + 2000,
          ...overrides,
        }),
      })
    ).json()
  const respond = async (method, result, error) => {
    await wait(() => pending.some((p) => p.message.method === method))
    const index = pending.findIndex((p) => p.message.method === method)
    const entry = pending.splice(index, 1)[0]
    entry.socket.send(JSON.stringify({ id: entry.message.id, ...(error ? { error } : { result }) }))
    return entry.message
  }
  return { broker, native, requests, pending, sockets, wait, connect, read, deliver, respond }
}

/**
 * The TUI starts its main thread through the broker, and Codex answers it.
 * The broker takes the thread before it passes the answer on, so the TUI
 * holding the answer means the broker has it.
 */
async function startThread(f, tui, id, thread) {
  const answered = new Promise((resolve) => {
    const seen = (raw) => {
      if (JSON.parse(raw).id !== id) return
      tui.off('message', seen)
      resolve()
    }
    tui.on('message', seen)
  })
  tui.send(
    JSON.stringify({
      id,
      method: 'thread/start',
      params: { ephemeral: false, threadSource: 'user' },
    }),
  )
  await f.respond('thread/start', { thread })
  await answered
}

it('follows successful main new/resume while ignoring title threads, child focus and picker connections', async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  assert.equal((await f.read()).sessionId, null)
  tui.send(
    JSON.stringify({
      id: 1,
      method: 'thread/start',
      params: { ephemeral: false, threadSource: 'user' },
    }),
  )
  await f.respond('thread/start', { thread: { id: A } })
  await f.wait(() => f.requests.some((r) => r.id === 1))
  assert.equal((await f.read()).sessionId, A)
  const picker = await f.connect()
  picker.close()
  assert.equal((await f.read()).sessionId, A)
  tui.send(
    JSON.stringify({
      id: 2,
      method: 'thread/start',
      params: { ephemeral: true, threadSource: 'system' },
    }),
  )
  await f.respond('thread/start', { thread: { id: B } })
  assert.equal((await f.read()).sessionId, A)
  tui.send(JSON.stringify({ id: 3, method: 'thread/resume', params: { threadId: B } }))
  await f.respond('thread/resume', { thread: { id: B } })
  assert.equal((await f.read()).sessionId, A)
  tui.send(
    JSON.stringify({
      id: 4,
      method: 'thread/resume',
      params: { threadId: B, runtimeWorkspaceRoots: [] },
    }),
  )
  await f.wait(() => f.pending.some((p) => p.message.id === 4))
  assert.equal((await f.read()).sessionId, null)
  assert.deepEqual(await f.deliver(A), {
    ok: false,
    admitted: false,
    bytesWritten: 0,
    error: 'native-session-unavailable',
  })
  await f.respond('thread/resume', { thread: { id: B } })
  assert.equal((await f.read()).sessionId, B)
  assert.equal((await f.deliver(A)).error, 'native-session-changed')
  assert.equal(f.requests.filter((r) => r.method === 'thread/queue/add').length, 0)
  const delivery = f.deliver(B)
  const queued = await f.respond('thread/queue/add', { queuedMessage: { id: 'queue-1' } })
  assert.equal(queued.params.threadId, B)
  assert.equal(queued.params.input[0].text, 'complete\nworker result')
  assert.deepEqual(await delivery, { ok: true, admitted: true })
})

it('a window opened on its thread is named by its resume, whose roots Codex 0.159 sends as null', async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  // `codex resume <thread> <message>`, as Codex 0.159.2 sends it (traced 2026-10-03).
  const resume = {
    threadId: B,
    history: null,
    path: null,
    model: 'gpt-5.6-luna',
    modelProvider: null,
    serviceTier: 'default',
    cwd: null,
    runtimeWorkspaceRoots: null,
    approvalPolicy: null,
    approvalsReviewer: null,
    sandbox: null,
    permissions: null,
    config: { model_reasoning_effort: 'low' },
    baseInstructions: null,
    developerInstructions: null,
    personality: null,
    excludeTurns: true,
    initialTurnsPage: null,
  }
  tui.send(JSON.stringify({ id: 5, method: 'thread/resume', params: resume }))
  await f.wait(() => f.pending.some((p) => p.message.id === 5))
  assert.equal((await f.read()).available, false, 'nothing is taken while it resumes')
  await f.respond('thread/resume', { thread: { id: B, status: { type: 'idle' } } })
  assert.deepEqual([(await f.read()).sessionId, (await f.read()).available], [B, true])
  const delivery = f.deliver(B)
  const started = await f.respond('turn/start', { turn: { id: 'turn-1' } })
  assert.equal(started.params.threadId, B)
  assert.deepEqual(await delivery, { ok: true, admitted: true })
})

it('starts a turn with a delivery when the thread is idle, queues it only while a turn runs, and says when it can take one', async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  assert.equal((await f.read()).available, false, 'no thread yet')
  tui.send(
    JSON.stringify({
      id: 1,
      method: 'thread/start',
      params: { ephemeral: false, threadSource: 'user' },
    }),
  )
  await f.respond('thread/start', { thread: { id: A, status: { type: 'idle' } } })
  await f.wait(() => f.requests.some((r) => r.id === 1))
  assert.deepEqual([(await f.read()).sessionId, (await f.read()).available], [A, true])
  // Idle, as after an interrupt: the message starts the turn itself.
  const first = f.deliver(A)
  const started = await f.respond('turn/start', { turn: { id: 'turn-1' } })
  assert.deepEqual(
    [started.params.threadId, started.params.input[0].text],
    [A, 'complete\nworker result'],
  )
  assert.deepEqual(await first, { ok: true, admitted: true })
  // A turn runs: the next one waits in Codex's queue.
  const second = f.deliver(A)
  const queued = await f.respond('thread/queue/add', { queuedMessage: { id: 'queue-1' } })
  assert.equal(queued.params.threadId, A)
  assert.deepEqual(await second, { ok: true, admitted: true })
  // The turn ends (completed or interrupted): idle again, a turn again.
  f.sockets[0].send(JSON.stringify({ method: 'turn/completed', params: { threadId: A } }))
  await f.wait(() => true)
  await new Promise((resolve) => setTimeout(resolve, 20))
  const third = f.deliver(A)
  await f.respond('turn/start', { turn: { id: 'turn-2' } })
  assert.deepEqual(await third, { ok: true, admitted: true })
  // While the TUI switches threads, nothing can be taken.
  tui.send(
    JSON.stringify({
      id: 2,
      method: 'thread/resume',
      params: { threadId: B, runtimeWorkspaceRoots: [] },
    }),
  )
  await f.wait(() => f.pending.some((p) => p.message.id === 2))
  assert.equal((await f.read()).available, false)
})

it('promotes durable forks and preserves native permission changes after the initial launch', async (t) => {
  const f = await fixture(t, { freshBypass: true })
  const tui = await f.connect()
  const call = async (id, method, params, result) => {
    tui.send(JSON.stringify({ id, method, params }))
    const request = await f.respond(method, result)
    await f.read()
    return request.params
  }
  const start = {
    ephemeral: false,
    threadSource: 'user',
    approvalPolicy: 'on-request',
    sandbox: 'read-only',
    permissions: { profile: 'home' },
  }
  const initial = await call(1, 'thread/start', start, {
    thread: { id: A, turns: [], status: { type: 'idle' } },
  })
  assert.equal(initial.approvalPolicy, 'never')
  assert.equal(initial.sandbox, 'danger-full-access')
  assert.equal(initial.permissions, null)
  assert.equal((await f.read()).empty, true)
  await call(
    2,
    'thread/fork',
    { threadId: A, threadSource: 'user', runtimeWorkspaceRoots: [], ephemeral: true },
    { thread: { id: B } },
  )
  assert.equal((await f.read()).sessionId, A)
  await call(
    3,
    'thread/fork',
    { threadId: A, threadSource: 'user', runtimeWorkspaceRoots: [] },
    { thread: { id: B, turns: [{}] } },
  )
  assert.equal((await f.read()).sessionId, B)
  assert.equal((await f.read()).empty, false)
  await call(4, 'thread/settings/update', { threadId: B, approvalPolicy: 'on-request' }, {})
  assert.deepEqual(await call(5, 'thread/start', start, { thread: { id: A } }), start)
})

it('consumes native empty-thread proof at admission and never forces fresh permissions onto resume', async (t) => {
  const f = await fixture(t, { freshBypass: true })
  const tui = await f.connect()
  tui.send(
    JSON.stringify({
      id: 1,
      method: 'thread/start',
      params: { ephemeral: false, threadSource: 'user' },
    }),
  )
  await f.respond('thread/start', { thread: { id: A, turns: [], status: { type: 'idle' } } })
  assert.equal((await f.read()).empty, true)
  const delivery = f.deliver(A)
  // An idle thread takes the delivery as its turn.
  await f.respond('turn/start', {})
  await delivery
  assert.equal((await f.read()).empty, false)
  const resume = { threadId: B, runtimeWorkspaceRoots: [], approvalPolicy: null, sandbox: null }
  tui.send(JSON.stringify({ id: 2, method: 'thread/resume', params: resume }))
  assert.deepEqual((await f.respond('thread/resume', { thread: { id: B } })).params, resume)
  await f.read()
  const start = { ephemeral: false, threadSource: 'user', sandbox: 'read-only' }
  tui.send(JSON.stringify({ id: 3, method: 'thread/start', params: start }))
  assert.deepEqual((await f.respond('thread/start', { thread: { id: A } })).params, start)
})

it('restores a rejected switch, rejects invalid ingress, and reports a possible write as uncertain', async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  tui.send(
    JSON.stringify({
      id: 1,
      method: 'thread/start',
      params: { ephemeral: false, threadSource: 'user' },
    }),
  )
  await f.respond('thread/start', { thread: { id: A } })
  tui.send(
    JSON.stringify({
      id: 2,
      method: 'thread/resume',
      params: { threadId: B, runtimeWorkspaceRoots: [] },
    }),
  )
  await f.respond('thread/resume', null, { code: -1, message: 'not found' })
  assert.equal((await f.read()).sessionId, A)
  assert.equal((await fetch(`${f.broker.endpoint}/session`)).status, 401)
  // A window's connection without the launch's token, or to another path, never opens.
  const address = f.broker.endpoint.replace('http:', 'ws:')
  for (const [url, headers] of [
    [address, {}],
    [`${address}/other`, { authorization: `Bearer ${TOKEN}` }],
  ]) {
    const stranger = new WebSocket(url, { headers })
    const outcome = await new Promise((resolve) => {
      stranger.on('open', () => resolve('opened'))
      stranger.on('error', () => {})
      stranger.on('close', () => resolve('turned away'))
    })
    stranger.terminate()
    assert.equal(outcome, 'turned away', url)
  }
  const garbled = await fetch(`${f.broker.endpoint}/deliver`, {
    method: 'POST',
    headers: { authorization: `Bearer ${TOKEN}`, 'content-type': 'application/json' },
    body: '{"launchId":',
  })
  assert.deepEqual(
    [garbled.status, await garbled.json()],
    [400, { ok: false, admitted: false, bytesWritten: 0, error: 'invalid-record' }],
  )
  assert.equal((await f.deliver(A, { launchId: 'foreign' })).error, 'invalid-record')
  assert.equal((await f.deliver(A, { expiresAt: Date.now() - 1 })).error, 'expired')
  const delivery = f.deliver(A)
  await f.wait(() => f.pending.some((p) => p.message.method === 'thread/queue/add'))
  f.pending.find((p) => p.message.method === 'thread/queue/add').socket.terminate()
  assert.deepEqual(await delivery, { ok: false, admitted: null, error: 'uncertain' })
  tui.terminate()
  await new Promise((resolve) => setTimeout(resolve, 20))
  assert.equal((await f.read()).sessionId, null)
})

it("a turn the human starts in the window makes the next delivery wait in Codex's queue", async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  await startThread(f, tui, 1, { id: A, turns: [], status: { type: 'idle' } })
  assert.equal((await f.read()).empty, true)
  tui.send(JSON.stringify({ id: 2, method: 'turn/start', params: { threadId: A, input: [] } }))
  await f.wait(() => f.pending.some((p) => p.message.id === 2))
  assert.equal((await f.read()).empty, false)
  const delivery = f.deliver(A)
  const queued = await f.respond('thread/queue/add', { queuedMessage: { id: 'queue-1' } })
  assert.equal(queued.params.input[0].text, 'complete\nworker result')
  assert.deepEqual(await delivery, { ok: true, admitted: true })
  assert.deepEqual(
    f.requests.filter((r) => r.method === 'turn/start').map((r) => r.id),
    [2],
    'the only turn is the one the human started',
  )
})

it('a delivery whose turn Codex refuses, a turn having just started, waits in the queue instead', async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  await startThread(f, tui, 1, { id: A, status: { type: 'idle' } })
  const delivery = f.deliver(A)
  await f.respond('turn/start', null, { code: -32600, message: 'a turn is already running' })
  const queued = await f.respond('thread/queue/add', { queuedMessage: { id: 'queue-1' } })
  assert.deepEqual(
    [queued.params.threadId, queued.params.input[0].text],
    [A, 'complete\nworker result'],
  )
  assert.deepEqual(await delivery, { ok: true, admitted: true })
})

it("a request of Codex's own that shares an id with the TUI's thread switch is not its answer", async (t) => {
  // The server numbers the requests it sends the TUI on its own; one may
  // carry the id of a switch the TUI is waiting on.
  const f = await fixture(t)
  const tui = await f.connect()
  tui.send(
    JSON.stringify({
      id: 1,
      method: 'thread/start',
      params: { ephemeral: false, threadSource: 'user' },
    }),
  )
  await f.wait(() => f.pending.some((p) => p.message.method === 'thread/start'))
  const { socket } = f.pending.find((p) => p.message.method === 'thread/start')
  const asked = new Promise((resolve) =>
    tui.on('message', (raw) => {
      if (JSON.parse(raw).method === 'item/commandExecution/requestApproval') resolve()
    }),
  )
  socket.send(
    JSON.stringify({ id: 1, method: 'item/commandExecution/requestApproval', params: {} }),
  )
  await asked
  const answered = new Promise((resolve) =>
    tui.on('message', (raw) => {
      const message = JSON.parse(raw)
      if (message.id === 1 && message.result !== undefined) resolve()
    }),
  )
  await f.respond('thread/start', { thread: { id: A, status: { type: 'idle' }, turns: [] } })
  await answered
  assert.equal((await f.read()).sessionId, A)
})

it('a refused turn is never queued on a thread the window moved to meanwhile', async (t) => {
  // A delivery to idle thread A starts a turn; while Codex weighs it, the
  // window opens thread B. A's refusal must not put A's message in B's queue.
  const f = await fixture(t)
  const tui = await f.connect()
  await startThread(f, tui, 1, { id: A, status: { type: 'idle' } })
  const delivery = f.deliver(A)
  await f.wait(() => f.pending.some((p) => p.message.method === 'turn/start'))
  await startThread(f, tui, 2, { id: B, status: { type: 'idle' } })
  await f.respond('turn/start', null, { code: -32600, message: 'a turn is already running' })
  assert.deepEqual(await delivery, {
    ok: false,
    admitted: false,
    bytesWritten: 0,
    error: 'native-session-changed',
  })
  assert.equal(
    f.requests.some((m) => m.method === 'thread/queue/add'),
    false,
    'nothing was queued',
  )
})

it('a connection that is not speaking JSON is closed, and the thread it chose is forgotten', async (t) => {
  const f = await fixture(t)
  const tui = await f.connect()
  await startThread(f, tui, 1, { id: A })
  assert.equal((await f.read()).sessionId, A)
  const closed = once(tui, 'close')
  tui.send('not json')
  await closed
  assert.equal((await f.read()).sessionId, null)
  // The same from Codex's end of a window's connection.
  const next = await f.connect()
  await startThread(f, next, 2, { id: B })
  assert.equal((await f.read()).sessionId, B)
  const gone = once(next, 'close')
  f.sockets.at(-1).send('not json')
  await gone
  assert.equal((await f.read()).sessionId, null)
  // And from Codex's end of the broker's own: nothing can be delivered then.
  const window = await f.connect()
  await startThread(f, window, 3, { id: A })
  assert.equal((await f.read()).available, true)
  const control = f.sockets[0]
  control.send('not json')
  await once(control, 'close')
  assert.equal((await f.read()).available, false)
  assert.equal((await f.deliver(A)).error, 'native-session-unavailable')
})

it('does not start on a Codex server that will not initialize, nor on a port already taken', async (t) => {
  const refusing = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  await once(refusing, 'listening')
  t.after(() => new Promise((resolve) => refusing.close(resolve)))
  refusing.on('connection', (socket) =>
    socket.on('message', (raw) =>
      socket.send(JSON.stringify({ id: JSON.parse(raw).id, error: { message: 'not ready' } })),
    ),
  )
  const options = { port: 0, token: TOKEN, launchId: 'launch-1' }
  await assert.rejects(
    startBroker({ ...options, upstream: `ws://127.0.0.1:${refusing.address().port}` }),
    /^Error: Codex native server did not initialize$/,
  )
  const f = await fixture(t)
  const connections = f.sockets.length
  await assert.rejects(
    startBroker({ ...options, port: Number(new URL(f.broker.endpoint).port), upstream: f.native }),
    { code: 'EADDRINUSE' },
  )
  // Neither leaves its own connection to Codex's server open.
  await f.wait(
    () => refusing.clients.size === 0 && f.sockets[connections]?.readyState === WebSocket.CLOSED,
  )
})

it("sets ConsensFlow's own variables in Codex's shell policy, so a user policy that inherits only core ones keeps them", () => {
  assert.deepEqual(
    consensflowShellEnvironment({
      CONSENSFLOW_URL: 'http://127.0.0.1:4100',
      CONSENSFLOW_TOKEN: 'tok"en',
      CF_CODEX_TUI_TOKEN: 'internal',
      HOME: '/home/user',
    }),
    [
      '-c',
      'shell_environment_policy.set.CONSENSFLOW_TOKEN="tok\\"en"',
      '-c',
      'shell_environment_policy.set.CONSENSFLOW_URL="http://127.0.0.1:4100"',
    ],
  )
  assert.deepEqual(consensflowShellEnvironment({ HOME: '/home/user' }), [])
})

it('keeps native TUI arguments while explicitly forwarding backend model, effort and full role configuration', () => {
  const role = 'developer_instructions="existing instructions\\ncomplete chief role"'
  const args = [
    '-c',
    role,
    '--model',
    'native-model',
    '-c',
    'model_reasoning_effort="high"',
    '--dangerously-bypass-approvals-and-sandbox',
    'real worker task',
  ]
  const split = codexProcessArguments(args)
  assert.deepEqual(split.backend, [
    '-c',
    role,
    '-c',
    'model="native-model"',
    '-c',
    'model_reasoning_effort="high"',
    '-c',
    'approval_policy="never"',
    '-c',
    'sandbox_mode="danger-full-access"',
  ])
  assert.deepEqual(
    split.tui,
    args.filter((arg) => arg !== '--dangerously-bypass-approvals-and-sandbox'),
  )
  assert.deepEqual(codexProcessArguments(['-c', role, 'resume', A]), {
    backend: ['-c', role],
    tui: ['-c', role, 'resume', A],
  })
})

it('launches the bundled supervisor with the Codex invocation it supervises', async () => {
  const { withNativeBridge } = await import('../src/channels.js')
  const invocation = {
    command: '/native/codex',
    args: ['resume', A],
    env: { EXISTING: 'preserved' },
    dropEnv: ['OPENAI_API_KEY'],
  }
  const configured = {
    channel: {
      kind: 'codex-queue',
      executable: '/native/codex',
      sessionBridge: { endpoint: 'http://127.0.0.1:1234', token: TOKEN },
    },
  }
  const wrapped = withNativeBridge(invocation, configured, process.execPath)
  assert.equal(wrapped.command, process.execPath)
  assert.match(wrapped.args[0].replaceAll('\\', '/'), /hosts\/codex-session\.mjs$/)
  assert.deepEqual(wrapped.args.slice(1), ['/native/codex', 'resume', A])
  assert.deepEqual(wrapped.env, invocation.env)
  assert.deepEqual(wrapped.dropEnv, invocation.dropEnv)
})

it('keeps native Codex sockets private and inside ConsensFlow home', {
  skip: process.platform === 'win32' && 'Unix sockets only',
}, async (t) => {
  const root = await mkdtemp('/tmp/cf-socket-home-')
  t.after(() => rm(root, { recursive: true, force: true }))
  const home = join(root, '.consensflow')
  const directory = await codexSession.createSocketDirectory({ CONSENSFLOW_HOME: home })
  assert.ok(directory.startsWith(`${join(home, 'tmp')}/`))
  assert.equal((await stat(directory)).mode & 0o777, 0o700)
  assert.ok(Buffer.byteLength(join(directory, 'native.sock')) < 104)
  // A home too deep for a Unix socket (a temporary tree) falls back to the
  // user's own temporary directory, still private; nowhere short and it fails.
  const deep = join(root, 'x'.repeat(110))
  const fallback = await codexSession.createSocketDirectory({
    CONSENSFLOW_HOME: deep,
    TMPDIR: root,
  })
  assert.ok(fallback.startsWith(`${join(root, 'consensflow')}/`))
  assert.equal((await stat(fallback)).mode & 0o777, 0o700)
  assert.ok(Buffer.byteLength(join(fallback, 'native.sock')) < 104)
  await assert.rejects(
    codexSession.createSocketDirectory({ CONSENSFLOW_HOME: deep, TMPDIR: deep }),
    /socket path.*too long/i,
  )
})

it("listens, on Windows, on loopback for the window's own token, and is up once it says where", async () => {
  const server = await codexSession.serverEndpoint('win32')
  assert.equal(server.directory, null, 'no Unix socket folder')
  const [hash] = server.listen.slice(-1)
  assert.deepEqual(server.listen, [
    '--listen',
    'ws://127.0.0.1:0',
    '--ws-auth',
    'capability-token',
    '--ws-token-sha256',
    hash,
  ])
  const token = server.headers.authorization.replace(/^Bearer /, '')
  assert.equal(createHash('sha256').update(token).digest('hex'), hash)
  assert.equal(
    await server.upstream('codex app-server (WebSockets)\n'),
    null,
    'not before it says where',
  )
  assert.equal(
    await server.upstream('codex app-server (WebSockets)\n  listening on: ws://127.0.0.1:53111\n'),
    'ws://127.0.0.1:53111',
  )
  const other = await codexSession.serverEndpoint('win32')
  assert.notEqual(other.headers.authorization, server.headers.authorization, 'each window its own')
})

/** A stand-in for the board's API: the questions posted, the answer once the test gives it. */
async function fakeBoard(t) {
  const { createServer } = await import('node:http')
  const state = { posted: [], answer: null, refuse: null }
  const server = createServer(async (request, response) => {
    const chunks = []
    for await (const chunk of request) chunks.push(chunk)
    const json = (status, value) => {
      response.writeHead(status, { 'content-type': 'application/json' })
      response.end(JSON.stringify(value))
    }
    if (request.method === 'POST' && request.url === '/api/questions') {
      state.posted.push(JSON.parse(Buffer.concat(chunks).toString('utf8')))
      if (state.refuse) return json(400, { error: 'bad-questions', message: state.refuse })
      return json(201, { message: { id: 61 } })
    }
    if (request.method === 'GET' && request.url.startsWith('/api/questions/61')) {
      for (let i = 0; i < 40 && state.answer === null; i++)
        await new Promise((r) => setTimeout(r, 25))
      return json(200, { question: {}, answer: state.answer })
    }
    json(404, {})
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => new Promise((resolve) => server.close(resolve)))
  return { state, url: `http://127.0.0.1:${server.address().port}` }
}

const REQUEST_USER_INPUT = {
  id: 'ask-1',
  method: 'item/tool/requestUserInput',
  params: {
    threadId: A,
    turnId: 'turn-1',
    itemId: 'item-1',
    isBlocking: true,
    questions: [
      {
        id: 'colour',
        header: 'Colour',
        question: 'Which colour?',
        options: [
          { label: 'red', description: 'Warm' },
          { label: 'blue', description: 'Cool' },
        ],
      },
    ],
  },
}

it("Codex's question tool is answered from the board by the broker, and the TUI never sees it", async (t) => {
  const board = await fakeBoard(t)
  const f = await fixture(t, { board: { url: board.url, token: 'window-token' } })
  const tui = await f.connect()
  const seen = []
  tui.on('message', (raw) => seen.push(JSON.parse(raw)))
  f.sockets.at(-1).send(JSON.stringify(REQUEST_USER_INPUT))
  await f.wait(() => board.state.posted.length === 1)
  assert.deepEqual(board.state.posted[0].questions, [
    {
      question: 'Which colour?',
      header: 'Colour',
      options: [
        { label: 'red', description: 'Warm' },
        { label: 'blue', description: 'Cool' },
      ],
      multiple: false,
    },
  ])
  board.state.answer = { id: 70, from: 'chief', body: 'Colour: blue', choices: [['blue']] }
  await f.wait(() => f.requests.some((m) => m.id === 'ask-1'))
  assert.deepEqual(
    f.requests.find((m) => m.id === 'ask-1'),
    { id: 'ask-1', result: { answers: { colour: { answers: ['blue'] } } } },
  )
  assert.deepEqual(
    seen.filter((m) => m.method === 'item/tool/requestUserInput'),
    [],
    'the window showed no dialog',
  )
})

it("Codex's question the board refuses is answered with the reason, never left to a dialog nobody sees", async (t) => {
  const board = await fakeBoard(t)
  board.state.refuse = 'questions: one to 4 questions'
  const f = await fixture(t, { board: { url: board.url, token: 'window-token' } })
  const tui = await f.connect()
  const seen = []
  tui.on('message', (raw) => seen.push(JSON.parse(raw)))
  f.sockets.at(-1).send(JSON.stringify(REQUEST_USER_INPUT))
  await f.wait(() => f.requests.some((m) => m.id === 'ask-1'))
  assert.deepEqual(
    f.requests.find((m) => m.id === 'ask-1'),
    {
      id: 'ask-1',
      result: {
        answers: {
          colour: {
            answers: [
              'ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask "…" instead.',
            ],
          },
        },
      },
    },
  )
  assert.deepEqual(
    seen.filter((m) => m.method === 'item/tool/requestUserInput'),
    [],
  )
})

it("Codex's question goes on to the TUI when the board does not answer in time, or there is no board", async (t) => {
  const board = await fakeBoard(t)
  const f = await fixture(t, {
    board: { url: board.url, token: 'window-token' },
    questionWaitMs: 50,
  })
  const tui = await f.connect()
  const seen = []
  tui.on('message', (raw) => seen.push(JSON.parse(raw)))
  f.sockets.at(-1).send(JSON.stringify(REQUEST_USER_INPUT))
  await f.wait(() => seen.some((m) => m.id === 'ask-1'))
  assert.equal(seen.find((m) => m.id === 'ask-1').method, 'item/tool/requestUserInput')
  assert.equal(board.state.posted.length, 1, 'it was asked on the board first')

  const plain = await fixture(t)
  const plainTui = await plain.connect()
  const shown = []
  plainTui.on('message', (raw) => shown.push(JSON.parse(raw)))
  plain.sockets.at(-1).send(JSON.stringify(REQUEST_USER_INPUT))
  await plain.wait(() => shown.some((m) => m.id === 'ask-1'))
})

const SUPERVISOR = fileURLToPath(new URL('../hosts/codex-session.mjs', import.meta.url))
const REPO = fileURLToPath(new URL('..', import.meta.url))

/**
 * A stand-in `codex` for the supervisor: given `app-server --listen unix://…`
 * it is Codex's server on that socket, starting thread A when asked; given
 * `--remote …` it is the TUI, which starts a thread through the broker, reads
 * the broker's /session with its token, and exits 7 (or waits to be ended).
 * Each run writes what it was given, and what it saw, to CF_TEST_CODEX_LOG.
 * CF_TEST_CODEX_BACKEND says how the server goes: `fail` at once, `die` once
 * a thread starts.
 */
function fakeCodex(root) {
  return fakeNodeExecutable(
    join(root, 'codex'),
    `#!${process.execPath}
import { appendFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
const WebSocket = createRequire(${JSON.stringify(join(REPO, 'package.json'))})('ws')
const args = process.argv.slice(2)
const record = (entry) =>
  appendFileSync(process.env.CF_TEST_CODEX_LOG, JSON.stringify({ pid: process.pid, ...entry }) + '\\n')
const how = process.env.CF_TEST_CODEX_BACKEND
if (args.includes('app-server')) {
  record({ run: 'server', args, apiKey: process.env.OPENAI_API_KEY ?? null })
  if (how === 'fail') {
    process.stderr.write('not logged in\\n')
    process.exitCode = 1
  } else {
    const server = createServer()
    new WebSocket.WebSocketServer({ server }).on('connection', (socket) => {
      socket.on('message', (raw) => {
        const message = JSON.parse(raw)
        if (message.id === undefined) return
        const start = message.method === 'thread/start'
        if (start) record({ run: 'server', started: message.params })
        const result = start ? { thread: { id: ${JSON.stringify(A)}, turns: [], status: { type: 'idle' } } } : {}
        socket.send(JSON.stringify({ id: message.id, result }), () => {
          if (start && how === 'die') process.exit(0)
        })
      })
    })
    server.listen(args[args.indexOf('--listen') + 1].slice('unix://'.length))
  }
} else {
  const [, remote, , tokenName] = args
  const token = process.env[tokenName]
  record({ run: 'tui', args, token, apiKey: process.env.OPENAI_API_KEY ?? null })
  const socket = new WebSocket(remote, { headers: { authorization: 'Bearer ' + token } })
  socket.on('error', () => {})
  let last = 0
  const call = (method, params) =>
    new Promise((resolve) => {
      const id = ++last
      const answer = (raw) => {
        if (JSON.parse(raw).id !== id) return
        socket.off('message', answer)
        resolve()
      }
      socket.on('message', answer)
      socket.send(JSON.stringify({ id, method, params }))
    })
  socket.on('open', async () => {
    await call('initialize', { clientInfo: { name: 'codex-tui', version: 'test' } })
    await call('thread/start', { ephemeral: false, threadSource: 'user' })
    const session = await fetch(remote.replace('ws:', 'http:') + '/session', {
      headers: { authorization: 'Bearer ' + token },
    })
    record({ run: 'tui', session: await session.json() })
    if (process.env.CF_TEST_CODEX_TUI !== 'wait') process.exit(7)
  })
}
`,
  )
}

async function freePort() {
  const server = createNetServer()
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  await new Promise((resolve) => server.close(resolve))
  return port
}

const alive = (pid) => {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/**
 * The supervisor as a Codex window runs it: `node codex-session.mjs <codex> <args>`,
 * with the session bridge the channel configured, in a window's environment.
 */
async function supervised(t, args, extraEnv = {}, executable = fakeCodex) {
  const root = await mkdtemp(join(tmpdir(), 'cf-cx-'))
  let child = null
  let exited = null
  // A test that ends early closes the window as the app would: the
  // supervisor ends Codex's processes, then their files go.
  t.after(async () => {
    if (child !== null && child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM')
      const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
      await exited
      clearTimeout(stuck)
    }
    await rm(root, { recursive: true, force: true })
  })
  const codex = executable(root)
  const port = await freePort()
  const log = join(root, 'codex.jsonl')
  const inherited = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !name.startsWith('CONSENSFLOW_')),
  )
  child = spawn(process.execPath, [SUPERVISOR, codex, ...args], {
    env: {
      ...inherited,
      CONSENSFLOW_HOME: root,
      CONSENSFLOW_URL: 'http://127.0.0.1:1',
      CONSENSFLOW_TOKEN: 'window-token',
      CF_CODEX_SESSION_BRIDGE: JSON.stringify({ launchId: 'launch-1', port, token: TOKEN }),
      CF_TEST_CODEX_LOG: log,
      OPENAI_API_KEY: 'sk-not-for-codex',
      ...extraEnv,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  exited = once(child, 'exit')
  let stderr = ''
  child.stderr.on('data', (chunk) => {
    stderr += chunk
  })
  const runs = async () =>
    (await readFile(log, 'utf8').catch(() => ''))
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line))
  const until = async (predicate) => {
    for (let i = 0; i < 400; i++) {
      const seen = await runs()
      if (predicate(seen)) return seen
      await new Promise((resolve) => setTimeout(resolve, 25))
    }
    assert.fail(`the supervised Codex never got there: ${stderr}`)
  }
  return {
    root,
    port,
    child,
    runs,
    until,
    stderr: () => stderr,
    exit: async () => {
      const [code, signal] = await exited
      return { code, signal }
    },
  }
}

const UNIX_ONLY = { skip: process.platform === 'win32' && 'Unix sockets only' }
/** The private folder of the socket Codex's server was told to listen on. */
const socketFolder = (server) =>
  dirname(server.args[server.args.indexOf('--listen') + 1].slice('unix://'.length))

it(
  'opens Codex as its server on a private socket and its TUI through the broker, and leaves nothing behind',
  UNIX_ONLY,
  async (t) => {
    const s = await supervised(t, [
      '--model',
      'native-model',
      '--dangerously-bypass-approvals-and-sandbox',
    ])
    assert.deepEqual(await s.exit(), { code: 7, signal: null }, s.stderr())
    const runs = await s.runs()
    const serverRuns = runs.filter((entry) => entry.run === 'server')
    const tuiRuns = runs.filter((entry) => entry.run === 'tui')
    const socket = join(socketFolder(serverRuns[0]), 'native.sock')
    assert.ok(socket.startsWith(join(s.root, 'tmp', 'codex-')), socket)
    // The server gets the model and the bypass as configuration, and
    // ConsensFlow's own variables set in its shell policy.
    assert.deepEqual(serverRuns[0].args, [
      '-c',
      'model="native-model"',
      '-c',
      'approval_policy="never"',
      '-c',
      'sandbox_mode="danger-full-access"',
      '-c',
      `shell_environment_policy.set.CONSENSFLOW_HOME=${JSON.stringify(s.root)}`,
      '-c',
      'shell_environment_policy.set.CONSENSFLOW_TOKEN="window-token"',
      '-c',
      'shell_environment_policy.set.CONSENSFLOW_URL="http://127.0.0.1:1"',
      'app-server',
      '--listen',
      `unix://${socket}`,
    ])
    // The TUI reaches the server only through the broker, its token in its
    // environment and never on its command line.
    assert.deepEqual(tuiRuns[0].args, [
      '--remote',
      `ws://127.0.0.1:${s.port}`,
      '--remote-auth-token-env',
      'CF_CODEX_TUI_TOKEN',
      '--model',
      'native-model',
    ])
    assert.equal(tuiRuns[0].token, TOKEN)
    assert.deepEqual(
      [serverRuns[0].apiKey, tuiRuns[0].apiKey],
      [null, null],
      'an OpenAI API key never reaches Codex',
    )
    // The fresh thread the TUI started is the broker's to deliver to, with the bypass.
    assert.deepEqual(
      [serverRuns[1].started.approvalPolicy, serverRuns[1].started.sandbox],
      ['never', 'danger-full-access'],
    )
    assert.deepEqual(tuiRuns[1].session, {
      launchId: 'launch-1',
      sessionId: A,
      revision: 1,
      empty: true,
      available: true,
    })
    assert.equal(alive(serverRuns[0].pid), false, 'the server went with the window')
    assert.equal(existsSync(dirname(socket)), false, 'the socket folder went with the session')
  },
)

it(
  "says why Codex's server could not start, in the window, and leaves nothing behind",
  UNIX_ONLY,
  async (t) => {
    const s = await supervised(t, [], { CF_TEST_CODEX_BACKEND: 'fail' })
    assert.deepEqual(await s.exit(), { code: 1, signal: null })
    assert.equal(
      s.stderr(),
      'ConsensFlow could not open Codex: Codex server could not start: not logged in\n\n',
    )
    const runs = await s.runs()
    assert.deepEqual(
      runs.map((entry) => entry.run),
      ['server'],
      'no TUI was opened',
    )
    assert.equal(existsSync(socketFolder(runs[0])), false)
    // A Codex gone from where the launch found it fails the same way, in its own words.
    const missing = await supervised(t, [], {}, (root) => join(root, 'gone', 'codex'))
    assert.deepEqual(await missing.exit(), { code: 1, signal: null })
    assert.equal(
      missing.stderr(),
      `ConsensFlow could not open Codex: Codex server could not start: spawn ${join(missing.root, 'gone', 'codex')} ENOENT\n`,
    )
    assert.deepEqual(readdirSync(join(missing.root, 'tmp')), [])
  },
)

it('ends both Codex processes when its window is closed', {
  skip: process.platform === 'win32' && 'Unix sockets only, and no SIGTERM on Windows',
}, async (t) => {
  const s = await supervised(t, [], { CF_TEST_CODEX_TUI: 'wait' })
  const seen = await s.until((runs) => runs.some((entry) => entry.session !== undefined))
  const server = seen.find((entry) => entry.run === 'server')
  const tui = seen.find((entry) => entry.run === 'tui')
  s.child.kill('SIGTERM')
  assert.deepEqual(await s.exit(), { code: 0, signal: null }, s.stderr())
  assert.deepEqual([alive(server.pid), alive(tui.pid)], [false, false])
  assert.equal(existsSync(socketFolder(server)), false)
})

it(
  "ends the TUI when Codex's server dies under it, so the window does not hang",
  UNIX_ONLY,
  async (t) => {
    const s = await supervised(t, [], { CF_TEST_CODEX_BACKEND: 'die', CF_TEST_CODEX_TUI: 'wait' })
    assert.equal((await s.exit()).code, 0, s.stderr())
    const runs = await s.runs()
    assert.equal(alive(runs.find((entry) => entry.run === 'tui').pid), false)
    assert.equal(existsSync(socketFolder(runs.find((entry) => entry.run === 'server'))), false)
  },
)
