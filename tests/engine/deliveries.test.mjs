import assert from 'node:assert/strict'
import test from 'node:test'
import { digest, envelope, legacyReceipt, partsFor } from '../../hosts/lib/deliveries.js'

const MANY_PARTS = { bytes: 130, lines: 10 }
const ONE_PART = { bytes: 400, lines: 40 }
const LONG = 'alpha\nbeta\ngamma\ndelta\n'.repeat(4)
const byteLength = (text) => Buffer.byteLength(text, 'utf8')
const countLines = (text) => text.split('\n').length - 1

test('partsFor: every part opens with its size and closes with the delivery end marker', () => {
  const parts = partsFor(LONG, 'd-7', MANY_PARTS)
  assert.ok(parts.length > 1)
  parts.forEach((part, index) => {
    assert.equal(part.open, `[part ${index + 1} of ${parts.length} — ${part.bodyBytes} bytes]`)
    assert.equal(part.close, `[end of part ${index + 1} of ${parts.length} — delivery d-7]`)
    assert.equal(part.bodyBytes, byteLength(part.body))
  })
  assert.equal(parts.map((part) => part.body).join(''), LONG)
})

// finding 2 — the framing is injective: one printed part, one body, one digest
test('partsFor: bodies that differ by one newline print differently', () => {
  const short = partsFor('abc', 'd-1', ONE_PART)[0]
  const long = partsFor('abc\n', 'd-1', ONE_PART)[0]
  assert.notEqual(short.text, long.text, 'the same delivery id, two bodies, two printed parts')
  assert.equal(short.bodyBytes, 3)
  assert.equal(long.bodyBytes, 4)
  assert.notEqual(short.digest, long.digest)
})

test('partsFor: the printed part carries its framing and the next-part line', () => {
  const parts = partsFor(LONG, 'd-7', MANY_PARTS)
  assert.ok(parts.length > 1)
  parts.forEach((part, index) => {
    const lines = part.text.split('\n')
    assert.equal(lines[0], part.open)
    assert.ok(part.text.includes(part.body))
    assert.ok(part.text.endsWith('\n'))
    if (index < parts.length - 1) {
      assert.match(part.next, /cf read d-7 --part \d+/)
      assert.ok(part.text.includes(part.next))
    } else {
      assert.equal(part.next, null, 'the last part has nowhere to point')
    }
  })
})

test('partsFor: bytes and lines describe the printed part, not the bare body', () => {
  for (const part of partsFor(LONG, 'd-7', MANY_PARTS)) {
    assert.equal(part.bytes, byteLength(part.text))
    assert.equal(part.lines, countLines(part.text))
    assert.ok(part.bytes > part.bodyBytes, 'framing counts')
  }
})

test('partsFor: a 130-byte budget never prints more than 130 bytes', () => {
  const parts = partsFor('z'.repeat(200), 'd-7', { bytes: 130, lines: 50 })
  assert.ok(parts.length > 1, 'one 200-byte body cannot be printed inside 130 bytes')
  for (const part of parts) {
    assert.ok(byteLength(part.text) <= 130, `part ${part.k} printed ${byteLength(part.text)} bytes`)
  }
  // The framing reserves the widest byte-count marker the budget could need,
  // so a full part lands within that one digit of the limit.
  assert.ok(parts[0].bytes >= 129, `the first part fills the budget: ${parts[0].bytes} of 130`)
  assert.equal(parts.map((part) => part.body).join(''), 'z'.repeat(200))
})

test('partsFor: a five-line budget never prints more than five lines', () => {
  const parts = partsFor('one\ntwo\nthree\nfour\n', 'd-7', { bytes: 10_000, lines: 5 })
  assert.ok(parts.length > 1)
  for (const part of parts) {
    assert.ok(countLines(part.text) <= 5, `part ${part.k} printed ${countLines(part.text)} lines`)
  }
  assert.equal(parts[0].lines, 5, 'the first part fills the budget to the line')
  assert.equal(parts.map((part) => part.body).join(''), 'one\ntwo\nthree\nfour\n')
})

test('partsFor: a budget that cannot hold the framing plus one character is refused', () => {
  assert.throws(() => partsFor('anything', 'd-7', { bytes: 1, lines: 50 }), /budget/)
  assert.throws(() => partsFor('anything', 'd-7', { bytes: 10_000, lines: 2 }), /budget/)
  assert.throws(() => partsFor('anything', 'd-7', { bytes: 0, lines: 0 }), /budget/)
  assert.throws(() => partsFor('anything', 'd-7', { bytes: 1.5, lines: 5 }), /budget/)
  assert.equal(partsFor('anything', 'd-7').length, 1, 'no budget is the 32 KiB default')
  assert.throws(() => partsFor('anything', 'd-7', { bytes: '100', lines: 5 }), /budget/)
  const framing = byteLength('[part 1 of 1 — 4 bytes]\n[end of part 1 of 1 — delivery d-7]\n')
  assert.throws(() => partsFor('🎉', 'd-7', { bytes: framing + 2, lines: 50 }), /budget/)
})

test('partsFor: multi-byte characters split on code-point boundaries inside the budget', () => {
  const text = '日本語のテキスト'.repeat(20)
  const parts = partsFor(text, 'd-7', { bytes: 130, lines: 50 })
  assert.ok(parts.length > 1)
  for (const part of parts) {
    assert.ok(byteLength(part.text) <= 130, `part ${part.k} printed ${byteLength(part.text)} bytes`)
    assert.ok(!part.body.includes('�'))
  }
  assert.equal(parts.map((part) => part.body).join(''), text)
})

test('partsFor: an empty answer is one empty part', () => {
  const parts = partsFor('', 'd-7', ONE_PART)
  assert.equal(parts.length, 1)
  assert.equal(parts[0].body, '')
  assert.equal(parts[0].bodyBytes, 0)
  assert.equal(parts[0].text, '[part 1 of 1 — 0 bytes]\n[end of part 1 of 1 — delivery d-7]\n')
})

test('partsFor: the id it frames with must be a delivery id', () => {
  assert.throws(() => partsFor('anything', '../../escaped', ONE_PART), /delivery id/)
})

test('historical receipt verifies full native bodies and UUIDs without rewriting the old record', () => {
  const old = {
    id: 'd-9',
    conversation: 'worker',
    answerId: 'answer',
    answer: 'Done.',
    snapshot: { targetSession: 'old' },
    submittedAt: 100,
    channel: 'pty-inline',
    state: 'uncertain',
    nativeSubmissionId: 'exact',
  }
  old.digest = digest(envelope(old))
  const saved = structuredClone(old)
  const item = { id: 'exact', role: 'user', text: envelope(old) }
  assert.deepEqual(legacyReceipt(old, { session: 'old', items: [item] }), { ids: ['exact'] })
  for (const bad of [
    { ...item, role: 'assistant' },
    { ...item, id: 'foreign' },
    { ...item, text: item.text.slice(1) },
  ])
    assert.equal(legacyReceipt(old, { session: 'old', items: [bad] }), null)
  assert.equal(legacyReceipt(old, { session: 'other', items: [item] }), null)
  assert.deepEqual(old, saved)
})
test('historical manual parts require verified continuation and complete timestamped tool evidence', () => {
  const old = {
    id: 'd-8',
    channel: 'cf-read',
    state: 'submitting',
    manualRead: true,
    snapshot: { targetSession: 'old' },
    submittedAt: 100,
    parts: partsFor('whole body'.repeat(100), 'd-8', ONE_PART),
  }
  const items = old.parts.map((part, n) => ({
    id: `tool-${n}`,
    role: 'tool',
    at: new Date(300).toISOString(),
    text: part.text,
  }))
  const input = {
    session: 'successor',
    items,
    continuation: { from: 'old', to: 'successor', at: 200 },
  }
  assert.equal(legacyReceipt(old, input).parts.length, old.parts.length)
  assert.equal(legacyReceipt(old, { ...input, continuation: null }), null)
  assert.equal(legacyReceipt(old, { ...input, items: items.slice(1) }), null)
  assert.equal(
    legacyReceipt(old, {
      ...input,
      items: items.map((item) => ({ ...item, at: new Date(50).toISOString() })),
    }),
    null,
  )
  assert.equal(
    legacyReceipt(old, {
      ...input,
      items: items.map((item) => ({ ...item, text: item.text.replace('whole', 'altered') })),
    }),
    null,
  )
})
