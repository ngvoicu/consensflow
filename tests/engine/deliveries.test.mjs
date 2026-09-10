import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { itemsAfterCursor } from '../../hosts/lib/completion.js'
import {
  cancel,
  coverage,
  DEFAULT_INLINE_BUDGET,
  DEFAULT_PART_BUDGETS,
  DEFAULT_RECEIPT_MS,
  digest,
  envelope,
  fail,
  isReserved,
  partsFor,
  plan,
  pointer,
  receipt,
  recover,
  resend,
  safelyRepresentable,
  seenAfter,
  submit,
  writeFailed,
} from '../../hosts/lib/deliveries.js'

/**
 * Phase 3, TEST-PANE-29: delivery records — envelopes, receipts by digest
 * after an OPAQUE cursor, `cf read` parts and coverage. Pure module: no I/O,
 * no env, no ids of its own and no clock of its own.
 *
 * Contract under test (SPEC.md Architecture, "Completion, readiness,
 * delivery", the Acceptance Criteria on deliveries and unread bookkeeping,
 * the Decision Log rows of 2026-09-07, and the lead's round-3 decisions):
 *
 *  - every delivery id comes from the store's allocator and is validated
 *    `d-<digits>` — filename-safe, refused anywhere else; `plan` refuses an id
 *    any record it can see already carries and `resend` refuses its
 *    predecessor's; every keyed map the module returns is null-prototype;
 *  - one `pending` record per completed, uncovered answer under `auto`, keyed
 *    by (conversation, answerId);
 *  - the wire form is the envelope and `digest` covers its canonical form;
 *  - `submit` validates the ownership captured at plan time and stores a
 *    snapshot {targetSession, generation, cursor, evidenceType};
 *  - `accepted` needs THIS delivery id and digest in evidence of the RIGHT
 *    provenance after the pre-submission cursor: a user item for
 *    `pty-inline`, a tool result for `cf-read`;
 *  - a cursor is an opaque token: the adapter's `itemsAfter(cursor)` is the
 *    only thing that positions it, and a transcript that cannot be read is
 *    told apart from one verified empty;
 *  - part framing is INJECTIVE — the open marker records the body's exact byte
 *    length, so one printed part parses to exactly one body and one digest;
 *  - the part budget covers the COMPLETE rendered output;
 *  - one transition table: `failed` needs affirmative zero bytes, an unknown
 *    write outcome is `uncertain`, `accepted` and `cancelled` are terminal;
 *  - `coverage` maps a delivery to the WORKER answer ids it covers; `seen` is
 *    keyed by lead identity AND conversation, an array of worker item ids,
 *    advancing from the start of the transcript over marked, covered or
 *    printed items and stopping at the first that is none of those.
 */

const AUTO = { mode: 'auto', source: 'default' }
const MANUAL = { mode: 'manual', source: 'tab-human' }

const TARGET = {
  leadId: 'tab:t-1:1',
  session: 'lead-1',
  tab: 't-1',
  pane: 'p-2',
  generation: 1,
}

const row = { name: 'nyx-coral-lane', agent: 'nyx' }

const PLANNED_AT = 100
const NOW = 1_000

function answer(overrides = {}) {
  return {
    id: 'a-1',
    role: 'assistant',
    text: 'Done.',
    complete: true,
    settled: true,
    at: '2026-09-06T20:00:00.000Z',
    ...overrides,
  }
}

/** The store's allocator: `d-<digits>` from a counter that only ever climbs. */
function allocator(start = 1) {
  let n = start - 1
  return () => {
    n += 1
    return `d-${n}`
  }
}

/** `plan` with the identity and the clock the caller owes it. */
function planned({ items = [answer()], ...rest } = {}) {
  return plan({
    row,
    conversation: row.name,
    agent: row.agent,
    target: TARGET,
    newId: allocator(),
    now: PLANNED_AT,
    policy: AUTO,
    items,
    ...rest,
  })
}

/**
 * The adapter side of the opaque-cursor contract: only the adapter positions a
 * cursor. An unknown cursor is not "everything" and not "nothing" — it is
 * evidence that could not be read, and comes back `null`, exactly as
 * `completion.js::itemsAfterCursor` reports it.
 */
function transcript(items) {
  return (cursor) => {
    const at = items.findIndex((entry) => entry?.id === cursor)
    return at === -1 ? null : items.slice(at + 1)
  }
}

function context(items, extra = {}) {
  return {
    session: TARGET.session,
    generation: TARGET.generation,
    itemsAfter: transcript(items),
    now: NOW,
    ...extra,
  }
}

function item(overrides = {}) {
  return { role: 'user', text: '', complete: true, settled: true, at: null, ...overrides }
}

const CURSOR = { id: 'i-0', role: 'assistant', text: 'settled', complete: true, settled: true }

function submitted(record, overrides = {}) {
  return submit(record, { target: TARGET, cursor: 'i-0', now: PLANNED_AT, ...overrides })
}

/** A budget that admits a real multi-part split; the framing alone costs ~90 bytes. */
const MANY_PARTS = { bytes: 130, lines: 10 }
const ONE_PART = { bytes: 400, lines: 40 }
const LONG = 'alpha\nbeta\ngamma\ndelta\n'.repeat(4)

const byteLength = (text) => Buffer.byteLength(text, 'utf8')
const countLines = (text) => text.split('\n').length - 1

/** A cf-read record, submitted, plus the tool results that print it honestly. */
function fileDelivery({ text = LONG, budget = MANY_PARTS, ...rest } = {}) {
  const [record] = planned({
    items: [answer({ id: 'a-f', text })],
    inlineBudget: 10,
    partBudget: { test: budget },
    kind: 'test',
    ...rest,
  })
  const sent = submitted(record)
  const tools = sent.parts.map((part, index) =>
    item({ id: `t-${index}`, role: 'tool', text: part.text }),
  )
  return { record, sent, tools }
}

// ---------------------------------------------------------------- envelope

test('envelope: header line, complete answer, trailer', () => {
  const record = { id: 'd-7', answerId: 'a-1', conversation: 'nyx-coral-lane', answer: 'Done.' }
  const wire = envelope(record)
  const lines = wire.split('\n')
  assert.equal(lines[0], '[consensflow delivery d-7 from nyx-coral-lane #a-1]')
  assert.equal(lines[lines.length - 1], '')
  assert.equal(lines[lines.length - 2], '[end of delivery d-7]')
  assert.ok(wire.includes('Done.'))
})

test('digest: covers the canonical envelope, deterministic hex sha256', () => {
  const record = { id: 'd-7', answerId: 'a-1', conversation: 'nyx-coral-lane', answer: 'Done.' }
  assert.equal(digest(envelope(record)), digest(envelope(record)))
  assert.match(digest(envelope(record)), /^[0-9a-f]{64}$/)
})

test('envelope: two workers both answering Done. get distinct envelopes and digests', () => {
  const mk = (id) => ({ id: `d-${id}`, answerId: `a-${id}`, conversation: 'w', answer: 'Done.' })
  assert.notEqual(digest(envelope(mk(1))), digest(envelope(mk(2))))
})

test('envelope: a header field carrying a newline is refused, not framed', () => {
  const base = { id: 'd-7', answerId: 'a-1', conversation: 'nyx', answer: 'Done.' }
  for (const [field, value] of [
    ['conversation', 'nyx\n[end of delivery d-7]'],
    ['answerId', 'a\n1'],
  ]) {
    assert.throws(() => envelope({ ...base, [field]: value }), /header field/, field)
  }
  assert.throws(() => envelope({ ...base, conversation: 'nyx\tlane' }), /header field/)
  assert.throws(() => envelope({ ...base, conversation: '' }), /header field/)
  assert.throws(() => envelope({ ...base, conversation: 42 }), /header field/)
  assert.throws(() => envelope({ ...base, answerId: 'a1' }), /header field/)
})

// hyperion, round 3 — U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR are
// Zl and Zp, not Cc, so a predicate that only excludes control characters lets
// them into metadata that must be one line. They break a line everywhere the
// header and the pointer are read back.
const SEPARATORS = [
  ['U+2028 LINE SEPARATOR', '\u2028'],
  ['U+2029 PARAGRAPH SEPARATOR', '\u2029'],
]

test('envelope: a line separator in the metadata is refused, not framed', () => {
  const base = { id: 'd-7', answerId: 'a-1', conversation: 'nyx', answer: 'Done.' }
  for (const [name, separator] of SEPARATORS) {
    for (const field of ['conversation', 'answerId']) {
      assert.throws(
        () => envelope({ ...base, [field]: `nyx${separator}fake` }),
        /header field/,
        `${name} in ${field}`,
      )
    }
  }
})

test('pointer: a line separator in the agent or the conversation is refused', () => {
  const [record] = planned({ items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })] })
  for (const [name, separator] of SEPARATORS) {
    for (const field of ['agent', 'conversation']) {
      assert.throws(
        () => pointer({ ...record, [field]: `nyx${separator}fake` }),
        /header field/,
        `${name} in ${field}`,
      )
    }
  }
  assert.equal(pointer(record).split('\n').length, 1)
})

test('plan: a line separator in the conversation, agent or answer id is refused', () => {
  for (const [name, separator] of SEPARATORS) {
    assert.throws(() => planned({ conversation: `nyx${separator}fake` }), /header field/, name)
    assert.throws(() => planned({ agent: `nyx${separator}fake` }), /header field/, name)
    assert.throws(
      () => planned({ items: [answer({ id: `a${separator}1` })] }),
      /header field/,
      `${name} in the answer id`,
    )
  }
})

// The line goes the other way too: an ordinary space is not a line break, and
// a name that holds one is still one line.
test('envelope and pointer: a space in the metadata is ordinary text', () => {
  const [record] = planned({
    items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })],
    conversation: 'nyx coral lane',
    agent: 'nyx the second',
  })
  assert.equal(
    envelope(record).split('\n')[0],
    '[consensflow delivery d-1 from nyx coral lane #a-big]',
  )
  assert.equal(
    pointer(record),
    '@nyx the second answered in nyx coral lane — run: cf read d-1  (it prints everything; read all of it)',
  )
  assert.equal(pointer(record).split('\n').length, 1)
})

// The other half of the boundary: the arbiter accepts these in body text, so
// the answer keeps them and stays inline. Metadata is one line; a body is not.
test('safelyRepresentable: a line separator in the body is fine, and stays inline', () => {
  for (const [name, separator] of SEPARATORS) {
    assert.equal(safelyRepresentable(`before${separator}after`), true, name)
    const [record] = planned({ items: [answer({ text: `before${separator}after` })] })
    assert.equal(record.channel, 'pty-inline', name)
    assert.equal(record.answer, `before${separator}after`)
    assert.equal(record.normalized, undefined)
  }
})

// -------------------------------------------------------- id namespace (1)

test('ids: only the store allocator’s d-<digits> is a delivery id', () => {
  for (const id of [
    '../../escaped',
    'd-1/../../escaped',
    'd-abc',
    'd-',
    '',
    '__proto__',
    'd-1.md',
    'd 1',
    'D-1',
    7,
    null,
  ]) {
    assert.throws(() => planned({ newId: () => id }), /delivery id/, JSON.stringify(id))
  }
  assert.equal(planned({ newId: () => 'd-42' })[0].id, 'd-42')
})

test('ids: the envelope refuses a delivery id it was not allocated', () => {
  const base = { answerId: 'a-1', conversation: 'nyx', answer: 'Done.' }
  assert.throws(() => envelope({ ...base, id: '../../escaped' }), /delivery id/)
  assert.equal(
    envelope({ ...base, id: 'd-7' })
      .split('\n')[0]
      .includes('d-7'),
    true,
  )
})

test('ids: a delivery file always resolves inside the workspace deliveries directory', () => {
  const [record] = planned({
    items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })],
    workspace: '/w/s',
    newId: () => 'd-9',
  })
  assert.equal(record.file, '/w/s/deliveries/d-9.md')
  assert.equal(path.resolve(record.file), '/w/s/deliveries/d-9.md')
  assert.ok(path.resolve(record.file).startsWith('/w/s/deliveries/'))
})

test('ids: plan refuses an id any record it can see already carries', () => {
  const [mine] = planned({ items: [answer({ id: 'a-9' })], newId: () => 'd-1' })
  assert.throws(
    () => planned({ items: [answer({ id: 'a-1' })], deliveries: [mine], newId: () => 'd-1' }),
    /already/,
  )
  assert.throws(
    () => planned({ items: [answer({ id: 'a-1' }), answer({ id: 'a-2' })], newId: () => 'd-1' }),
    /already/,
    'and one it minted a moment ago in this very call',
  )
})

test('ids: a resend refuses its predecessor’s id', () => {
  const [record] = planned()
  assert.throws(() => resend(record, { id: record.id, now: 2_000 }), /own id|predecessor/)
  assert.throws(() => resend(record, { id: '../../escaped', now: 2_000 }), /delivery id/)
  assert.equal(resend(record, { id: 'd-2', now: 2_000 }).id, 'd-2')
})

test('ids: coverage keys are data, so a __proto__ id is an entry like any other', () => {
  const covered = coverage([
    { id: '__proto__', state: 'accepted', answerId: 'a-1' },
    { id: 'd-1', state: 'accepted', answerId: 'a-2' },
  ])
  assert.equal(Object.getPrototypeOf(covered), null)
  assert.deepEqual(Object.keys(covered).sort(), ['__proto__', 'd-1'])
  assert.deepEqual(Object.getOwnPropertyDescriptor(covered, '__proto__')?.value, ['a-1'])
  assert.deepEqual(covered['d-1'], ['a-2'])
})

// ------------------------------------------------------- safelyRepresentable

test('safelyRepresentable: every C0 control but TAB and LF is refused', () => {
  assert.equal(safelyRepresentable('plain text'), true)
  assert.equal(safelyRepresentable('a\tb\nc'), true)
  for (let code = 0x00; code <= 0x1f; code++) {
    const text = `a${String.fromCharCode(code)}b`
    const expected = code === 0x09 || code === 0x0a
    assert.equal(safelyRepresentable(text), expected, `U+${code.toString(16)}`)
  }
  assert.equal(safelyRepresentable('a\x7fb'), false)
})

test('safelyRepresentable: C1 controls are refused, as the arbiter refuses them', () => {
  for (let code = 0x7f; code <= 0x9f; code++) {
    assert.equal(safelyRepresentable(`a${String.fromCodePoint(code)}b`), false, `U+${code}`)
  }
  assert.equal(safelyRepresentable('a\u009bb'), false)
  assert.equal(safelyRepresentable('café “naïve” 日本語 🎉'), true)
})

test('safelyRepresentable: a lone surrogate has no lossless UTF-8 form', () => {
  assert.equal(safelyRepresentable('a\ud800b'), false)
  assert.equal(safelyRepresentable('a\udfffb'), false)
  assert.equal(safelyRepresentable('a🎉b'), true, 'a well-formed pair is fine')
})

test('safelyRepresentable: CRLF is refused — the arbiter rewrites it and breaks the digest', () => {
  assert.equal(safelyRepresentable('first\r\nsecond'), false)
  assert.equal(safelyRepresentable('first\rsecond'), false)
})

// -------------------------------------------------------------------- plan

test('plan: one pending record per completed, uncovered answer under auto', () => {
  const records = planned({
    items: [answer({ id: 'a-1' }), answer({ id: 'a-2', text: 'Second.' })],
  })
  assert.equal(records.length, 2)
  for (const record of records) {
    assert.equal(record.state, 'pending')
    assert.ok(record.digest)
    assert.ok(record.channel)
    assert.equal(record.conversation, row.name)
    assert.equal(record.agent, row.agent)
    assert.equal(record.createdAt, PLANNED_AT)
  }
  assert.deepEqual(
    records.map((record) => record.id),
    ['d-1', 'd-2'],
    'ids come from the injected allocator, in order',
  )
  assert.notEqual(records[0].digest, records[1].digest)
})

test('plan: the caller injects the delivery id; a missing allocator is an error', () => {
  assert.throws(
    () =>
      plan({
        row,
        conversation: row.name,
        agent: row.agent,
        target: TARGET,
        now: PLANNED_AT,
        policy: AUTO,
        items: [answer()],
      }),
    /newId/,
  )
})

test('plan: the conversation, agent and target lead are required identity', () => {
  assert.throws(
    () => plan({ items: [answer()], policy: AUTO, target: TARGET, newId: allocator(), now: 1 }),
    /conversation/,
  )
  assert.throws(() => planned({ row: { name: row.name }, agent: undefined }), /agent/)
  assert.throws(() => planned({ agent: 'nyx\nfake' }), /header field/)
  assert.throws(() => planned({ target: undefined }), /target/)
  assert.throws(() => planned({ target: { ...TARGET, generation: 0 } }), /generation/)
  assert.throws(() => planned({ target: { ...TARGET, leadId: '' } }), /leadId/)
  assert.throws(() => planned({ target: [TARGET] }), /target/)
})

// finding 8 — the clock is injected, and there is no fallback to Date.now()
test('plan: the time is injected and required', () => {
  assert.throws(() => planned({ now: undefined }), /now/)
  assert.throws(() => planned({ now: '100' }), /now/)
  assert.throws(() => planned({ now: Number.NaN }), /now/)
  assert.equal(planned({ now: 7 })[0].createdAt, 7)
})

test('plan: the same native answer id in two conversations makes two distinct deliveries', () => {
  const items = [answer({ id: 'msg_01', text: 'Done.' })]
  const newId = allocator()
  const first = planned({ items, conversation: 'nyx-coral-lane', newId })
  const second = planned({ items, conversation: 'zeus-amber-fen', newId })
  assert.notEqual(first[0].id, second[0].id)
  assert.notEqual(first[0].digest, second[0].digest)
  assert.notEqual(first[0].conversation, second[0].conversation)
})

test('plan: dedupe is by (conversation, answerId), never by a bare answer id', () => {
  const items = [answer({ id: 'msg_01', text: 'Done.' })]
  const [mine] = planned({ items, conversation: 'nyx-coral-lane' })
  const live = submitted(mine)
  assert.equal(
    planned({ items, conversation: 'nyx-coral-lane', deliveries: [live], newId: allocator(9) })
      .length,
    0,
  )
  assert.equal(
    planned({ items, conversation: 'zeus-amber-fen', deliveries: [live], newId: allocator(9) })
      .length,
    1,
    'another conversation’s delivery covers nothing of mine',
  )
})

test('plan: incomplete answers are never planned', () => {
  const records = planned({
    items: [
      answer({ id: 'a-1', complete: false, settled: false }),
      answer({ id: 'u-1', role: 'user', text: 'hi' }),
    ],
  })
  assert.equal(records.length, 0)
})

test('plan: manual policy plans nothing unless asked manually', () => {
  assert.equal(planned({ policy: MANUAL }).length, 0)
  const records = planned({ policy: MANUAL, manual: true })
  assert.equal(records.length, 1)
  assert.equal(records[0].manual, true)
})

test('plan: derives the effective policy from tab, pane and row via policy.js', () => {
  assert.equal(planned({ policy: undefined, tab: { policy: 'manual' }, pane: {} }).length, 0)
  assert.equal(planned({ policy: undefined, tab: {}, pane: {} }).length, 1)
})

test('plan: terminal attempts never create another automatic delivery for the same answer', () => {
  const items = [answer({ id: 'a-1' }), answer({ id: 'a-2' }), answer({ id: 'a-3' })]
  const old = planned({ items })
  const deliveries = [
    { ...old[0], state: 'accepted' },
    { ...old[1], state: 'failed' },
    { ...old[2], state: 'cancelled' },
  ]
  const records = planned({ items, deliveries, newId: allocator(10) })
  assert.equal(records.length, 0)
})

test('plan: an uncertain delivery is never re-planned', () => {
  const items = [answer({ id: 'a-1' })]
  const [pending] = planned({ items })
  const stuck = recover(submitted(pending))
  assert.equal(stuck.state, 'uncertain')
  assert.equal(planned({ items, deliveries: [stuck], newId: allocator(10) }).length, 0)
})

test('plan: an answer already covered by the seen bookkeeping is not planned again', () => {
  const items = [answer({ id: 'a-1' }), answer({ id: 'a-2' })]
  const records = planned({ items, coveredIds: ['a-1'] })
  assert.equal(records.length, 1)
  assert.equal(records[0].answerId, 'a-2')
})

test('plan: small ASCII answers go pty-inline with the envelope digest', () => {
  const [record] = planned()
  assert.equal(record.channel, 'pty-inline')
  assert.equal(record.digest, digest(envelope(record)))
  assert.equal(record.answer, 'Done.')
  assert.equal(record.normalized, undefined)
})

test('plan: default inline budget is 4 000 bytes', () => {
  assert.equal(DEFAULT_INLINE_BUDGET, 4000)
})

test('plan: envelope over the inline budget goes cf-read, never truncated', () => {
  const big = 'x'.repeat(60_000)
  const [record] = planned({ items: [answer({ id: 'a-big', text: big })] })
  assert.equal(record.channel, 'cf-read')
  assert.equal(record.answer, big)
  assert.ok(envelope(record).includes(big))
  assert.ok(record.file.endsWith(`/deliveries/${record.id}.md`))
  assert.ok(record.parts.length > 1)
})

test('plan: a fixture with an ESC byte goes cf-read', () => {
  const [record] = planned({ items: [answer({ text: 'hello\x1b[2Jworld' })] })
  assert.equal(record.channel, 'cf-read')
})

test('plan: control bytes force cf-read, LF and TAB stay inline', () => {
  assert.equal(planned({ items: [answer({ text: 'a\tb\nc' })] })[0].channel, 'pty-inline')
  for (const text of ['a\x00b', 'a\x07b', 'a\rb', 'a\x7fb', 'a\u009bb']) {
    const [record] = planned({ items: [answer({ text })] })
    assert.equal(record.channel, 'cf-read', JSON.stringify(text))
  }
})

test('plan: inline budget is per lead-harness kind, or one number for all', () => {
  const items = [answer({ text: 'y'.repeat(5000) })]
  assert.equal(
    planned({ items, kind: 'huge', inlineBudget: { huge: 100_000 } })[0].channel,
    'pty-inline',
  )
  assert.equal(planned({ items, kind: 'tiny', inlineBudget: { tiny: 100 } })[0].channel, 'cf-read')
  assert.equal(planned({ items, inlineBudget: 100_000 })[0].channel, 'pty-inline')
  assert.equal(planned({ items, inlineBudget: { default: 100 } })[0].channel, 'cf-read')
})

test('plan: the record captures the intended lead, tab, pane and generation', () => {
  const [record] = planned()
  assert.deepEqual(record.target, TARGET)
  assert.equal(record.state, 'pending')
})

// finding 5 — the channel is the module's decision, and nothing overrides it
test('plan: no caller callback can override the channel the module decided', () => {
  const forced = planned({
    items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })],
    channelFor: () => 'pty-inline',
  })[0]
  assert.equal(forced.channel, 'cf-read', 'an over-budget answer is a file, whatever a caller asks')
  assert.ok(forced.file)
  assert.ok(forced.parts.length > 1)
  const unsafe = planned({
    items: [answer({ text: 'hello\x1b[2Jworld' })],
    channelFor: () => 'pty-inline',
  })[0]
  assert.equal(unsafe.channel, 'cf-read', 'and so is an unrepresentable one')
})

// finding 2 — normalisation happens once, at plan time
test('plan: a lone surrogate is normalised once, on the record, and marked', () => {
  const [record] = planned({ items: [answer({ id: 'a-s', text: 'a\ud800b' })] })
  assert.equal(record.answer, 'a�b', 'the record holds what will actually be written')
  assert.equal(record.normalized, true)
  assert.equal(record.answer.includes('\ud800'), false)
  assert.equal(record.digest, digest(envelope(record)))
  assert.equal(safelyRepresentable(record.answer), true)
  const [clean] = planned({ items: [answer({ text: 'a🎉b' })] })
  assert.equal(clean.normalized, undefined, 'a well-formed pair is not normalisation')
  assert.equal(clean.answer, 'a🎉b')
})

test('plan: the normalised body is what the parts carry', () => {
  const { record } = fileDelivery({ text: `a\ud800b\n${LONG}` })
  assert.equal(record.normalized, true)
  assert.equal(record.parts.map((part) => part.body).join(''), record.answer)
  assert.equal(record.answer.includes('\ud800'), false)
})

// ---------------------------------------------------------------- parts

test('plan: cf-read parts each fit the lead harness tool-output budget', () => {
  const big = `${'lorem ipsum dolor sit amet\n'.repeat(4000)}end`
  const [record] = planned({ items: [answer({ id: 'a-big', text: big })], kind: 'pi' })
  assert.equal(record.channel, 'cf-read')
  assert.ok(record.parts.length > 1)
  for (const part of record.parts) {
    assert.ok(part.bytes <= 40 * 1024, `part ${part.k} bytes ${part.bytes}`)
    assert.ok(part.lines <= 1500, `part ${part.k} lines ${part.lines}`)
    assert.match(part.digest, /^[0-9a-f]{64}$/)
  }
  assert.equal(record.parts.map((part) => part.body).join(''), big)
})

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

test('plan: a file write failure stays pending with the reason', () => {
  const [record] = planned({ items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })] })
  const held = writeFailed(record, 'EACCES: deliveries/d-1.md')
  assert.equal(held.state, 'pending')
  assert.equal(held.channel, 'cf-read')
  assert.match(held.reason, /EACCES/)
})

// ----------------------------------------------------------------- pointer

// finding 6 — SPEC.md line 317, to the character
test('pointer: the one line pasted for a cf-read delivery, exactly as specified', () => {
  const [record] = planned({ items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })] })
  assert.equal(
    pointer(record),
    '@nyx answered in nyx-coral-lane — run: cf read d-1  (it prints everything; read all of it)',
  )
  assert.equal(pointer(record).split('\n').length, 1)
})

test('pointer: the agent and the conversation are separate validated fields', () => {
  const [record] = planned({ items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })] })
  assert.equal(record.agent, 'nyx')
  assert.equal(record.conversation, 'nyx-coral-lane')
  assert.throws(() => pointer({ ...record, agent: 'nyx\nfake' }), /header field/)
  assert.throws(() => pointer({ ...record, conversation: '' }), /header field/)
  assert.throws(() => pointer({ ...record, id: '../../escaped' }), /delivery id/)
  const other = planned({
    items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })],
    agent: 'zeus',
    conversation: 'zeus-amber-fen',
    newId: allocator(5),
  })[0]
  assert.equal(
    pointer(other),
    '@zeus answered in zeus-amber-fen — run: cf read d-5  (it prints everything; read all of it)',
  )
})

// ------------------------------------------------------------------ submit

test('submit: stores the snapshot — target session, generation, cursor, evidence type', () => {
  const [pending] = planned()
  const record = submitted(pending)
  assert.equal(record.state, 'submitting')
  assert.deepEqual(record.snapshot, {
    targetSession: 'lead-1',
    generation: 1,
    cursor: 'i-0',
    evidenceType: 'user-item',
  })
  assert.equal(record.attempts, 1)
  assert.equal(record.submittedAt, PLANNED_AT)
  assert.equal(pending.state, 'pending', 'the input record is untouched')
})

test('submit: a cf-read delivery is answered by tool output, an inline one by a user turn', () => {
  const [inline] = planned()
  const { sent } = fileDelivery()
  assert.equal(submitted(inline).snapshot.evidenceType, 'user-item')
  assert.equal(sent.snapshot.evidenceType, 'tool-result')
  assert.throws(() => submitted(inline, { evidenceType: 'tool-result' }), /answered by/)
  assert.equal(submitted(inline, { evidenceType: 'user-item' }).state, 'submitting')
  assert.throws(() => submitted({ ...inline, channel: 'carrier-pigeon' }), /channel/)
})

test('submit: the pre-submission cursor is required, never defaulted', () => {
  const [pending] = planned()
  assert.throws(() => submit(pending, { target: TARGET, now: 1 }), /cursor/)
  assert.throws(() => submit(pending, { target: TARGET, cursor: null, now: 1 }), /cursor/)
  assert.throws(() => submit(pending, { target: TARGET, cursor: '', now: 1 }), /cursor/)
})

test('submit: the time is injected and required', () => {
  const [pending] = planned()
  assert.throws(() => submit(pending, { target: TARGET, cursor: 'i-0' }), /now/)
  assert.throws(() => submit(pending, { target: TARGET, cursor: 'i-0', now: null }), /now/)
})

test('submit: a target other than the one planned for is refused', () => {
  const [pending] = planned()
  for (const target of [
    { ...TARGET, generation: 2, leadId: 'tab:t-1:2' },
    { ...TARGET, session: 'lead-2' },
    { ...TARGET, pane: 'p-9' },
    { ...TARGET, tab: 't-2' },
  ]) {
    assert.throws(() => submitted(pending, { target }), /planned for/, target.leadId)
  }
  assert.throws(() => submit(pending, { cursor: 'i-0', now: 1 }), /target/)
})

test('submit: only a pending delivery is submitted', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  for (const state of ['submitting', 'accepted', 'uncertain', 'failed', 'cancelled']) {
    assert.throws(() => submitted({ ...sent, state }), /pending/, state)
  }
})

// ------------------------------------------------------------------ receipt

test('receipt: inline accepted for the envelope in a user turn after the cursor', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  const after = receipt(sent, sent.snapshot, context(items))
  assert.equal(after.state, 'accepted')
  assert.deepEqual(after.evidenceIds, ['i-1'])
  assert.equal(after.acceptedAt, NOW)
})

test('receipt: native peer delivery also requires the submitted user UUID', () => {
  const [pending] = planned()
  const sent = { ...submitted(pending), nativeSubmissionId: 'peer-submission-1' }
  const nativeText = `Another session sent a message.\n${envelope(sent)}`
  const wrong = [CURSOR, item({ id: 'other-submission', text: nativeText })]
  assert.equal(receipt(sent, sent.snapshot, context(wrong)).state, 'submitting')
  const correct = [CURSOR, item({ id: sent.nativeSubmissionId, text: nativeText })]
  assert.equal(receipt(sent, sent.snapshot, context(correct)).state, 'accepted')
})

test('receipt: an assistant message carrying the envelope is not a receipt', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const quoted = [CURSOR, item({ id: 'i-1', role: 'assistant', text: envelope(sent) })]
  assert.notEqual(receipt(sent, sent.snapshot, context(quoted)).state, 'accepted')
})

test('receipt: cf-read framing outside a tool result is not a receipt', () => {
  const { sent } = fileDelivery()
  for (const role of ['assistant', 'user']) {
    const items = [
      CURSOR,
      ...sent.parts.map((part, k) => item({ id: `x-${k}`, role, text: part.text })),
    ]
    assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted', role)
  }
})

test('receipt: the time is injected and required', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  assert.throws(() => receipt(sent, sent.snapshot, context(items, { now: undefined })), /now/)
})

test('receipt: a delivery that was never submitted cannot be accepted', () => {
  const [pending] = planned()
  const snapshot = {
    targetSession: 'lead-1',
    generation: 1,
    cursor: 'i-0',
    evidenceType: 'user-item',
  }
  const items = [CURSOR, item({ id: 'i-1', text: envelope(pending) })]
  assert.throws(() => receipt(pending, snapshot, context(items)), /never submitted/)
})

test('receipt: a snapshot other than the one submitted with is refused', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  for (const snapshot of [
    { ...sent.snapshot, cursor: 'i-9' },
    { ...sent.snapshot, generation: 2 },
    { ...sent.snapshot, targetSession: 'lead-2' },
    { ...sent.snapshot, evidenceType: 'tool-result' },
    null,
    undefined,
  ]) {
    assert.throws(() => receipt(sent, snapshot, context(items)), /snapshot/)
  }
  assert.throws(
    () => receipt({ ...sent, snapshot: undefined }, sent.snapshot, context(items)),
    /snapshot/,
  )
})

test('receipt: evidence from another session or generation is refused', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  assert.throws(
    () => receipt(sent, sent.snapshot, context(items, { session: 'lead-2' })),
    /session/,
  )
  assert.throws(() => receipt(sent, sent.snapshot, context(items, { generation: 2 })), /generation/)
  assert.throws(() => receipt(sent, sent.snapshot, context(items, { replaced: true })), /replaced/)
})

test('receipt: a transcript that cannot be read never times out; a verified empty one does', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const late = PLANNED_AT + DEFAULT_RECEIPT_MS + 1
  const unreadable = receipt(sent, sent.snapshot, { ...context([]), now: late })
  assert.equal(unreadable.state, 'submitting', 'the cursor was not found: no evidence was read')
  assert.equal(unreadable.reason, undefined)
  const missing = receipt(sent, sent.snapshot, {
    ...context([CURSOR]),
    itemsAfter: undefined,
    now: late,
  })
  assert.equal(missing.state, 'submitting', 'no reader at all decides nothing')
  const empty = receipt(sent, sent.snapshot, { ...context([CURSOR]), now: late })
  assert.equal(empty.state, 'uncertain', 'the transcript was read and holds no receipt')
  assert.match(empty.reason, /no receipt/)
})

test('receipt: two Done. answers yield two distinct acceptances', () => {
  const records = planned({
    items: [answer({ id: 'a-1', text: 'Done.' }), answer({ id: 'a-2', text: 'Done.' })],
  }).map((record) => submitted(record))
  const items = [
    CURSOR,
    item({ id: 'i-1', text: envelope(records[0]) }),
    item({ id: 'i-2', text: envelope(records[1]) }),
  ]
  const first = receipt(records[0], records[0].snapshot, context(items))
  const second = receipt(records[1], records[1].snapshot, context(items))
  assert.equal(first.state, 'accepted')
  assert.equal(second.state, 'accepted')
  assert.deepEqual(first.evidenceIds, ['i-1'])
  assert.deepEqual(second.evidenceIds, ['i-2'])
})

test('receipt: an older matching turn before the cursor is not a receipt', () => {
  const [pending] = planned()
  const sent = submitted(pending, { cursor: 'i-1' })
  const items = [
    item({ id: 'i-0', text: envelope(sent) }),
    item({ id: 'i-1', role: 'assistant', text: 'the cursor' }),
    item({ id: 'i-2', role: 'assistant', text: 'other work' }),
  ]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
})

test('receipt: an unrelated identical user message after the cursor is not a receipt', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: 'Done.' })]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
})

test('receipt: the same framing around an altered body is not a receipt', () => {
  const [pending] = planned({ items: [answer({ text: 'Ship it.' })] })
  const sent = submitted(pending)
  const forged = envelope(sent).replace('Ship it.', 'Do not ship it.')
  assert.ok(forged.includes('[consensflow delivery'))
  assert.ok(forged.includes(`[end of delivery ${sent.id}]`))
  const items = [CURSOR, item({ id: 'i-1', text: forged })]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
})

test('receipt: cf-read accepted only when every part shows framing AND digest', () => {
  const { sent, tools } = fileDelivery()
  assert.ok(sent.parts.length > 1)
  const after = receipt(sent, sent.snapshot, context([CURSOR, ...tools]))
  assert.equal(after.state, 'accepted')
  assert.deepEqual(
    after.evidenceIds,
    tools.map((tool) => tool.id),
  )
})

// finding 2 — a body one byte different, framed under the same delivery id
test('receipt: a cf-read body that differs by one byte is not a receipt', () => {
  const { record, sent } = fileDelivery({ text: 'abc', budget: ONE_PART })
  assert.equal(record.parts.length, 1)
  assert.equal(record.parts[0].body, 'abc')
  const forged = partsFor('abc\n', record.id, ONE_PART)[0]
  const items = [CURSOR, item({ id: 't-0', role: 'tool', text: forged.text })]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
  const honest = [CURSOR, item({ id: 't-0', role: 'tool', text: record.parts[0].text })]
  assert.equal(receipt(sent, sent.snapshot, context(honest)).state, 'accepted')
})

// finding 2 — the recorded length is what parses the body, so a receiver that
// adds a line inside the framing no longer satisfies a second reading of the
// same region. This is the case the marker alone cannot catch.
test('receipt: a printed part with a blank line added is not a receipt', () => {
  const { record, sent } = fileDelivery({ text: 'alpha\n', budget: ONE_PART })
  const part = record.parts[0]
  assert.ok(part.body.endsWith('\n'), 'a body that already closes its own line')
  const padded = part.text.replace(part.close, `\n${part.close}`)
  assert.ok(padded.includes(part.open), 'the open marker, byte count and all, is intact')
  assert.ok(padded.includes(part.close), 'and so is the end marker')
  assert.equal(padded.split('\n').length, part.text.split('\n').length + 1)
  const items = [CURSOR, item({ id: 't-0', role: 'tool', text: padded })]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
  const honest = [CURSOR, item({ id: 't-0', role: 'tool', text: part.text })]
  assert.equal(receipt(sent, sent.snapshot, context(honest)).state, 'accepted')
})

test('receipt: a body of the recorded length but altered content is not a receipt', () => {
  const { record, sent } = fileDelivery({ text: 'alpha\nbeta\n', budget: ONE_PART })
  const altered = record.parts[0].text.replace('beta', 'beto')
  assert.equal(byteLength(altered), byteLength(record.parts[0].text), 'same length, one letter')
  const items = [CURSOR, item({ id: 't-0', role: 'tool', text: altered })]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
})

test('receipt: end markers kept but text dropped cover nothing', () => {
  const { sent } = fileDelivery({ text: 'the real answer body', budget: ONE_PART })
  const stripped = sent.parts.map((part, index) =>
    item({ id: `t-${index}`, role: 'tool', text: `${part.open}\n[trimmed]\n${part.close}\n` }),
  )
  const after = receipt(sent, sent.snapshot, context([CURSOR, ...stripped]))
  assert.notEqual(after.state, 'accepted')
  assert.deepEqual(coverage([after])[sent.id], [])
})

test('receipt: a missing part leaves its range uncovered', () => {
  const { sent, tools } = fileDelivery()
  const after = receipt(sent, sent.snapshot, context([CURSOR, tools[0]]))
  assert.notEqual(after.state, 'accepted')
  assert.deepEqual(after.partCoverage[0], ['t-0'])
  assert.deepEqual(after.partCoverage[1], [])
})

test('receipt: a part whose body ends mid-line is covered by the printed form', () => {
  const { record, sent } = fileDelivery({ text: 'abc', budget: ONE_PART })
  assert.ok(!record.parts[0].body.endsWith('\n'), 'the body has no trailing newline')
  const printed = item({ id: 't-0', role: 'tool', text: record.parts[0].text })
  assert.equal(receipt(sent, sent.snapshot, context([CURSOR, printed])).state, 'accepted')
})

test('receipt: a cf-read record with no parts is never accepted', () => {
  const { sent } = fileDelivery()
  const empty = { ...sent, parts: [] }
  const after = receipt(empty, empty.snapshot, context([CURSOR]))
  assert.notEqual(after.state, 'accepted')
  const late = receipt(empty, empty.snapshot, {
    ...context([CURSOR]),
    now: PLANNED_AT + DEFAULT_RECEIPT_MS + 1,
  })
  assert.equal(late.state, 'uncertain')
})

test('receipt: uncertain after receiptMs', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const after = receipt(sent, sent.snapshot, {
    ...context([CURSOR]),
    now: PLANNED_AT + 60_001,
    receiptMs: 60_000,
  })
  assert.equal(after.state, 'uncertain')
})

test('receipt: terminal records pass through untouched', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  for (const state of ['accepted', 'cancelled', 'failed', 'uncertain']) {
    const record = { ...sent, state }
    assert.deepEqual(receipt(record, sent.snapshot, context(items)), record, state)
  }
})

// finding 7 — the real adapter function, bound to a kind and its items
test('receipt: composes completion.js itemsAfterCursor as the transcript reader', () => {
  // completion.js mints an integer cursor inside a per-harness namespace. The
  // deliveries module never reads one — it hands it back to the adapter — but
  // composing the REAL reader needs real ones.
  const cursorAt = (position) => 4_000_000_000_000_000 + 2 * 100_000_000_000 + position
  // The adapter validates the normalised item shape before it positions
  // anything, so these carry every field `answers()` emits — id, role, text,
  // complete, settled, at, seq — not just the ones a receipt reads.
  const leadItem = (overrides) => ({
    id: 'i-0',
    role: 'assistant',
    text: 'settled',
    complete: true,
    settled: true,
    at: '2026-09-06T20:00:00.000Z',
    seq: cursorAt(0),
    ...overrides,
  })
  const [pending] = planned()
  const sent = submitted(pending, { cursor: cursorAt(0) })
  const leadItems = [
    leadItem({}),
    leadItem({ id: 'i-1', role: 'user', text: envelope(sent), seq: cursorAt(1) }),
  ]
  const itemsAfter = (cursor) => itemsAfterCursor('claude-code', leadItems, cursor)
  assert.ok(
    Array.isArray(itemsAfter(cursorAt(0))),
    'completion.js still mints claude-code cursors in this namespace and still calls these ' +
      'items normalised — if it does not, fix leadItem/cursorAt here, not the module',
  )
  const after = receipt(sent, sent.snapshot, {
    session: TARGET.session,
    generation: TARGET.generation,
    itemsAfter,
    now: NOW,
  })
  assert.equal(after.state, 'accepted')
  assert.deepEqual(after.evidenceIds, ['i-1'])

  const foreign = submitted(planned({ newId: allocator(9) })[0], { cursor: 'not-a-cursor' })
  const unread = receipt(foreign, foreign.snapshot, {
    session: TARGET.session,
    generation: TARGET.generation,
    itemsAfter,
    now: PLANNED_AT + DEFAULT_RECEIPT_MS + 1,
  })
  assert.equal(unread.state, 'submitting', 'a cursor the adapter cannot place decides nothing')
})

// -------------------------------------------------------------- transitions

test('recover: crash recovery from submitting yields uncertain', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  assert.equal(recover(sent).state, 'uncertain')
  assert.match(recover(sent).reason, /restart recovery/)
  assert.equal(recover(sent, { reason: 'bridge EOF' }).reason, 'bridge EOF')
  assert.equal(recover({ ...sent, state: 'pending' }).state, 'pending')
  assert.equal(recover({ ...sent, state: 'accepted' }).state, 'accepted')
})

test('fail: failed needs an affirmative zero, an unknown outcome is uncertain', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  assert.equal(fail(sent, { bytesWritten: 0 }).state, 'failed')
  assert.equal(fail(sent, { bytesWritten: 128 }).state, 'uncertain')
  assert.match(fail(sent, { bytesWritten: 128 }).reason, /128 byte/)
  assert.equal(fail(sent, {}).state, 'uncertain', 'no byte count is not zero bytes')
  assert.equal(fail(sent).state, 'uncertain')
  assert.equal(fail(sent, { bytesWritten: null }).state, 'uncertain')
  assert.equal(fail(sent, { bytesWritten: '0' }).state, 'uncertain')
  assert.match(fail(sent, {}).reason, /unknown/)
  assert.equal(fail(sent, { bytesWritten: 0, reason: 'Draft' }).reason, 'Draft')
})

test('transitions: terminal records refuse every mutator', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  for (const state of ['accepted', 'cancelled']) {
    const record = { ...sent, state }
    assert.equal(fail(record, { bytesWritten: 0 }).state, state, `fail(${state})`)
    assert.equal(cancel(record).state, state, `cancel(${state})`)
    assert.equal(writeFailed(record, 'EACCES').state, state, `writeFailed(${state})`)
    assert.equal(recover(record).state, state, `recover(${state})`)
    assert.deepEqual(writeFailed(record, 'EACCES'), record, 'no reason is stamped either')
  }
})

test('transitions: an uncertain record may be cancelled but never resubmitted', () => {
  const [pending] = planned()
  const stuck = recover(submitted(pending))
  assert.equal(cancel(stuck).state, 'cancelled')
  assert.equal(cancel(stuck, { reason: 'human' }).reason, 'human')
  assert.equal(fail(stuck, { bytesWritten: 0 }).state, 'uncertain')
  assert.throws(() => submitted(stuck), /pending/)
})

test('writeFailed: only a pending record is held back for a write failure', () => {
  const [pending] = planned({ items: [answer({ id: 'a-big', text: 'z'.repeat(60_000) })] })
  assert.equal(writeFailed(pending, 'EACCES').state, 'pending')
  const sent = submitted(pending)
  assert.equal(writeFailed(sent, 'EACCES').state, 'submitting', 'a submitted delivery is past it')
})

test('cancel: terminal, never re-planned, receipt cannot revive it', () => {
  const [pending] = planned({ items: [answer({ id: 'a-1' })] })
  const sent = submitted(pending)
  const cancelled = cancel(sent)
  assert.equal(cancelled.state, 'cancelled')
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  assert.equal(receipt(cancelled, sent.snapshot, context(items)).state, 'cancelled')
  assert.equal(
    planned({ items: [answer({ id: 'a-1' })], deliveries: [cancelled], newId: allocator(9) })
      .length,
    0,
  )
})

test('reservation: target stays reserved until accepted or uncertain', () => {
  const [pending] = planned()
  const target = { session: 'lead-1', generation: 1 }
  assert.equal(isReserved([pending], target), false, 'nothing is in flight until it is submitted')
  const sent = submitted(pending)
  assert.equal(isReserved([sent], target), true)
  assert.equal(isReserved([{ ...sent, state: 'accepted' }], target), false)
  assert.equal(isReserved([{ ...sent, state: 'uncertain' }], target), false)
  assert.equal(isReserved([sent], { session: 'lead-2', generation: 1 }), false)
  assert.equal(isReserved([sent], { session: 'lead-1', generation: 2 }), false)
  assert.equal(isReserved([sent]), false, 'no target is no reservation')
})

// ------------------------------------------------------------------ resend

test('resend: the id is injected, never derived from the clock', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  assert.throws(() => resend(sent, { now: 2_000 }), /delivery id/)
  const first = resend(sent, { id: 'd-11', now: 2_000 })
  const second = resend(sent, { id: 'd-12', now: 2_000 })
  assert.equal(first.id, 'd-11')
  assert.equal(second.id, 'd-12')
  assert.notEqual(first.id, sent.id)
  assert.notEqual(first.digest, sent.digest)
  assert.notEqual(first.digest, second.digest)
})

test('resend: the time is injected and required', () => {
  const [pending] = planned()
  assert.throws(() => resend(pending, { id: 'd-11' }), /now/)
  assert.equal(resend(pending, { id: 'd-11', now: 7 }).createdAt, 7)
})

test('resend: native admission expiry and UUID belong only to the original attempt', () => {
  const [pending] = planned()
  const old = {
    ...submitted(pending),
    expiresAt: PLANNED_AT + 1,
    nativeSubmissionId: 'old-peer-uuid',
    submissionOrder: 7,
  }
  const next = resend(old, { id: 'd-99', now: NOW })
  assert.equal(next.expiresAt, undefined)
  assert.equal(next.nativeSubmissionId, undefined)
  assert.equal(next.submissionOrder, undefined)
  assert.equal(old.nativeSubmissionId, 'old-peer-uuid')
})

test('resend: a new pending record, same answer, the old one left as history', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const again = resend(sent, { id: 'd-11', now: 2_000 })
  assert.equal(again.answerId, 'a-1')
  assert.equal(again.conversation, row.name)
  assert.equal(again.agent, row.agent)
  assert.deepEqual(again.target, TARGET)
  assert.equal(again.state, 'pending')
  assert.equal(again.resendOf, sent.id)
  assert.equal(again.snapshot, undefined)
  assert.equal(again.attempts, 0)
  assert.equal(again.digest, digest(envelope(again)))
  assert.equal(sent.state, 'submitting', 'the predecessor is untouched')
})

test('resend: an accepted or cancelled delivery is resent as a separate record', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  let n = 20
  for (const state of ['accepted', 'cancelled', 'uncertain']) {
    n += 1
    const again = resend({ ...sent, state }, { id: `d-${n}`, now: 2_000 })
    assert.equal(again.state, 'pending')
    assert.equal(again.resendOf, sent.id)
  }
})

test('resend: the old envelope never covers the resend, cursor present', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const again = submitted(resend(sent, { id: 'd-11', now: 2_000 }))
  const stale = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  assert.ok(transcript(stale)('i-0').length > 0, 'the cursor is found and evidence follows it')
  assert.notEqual(receipt(again, again.snapshot, context(stale)).state, 'accepted')
  const fresh = [CURSOR, item({ id: 'i-2', text: envelope(again) })]
  assert.equal(receipt(again, again.snapshot, context(fresh)).state, 'accepted')
})

test('resend: a cf-read resend rebuilds parts under the new id', () => {
  const { record } = fileDelivery()
  const again = resend(record, { id: 'd-11', now: 2_000 })
  assert.equal(again.channel, 'cf-read')
  assert.ok(again.file.endsWith('/deliveries/d-11.md'))
  assert.equal(again.parts.map((part) => part.body).join(''), LONG)
  for (const part of again.parts) assert.ok(part.close.includes('d-11'))
  const moved = resend(record, {
    id: 'd-12',
    now: 2_000,
    workspace: '/elsewhere/',
    partBudget: ONE_PART,
  })
  assert.equal(moved.file, '/elsewhere/deliveries/d-12.md')
  assert.equal(moved.parts.length, 1)
})

// ------------------------------------------------------- coverage and seen

test('coverage: maps a delivery to the worker answer ids it covers', () => {
  const records = planned({
    items: [answer({ id: 'a-1', text: 'Done.' }), answer({ id: 'a-2', text: 'Done.' })],
  }).map((record) => submitted(record))
  const items = [
    CURSOR,
    item({ id: 'i-1', text: envelope(records[0]) }),
    item({ id: 'i-2', text: envelope(records[1]) }),
  ]
  const accepted = records.map((record) => receipt(record, record.snapshot, context(items)))
  const covered = coverage(accepted)
  assert.deepEqual(covered[records[0].id], ['a-1'])
  assert.deepEqual(covered[records[1].id], ['a-2'])
  assert.deepEqual(accepted[0].evidenceIds, ['i-1'], 'lead item ids stay evidence, not coverage')
})

test('coverage: an accepted cf-read delivery covers its answer', () => {
  const { sent, tools } = fileDelivery()
  const after = receipt(sent, sent.snapshot, context([CURSOR, ...tools]))
  assert.equal(after.state, 'accepted')
  assert.deepEqual(coverage([after])[after.id], ['a-f'])
})

test('coverage: only accepted evidence covers — every other state covers nothing', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  for (const state of ['pending', 'submitting', 'uncertain', 'failed', 'cancelled']) {
    assert.deepEqual(coverage([{ ...sent, state }])[sent.id], [], state)
  }
  assert.deepEqual(coverage(), Object.create(null))
})

/** The store's row: `seen[<leadId>]` is an array of worker item ids. */
function seenRow(marks = {}) {
  return { name: row.name, seen: marks }
}

function acceptedFor(items, { conversation = row.name, newId = allocator() } = {}) {
  const records = planned({ items, conversation, newId }).map((record) => submitted(record))
  const lead = [
    CURSOR,
    ...records.map((record, index) => item({ id: `i-${index + 1}`, text: envelope(record) })),
  ]
  return records.map((record) => receipt(record, record.snapshot, context(lead)))
}

test('seenAfter: advances over covered-or-printed worker items contiguous with the start', () => {
  const items = [
    answer({ id: 'a-1', text: 'Done.' }),
    answer({ id: 'a-2', text: 'Second.' }),
    answer({ id: 'a-3', text: 'Third.' }),
  ]
  const accepted = acceptedFor(items).slice(0, 2)
  assert.deepEqual(
    seenAfter({
      row: seenRow({ 'tab:t-1:1': [] }),
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: accepted,
      items,
    }),
    ['a-1', 'a-2'],
  )
})

test('seenAfter: a gap stays unread and the mark does not jump it', () => {
  const items = [answer({ id: 'a-1' }), answer({ id: 'a-2' }), answer({ id: 'a-3' })]
  const accepted = acceptedFor(items)
  assert.deepEqual(
    seenAfter({
      row: seenRow(),
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: [accepted[2]],
      items,
    }),
    [],
    'a-1 is still unread, so a-3 stays unread too',
  )
})

// finding 4 — a stored mark past a gap must not carry the frontier over it
test('seenAfter: a non-contiguous stored mark never carries the frontier over the gap', () => {
  const items = [
    answer({ id: 'a-1' }),
    answer({ id: 'a-2' }),
    answer({ id: 'a-3' }),
    { ...answer({ id: 'a-4' }), printed: true },
  ]
  const accepted = acceptedFor(items).slice(0, 1)
  const marks = seenAfter({
    row: seenRow({ 'tab:t-1:1': ['a-3'] }),
    leadId: 'tab:t-1:1',
    conversation: row.name,
    deliveries: accepted,
    items,
  })
  assert.ok(marks.includes('a-1'), 'the covered first item is read')
  assert.ok(!marks.includes('a-2'), 'the uncovered gap is not')
  assert.ok(!marks.includes('a-4'), 'and nothing past the gap is, printed or not')
  assert.ok(marks.includes('a-3'), 'the mark already stored is never unseen')
})

test('seenAfter: an already-marked item is walked over, not stopped at', () => {
  const items = [answer({ id: 'a-1' }), answer({ id: 'a-2' }), answer({ id: 'a-3' })]
  const accepted = acceptedFor(items)
  const marks = seenAfter({
    row: seenRow({ 'tab:t-1:1': ['a-1'] }),
    leadId: 'tab:t-1:1',
    conversation: row.name,
    deliveries: [accepted[1], accepted[2]],
    items,
  })
  assert.deepEqual(marks, ['a-1', 'a-2', 'a-3'])
  assert.deepEqual(
    seenAfter({
      row: seenRow({ 'tab:t-1:1': marks }),
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: [accepted[1], accepted[2]],
      items,
    }),
    marks,
    'running it again marks nothing new',
  )
})

// finding 3 — coverage belongs to one conversation, not to every worker whose
// transcript happens to use the same native item id
test('seenAfter: another conversation’s accepted delivery never marks this one seen', () => {
  const items = [answer({ id: 'msg_01', text: 'w2 said something else' })]
  const elsewhere = acceptedFor([answer({ id: 'msg_01', text: 'Done.' })], {
    conversation: 'zeus-amber-fen',
    newId: allocator(30),
  })
  assert.equal(elsewhere[0].state, 'accepted')
  assert.deepEqual(
    seenAfter({
      row: seenRow(),
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: elsewhere,
      items,
    }),
    [],
  )
  assert.deepEqual(
    seenAfter({
      row: seenRow(),
      leadId: 'tab:t-1:1',
      conversation: 'zeus-amber-fen',
      deliveries: elsewhere,
      items,
    }),
    ['msg_01'],
    'and it does mark its own conversation seen',
  )
})

test('seenAfter: another lead’s marks and another lead’s deliveries stay out of it', () => {
  const items = [answer({ id: 'a-1' })]
  const accepted = acceptedFor(items)
  assert.deepEqual(
    seenAfter({
      row: seenRow({ 'tab:t-1:2': ['a-1'] }),
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: accepted,
      items,
    }),
    ['a-1'],
  )
  assert.deepEqual(
    seenAfter({
      row: seenRow(),
      leadId: 'tab:t-1:2',
      conversation: row.name,
      deliveries: accepted,
      items,
    }),
    [],
    'a delivery to generation 1 is not read by generation 2',
  )
})

test('seenAfter: printed items count, and the store’s initial empty seen is honoured', () => {
  const items = [{ ...answer({ id: 'a-1' }), printed: true }, answer({ id: 'a-2' })]
  const call = (rowValue) =>
    seenAfter({
      row: rowValue,
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: [],
      items,
    })
  assert.deepEqual(call(seenRow([])), ['a-1'], 'a fresh row carries seen: []')
  assert.deepEqual(call(undefined), ['a-1'])
  assert.deepEqual(call({ seen: { 'tab:t-1:1': 'a-1' } }), ['a-1'], 'a scalar is not the schema')
})

test('seenAfter: the lead identity and the conversation are required', () => {
  assert.throws(() => seenAfter({ row: seenRow(), conversation: row.name, items: [] }), /leadId/)
  assert.throws(() => seenAfter({ row: seenRow(), leadId: 'tab:t-1:1', items: [] }), /conversation/)
  assert.throws(() => seenAfter(), /leadId/)
})

test('seenAfter: a cf-read delivery covers nothing until it is accepted', () => {
  const { sent } = fileDelivery()
  const items = [answer({ id: 'a-f', text: LONG })]
  assert.deepEqual(
    seenAfter({
      row: seenRow(),
      leadId: 'tab:t-1:1',
      conversation: row.name,
      deliveries: [sent],
      items,
    }),
    [],
  )
})

// ----------------------------------------------------------------- Unicode

test('unicode: a non-ASCII body round-trips through envelope, digest and receipt', () => {
  const text = 'Gata, am terminat ✔ — café “naïve” 日本語 emoji 🎉'
  const [pending] = planned({ items: [answer({ id: 'a-u', text })] })
  assert.equal(pending.channel, 'pty-inline')
  const sent = submitted(pending)
  assert.equal(sent.digest, digest(envelope(sent)))
  const items = [CURSOR, item({ id: 'i-1', text: envelope(sent) })]
  assert.equal(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
})

// ------------------------------------------------------------ brand checks

test('large and ESC fixtures: bodies are whole, digests stable', () => {
  const big = 'x'.repeat(60_000)
  const esc = 'hello\x1b[2Jworld'
  const [bigRecord] = planned({ items: [answer({ id: 'a-big', text: big })] })
  const [escRecord] = planned({ items: [answer({ id: 'a-esc', text: esc })] })
  assert.equal(bigRecord.parts.map((part) => part.body).join(''), big)
  assert.equal(escRecord.answer, esc)
  assert.equal(bigRecord.digest, digest(envelope(bigRecord)))
  assert.equal(escRecord.digest, digest(envelope(escRecord)))
})

// finding 7 — the fallbacks each module caller may rely on, asserted rather
// than assumed: the conversation row, the default budgets, an evidence item
// with no text at all, an empty file body, and a record that predates a field.
test('fallbacks: the conversation and the agent come from the row when not named', () => {
  const [record] = plan({
    row,
    target: TARGET,
    newId: allocator(),
    now: PLANNED_AT,
    policy: AUTO,
    items: [answer()],
  })
  assert.equal(record.conversation, row.name)
  assert.equal(record.agent, row.agent)
})

test('fallbacks: a part budget with only a default covers every harness kind', () => {
  const [record] = planned({
    items: [answer({ id: 'a-f', text: LONG })],
    inlineBudget: 10,
    partBudget: { default: ONE_PART },
    kind: 'a-harness-with-no-entry',
  })
  assert.equal(record.partBudget, ONE_PART)
  assert.equal(record.parts.length, 1)
})

test('fallbacks: an over-long line ending in a newline is cut and still joins back', () => {
  const text = `${'z'.repeat(300)}\n`
  const parts = partsFor(text, 'd-7', MANY_PARTS)
  assert.ok(parts.length > 1)
  for (const part of parts) assert.ok(part.bytes <= MANY_PARTS.bytes)
  assert.equal(parts.map((part) => part.body).join(''), text)
})

test('fallbacks: an empty answer is delivered, framed and accepted as a file', () => {
  const { record, sent } = fileDelivery({ text: '', budget: ONE_PART })
  assert.equal(record.parts[0].body, '')
  assert.equal(record.parts[0].bodyBytes, 0)
  const printed = item({ id: 't-0', role: 'tool', text: record.parts[0].text })
  assert.equal(receipt(sent, sent.snapshot, context([CURSOR, printed])).state, 'accepted')
})

test('fallbacks: evidence with no text at all is simply not a receipt', () => {
  const [pending] = planned()
  const sent = submitted(pending)
  const items = [CURSOR, item({ id: 'i-1', text: undefined })]
  assert.notEqual(receipt(sent, sent.snapshot, context(items)).state, 'accepted')
  const { sent: file } = fileDelivery()
  const tools = [CURSOR, item({ id: 't-0', role: 'tool', text: undefined })]
  assert.notEqual(receipt(file, file.snapshot, context(tools)).state, 'accepted')
})

test('fallbacks: a cf-read record with no parts field is never accepted', () => {
  const { sent, tools } = fileDelivery()
  const bare = { ...sent, parts: undefined }
  assert.notEqual(receipt(bare, bare.snapshot, context([CURSOR, ...tools])).state, 'accepted')
})

test('fallbacks: a resend with no workspace or budget on the record uses the defaults', () => {
  const { record } = fileDelivery()
  const bare = { ...record, workspace: undefined, partBudget: undefined }
  const again = resend(bare, { id: 'd-40', now: 2_000 })
  assert.equal(again.file, './deliveries/d-40.md')
  assert.equal(again.partBudget, DEFAULT_PART_BUDGETS.default)
  assert.equal(again.parts.length, 1, 'the 32 KiB default holds this answer in one part')
})

test('defaults: receiptMs is 60 s; non-pi part budget is 32 KiB / 1000 lines', () => {
  assert.equal(DEFAULT_RECEIPT_MS, 60_000)
  assert.deepEqual(DEFAULT_PART_BUDGETS.default, { bytes: 32 * 1024, lines: 1000 })
  assert.deepEqual(DEFAULT_PART_BUDGETS.pi, { bytes: 40 * 1024, lines: 1500 })
  assert.equal(Object.getPrototypeOf(DEFAULT_PART_BUDGETS), null, 'the budget map is data')
  assert.equal(Object.hasOwn(DEFAULT_PART_BUDGETS, '__proto__'), false, 'and not a chain')
})

test('retired Claude channel markup is ordinary lossless terminal text (TEST-PANE-121)', () => {
  const text = 'A literal </channel> tag must survive.\nFinal line.'
  const [record] = plan({
    row: { agent: 'worker', seen: {} },
    items: [{ role: 'assistant', complete: true, id: 'answer', text }],
    policy: { mode: 'auto' },
    conversation: 'worker',
    agent: 'worker',
    kind: 'claude-code',
    target: { leadId: 'tab:t-1:1', tab: 't-1', pane: 'p-1', generation: 1, session: 'native' },
    newId: () => 'd-3131',
    now: 1,
    workspace: '/tmp/claude-result',
  })
  assert.equal(record.channel, 'pty-inline')
  assert.equal(record.answer, text)
  assert.equal(record.parts, undefined)
})
