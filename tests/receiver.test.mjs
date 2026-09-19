import assert from 'node:assert/strict'
import test from 'node:test'
import { createReceiver } from '../hosts/lib/receiver.js'

function fixture(overrides = {}) {
  let session = 'native-a'
  let lease = null
  let next = 0
  const inserted = [],
    calls = []
  const queue = []
  const receiver = createReceiver({
    session: () => session,
    ready: () => true,
    insert: async (claim) => {
      inserted.push(claim)
      return { admitted: true }
    },
    request: async (op, body) => {
      calls.push({ op, body })
      if (op === 'state') return lease
      if (op === 'register') {
        lease = { lease: `epoch-${++next}`, session: body.session }
        return lease
      }
      if (op === 'retire') {
        lease = null
        return null
      }
      if (op === 'claim') return queue.shift() ?? null
      if (op === 'receipt') return { received: false }
      return {}
    },
    ...overrides,
  })
  return {
    receiver,
    calls,
    inserted,
    queue,
    select: (value) => {
      session = value
    },
  }
}
test('receiver records insertion intent before native insertion and never treats it as receipt', async () => {
  const s = fixture()
  s.queue.push({
    id: 'claim',
    result: 'd-1',
    text: 'whole body',
    receiver: { session: 'native-a', lease: 'epoch-1' },
  })
  await s.receiver.poll()
  assert.equal(s.inserted[0].text, 'whole body')
  assert.deepEqual(
    s.calls.map((call) => call.op),
    ['state', 'register', 'claim', 'begin', 'receipt'],
  )
  await s.receiver.poll()
  assert.equal(s.inserted.length, 1)
})

test('native selection changing during claim prevents insertion into the predecessor', async () => {
  let selected = 'a'
  const sent = [],
    ops = []
  const receiver = createReceiver({
    session: () => selected,
    ready: () => true,
    insert: () => sent.push('bad'),
    request: async (op) => {
      ops.push(op)
      if (op === 'state') return null
      if (op === 'register') return { lease: 'epoch', session: selected }
      if (op === 'claim') {
        selected = 'b'
        return { id: 'c', result: 'd-1', text: 'body', receiver: { lease: 'epoch', session: 'a' } }
      }
      return {}
    },
  })
  await receiver.poll()
  assert.equal(sent.length, 0)
  assert.deepEqual(ops, ['state', 'register', 'claim', 'release'])
})

test('a native insertion exception remains uncertain and is not retried by the receiver', async () => {
  const s = fixture({
    insert: async () => {
      throw new Error('native response lost')
    },
  })
  s.queue.push({
    id: 'c',
    result: 'd-1',
    text: 'body',
    receiver: { session: 'native-a', lease: 'epoch-1' },
  })
  await assert.rejects(s.receiver.poll(), /native response lost/)
  const release = s.calls.find((call) => call.op === 'release')
  assert.equal(release.body.admitted, null)
  assert.equal(release.body.bytesWritten, undefined)
  await s.receiver.poll()
  assert.equal(s.calls.filter((call) => call.op === 'begin').length, 1)
})

test('new, home route, resume and shutdown use fresh native selection and retire authority', async () => {
  const s = fixture()
  await s.receiver.poll()
  s.select(null)
  await s.receiver.poll()
  s.select('native-b')
  await s.receiver.poll()
  s.select('native-a')
  await s.receiver.poll()
  await s.receiver.stop()
  assert.deepEqual(
    s.calls.filter((call) => call.op === 'register').map((call) => call.body.session),
    ['native-a', 'native-b', 'native-a'],
  )
  assert.equal(s.calls.filter((call) => call.op === 'retire').length, 2)
  const before = s.calls.length
  await s.receiver.poll()
  assert.equal(s.calls.length, before)
})
