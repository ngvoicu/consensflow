import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { it } from 'node:test'
import { currentSession, send } from '../src/channels/codex.js'

const SESSION = '01a0817b-e6b0-7f32-8e11-370dc000cbc0'

/**
 * A stand-in for the supervisor's broker (`hosts/codex-session.mjs`), the
 * only way into a Codex window: it shows one thread and admits a message for
 * that thread alone.
 */
async function broker(t) {
  let selected = SESSION
  const received = []
  const server = createServer(async (request, response) => {
    response.setHeader('content-type', 'application/json')
    if (request.url === '/session')
      return response.end(JSON.stringify({ launchId: 'owned', sessionId: selected }))
    let text = ''
    for await (const chunk of request) text += chunk
    const input = JSON.parse(text)
    received.push(input)
    response.end(
      JSON.stringify(
        input.sessionId === selected
          ? { ok: true, admitted: true }
          : { ok: false, admitted: false, bytesWritten: 0, error: 'native-session-changed' },
      ),
    )
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(async () => {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
  })
  const launch = {
    kind: 'codex-queue',
    launchId: 'owned',
    sessionBridge: {
      endpoint: `http://127.0.0.1:${server.address().port}`,
      token: 'private-owned-bridge-token-123',
    },
  }
  return {
    received,
    launch,
    select: (thread) => {
      selected = thread
    },
    target: { session: SESSION, pane: 'codex-pane', generation: 3, deadlineMs: 3_000, launch },
  }
}

it('claims the pane, then hands the exact text to the broker for the thread it names', async (t) => {
  const f = await broker(t)
  const order = []
  f.target.claim = async (request) => {
    order.push(request)
    return { ok: true }
  }
  assert.deepEqual(await send(f.target, 'a message with spaces\nand newlines'), {
    ok: true,
    admitted: true,
  })
  assert.deepEqual(order, [{ pane: 'codex-pane', generation: 3 }])
  assert.deepEqual(
    [f.received[0].launchId, f.received[0].sessionId, f.received[0].text],
    ['owned', SESSION, 'a message with spaces\nand newlines'],
  )
})

it('rejects malformed native session UUIDs before claiming', async (t) => {
  const f = await broker(t)
  for (const session of ['', 'not-a-uuid', 'aaaaaaaa-bbbb-4ccc-8ddd-40940940940', `${SESSION}\n`]) {
    let claims = 0
    f.target.session = session
    f.target.claim = async () => {
      claims += 1
      return { ok: true }
    }
    await assert.rejects(() => send(f.target, 'invalid session'), /UUID/)
    assert.equal(claims, 0)
  }
  assert.deepEqual(f.received, [])
})

it('rejects an unscoped launch, one without its broker, or a malformed pane before claiming', async (t) => {
  const f = await broker(t)
  const cases = [
    [
      (target) => {
        target.launch = { ...target.launch, kind: 'codex' }
      },
      /codex-queue launch configuration/,
    ],
    [
      (target) => {
        target.launch = { ...target.launch, sessionBridge: undefined }
      },
      /session broker/,
    ],
    [
      (target) => {
        target.pane = ''
      },
      /pane \{id, generation\}/,
    ],
    [
      (target) => {
        target.generation = 0
      },
      /pane \{id, generation\}/,
    ],
  ]
  for (const [mutate, expected] of cases) {
    const target = { ...f.target }
    let claims = 0
    mutate(target)
    target.claim = async () => {
      claims += 1
      return { ok: true }
    }
    await assert.rejects(() => send(target, 'invalid caller'), expected)
    assert.equal(claims, 0)
  }
  assert.deepEqual(f.received, [])
})

it('turns a stale native claim into an affirmative zero-byte refusal without asking the broker', async (t) => {
  const f = await broker(t)
  f.target.claim = async () => ({ ok: false, error: 'stale' })
  assert.deepEqual(await send(f.target, 'stale message'), {
    ok: false,
    admitted: false,
    error: 'failed-with-zero-bytes',
    bytesWritten: 0,
    cause: 'stale',
  })
  assert.deepEqual(f.received, [])
})

it('uses the owned bridge for exact identity and rejects a session switch after the pane claim', async (t) => {
  const f = await broker(t)
  assert.equal(await currentSession(f.launch), SESSION)
  const switched = '01a09094-a559-7db0-bf50-e2309856c3c0'
  f.target.claim = async () => {
    f.select(switched)
    return { ok: true }
  }
  assert.deepEqual(await send(f.target, 'complete reply'), {
    ok: false,
    admitted: false,
    bytesWritten: 0,
    error: 'native-session-changed',
  })
  assert.equal(f.received[0].sessionId, SESSION)
  f.target.session = switched
  assert.deepEqual(await send(f.target, 'complete reply'), { ok: true, admitted: true })
  assert.equal(f.received[1].text, 'complete reply')
})
