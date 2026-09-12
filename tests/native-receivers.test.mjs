import assert from 'node:assert/strict'
import { createServer } from 'node:net'
import test from 'node:test'
import { tui } from '../hosts/opencode-extension/consensflow-session.mjs'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'

function inbox() {
  let registered = null,
    revision = 0,
    next = 0
  const queue = [],
    calls = []
  const request = async (op, body) => {
    calls.push({ op, body })
    if (op === 'state') return registered
    if (op === 'register') {
      registered = { session: body.session, lease: `lease-${++revision}` }
      return registered
    }
    if (op === 'claim')
      return queue.length
        ? {
            id: `claim-${++next}`,
            result: `d-${next}`,
            text: queue.shift(),
            receiver: { ...registered },
          }
        : null
    return {}
  }
  return { request, queue, calls }
}

test('Pi receiver handles all replies, busy/draft holds, new/resume and native custom result visibility', async (t) => {
  const f = inbox(),
    handlers = new Map(),
    sent = []
  let session = 'first',
    busy = false,
    draft = ''
  const ctx = {
    mode: 'tui',
    hasUI: true,
    ui: { getEditorText: () => draft },
    isIdle: () => !busy,
    hasPendingMessages: () => false,
    sessionManager: { getSessionId: () => session },
  }
  const extension = createDeliveryExtension(
    {
      on: (event, handler) => handlers.set(event, handler),
      sendMessage: (message, options) => sent.push({ session, message, options }),
    },
    { receiver: { request: f.request } },
  )
  t.after(() => handlers.get('session_shutdown')())
  await handlers.get('session_start')({}, ctx)
  assert.ok(extension.receiver)
  f.queue.push('first complete result')
  busy = true
  await extension.receiver.poll()
  assert.equal(sent.length, 0)
  busy = false
  draft = 'unsent question'
  await extension.receiver.poll()
  assert.equal(sent.length, 0)
  draft = ''
  await extension.receiver.poll()
  assert.equal(sent[0].message.content, 'first complete result')
  assert.equal(sent[0].message.display, true)
  assert.equal(sent[0].message.customType, 'consensflow-worker-result')
  assert.deepEqual(sent[0].options, { triggerTurn: true, deliverAs: 'followUp' })
  for (const selected of ['second', 'first']) {
    await handlers.get('session_shutdown')({ reason: 'switch' })
    session = selected
    f.queue.push(`reply in ${selected}`)
    await handlers.get('session_start')({}, ctx)
    await extension.receiver.poll()
  }
  assert.deepEqual(
    sent.map((value) => value.session),
    ['first', 'second', 'first'],
  )
  assert.equal(f.calls.filter((call) => call.op === 'receipt').length, 3)
})

test('OpenCode receiver follows native home/new/resume, holds busy or modal state and keeps full body', async (t) => {
  const listener = createServer()
  await new Promise((resolve) => listener.listen(0, '127.0.0.1', resolve))
  const port = listener.address().port
  await new Promise((resolve) => listener.close(resolve))
  const f = inbox(),
    sent = []
  let route = { name: 'home' },
    status = 'idle',
    mode = 'base',
    dispose
  const adapter = await tui(
    {
      route: {
        get current() {
          return route
        },
      },
      state: { ready: true, session: { status: () => ({ type: status }) } },
      mode: { current: () => mode },
      client: {
        session: {
          promptAsync: async (input) => {
            sent.push(input)
            return { response: { status: 204 } }
          },
        },
      },
      lifecycle: {
        onDispose: (fn) => {
          dispose = fn
        },
      },
    },
    {
      port,
      launchId: 'receiver-test',
      token: 'private-test-token-for-receiver',
      receiver: { request: f.request },
    },
  )
  t.after(() => dispose())
  assert.ok(adapter?.receiver)
  await adapter.receiver.poll()
  assert.equal(f.calls.length, 0)
  route = { name: 'session', params: { sessionID: 'ses_first' } }
  f.queue.push('complete body\n'.repeat(1000))
  status = 'busy'
  await adapter.receiver.poll()
  status = 'idle'
  mode = 'dialog'
  await adapter.receiver.poll()
  assert.equal(sent.length, 0)
  mode = 'base'
  await adapter.receiver.poll()
  assert.equal(sent[0].parts[0].text, 'complete body\n'.repeat(1000))
  for (const sessionID of ['ses_second', 'ses_first']) {
    route = { name: 'session', params: { sessionID } }
    f.queue.push(`reply in ${sessionID}`)
    await adapter.receiver.poll()
  }
  assert.deepEqual(
    sent.map((value) => value.sessionID),
    ['ses_first', 'ses_second', 'ses_first'],
  )
})
