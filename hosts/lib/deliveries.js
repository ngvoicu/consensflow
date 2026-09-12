import { createHash } from 'node:crypto'

/** Lossless result framing; the previous automatic sender and retry state machine are retired. */
const DEFAULT_PART_BUDGET = Object.freeze({ bytes: 32 * 1024, lines: 1000 })
const sha256 = (text) => createHash('sha256').update(text, 'utf8').digest('hex')
const byteLength = (text) => Buffer.byteLength(text, 'utf8')

/** Under the `u` flag a well-formed pair is one code point; only a LONE surrogate matches. */
/**
 * A header field is ONE LINE: no control character at all (TAB included), no
 * lone surrogate, and no Unicode line break of its own — U+2028 LINE SEPARATOR
 * and U+2029 PARAGRAPH SEPARATOR are Zl and Zp rather than Cc, so a predicate
 * built on control characters alone lets them through and the header stops
 * being a line. Result bodies may carry them; metadata may not.
 */
const HEADER_FIELD = /^[^\p{Cc}\p{Cs}\p{Zl}\p{Zp}]+$/u
/** The store allocator's namespace, and the only shape a delivery id may have. */
const DELIVERY_ID = /^d-\d+$/

function headerField(value, label) {
  const text = typeof value === 'string' ? value : ''
  if (!HEADER_FIELD.test(text)) {
    throw new Error(
      `a delivery header field is one printable line: ${label} is ${JSON.stringify(value)}`,
    )
  }
  return text
}

/** Stable, filename-safe result identities issued by the store allocator. */
function deliveryId(value, label = 'delivery id') {
  if (typeof value !== 'string' || !DELIVERY_ID.test(value)) {
    throw new Error(
      `a delivery id is the store allocator's d-<digits>, filename-safe and never reissued: ` +
        `${label} is ${JSON.stringify(value)}`,
    )
  }
  return value
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
export function partsFor(answer, id, budget = DEFAULT_PART_BUDGET) {
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

/** Read-only verification of historical writes. No retry, target rewrite or old-ledger mutation. */
export function legacyReceipt(record, { session, items, continuation } = {}) {
  if (!record.snapshot || !Number.isFinite(record.submittedAt) || !Array.isArray(items)) return null
  if (record.snapshot.targetSession !== session) {
    if (
      !record.manualRead ||
      record.channel !== 'cf-read' ||
      continuation?.from !== record.snapshot.targetSession ||
      continuation?.to !== session ||
      !Number.isFinite(continuation.at)
    )
      return null
    items = items.filter(
      (item) =>
        typeof item.at === 'string' &&
        Date.parse(item.at) >= Math.max(record.submittedAt, continuation.at),
    )
  }
  if (record.channel === 'cf-read') {
    const parts = record.parts ?? []
    const evidence = parts.map((part) =>
      items
        .filter((item) => item.role === 'tool' && matchPart(part, item.text))
        .map((item) => item.id),
    )
    return parts.length && evidence.every((ids) => ids.length)
      ? { parts: evidence, ids: [...new Set(evidence.flat())] }
      : null
  }
  const expected = envelope(record)
  if (digest(expected) !== record.digest) return null
  const ids = items
    .filter(
      (item) =>
        item.role === 'user' &&
        typeof item.text === 'string' &&
        item.text.includes(expected) &&
        (!record.nativeSubmissionId || item.id === record.nativeSubmissionId),
    )
    .map((item) => item.id)
  return ids.length ? { ids } : null
}

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
