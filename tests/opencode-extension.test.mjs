import assert from 'node:assert/strict'
import { createServer } from 'node:net'
import test from 'node:test'

async function fixture(t, useEnvironment = false) {
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
  let current = { name: 'session', params: { sessionID: 'ses_first' } }
  let dispose
  let send = async (input) => {
    calls.push(input)
    return { data: undefined, response: { status: 204 } }
  }
  const previous = process.env.CF_OPENCODE_SESSION_BRIDGE
  if (useEnvironment) process.env.CF_OPENCODE_SESSION_BRIDGE = JSON.stringify(configuration)
  try {
    await tui(
      {
        route: {
          get current() {
            return current
          },
        },
        client: { session: { promptAsync: (input) => send(input) } },
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
    setCurrent(value) {
      current = value
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
  })
  f.setCurrent({ name: 'home' })
  assert.equal((await f.request('/session')).body.sessionId, null)
  f.setCurrent({ name: 'session', params: { sessionID: 'ses_second' } })
  assert.equal((await f.request('/session')).body.sessionId, 'ses_second')
  f.setCurrent({ name: 'session', params: { sessionID: 'ses_first' } })
  assert.equal((await f.request('/session')).body.sessionId, 'ses_first')
  assert.deepEqual(f.calls, [])
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
