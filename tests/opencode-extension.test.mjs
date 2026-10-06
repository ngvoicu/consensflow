import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { createServer } from 'node:net'
import { homedir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

/**
 * What `question.reply()` of the SDK resolves with, which does not throw
 * (`throwOnError` is off): the shape of @opencode-ai/sdk's client, read from
 * its generated code, v2/gen/client/client.gen.js.
 */
const sdk = {
  took: () => ({ data: true, request: {}, response: { ok: true, status: 200 } }),
  refused: (status) => ({
    error: { name: 'NotFoundError', data: { message: 'no such request' } },
    request: {},
    response: { ok: false, status },
  }),
  lost: () => ({ error: new TypeError('fetch failed'), request: {}, response: undefined }),
}

async function fixture(t, useEnvironment = false, boardUrl = null, question = null) {
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
  const replyOptions = []
  const handlers = new Map()
  let current = { name: 'session', params: { sessionID: 'ses_first' } }
  const statuses = new Map()
  let dispose
  let send = async (input) => {
    calls.push(input)
    return { data: undefined, response: { status: 204 } }
  }
  let failReply = false
  let answerReply = sdk.took
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
          question: question ?? {
            reply: async (input, options) => {
              if (failReply) throw new Error('the question was rejected')
              replies.push(input)
              replyOptions.push(options)
              return answerReply()
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
    replyOptions,
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
    failReplies() {
      failReply = true
    },
    /** What the SDK's `reply()` resolves with from now on (one of `sdk`). */
    answerReplyWith(make) {
      answerReply = make
    },
  }
}

test('OpenCode reports the displayed session after new and resume, including the empty home route', async (t) => {
  const f = await fixture(t)
  assert.deepEqual((await f.request('/session')).body, {
    launchId: 'test-launch',
    sessionId: 'ses_first',
    status: { type: 'idle' },
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
  // A conversation OpenCode has no status for has not worked since the window opened: idle.
  assert.deepEqual((await f.request('/session')).body.status, { type: 'idle' })
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
  const state = {
    posted: [],
    answered: [],
    answer: null,
    tokens: [],
    refuse: null,
    shut: null,
    receipts: [],
    /** How many polls were asked, and how many of them lose their reply: the connection is cut. */
    polls: 0,
    dropPolls: 0,
  }
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
      if (state.refuse) return json(400, { error: 'bad-questions', message: state.refuse })
      return json(201, { message: { id: 40 + state.posted.length } })
    }
    if (request.method === 'GET' && request.url.startsWith('/api/questions/')) {
      state.polls++
      if (state.shut) return json(409, { error: 'door-closed', message: state.shut })
      for (let i = 0; i < 40 && state.answer === null; i++)
        await new Promise((r) => setTimeout(r, 25))
      if (state.dropPolls > 0 && state.answer !== null) {
        state.dropPolls--
        request.socket.destroy()
        return
      }
      return json(200, { question: {}, answer: state.answer })
    }
    if (request.method === 'POST' && /^\/api\/answers\/\d+\/receipt$/.test(request.url)) {
      state.receipts.push({ at: request.url, ...body })
      return json(200, { message: null })
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
  board.state.answer = { id: 50, from: 'chief', body: 'Colour: blue', choices: [['blue']] }
  for (let i = 0; i < 100 && f.replies.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(f.replies, [{ requestID: 'q-1', answers: [['blue']] }])
})

test('OpenCode: an answer handed to the tool is acknowledged to the board after it, and a reply that failed gives the claim back', async (t) => {
  const board = await fakeBoard(t)
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  board.state.answer = { id: 50, from: 'chief', body: 'Colour: blue', choices: [['blue']] }
  for (let i = 0; i < 100 && board.state.receipts.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(f.replies, [{ requestID: 'q-1', answers: [['blue']] }], 'handed over')
  assert.deepEqual(board.state.receipts, [{ at: '/api/answers/50/receipt', received: true }])

  const failing = await fakeBoard(t)
  const g = await fixture(t, false, failing.url)
  g.failReplies()
  g.emit('question.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  failing.state.answer = { id: 51, from: 'chief', body: 'Colour: red', choices: [['red']] }
  for (let i = 0; i < 100 && failing.state.receipts.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(failing.state.receipts, [{ at: '/api/answers/51/receipt', received: false }])
})

test("OpenCode: a door the board shut is answered with the board's words, as they are", async (t) => {
  const board = await fakeBoard(t)
  board.state.shut =
    'T-1 was stopped, so m-41 is not answered here: its answer comes to you as a message when the task goes on. Do not ask it again; end your turn now.'
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', ASKED)
  for (let i = 0; i < 100 && f.replies.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(f.replies, [{ requestID: 'q-1', answers: [[board.state.shut]] }])
  assert.deepEqual(board.state.receipts, [], 'nothing was handed over')
})

test('OpenCode: a question the board refuses is answered with the reason, never left to a dialog nobody sees', async (t) => {
  const board = await fakeBoard(t)
  board.state.refuse = 'questions: one to 4 questions'
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', ASKED)
  for (let i = 0; i < 100 && f.replies.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.deepEqual(f.replies, [
    {
      requestID: 'q-1',
      answers: [
        [
          'ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask "…" instead.',
        ],
      ],
    },
  ])
})

test("OpenCode: the chief's question tool stays in its window, where the human answers it", async (t) => {
  const board = await fakeBoard(t)
  const previous = process.env.CONSENSFLOW_PARTICIPANT
  process.env.CONSENSFLOW_PARTICIPANT = 'chief'
  let f
  try {
    f = await fixture(t, false, board.url)
  } finally {
    if (previous === undefined) delete process.env.CONSENSFLOW_PARTICIPANT
    else process.env.CONSENSFLOW_PARTICIPANT = previous
  }
  f.emit('question.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.deepEqual(board.state.posted, [], 'nothing goes to the board')
  assert.deepEqual(f.replies, [], "OpenCode's own dialog answers it")
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

/** The pending question, answered by the board with the choice `blue`: until the board is told what came of it. */
async function answeredByTheBoard(f, board, id = 50) {
  f.emit('question.asked', ASKED)
  await new Promise((resolve) => setTimeout(resolve, 100))
  board.state.answer = { id, from: 'chief', body: 'Colour: blue', choices: [['blue']] }
  for (let i = 0; i < 200 && board.state.receipts.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
}

test("OpenCode: the answer is received only when the SDK's reply took: an error or a status that is no success comes back as a result, not a throw", async (t) => {
  for (const [what, result, received] of [
    ['a reply that took', sdk.took, true],
    ['a native 404, the question gone', () => sdk.refused(404), false],
    ['a server error', () => sdk.refused(500), false],
    ['a request that got no response', sdk.lost, false],
    ['a result with no word of success', () => ({}), false],
    ['no result at all', () => undefined, false],
    ['an SDK that answers with the data alone', () => true, true],
    ['a result with the data and no response', () => ({ data: true }), true],
    ['a result with an error beside its data', () => ({ data: true, error: {} }), false],
  ]) {
    const board = await fakeBoard(t)
    const f = await fixture(t, false, board.url)
    f.answerReplyWith(result)
    await answeredByTheBoard(f, board)
    assert.deepEqual(f.replies, [{ requestID: 'q-1', answers: [['blue']] }], what)
    assert.deepEqual(
      f.replyOptions,
      [{ throwOnError: true }],
      `${what}: the SDK is asked to throw, for the ones that honour it`,
    )
    assert.deepEqual(
      board.state.receipts,
      [{ at: '/api/answers/50/receipt', received }],
      `${what}: said ${received}`,
    )
  }
})

/** The OpenCode SDK installed on this machine (its v2 client), or null where there is none. */
async function installedSdk() {
  const config = process.env.XDG_CONFIG_HOME ?? join(homedir(), '.config')
  const folders = [
    process.env.OPENCODE_SDK,
    join(config, 'opencode', 'node_modules', '@opencode-ai', 'sdk'),
  ]
  for (const folder of folders) {
    if (!folder) continue
    const client = join(folder, 'dist', 'v2', 'client.js')
    if (!existsSync(client)) continue
    const { version } = JSON.parse(readFileSync(join(folder, 'package.json'), 'utf8'))
    return { version, ...(await import(pathToFileURL(client))) }
  }
  return null
}
const installed = await installedSdk()

const json = (status, text) => () =>
  new Response(text, { status, headers: { 'content-type': 'application/json' } })

test('OpenCode: with the SDK installed here and its transport mocked, only a reply that succeeded is a receipt', {
  skip: installed === null ? 'left out: the OpenCode SDK is not installed on this machine' : false,
}, async (t) => {
  process.stdout.write(`# @opencode-ai/sdk ${installed.version}\n`)
  for (const [what, transport, received] of [
    ['a 200', json(200, 'true'), true],
    ['a native 404', json(404, '{"name":"NotFoundError"}'), false],
    ['a 500', () => new Response('boom', { status: 500 }), false],
    ['a transport that fails', () => Promise.reject(new TypeError('fetch failed')), false],
  ]) {
    const board = await fakeBoard(t)
    const requests = []
    const client = installed.createOpencodeClient({
      baseUrl: 'http://opencode.test',
      fetch: async (request) => {
        requests.push(`${request.method} ${new URL(request.url).pathname}`)
        return transport(request)
      },
    })
    const f = await fixture(t, false, board.url, client.question)
    await answeredByTheBoard(f, board)
    assert.deepEqual(requests, ['POST /question/q-1/reply'], what)
    assert.deepEqual(
      board.state.receipts,
      [{ at: '/api/answers/50/receipt', received }],
      `${what}: said ${received}`,
    )
  }
})

test('OpenCode: a poll whose reply was lost is asked again and the answer is handed over once, then acknowledged', async (t) => {
  const board = await fakeBoard(t)
  board.state.answer = { id: 50, from: 'chief', body: 'Colour: blue', choices: [['blue']] }
  board.state.dropPolls = 1
  const f = await fixture(t, false, board.url)
  f.emit('question.asked', ASKED)
  for (let i = 0; i < 300 && board.state.receipts.length === 0; i++)
    await new Promise((r) => setTimeout(r, 10))
  assert.equal(board.state.polls, 2, 'the poll was asked again')
  assert.equal(board.state.posted.length, 1, 'the question was put once')
  assert.deepEqual(f.replies, [{ requestID: 'q-1', answers: [['blue']] }], 'handed over once')
  assert.deepEqual(board.state.receipts, [{ at: '/api/answers/50/receipt', received: true }])
})

test('OpenCode: a door asks a lost poll again a few times and then lets its error through; a refusal is final', async () => {
  const { askTheBoard } = await import('../hosts/lib/question-door.js')
  const calls = []
  const lost = async (method, path) => {
    calls.push(`${method} ${path}`)
    if (method === 'POST') return { message: { id: 7 } }
    throw new TypeError('fetch failed')
  }
  await assert.rejects(askTheBoard(lost, [], { retries: [1, 1, 1, 1] }), /fetch failed/)
  assert.equal(calls.filter((call) => call.startsWith('GET')).length, 5, 'one poll and four more')
  assert.equal(
    calls.filter((call) => call.startsWith('POST')).length,
    1,
    'the question is put once',
  )

  const polls = []
  const shut = async (method) => {
    polls.push(method)
    if (method === 'POST') return { message: { id: 7 } }
    throw Object.assign(new Error('shut'), { refused: true, code: 'door-closed' })
  }
  await assert.rejects(askTheBoard(shut, [], { retries: [1, 1, 1, 1] }), /shut/)
  assert.deepEqual(polls, ['POST', 'GET'], 'a refusal is not asked again')

  // A poll lost in every other one, never twice in a row: each is forgiven.
  let step = 0
  const patchy = async (method) => {
    if (method === 'POST') return { message: { id: 7 } }
    step++
    if (step === 11) return { answer: { id: 9, choices: [['blue']] } }
    if (step % 2 === 1) throw new TypeError('fetch failed')
    return { answer: null }
  }
  assert.deepEqual((await askTheBoard(patchy, [], { retries: [1] })).answer, {
    id: 9,
    choices: [['blue']],
  })
})
