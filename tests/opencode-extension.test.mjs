import assert from 'node:assert/strict'
import { createServer } from 'node:net'
import test from 'node:test'

async function fixture(t, useEnvironment = false, boardUrl = null) {
  const { tui } = await import('../hosts/opencode-extension/consensflow-session.mjs')
  const portServer = createServer()
  await new Promise((resolve) => portServer.listen(0, '127.0.0.1', resolve))
  const port = portServer.address().port
  await new Promise((resolve) => portServer.close(resolve))
  const configuration = {
    launchId: 'test-launch',
    port,
    token: 'private-token-for-local-tests-only',
  }
  const calls = []
  const replies = []
  const handlers = new Map()
  let current = { name: 'session', params: { sessionID: 'ses_first' } }
  const statuses = new Map()
  let dispose
  let send = async (input) => {
    calls.push(input)
    return { data: undefined, response: { status: 204 } }
  }
  const previous = process.env.CF_OPENCODE_SESSION_BRIDGE
  const board = { url: process.env.CONSENSFLOW_URL, token: process.env.CONSENSFLOW_TOKEN }
  if (useEnvironment) process.env.CF_OPENCODE_SESSION_BRIDGE = JSON.stringify(configuration)
  if (boardUrl !== null) {
    process.env.CONSENSFLOW_URL = boardUrl
    process.env.CONSENSFLOW_TOKEN = 'window-token'
  }
  try {
    await tui(
      {
        route: {
          get current() {
            return current
          },
        },
        state: { session: { status: (sessionID) => statuses.get(sessionID) } },
        client: {
          session: { promptAsync: (input) => send(input) },
          question: {
            reply: async (input) => {
              replies.push(input)
              return { data: true }
            },
          },
        },
        event: {
          on(type, handler) {
            handlers.set(type, handler)
            return () => handlers.delete(type)
          },
        },
        lifecycle: {
          onDispose(fn) {
            dispose = fn
          },
        },
      },
      useEnvironment ? undefined : configuration,
    )
  } finally {
    if (previous === undefined) delete process.env.CF_OPENCODE_SESSION_BRIDGE
    else process.env.CF_OPENCODE_SESSION_BRIDGE = previous
    for (const [key, value] of [
      ['CONSENSFLOW_URL', board.url],
      ['CONSENSFLOW_TOKEN', board.token],
    ]) {
      if (value === undefined) delete process.env[key]
      else process.env[key] = value
    }
  }
  t.after(() => dispose())
  const request = async (path, body, token = configuration.token) => {
    const response = await fetch(`http://127.0.0.1:${port}${path}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      signal: AbortSignal.timeout(2000),
    })
    return { status: response.status, body: await response.json() }
  }
  return {
    request,
    calls,
    replies,
    emit: (type, properties) => handlers.get(type)?.({ type, properties }),
    setCurrent(value) {
      current = value
    },
    setStatus(sessionID, status) {
      statuses.set(sessionID, status)
    },
    setSend(value) {
      send = value
    },
  }
}

test('OpenCode reports the displayed session after new and resume, including the empty home route', async (t) => {
  const f = await fixture(t)
  assert.deepEqual((await f.request('/session')).body, {
    launchId: 'test-launch',
    sessionId: 'ses_first',
    status: null,
  })
  f.setCurrent({ name: 'home' })
  assert.equal((await f.request('/session')).body.sessionId, null)
  f.setCurrent({ name: 'session', params: { sessionID: 'ses_second' } })
  assert.equal((await f.request('/session')).body.sessionId, 'ses_second')
  f.setCurrent({ name: 'session', params: { sessionID: 'ses_first' } })
  assert.equal((await f.request('/session')).body.sessionId, 'ses_first')
  assert.deepEqual(f.calls, [])
})

test('OpenCode reports what the displayed session is doing, a retry of a refused request included', async (t) => {
  const f = await fixture(t)
  f.setStatus('ses_first', { type: 'busy' })
  assert.deepEqual((await f.request('/session')).body.status, { type: 'busy' })
  // OpenCode 1.18.31, probed on 2026-09-21: a spent free tier waits here, never in its store.
  const retry = {
    type: 'retry',
    attempt: 1,
    message: 'Free usage exceeded, subscribe to Go',
    action: { reason: 'free_tier_limit', provider: 'opencode', title: 'Free limit reached' },
    next: 1790035200967,
  }
  f.setStatus('ses_first', retry)
  assert.deepEqual((await f.request('/session')).body.status, retry)
  f.setStatus('ses_second', { type: 'busy' })
  f.setCurrent({ name: 'home' })
  assert.deepEqual((await f.request('/session')).body, {
    launchId: 'test-launch',
    sessionId: null,
    status: null,
  })
})

test('OpenCode refuses retired sessions at native admission and preserves the composer', async (t) => {
  const f = await fixture(t)
  const body = {
    launchId: 'test-launch',
    sessionId: 'ses_first',
    text: 'whole worker answer',
    expiresAt: Date.now() + 3000,
  }
  f.setCurrent({ name: 'session', params: { sessionID: 'ses_second' } })
  assert.deepEqual((await f.request('/deliver', body)).body, {
    ok: false,
    admitted: false,
    bytesWritten: 0,
    error: 'native-session-changed',
  })
  assert.deepEqual(f.calls, [])
  assert.equal(
    (await f.request('/deliver', { ...body, sessionId: 'ses_second' })).body.admitted,
    true,
  )
  assert.deepEqual(f.calls, [
    { sessionID: 'ses_second', parts: [{ type: 'text', text: body.text }] },
  ])
})

test('OpenCode rejects wrong launch, unauthorized, expired and malformed requests before native effects', async (t) => {
  const f = await fixture(t)
  const body = {
    launchId: 'test-launch',
    sessionId: 'ses_first',
    text: 'answer',
    expiresAt: Date.now() + 3000,
  }
  assert.equal((await f.request('/session', undefined, 'wrong-token')).status, 401)
  for (const patch of [
    { launchId: 'other-launch' },
    { expiresAt: Date.now() - 1 },
    { text: '' },
    { sessionId: '../invalid' },
  ]) {
    const response = await f.request('/deliver', { ...body, ...patch })
    assert.equal(response.body.admitted, false)
    assert.equal(response.body.bytesWritten, 0)
  }
  assert.deepEqual(f.calls, [])
})

test('OpenCode native transport failure remains uncertain and is not retried', async (t) => {
  const f = await fixture(t)
  let sends = 0
  f.setSend(async () => {
    sends++
    throw Error('transport lost after admission may have started')
  })
  const response = await f.request('/deliver', {
    launchId: 'test-launch',
    sessionId: 'ses_first',
    text: 'answer',
    expiresAt: Date.now() + 1000,
  })
  assert.equal(response.body.admitted, null)
  assert.equal(sends, 1)
})

test('OpenCode loads the native default export and process-local launch configuration', async (t) => {
  const module = await import('../hosts/opencode-extension/consensflow-session.mjs')
  assert.equal(module.default?.tui, module.tui)
  const f = await fixture(t, true)
  assert.equal((await f.request('/session')).body.launchId, 'test-launch')
})

/** A stand-in for the board's API: the question posted, the answer when the test gives it. */
async function fakeBoard(t) {
  const { createServer: createHttpServer } = await import('node:http')
  const state = { posted: [], answered: [], answer: null, tokens: [] }
  const server = createHttpServer(async (request, response) => {
    state.tokens.push(request.headers.authorization)
    const chunks = []
    for await (const chunk of request) chunks.push(chunk)
    const body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : null
    const json = (status, value) => {
      response.writeHead(status, { 'content-type': 'application/json' })
      response.end(JSON.stringify(value))
    }
    if (request.method === 'POST' && request.url === '/api/questions') {
      state.posted.push(body)
      return json(201, { message: { id: 40 + state.posted.length } })
    }
    if (request.method === 'GET' && request.url.startsWith('/api/questions/')) {
      for (let i = 0; i < 40 && state.answer === null; i++)
        await new Promise((r) => setTimeout(r, 25))
      return json(200, { question: {}, answer: state.answer })
    }
    if (request.method === 'POST' && request.url === '/api/answers') {
      state.answered.push(body)
      return json(201, { message: { id: 99, state: 'read' } })
    }
    json(404, { error: 'unknown-route' })
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => new Promise((resolve) => server.close(resolve)))
  return { state, url: `http://127.0.0.1:${server.address().port}` }
}

const ASKED = {
  id: 'q-1',
  sessionID: 'ses_first',
  questions: [
    {
      question: 'Which colour?',
      header: 'Colour',
      options: [
        { label: 'red', description: 'Warm' },
        { label: 'blue', description: '' },
      ],
    },
  ],
  tool: { messageID: 'm', callID: 'c' },
}

test("OpenCode's question tool is answered from the board through the plugin", async (t) => {
  const board = await fakeBoard(t)
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.deepEqual(board.state.posted, [
    {
      questions: [
        {
          question: 'Which colour?',
          header: 'Colour',
          options: [
            { label: 'red', description: 'Warm' },
            { label: 'blue', description: '' },
          ],
          multiple: false,
        },
      ],
    },
  ])
  assert.deepEqual([...new Set(board.state.tokens)], ['Bearer window-token'])
  assert.deepEqual(f.replies, [], 'nothing replied while the board has no answer')
  board.state.answer = { id: 50, from: 'lead', body: 'Colour: blue', choices: [['blue']] }
  for (let i = 0; i < 100 && f.replies.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(f.replies, [{ requestID: 'q-1', answers: [['blue']] }])
})

test('OpenCode: a question answered in the window first is recorded on the board, never replied twice', async (t) => {
  const board = await fakeBoard(t)
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.equal(board.state.posted.length, 1)
  f.emit('question.replied', { sessionID: 'ses_first', requestID: 'q-1', answers: [['red']] })
  for (let i = 0; i < 100 && board.state.answered.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(board.state.answered, [{ question: 41, choices: [['red']] }])
  assert.deepEqual(f.replies, [])
})

test('OpenCode: a question of another session, or outside a window, is left to the TUI', async (t) => {
  const board = await fakeBoard(t)
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', { ...ASKED, sessionID: 'ses_other' })
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.deepEqual(board.state.posted, [])
  const outside = await fixture(t)
  outside.emit('question.v2.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.deepEqual(board.state.posted, [])
})
