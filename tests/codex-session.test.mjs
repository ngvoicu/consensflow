import assert from 'node:assert/strict'
import { once } from 'node:events'
import { mkdtemp, rm, stat } from 'node:fs/promises'
import { join } from 'node:path'
import { it } from 'node:test'
import WebSocket, { WebSocketServer } from 'ws'
import * as codexSession from '../hosts/codex-session.mjs'
import { codexProcessArguments, startBroker } from '../hosts/codex-session.mjs'

const A = '01a09094-938f-7fd1-a2d3-315cf92b4559'
const B = '01a09094-a559-7db0-bf50-e2309856c3c0'
const TOKEN = 'private-launch-token-1234567890'

async function fixture(t, options = {}) {
  const upstream = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  await once(upstream, 'listening')
  const requests = []
  const pending = []
  upstream.on('connection', (socket) =>
    socket.on('message', (raw) => {
      const message = JSON.parse(raw)
      requests.push(message)
      if (message.method === 'initialize')
        socket.send(JSON.stringify({ id: message.id, result: {} }))
      else if (message.id !== undefined) pending.push({ socket, message })
    }),
  )
  const broker = await startBroker({
    ...options,
    port: 0,
    token: TOKEN,
    launchId: 'launch-1',
    upstream: `ws://127.0.0.1:${upstream.address().port}`,
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
  return { broker, requests, pending, wait, connect, read, deliver, respond }
}

it('Codex receiver pulls full bodies into the selected main, holds busy, and fences a concurrent new', async (t) => {
  let registered,
    next = 0,
    switchOnClaim = null
  const bodies = [],
    operations = []
  const f = await fixture(t, {
    receiver: {
      request: async (op, body) => {
        operations.push(op)
        if (op === 'state') return registered ?? null
        if (op === 'register') {
          registered = { session: body.session, lease: `lease-${++next}` }
          return registered
        }
        if (op === 'claim') {
          if (switchOnClaim) await switchOnClaim()
          return bodies.length
            ? { id: `c-${next}`, result: 'd-1', receiver: { ...registered }, text: bodies.shift() }
            : null
        }
        return {}
      },
    },
  })
  assert.ok(f.broker.receiver)
  const tui = await f.connect()
  const select = async (id, session) => {
    tui.send(
      JSON.stringify({
        id,
        method: 'thread/resume',
        params: { threadId: session, runtimeWorkspaceRoots: [] },
      }),
    )
    await f.respond('thread/resume', {
      thread: { id: session, status: { type: 'idle' }, turns: [] },
    })
    for (let attempt = 0; (await f.read()).sessionId !== session; attempt++) {
      assert.ok(attempt < 100)
      await new Promise((resolve) => setTimeout(resolve, 5))
    }
  }
  await select('first', A)
  bodies.push('complete\n'.repeat(1000))
  const pulling = f.broker.receiver.poll()
  const native = await f.respond('turn/start', { turn: { id: 'turn-one' } })
  await pulling
  assert.equal(native.params.threadId, A)
  assert.deepEqual(native.params.toolOutput, {
    name: 'consensflow_inbox',
    namespace: null,
    output: 'complete\n'.repeat(1000),
  })
  assert.deepEqual(native.params.input, [])
  bodies.push('held while busy')
  await f.broker.receiver.poll()
  assert.equal(bodies.length, 1)
  await select('second', B)
  switchOnClaim = () => select('resume', A)
  await f.broker.receiver.poll()
  assert.equal(f.requests.filter((r) => r.method === 'turn/start').length, 1)
  assert.equal(operations.at(-1), 'release')
})

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
  await f.respond('thread/queue/add', {})
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

it('requires an authenticated native empty-session observation before delivering without a rollout', async () => {
  const { tempEnv } = await import('./helpers.mjs')
  const { answers } = await import('../hosts/lib/completion.js')
  const temporary = tempEnv()
  try {
    const options = { codexSession: { sessionId: A, empty: true } }
    const empty = await answers('codex', A, temporary.env, options)
    assert.equal(empty.settlement.state, 'settled')
    assert.equal((await answers('codex', B, temporary.env, options)).unknown, true)
    assert.equal((await answers('codex', A, temporary.env)).unknown, true)
  } finally {
    temporary.cleanup()
  }
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

it('keeps native TUI arguments while explicitly forwarding backend model, effort and full role configuration', () => {
  const role = 'developer_instructions="existing instructions\\ncomplete lead role"'
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

it('launches the bundled supervisor for owned panes without changing legacy Codex invocations', async () => {
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
  assert.match(wrapped.args[0], /hosts\/codex-session\.mjs$/)
  assert.deepEqual(wrapped.args.slice(1), ['/native/codex', 'resume', A])
  assert.deepEqual(wrapped.env, invocation.env)
  assert.deepEqual(wrapped.dropEnv, invocation.dropEnv)
  assert.deepEqual(withNativeBridge(invocation, { channel: null }, process.execPath), invocation)
})

it('keeps native Codex sockets private and inside ConsensFlow home', async (t) => {
  const root = await mkdtemp('/tmp/cf-socket-home-')
  t.after(() => rm(root, { recursive: true, force: true }))
  const home = join(root, '.consensflow')
  const directory = await codexSession.createSocketDirectory({ CONSENSFLOW_HOME: home })
  assert.ok(directory.startsWith(`${join(home, 'tmp')}/`))
  assert.equal((await stat(directory)).mode & 0o777, 0o700)
  assert.ok(Buffer.byteLength(join(directory, 'native.sock')) < 104)
  await assert.rejects(
    codexSession.createSocketDirectory({ CONSENSFLOW_HOME: join(root, 'x'.repeat(110)) }),
    /socket path.*too long/i,
  )
})
