import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import {
  beginInsertion,
  claimForRead,
  claimNext,
  emptyInbox,
  indexResult,
  observeReceipt,
  registerReceiver,
  releaseClaim,
  resultStatus,
} from '../hosts/lib/inbox.js'
import { Store } from '../src/store.js'
import { tempEnv } from './helpers.mjs'

const registration = (session = 'native-a', previous = null) => ({
  owner: 'lead',
  launch: 'launch-a',
  pane: 'p-1',
  generation: 1,
  kind: 'codex',
  session,
  previous,
  lease: `lease-${session}`,
  now: 100,
})
const add = (state, n, overrides = {}) =>
  indexResult(state, {
    id: `d-${n}`,
    owner: 'lead',
    conversation: 'worker',
    agent: 'zeus',
    kind: 'codex',
    session: 'worker-native',
    answerId: `answer-${n}`,
    answer: `Complete answer ${n}`,
    now: n,
    ...overrides,
  })
const claim = (state, id, overrides = {}) =>
  claimNext(state, {
    owner: 'lead',
    lease: 'lease-native-a',
    id,
    now: 101,
    ...overrides,
  })
const insert = (state, selected) =>
  beginInsertion(state, {
    owner: selected.receiver.owner,
    lease: selected.receiver.lease,
    result: selected.result,
    claim: selected.id,
    now: 102,
  })
const receipt = (state, selected, overrides = {}) =>
  observeReceipt(state, {
    result: selected.result,
    claim: selected.id,
    session: selected.receiver.session,
    kind: selected.receiver.kind,
    now: 103,
    items: [{ id: `native-${selected.id}`, role: 'tool', text: selected.text }],
    ...overrides,
  })

test('every completed reply has independent status; no first-N or latest-only limit', () => {
  const state = emptyInbox()
  for (let n = 1; n <= 100; n++) add(state, n)
  registerReceiver(state, registration())
  for (let n = 1; n <= 100; n++) {
    const selected = claim(state, `claim-${n}`)
    assert.equal(selected.result, `d-${n}`)
    assert.equal(resultStatus(state.results[selected.result]), 'collecting')
    insert(state, selected)
    receipt(state, selected)
    assert.equal(resultStatus(state.results[selected.result]), 'received')
  }
  assert.equal(claim(state, 'empty'), null)
  assert.equal(Object.keys(state.results).length, 100)
})

test('canonical parts fit the verified Claude synchronous hook context limit without native truncation', () => {
  const state = emptyInbox()
  const result = add(state, 1, { answer: 'A'.repeat(30_000) })
  assert.ok(result.parts.length >= 4)
  for (const part of result.parts) assert.ok(Buffer.byteLength(part.text) <= 8200)
})

test('explicit reads share the inbox result and require complete native receipt, including uncertain results', () => {
  const state = emptyInbox()
  const result = add(state, 1)
  registerReceiver(state, registration())
  const automatic = claim(state, 'automatic')
  insert(state, automatic)
  releaseClaim(state, { result: result.id, claim: automatic.id, admitted: null, now: 104 })
  const reading = claimForRead(state, {
    owner: 'lead',
    lease: 'lease-native-a',
    result: result.id,
    part: 1,
    id: 'reader',
    now: 105,
  })
  assert.equal(resultStatus(result), 'uncertain')
  assert.equal(reading.receiver.session, 'native-a')
  assert.equal(reading.state, 'submitting')
  assert.equal(receipt(state, reading), true)
  assert.equal(resultStatus(result), 'received')
  assert.throws(
    () =>
      claimForRead(state, {
        owner: 'pm',
        lease: 'lease-native-a',
        result: result.id,
        part: 1,
        id: 'other',
        now: 106,
      }),
    /receiver/,
  )
  assert.throws(
    () =>
      claimForRead(state, {
        owner: 'lead',
        lease: 'lease-native-a',
        result: result.id,
        part: 99,
        id: 'invalid',
        now: 106,
      }),
    /part/,
  )
})

test('immutable identity separates owners and source sessions and rejects changed bodies', () => {
  const state = emptyInbox()
  const original = add(state, 1)
  assert.equal(
    add(state, 999, { answerId: 'answer-1', answer: 'Complete answer 1' }).id,
    original.id,
  )
  assert.throws(() => add(state, 2, { answerId: 'answer-1', answer: 'rewritten' }), /immutable/)
  add(state, 2, { answerId: 'answer-1', owner: 'pm' })
  add(state, 3, { answerId: 'answer-1', session: 'another-native' })
  registerReceiver(state, registration())
  assert.equal(claim(state, 'one').result, 'd-1')
  assert.equal(claim(state, 'two').result, 'd-3')
  assert.equal(claim(state, 'three'), null)
  assert.equal(resultStatus(state.results['d-2']), 'waiting')
})

test('concurrent collectors cannot claim the same part; stale receivers cannot begin insertion', () => {
  const state = emptyInbox()
  add(state, 1)
  const first = registerReceiver(state, registration())
  const selected = claim(state, 'first')
  assert.equal(claim(state, 'competing'), null)
  registerReceiver(state, registration('native-b', first.lease))
  assert.throws(() => insert(state, selected), /receiver changed/)
  assert.throws(
    () => registerReceiver(state, registration('native-c', first.lease)),
    /receiver changed/,
  )
  const next = claim(state, 'successor', { lease: 'lease-native-b' })
  assert.equal(next.result, selected.result)
})

test('expiry before insertion releases a claim; possible insertion survives restart as uncertain', () => {
  let state = emptyInbox()
  add(state, 1)
  registerReceiver(state, registration())
  claim(state, 'expired')
  const selected = claim(state, 'live', { now: 100_000 })
  beginInsertion(state, {
    owner: 'lead',
    lease: 'lease-native-a',
    result: selected.result,
    claim: selected.id,
    now: 100_001,
  })
  state = JSON.parse(JSON.stringify(state))
  registerReceiver(state, registration('native-b', 'lease-native-a'))
  assert.equal(claim(state, 'never-replay', { lease: 'lease-native-b', now: 200_000 }), null)
  assert.equal(resultStatus(state.results['d-1']), 'uncertain')
  receipt(state, selected)
  assert.equal(resultStatus(state.results['d-1']), 'received')
  assert.equal(state.results['d-1'].claims.at(-1).receiver.session, 'native-a')
})

test('only an affirmative no-insertion refusal permits retry', () => {
  const state = emptyInbox()
  add(state, 1)
  registerReceiver(state, registration())
  const first = claim(state, 'one')
  insert(state, first)
  releaseClaim(state, {
    result: first.result,
    claim: first.id,
    admitted: false,
    bytesWritten: 0,
    now: 104,
  })
  const second = claim(state, 'two')
  insert(state, second)
  releaseClaim(state, { result: second.result, claim: second.id, admitted: null, now: 104 })
  assert.equal(claim(state, 'three'), null)
  assert.equal(resultStatus(state.results['d-1']), 'uncertain')
})

test('receipt requires complete native body, correct conversation and native provenance', () => {
  const state = emptyInbox()
  add(state, 1)
  registerReceiver(state, registration())
  const selected = claim(state, 'one')
  insert(state, selected)
  receipt(state, selected, { session: 'other' })
  receipt(state, selected, {
    items: [{ id: 'truncated', role: 'tool', text: selected.text.slice(-50) }],
  })
  receipt(state, selected, { items: [{ id: 'quote', role: 'assistant', text: selected.text }] })
  assert.equal(resultStatus(state.results['d-1']), 'collecting')
  receipt(state, selected)
  assert.equal(resultStatus(state.results['d-1']), 'received')
})

test('multipart coverage is complete in one receiver epoch; Unicode is retained exactly', () => {
  const state = emptyInbox()
  const result = add(state, 1, { answer: 'λ🙂\n'.repeat(500), budget: { bytes: 700, lines: 60 } })
  registerReceiver(state, registration())
  const first = claim(state, 'a')
  insert(state, first)
  receipt(state, first)
  assert.equal(resultStatus(result), 'waiting')
  registerReceiver(state, registration('native-b', 'lease-native-a'))
  let n = 0
  for (;;) {
    const selected = claim(state, `b-${n++}`, { lease: 'lease-native-b' })
    if (!selected) break
    if (n === 1) assert.equal(selected.part, 1)
    insert(state, selected)
    receipt(state, selected)
  }
  assert.equal(n - 1, result.parts.length)
  assert.equal(resultStatus(result), 'received')
  assert.ok(result.parts.length > 3)
})

test('Store serializes inbox updates under its private root and preserves them across restart', async () => {
  const fixture = tempEnv()
  const project = path.join(fixture.root, 'project')
  await fs.mkdir(project)
  const store = new Store(fixture.env.CONSENSFLOW_HOME)
  try {
    await store.open()
    await Promise.all(
      Array.from({ length: 30 }, (_, index) =>
        store.mutate(project, 'inbox.test', async (io) => {
          const state = await io.readInbox()
          add(state, index + 1)
          await io.writeInbox(state)
        }),
      ),
    )
    await store.close()
    await store.open()
    assert.equal(Object.keys((await store.readInbox(project)).results).length, 30)
    assert.deepEqual(await fs.readdir(project), [])
    const directory = path.join(store.root, 'workspaces')
    const [workspace] = await fs.readdir(directory)
    assert.equal(
      JSON.parse(await fs.readFile(path.join(directory, workspace, 'inbox.json'))).version,
      1,
    )
  } finally {
    await store.close()
    fixture.cleanup()
  }
})

test('an empty but completed answer is retained with its own immutable result identity', () => {
  const state = emptyInbox()
  const result = add(state, 1, { answer: '' })
  assert.equal(result.answer, '')
  assert.equal(result.parts.length, 1)
})
