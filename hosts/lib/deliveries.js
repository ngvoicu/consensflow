import { createHash } from 'node:crypto'
import { effectivePolicy } from './policy.js'

/**
 * Delivery records — pure, no I/O, no env, no clock and no ids of its own.
 *
 * A delivery carries one completed worker answer, whole, into the lead's
 * pane. Its wire form is the ENVELOPE: a header line
 * `[consensflow delivery <id> from <conversation> #<item>]`, the complete
 * answer, a trailer `[end of delivery <id>]`. `digest` covers the canonical
 * envelope, and `accepted` needs that id and digest found AFTER the
 * pre-submission cursor, in evidence of the right PROVENANCE — answer-text
 * equality is never a receipt, so two workers answering `Done.` are two
 * envelopes and two acceptances.
 *
 * Five things this module refuses to invent, because inventing them is how
 * the earlier versions lost or forged records:
 *
 *  - **Identity.** A delivery id is allocated by the store
 *    (`Store.allocateDeliveryId`) and injected: `plan({newId})`,
 *    `resend({id})`. Every id the module is handed is validated `d-<digits>`
 *    — the allocator's own namespace, filename-safe, so `<workspace>/
 *    deliveries/<id>.md` can never resolve anywhere else. `plan` refuses an
 *    id any record it can see already carries and `resend` refuses its
 *    predecessor's, so the namespace is unique end to end and not merely
 *    within one call. Planning dedupes by `(conversation, answerId)` — never
 *    by a bare answer id, which is unique only inside its own transcript.
 *  - **Time.** Every timestamp is injected and required. There is no
 *    `Date.now()` here, so "no clock of its own" is a fact the tests hold.
 *  - **Position.** A transcript cursor is an OPAQUE token minted by the
 *    completion adapter, and only that adapter compares it (Decision Log,
 *    2026-09-07). Every read of "what came after the cursor" goes through the
 *    injected `itemsAfter(cursor)` — in production
 *    `completion.js::itemsAfterCursor` bound to a kind and its items. A
 *    cursor is never an array index.
 *  - **Provenance.** `pty-inline` is answered by a USER turn, `cf-read` by a
 *    TOOL RESULT — the two shapes `completion.js` normalises every harness's
 *    native records into. An assistant message that quotes the framing is the
 *    model talking about the delivery, not the harness admitting it.
 *  - **A boundary.** `receipt` takes the submission snapshot it was sent
 *    with; an unsubmitted, unknown or replaced snapshot proves nothing, and a
 *    transcript that could not be read (`itemsAfter` gives back no array) is
 *    told apart from one verified empty. Only the verified one can time out.
 *
 * The channel is the module's own decision and no caller overrides it:
 * `pty-inline` when the envelope fits the lead harness's inline budget
 * (default 4 000 bytes) and is safely representable — the arbiter's rule,
 * `app/src-tauri/src/arbiter.rs::sanitize`: every Unicode control but LF and
 * TAB is refused. Otherwise `cf-read`, where the whole answer is written
 * immutable to `<workspace>/deliveries/<id>.md` by the caller, `cf read`
 * prints it in numbered parts, and one line is pasted instead (`pointer`).
 * There is no cap on the answer and never a truncated body. A file write
 * failure is the caller's: `plan` marks `channel: 'cf-read'` and the caller
 * reports `pending` with the reason via `writeFailed`.
 *
 * TEXT IS NORMALISED ONCE, at plan time. A lone surrogate has no UTF-8 form,
 * and hashing one silently turns it into U+FFFD — so a record could hold text
 * whose digest belonged to different text, and evidence carrying U+FFFD was
 * accepted for a record carrying U+D800. `plan` replaces lone surrogates with
 * U+FFFD, records `normalized: true`, and every later hash, part and
 * comparison uses that body: what is hashed is exactly what will be written.
 *
 * A part's budget is the COMPLETE PRINTED PART — `part.text`, which `cf read`
 * prints verbatim: the open marker, the body, the end marker and the
 * next-part instruction, each on its own line. Budgeting the bare body is
 * what made a configured 100-byte part print 156 bytes into a lead that
 * allows 100. The open marker records the body's EXACT BYTE LENGTH, which
 * makes the framing injective: `abc` and `abc\n` print differently, and one
 * printed part parses to exactly one body and one digest comparison. A part
 * is covered only when its complete framing appears in the lead's tool result
 * after the cursor AND that one parsed body's digest equals the part's — a
 * marker alone never covers, because a receiver that keeps only the tail
 * keeps the marker and drops the text.
 *
 * States: `pending | submitting | accepted | uncertain | failed |
 * cancelled`, and ONE transition table (`TRANSITIONS`) that every mutator
 * applies. `failed` needs affirmative zero-byte evidence and is the only
 * replayable end; an unknown write outcome is `uncertain` and stays out of
 * the planner; `accepted` and `cancelled` are terminal and refuse every
 * mutator; an explicit resend is a separate record with its own id. The
 * target stays reserved until accepted or uncertain.
 */

export const DEFAULT_INLINE_BUDGET = 4000
export const DEFAULT_RECEIPT_MS = 60_000

/** Keyed maps are data: a null prototype, so `__proto__` is a key like any other. */
const table = (entries) => Object.assign(Object.create(null), entries)

export const DEFAULT_PART_BUDGETS = table({
  pi: { bytes: 40 * 1024, lines: 1500 },
  default: { bytes: 32 * 1024, lines: 1000 },
})

/**
 * The transitions a record may take. Read it as "from → the states it may
 * still become": `accepted` and `cancelled` name nothing, which is what
 * terminal means, and `uncertain` and `failed` only ever end in `cancelled` —
 * a delivery that may have landed is never replayed, and the human's way back
 * is `resend`, a NEW record.
 */
const TRANSITIONS = table({
  pending: new Set(['pending', 'submitting', 'uncertain', 'failed', 'cancelled']),
  submitting: new Set(['accepted', 'uncertain', 'failed', 'cancelled']),
  uncertain: new Set(['cancelled']),
  failed: new Set(['cancelled']),
  accepted: new Set(),
  cancelled: new Set(),
})

/** What counts as a receipt for each channel, and the item role that carries it. */
const EVIDENCE = table({ 'pty-inline': 'user-item', 'cf-read': 'tool-result' })

/** `completion.js` normalises every harness's native records into these roles. */
const PROVENANCE = table({ user: 'user-item', tool: 'tool-result' })

const sha256 = (text) => createHash('sha256').update(text, 'utf8').digest('hex')
const byteLength = (text) => Buffer.byteLength(text, 'utf8')

/** Under the `u` flag a well-formed pair is one code point; only a LONE surrogate matches. */
const LONE_SURROGATES = /\p{Surrogate}/gu
/**
 * A header field is ONE LINE: no control character at all (TAB included), no
 * lone surrogate, and no Unicode line break of its own — U+2028 LINE SEPARATOR
 * and U+2029 PARAGRAPH SEPARATOR are Zl and Zp rather than Cc, so a predicate
 * built on control characters alone lets them through and the header stops
 * being a line. The body may carry them (the arbiter allows them, and
 * `safelyRepresentable` still does); metadata may not.
 */
const HEADER_FIELD = /^[^\p{Cc}\p{Cs}\p{Zl}\p{Zp}]+$/u
/** The store allocator's namespace, and the only shape a delivery id may have. */
const DELIVERY_ID = /^d-\d+$/

/**
 * The character is one the PTY arbiter refuses: the whole Unicode Cc category
 * (C0 `0x00-0x1F`, DEL and the C1 range `0x80-0x9F`) but LF and TAB. Written
 * as a scan rather than a character class because it is the Rust rule
 * character for character — `character.is_control() && !matches!(character,
 * '\t' | '\n')` — and a regex of raw control escapes reads like a typo.
 */
function isRefusedControl(code) {
  if (code === 0x09 || code === 0x0a) return false
  return code <= 0x1f || (code >= 0x7f && code <= 0x9f)
}

/** A code point with no UTF-8 form of its own: half of a pair that never arrived. */
const isLoneSurrogate = (code) => code >= 0xd800 && code <= 0xdfff

/** Safely representable inline: the arbiter's rule, plus a lossless UTF-8 form. */
export function safelyRepresentable(text) {
  for (const character of String(text)) {
    const code = character.codePointAt(0)
    if (isRefusedControl(code) || isLoneSurrogate(code)) return false
  }
  return true
}

/**
 * The text as it will really be written and hashed. A lone surrogate cannot be
 * encoded, and every UTF-8 encoder — `Buffer`, the PTY, the file write —
 * substitutes U+FFFD for it. Substituting once, here, is what keeps a record's
 * body and its digest the same text.
 */
function normalizeText(text) {
  return String(text).replace(LONE_SURROGATES, '�')
}

/**
 * One header field, validated. The header is a LINE, and the matcher compares
 * whole lines: a conversation name carrying a newline would frame an envelope
 * its own matcher can never recognise, and one carrying an ESC would be
 * rewritten by the arbiter before the lead ever saw it.
 */
function headerField(value, label) {
  const text = typeof value === 'string' ? value : ''
  if (!HEADER_FIELD.test(text)) {
    throw new Error(
      `a delivery header field is one printable line: ${label} is ${JSON.stringify(value)}`,
    )
  }
  return text
}

/**
 * One delivery id, validated. `d-<digits>` is `Store.allocateDeliveryId`'s
 * namespace: it is the only id this module accepts, from `plan`, from
 * `resend` or off a record, and it is what keeps `<workspace>/deliveries/
 * <id>.md` inside the deliveries directory — `../../escaped` is not an id.
 */
function deliveryId(value, label = 'delivery id') {
  if (typeof value !== 'string' || !DELIVERY_ID.test(value)) {
    throw new Error(
      `a delivery id is the store allocator's d-<digits>, filename-safe and never reissued: ` +
        `${label} is ${JSON.stringify(value)}`,
    )
  }
  return value
}

/** Every timestamp is the caller's: this module has no clock. */
function requireTime(now, label) {
  if (!Number.isFinite(now)) {
    throw new Error(
      `${label} needs the time injected: now is ${JSON.stringify(now)} — this module has no clock`,
    )
  }
  return now
}

/** Record shape: `{id, conversation, agent, answerId, answer, digest, channel, state, target}`. */
const headerOf = (record) =>
  `[consensflow delivery ${deliveryId(record.id, 'record id')} from ${headerField(
    record.conversation,
    'conversation',
  )} #${headerField(record.answerId, 'answerId')}]`
const trailerOf = (record) => `[end of delivery ${deliveryId(record.id, 'record id')}]`

/** The wire form. Canonical: header, complete answer, trailer, each `\n`-closed. */
export function envelope(record) {
  return `${headerOf(record)}\n${record.answer}\n${trailerOf(record)}\n`
}

/** Hex sha256 over the canonical form (envelope or part body). */
export function digest(canonical) {
  return sha256(canonical)
}

function inlineLimit(inlineBudget, kind) {
  if (typeof inlineBudget === 'number') return inlineBudget
  return inlineBudget?.[kind] ?? inlineBudget?.default ?? DEFAULT_INLINE_BUDGET
}

function partLimit(partBudget, kind) {
  return (
    partBudget?.[kind] ??
    partBudget?.default ??
    DEFAULT_PART_BUDGETS[kind] ??
    DEFAULT_PART_BUDGETS.default
  )
}

// ------------------------------------------------------------------- parts

const partOpen = (k, of, bodyBytes) => `[part ${k} of ${of} — ${bodyBytes} bytes]`
const partClose = (k, of, id) => `[end of part ${k} of ${of} — delivery ${id}]`
const partNext = (k, of, id) => (k < of ? `[next: cf read ${id} --part ${k + 1}]` : null)

/** The body as printed: closed with a newline when it does not carry one. */
const closedBody = (body) => (body === '' || body.endsWith('\n') ? body : `${body}\n`)

/**
 * One printed part. The open marker carries the body's exact byte length, so
 * two bodies that would otherwise print the same (`abc` and `abc\n`) are told
 * apart, and a reader parses exactly one body back out of it.
 */
function renderPart(body, k, of, id, bodyBytes = byteLength(body)) {
  const open = partOpen(k, of, bodyBytes)
  const close = partClose(k, of, id)
  const next = partNext(k, of, id)
  return {
    open,
    close,
    next,
    text: `${open}\n${closedBody(body)}${close}\n${next === null ? '' : `${next}\n`}`,
  }
}

/**
 * What the framing alone costs, worst case over the parts of an `of`-part
 * delivery. The open marker grows with the body's byte count, so the count is
 * budgeted at its widest: no body can be longer than the budget itself.
 */
function framingCost(id, of, maxBytes) {
  let bytes = 0
  let lines = 0
  for (let k = 1; k <= of; k++) {
    const { text } = renderPart('', k, of, id, maxBytes)
    bytes = Math.max(bytes, byteLength(text))
    lines = Math.max(lines, countNewlines(text))
  }
  return { bytes, lines }
}

/** The printed size of a body inside its framing: its own bytes plus the closing newline. */
function bodyCost(body) {
  const closed = closedBody(body)
  return { bytes: byteLength(closed), lines: countNewlines(closed) }
}

const bodyFits = (body, room) => {
  const cost = bodyCost(body)
  return cost.bytes <= room.bytes && cost.lines <= room.lines
}

/**
 * Split an answer into numbered parts, each PRINTED form within `budget`
 * (`{bytes, lines}`). Lossless: the bodies join back to the answer exactly.
 * Over-long lines are cut on UTF-8 character boundaries, never mid-code-point.
 *
 * The framing widens with the number of parts (`[part 9 of 10 — …]` is longer
 * than `[part 1 of 1 — …]`, and every part but the last carries a next-part
 * line), so the split is a fixed point: budget the body under the framing for
 * `n` parts, see how many parts that gives, repeat until it stops moving. A
 * budget that cannot hold the framing plus one character of the answer is an
 * error — never a part that quietly exceeds it.
 */
export function partsFor(answer, id, budget = DEFAULT_PART_BUDGETS.default) {
  deliveryId(id, 'part id')
  const text = String(answer)
  const maxBytes = Number(budget?.bytes)
  const maxLines = Number(budget?.lines)
  if (
    typeof budget?.bytes !== 'number' ||
    typeof budget?.lines !== 'number' ||
    !Number.isInteger(maxBytes) ||
    maxBytes < 1 ||
    !Number.isInteger(maxLines) ||
    maxLines < 1
  ) {
    throw new Error(
      `a part budget is {bytes, lines}, both positive integers: got ${JSON.stringify(budget)}`,
    )
  }
  let of = 1
  let bodies = ['']
  for (let pass = 0; pass < 16; pass++) {
    const framing = framingCost(id, of, maxBytes)
    const room = { bytes: maxBytes - framing.bytes, lines: maxLines - framing.lines }
    if (room.bytes < 1 || room.lines < 1) {
      throw new Error(
        `part budget too small: printing one of ${of} part(s) of ${id} costs ${framing.bytes} bytes ` +
          `and ${framing.lines} lines of framing, and the budget is ${maxBytes} bytes / ${maxLines} lines`,
      )
    }
    bodies = splitBodies(text, room, id)
    if (bodies.length === of) break
    of = bodies.length
  }
  const parts = bodies.map((body, index) => {
    const bodyBytes = byteLength(body)
    const { open, close, next, text: printed } = renderPart(body, index + 1, bodies.length, id)
    return {
      k: index + 1,
      of: bodies.length,
      body,
      bodyBytes,
      text: printed,
      bytes: byteLength(printed),
      lines: countNewlines(printed),
      digest: sha256(body),
      open,
      close,
      next,
    }
  })
  // The invariant, asserted rather than trusted. It cannot fire while the fixed
  // point converges — `of` only ever grows, and the framing widens only at a
  // power of ten, so the loop settles in a few passes — which is exactly why it
  // is here and why no test can reach it: if that reasoning is ever wrong, this
  // throws instead of printing a part over the lead's budget.
  for (const part of parts) {
    if (part.bytes > maxBytes || part.lines > maxLines) {
      throw new Error(
        `part budget too small: part ${part.k} of ${part.of} prints ${part.bytes} bytes and ` +
          `${part.lines} lines, and the budget is ${maxBytes} bytes / ${maxLines} lines`,
      )
    }
  }
  return parts
}

/** Greedy fill: whole lines while they fit, cut lines only when one cannot. */
function splitBodies(text, room, id) {
  if (text === '') return ['']
  const bodies = []
  let body = ''
  for (const segment of cutSegments(text, room, id)) {
    if (body !== '' && !bodyFits(body + segment, room)) {
      bodies.push(body)
      body = segment
      continue
    }
    body += segment
  }
  bodies.push(body)
  return bodies
}

/** Cut the text into newline-terminated segments, lossless under `join('')`. */
function splitSegments(text) {
  const segments = []
  let start = 0
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\n') {
      segments.push(text.slice(start, i + 1))
      start = i + 1
    }
  }
  if (start < text.length) segments.push(text.slice(start))
  return segments
}

/** Cut over-long segments on code-point boundaries; a character that cannot fit is an error. */
function cutSegments(text, room, id) {
  const out = []
  for (const segment of splitSegments(text)) {
    if (bodyFits(segment, room)) {
      out.push(segment)
      continue
    }
    let current = ''
    let bytes = 0
    for (const char of segment) {
      const size = byteLength(char)
      const alone = size + (char === '\n' ? 0 : 1)
      if (alone > room.bytes) {
        throw new Error(
          `part budget too small: one character of ${id} prints ${alone} bytes and the body has ` +
            `room for ${room.bytes}`,
        )
      }
      if (current !== '' && bytes + alone > room.bytes) {
        out.push(current)
        current = ''
        bytes = 0
      }
      current += char
      bytes += size
    }
    if (current !== '') out.push(current)
  }
  return out
}

function countNewlines(text) {
  let count = 0
  for (const char of text) {
    if (char === '\n') count += 1
  }
  return count
}

/**
 * The one line pasted for a `cf-read` delivery, exactly as SPEC.md specifies
 * it: `@<agent> answered in <name> — run: cf read <id>  (it prints
 * everything; read all of it)`. The agent and the conversation are separate
 * validated fields on the record, so the line is built from identity, never
 * from a display string that might carry a newline of its own.
 */
export function pointer(record) {
  const agent = headerField(record?.agent, 'agent')
  const conversation = headerField(record?.conversation, 'conversation')
  const id = deliveryId(record?.id, 'record id')
  return `@${agent} answered in ${conversation} — run: cf read ${id}  (it prints everything; read all of it)`
}

// -------------------------------------------------------------- identity

/**
 * The lead a delivery is meant for, captured when it is PLANNED so a pending
 * record can say which lead generation it belongs to when that lead resumes
 * (task 31 holds old answers rather than delivering them to a new lead).
 * `leadId` is the app-owned identity `src/tabs.js::leadIdentity` mints; this
 * module compares it and never builds one, so the format lives in one place.
 */
function requireTarget(target) {
  if (target === null || typeof target !== 'object' || Array.isArray(target)) {
    throw new Error('a delivery needs its target lead: {leadId, session, tab, pane, generation}')
  }
  for (const field of ['leadId', 'session', 'tab', 'pane']) {
    if (typeof target[field] !== 'string' || target[field].length === 0) {
      throw new Error(`a delivery target needs ${field}: {leadId, session, tab, pane, generation}`)
    }
  }
  if (!Number.isInteger(target.generation) || target.generation < 1) {
    throw new Error(
      `a delivery target needs a positive lead generation: got ${JSON.stringify(target.generation)}`,
    )
  }
  return {
    leadId: target.leadId,
    session: target.session,
    tab: target.tab,
    pane: target.pane,
    generation: target.generation,
  }
}

const sameTarget = (a, b) =>
  a?.leadId === b?.leadId &&
  a?.session === b?.session &&
  a?.tab === b?.tab &&
  a?.pane === b?.pane &&
  a?.generation === b?.generation

/** The record must be in `want`; anything else is a caller bug, not a slow turn. */
function requireState(record, want, verb) {
  const state = record?.state
  if (state !== want) {
    throw new Error(`${verb}: ${record?.id} is ${state} — only a ${want} delivery is ${verb}ted`)
  }
}

/** The one transition table, applied. A refused transition changes nothing at all. */
function transition(record, next, patch = {}) {
  if (!TRANSITIONS[record?.state]?.has(next)) return record
  return { ...record, ...patch, state: next }
}

// ---------------------------------------------------------------- planning

/**
 * One `pending` record per completed, uncovered assistant answer when the
 * effective policy is `auto` — or always when `manual: true` (Deliver now
 * bypasses policy, never readiness). `policy` may be given directly as
 * `{mode}`; otherwise it is derived from `{tab, pane, row}` via `policy.js`.
 *
 * The caller owes this function the identity and the clock it must not
 * invent: the `conversation` and `agent` these answers came from, the
 * `target` lead they are for, `newId()` — the store's delivery-id allocator —
 * and `now`. An answer is skipped when THIS conversation already has a
 * delivery for it or the caller lists it in `coveredIds`. A failed attempt
 * needs an explicit resend; a daemon tick must never mint repeated attempts.
 */
export function plan({
  row,
  items = [],
  policy,
  tab,
  pane,
  kind = 'unknown',
  conversation,
  agent,
  target,
  newId,
  now,
  workspace = '.',
  manual = false,
  coveredIds = [],
  deliveries = [],
  inlineBudget,
  partBudget,
} = {}) {
  const name = conversation ?? row?.name
  if (typeof name !== 'string' || name.length === 0) {
    throw new Error(
      'plan needs a conversation: delivery records are keyed by (conversation, answerId), and a ' +
        'native answer id is only unique inside its own transcript',
    )
  }
  headerField(name, 'conversation')
  headerField(agent ?? row?.agent, 'agent')
  const lead = requireTarget(target)
  if (typeof newId !== 'function') {
    throw new Error(
      'plan needs newId(): a delivery id is allocated by the store, never derived from the answer',
    )
  }
  requireTime(now, 'plan')
  const mode = policy?.mode ?? effectivePolicy(tab, pane, row).mode
  if (mode !== 'auto' && !manual) return []

  const covered = new Set(coveredIds)
  const live = new Set()
  const taken = new Set()
  for (const record of deliveries) {
    if (typeof record?.id === 'string') taken.add(record.id)
    if (record?.answerId === undefined) continue
    live.add(`${record.conversation}\u0000${record.answerId}`)
  }
  const from = headerField(agent ?? row?.agent, 'agent')
  const records = []
  for (const item of items) {
    if (item?.role !== 'assistant' || item?.complete !== true) continue
    if (covered.has(item.id) || live.has(`${name}\u0000${item.id}`)) continue
    const id = deliveryId(newId(), 'newId()')
    if (taken.has(id)) {
      throw new Error(
        `newId() minted ${id}, which a delivery record already carries — a delivery id is ` +
          'allocated once and never reissued',
      )
    }
    taken.add(id)
    const answer = normalizeText(item.text)
    const candidate = { id, answerId: item.id, conversation: name, answer }
    const wire = envelope(candidate)
    const inline = byteLength(wire) <= inlineLimit(inlineBudget, kind) && safelyRepresentable(wire)
    const record = {
      id,
      conversation: name,
      agent: from,
      answerId: item.id,
      answer,
      digest: sha256(wire),
      channel: inline ? 'pty-inline' : 'cf-read',
      state: 'pending',
      kind,
      target: lead,
      createdAt: now,
    }
    if (answer !== item.text) record.normalized = true
    if (manual) record.manual = true
    if (record.channel === 'cf-read') attachFile(record, workspace, partLimit(partBudget, kind))
    records.push(record)
  }
  return records
}

function attachFile(record, workspace, budget) {
  record.workspace = String(workspace).replace(/\/+$/, '')
  record.partBudget = budget
  record.file = `${record.workspace}/deliveries/${deliveryId(record.id, 'record id')}.md`
  record.parts = partsFor(record.answer, record.id, budget)
}

/**
 * Hand the delivery to the lead it was planned for, and remember exactly what
 * would prove it landed: the SNAPSHOT `{targetSession, generation, cursor,
 * evidenceType}`. The cursor is required — an absent one used to mean "search
 * the whole transcript", which turns any old echo into a receipt — and the
 * target must be the one captured at plan time, so a resumed lead cannot
 * inherit a record meant for its predecessor.
 */
export function submit(record, { target, cursor, evidenceType, now, manualRead = false } = {}) {
  requireState(record, 'pending', 'submit')
  const lead = requireTarget(target)
  if (!sameTarget(record.target, lead)) {
    throw new Error(
      `submit: ${record.id} was planned for lead ${record.target?.leadId} (pane ` +
        `${record.target?.pane}, session ${record.target?.session}), not ${lead.leadId} (pane ` +
        `${lead.pane}, session ${lead.session})`,
    )
  }
  if (
    (cursor === null || cursor === undefined || cursor === '') &&
    !(manualRead === true && record.channel === 'cf-read' && cursor === null)
  ) {
    throw new Error(
      `submit: ${record.id} needs the pre-submission cursor — the completion adapter's opaque ` +
        'token for where the lead transcript stood before the write',
    )
  }
  const type = EVIDENCE[record.channel]
  if (type === undefined) {
    throw new Error(`submit: ${record.id} has no channel to take a receipt on: ${record.channel}`)
  }
  if (evidenceType !== undefined && evidenceType !== type) {
    throw new Error(
      `submit: a ${record.channel} delivery is answered by ${type}, not ${evidenceType}`,
    )
  }
  requireTime(now, 'submit')
  return transition(record, 'submitting', {
    snapshot: {
      targetSession: lead.session,
      generation: lead.generation,
      cursor,
      evidenceType: type,
    },
    submittedAt: now,
    attempts: (record.attempts ?? 0) + 1,
  })
}

/**
 * An explicit resend is a NEW delivery with an INJECTED id and its own
 * receipt: the answer, conversation and target stay the same and the old
 * record stays as history. Allowed from any state — an accepted or cancelled
 * answer is exactly what a human resends — but never under the predecessor's
 * id, and never from the clock, which is not an identity.
 */
export function resend(record, { id, now, workspace, partBudget } = {}) {
  const fresh = deliveryId(id, 'resend id')
  if (fresh === record?.id) {
    throw new Error(
      `resend: ${record.id} cannot be resent under its own id — a resend is a new record with a ` +
        'newly allocated id, so its receipt is its own',
    )
  }
  requireTime(now, 'resend')
  const next = {
    ...record,
    id: fresh,
    state: 'pending',
    createdAt: now,
    attempts: 0,
    resendOf: record.id,
  }
  for (const field of [
    'snapshot',
    'submittedAt',
    'expiresAt',
    'nativeSubmissionId',
    'submissionOrder',
    'acceptedAt',
    'evidenceIds',
    'partCoverage',
    'reason',
  ]) {
    delete next[field]
  }
  next.digest = sha256(envelope(next))
  if (next.channel === 'cf-read') {
    attachFile(
      next,
      workspace ?? record.workspace ?? '.',
      partBudget ?? record.partBudget ?? DEFAULT_PART_BUDGETS.default,
    )
  }
  return next
}

// ---------------------------------------------------------------- receipts

/** True when `text` carries this envelope whole: framing lines AND digest. */
function matchInline(record, text) {
  const header = headerOf(record)
  const trailer = trailerOf(record)
  const lines = String(text ?? '').split('\n')
  for (let i = 0; i < lines.length; i++) {
    if (lines[i] !== header) continue
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[j] !== trailer) continue
      if (
        sha256(`${header}\n${lines.slice(i + 1, j).join('\n')}\n${trailer}\n`) === record.digest
      ) {
        return true
      }
    }
  }
  return false
}

/**
 * True when `text` carries this part whole: the exact framing AND the body's
 * digest. The open marker names the body's byte length, so the printed region
 * between the markers parses to exactly ONE body — the one whose printed size
 * this part has — and exactly one digest is compared. That is what makes a
 * changed body impossible to pass off as the recorded one.
 */
function matchPart(part, text) {
  const closed = closedBody(part.body)
  const closedBytes = byteLength(closed)
  const trailing = closedBytes - part.bodyBytes
  const lines = String(text ?? '').split('\n')
  for (let i = 0; i < lines.length; i++) {
    if (lines[i] !== part.open) continue
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[j] !== part.close) continue
      const between = lines.slice(i + 1, j)
      const region = between.length === 0 ? '' : `${between.join('\n')}\n`
      if (byteLength(region) !== closedBytes) continue
      const body = trailing === 0 ? region : region.slice(0, -1)
      if (sha256(body) === part.digest) return true
    }
  }
  return false
}

/**
 * What the adapter says came after the cursor, or `null` when it could not
 * say. The distinction is the whole point: an empty array is a transcript
 * READ and found to hold nothing yet — that one may time out into
 * `uncertain`; `null` is no evidence at all, and decides nothing.
 */
function evidenceAfter(itemsAfter, cursor) {
  if (typeof itemsAfter !== 'function') return null
  const items = itemsAfter(cursor)
  return Array.isArray(items) ? items : null
}

/**
 * Fold the evidence into the record.
 *
 * `receipt(record, snapshot, context)` — `snapshot` is the one `submit`
 * returned on this record, and `context` is `{session, generation,
 * itemsAfter, now, receiptMs, replaced}`: who the transcript belongs to, the
 * adapter's opaque-cursor reader (in production `completion.js::
 * itemsAfterCursor` bound to a kind and its items) and the time. `accepted`
 * needs an item AFTER the cursor, of the channel's evidence type, carrying
 * THIS delivery id and digest — inline, the envelope in a user turn;
 * `cf-read`, every part's framing plus body digest in tool results.
 * Otherwise `uncertain` past `receiptMs`, else the record back (with
 * `partCoverage` progress).
 *
 * Refusals throw, because each one is a watcher bug or an invalidated state
 * and none may pass for "not yet": a record that was never submitted, a
 * snapshot that is not the one it was submitted with, evidence from another
 * session or generation, a session reported replaced. Terminal and
 * already-resolved records pass through untouched.
 */
export function receipt(record, snapshot, context = {}) {
  if (record?.state === 'pending') {
    throw new Error(
      `receipt: ${record.id} was never submitted — a receipt needs the submission snapshot`,
    )
  }
  if (record?.state !== 'submitting') return record
  const held = record.snapshot
  if (!sameSnapshot(held, snapshot)) {
    throw new Error(
      `receipt: unknown or replaced submission snapshot for ${record.id} — it was submitted with ` +
        `${JSON.stringify(held)}`,
    )
  }
  if (context.replaced === true) {
    throw new Error(
      `receipt: the lead session was replaced in place — ${record.id}'s snapshot proves nothing`,
    )
  }
  if (context.session !== held.targetSession) {
    throw new Error(
      `receipt: evidence from session ${context.session}, and ${record.id} went to session ` +
        `${held.targetSession}`,
    )
  }
  if (context.generation !== held.generation) {
    throw new Error(
      `receipt: evidence from lead generation ${context.generation}, and ${record.id} went to ` +
        `generation ${held.generation}`,
    )
  }
  const at = requireTime(context.now, 'receipt')
  const items = evidenceAfter(context.itemsAfter, held.cursor)
  if (items === null) return record
  const limit = context.receiptMs ?? DEFAULT_RECEIPT_MS
  const evidence = items.filter((item) => PROVENANCE[item?.role] === held.evidenceType)
  const expired = typeof record.submittedAt === 'number' && at - record.submittedAt > limit

  if (record.channel === 'cf-read') {
    const parts = record.parts ?? []
    const partCoverage = parts.map((part) =>
      evidence.filter((item) => matchPart(part, item?.text)).map((item) => item.id),
    )
    if (parts.length > 0 && partCoverage.every((ids) => ids.length > 0)) {
      return transition(record, 'accepted', {
        acceptedAt: at,
        partCoverage,
        evidenceIds: [...new Set(partCoverage.flat())],
      })
    }
    const waiting = { ...record, partCoverage }
    return expired
      ? transition(waiting, 'uncertain', { reason: `no receipt after ${limit} ms` })
      : waiting
  }

  const evidenceIds = evidence
    .filter(
      (item) => record.nativeSubmissionId === undefined || item.id === record.nativeSubmissionId,
    )
    .filter((item) => matchInline(record, item?.text))
    .map((item) => item.id)
  if (evidenceIds.length > 0) {
    return transition(record, 'accepted', { acceptedAt: at, evidenceIds })
  }
  return expired
    ? transition(record, 'uncertain', { reason: `no receipt after ${limit} ms` })
    : record
}

function sameSnapshot(held, given) {
  if (held === null || typeof held !== 'object') return false
  if (given === null || typeof given !== 'object') return false
  return (
    held.targetSession === given.targetSession &&
    held.generation === given.generation &&
    held.cursor === given.cursor &&
    held.evidenceType === given.evidenceType
  )
}

// -------------------------------------------------------------- lifecycle

/** Crash recovery: a record caught in `submitting` comes back `uncertain`, never replayed. */
export function recover(record, { reason } = {}) {
  if (record?.state !== 'submitting') return record
  return transition(record, 'uncertain', {
    reason: reason ?? 'restart recovery: submitting at crash',
  })
}

/**
 * `failed` — the one replayable end — needs affirmative evidence that NOTHING
 * was written: `bytesWritten === 0`, the number. Anything else, a partial
 * write or a bridge that never said, is `uncertain`: the answer may be in the
 * lead's pane, and a second copy is worse than a missing one.
 */
export function fail(record, { bytesWritten, reason } = {}) {
  if (bytesWritten === 0) {
    return transition(record, 'failed', { reason: reason ?? 'no byte was written' })
  }
  const why =
    typeof bytesWritten === 'number' && bytesWritten > 0
      ? `${bytesWritten} byte(s) may have reached the lead`
      : `the write outcome is unknown (bytesWritten: ${JSON.stringify(bytesWritten)})`
  return transition(record, 'uncertain', { reason: reason ?? why })
}

/** `cancelled` is terminal and never re-planned; an acceptance cannot be cancelled. */
export function cancel(record, { reason } = {}) {
  return transition(record, 'cancelled', { reason: reason ?? 'cancelled' })
}

/** A file write failure is the caller's: a PENDING record stays pending with the reason. */
export function writeFailed(record, reason) {
  return transition(record, 'pending', { channel: 'cf-read', reason })
}

/** The target stays reserved from submission until the delivery resolves. */
export function isReserved(deliveries, { session, generation } = {}) {
  return deliveries.some(
    (record) =>
      record?.state === 'submitting' &&
      record?.target?.session === session &&
      record?.target?.generation === generation,
  )
}

// ------------------------------------------------------ coverage and seen

/**
 * Map delivery id → the WORKER answer ids it covers. Only an ACCEPTED record
 * covers anything; every other state maps to `[]`, so a cancelled or
 * uncertain delivery covers nothing and never advances `seen`.
 *
 * Coverage and evidence are different identities and this is where they were
 * conflated: the lead item ids that PROVED the delivery stay on the record as
 * `evidenceIds`, and they belong to the lead's transcript. What the unread
 * bookkeeping needs is the worker answer the delivery carried, which is the
 * only id the worker's transcript knows. The map has a null prototype: a
 * delivery id is data, and `__proto__` is an entry like any other.
 */
export function coverage(deliveries = []) {
  const covered = Object.create(null)
  for (const record of deliveries) {
    covered[record.id] = record.state === 'accepted' ? [record.answerId] : []
  }
  return covered
}

/**
 * The lead's `seen` marks over ONE conversation's transcript, as
 * `src/store.js` persists them: `row.seen[<leadId>]` is an ARRAY of worker
 * item ids, scoped per lead identity AND generation, and `seen.set` unions
 * into it. This returns the array to persist — the marks already there plus
 * the items now read — so handing it straight to `Store.seenSet` is
 * idempotent.
 *
 * `{row, leadId, conversation, deliveries, items}`. The conversation is
 * explicit and a delivery covers nothing outside it: two workers' transcripts
 * can both hold an item called `msg_01`, and one worker's accepted answer is
 * not the other's.
 *
 * The walk starts at the FIRST item and continues while each item is already
 * marked, covered or printed; the first item that is none of those stops it.
 * Starting from a stored mark instead would let a mark that landed past a gap
 * carry the frontier over the unread items in between.
 */
export function seenAfter(options = {}) {
  const { row, leadId, conversation, deliveries = [], items = [] } = options
  if (typeof leadId !== 'string' || leadId.length === 0) {
    throw new Error(
      'seenAfter needs leadId, the app-owned lead identity: marks are per lead and per generation',
    )
  }
  if (typeof conversation !== 'string' || conversation.length === 0) {
    throw new Error(
      'seenAfter needs the conversation: an answer id is unique only inside its own transcript',
    )
  }
  const covered = new Set()
  for (const record of deliveries) {
    if (record?.state !== 'accepted') continue
    if (record?.target?.leadId !== leadId) continue
    if (record?.conversation !== conversation) continue
    covered.add(record.answerId)
  }
  const marks = Array.isArray(row?.seen?.[leadId]) ? row.seen[leadId] : []
  const known = new Set(marks)
  const seen = [...marks]
  for (const item of items) {
    const id = item?.id
    if (known.has(id)) continue
    if (!covered.has(id) && item?.printed !== true) break
    seen.push(id)
    known.add(id)
  }
  return seen
}
