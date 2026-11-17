import { paneKey } from './term.js'

const INPUT_CHUNK_BYTES = 32 * 1024
const MAX_CONTROL_BYTES = 64 * 1024
const ADMISSION_TIMEOUT_MS = 1500
const WAIT_TIMEOUT_MS = 30_000
const INPUT_RETRY_MS = 100
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

// Find escape sequences before choosing byte cuts, including OSC/DCS strings.
// A cut may be at either end of a sequence, never inside it.
function controlSpans(bytes) {
  const spans = []
  for (let i = 0; i < bytes.length; i += 1) {
    if (bytes[i] !== 0x1b) continue
    const start = i
    const kind = bytes[++i]
    if (kind === 0x5b) {
      while (++i < bytes.length && (bytes[i] < 0x40 || bytes[i] > 0x7e)) {}
    } else if ([0x5d, 0x50, 0x58, 0x5e, 0x5f].includes(kind)) {
      while (++i < bytes.length) {
        if (bytes[i] === 0x07) break
        if (bytes[i] === 0x1b && bytes[i + 1] === 0x5c) {
          i += 1
          break
        }
      }
    } else {
      while (i < bytes.length && bytes[i] >= 0x20 && bytes[i] <= 0x2f) i += 1
    }
    spans.push({ start, end: Math.min(i + 1, bytes.length) })
  }
  return spans
}

function inputChunks(data) {
  const bytes = new TextEncoder().encode(data)
  const spans = controlSpans(bytes)
  const marker = (span, value) =>
    span.end - span.start === value.length &&
    value.every((byte, index) => bytes[span.start + index] === byte)
  const startMarker = [0x1b, 0x5b, 0x32, 0x30, 0x30, 0x7e]
  const endMarker = [0x1b, 0x5b, 0x32, 0x30, 0x31, 0x7e]
  const regions = []
  let previous = 0
  let pasteStart = null
  for (const span of spans) {
    if (pasteStart === null && marker(span, startMarker)) {
      if (previous < span.start) regions.push({ start: previous, end: span.start, paste: false })
      pasteStart = span.start
    } else if (pasteStart !== null && marker(span, endMarker)) {
      regions.push({ start: pasteStart, end: span.end, paste: true })
      previous = span.end
      pasteStart = null
    }
  }
  if (pasteStart !== null) regions.push({ start: pasteStart, end: bytes.length, paste: true })
  else if (previous < bytes.length)
    regions.push({ start: previous, end: bytes.length, paste: false })

  return regions.map((region) => {
    const chunks = []
    let start = region.start
    while (start < region.end) {
      let end = Math.min(start + INPUT_CHUNK_BYTES, region.end)
      while (end < region.end && (bytes[end] & 0xc0) === 0x80) end -= 1
      const crossed = spans.find((span) => span.start < end && span.end > end)
      if (crossed) end = crossed.start === start ? crossed.end : crossed.start
      if (end - start > MAX_CONTROL_BYTES || end <= start) {
        throw new Error(
          'This input contains a control sequence too long to send. Remove that sequence and paste again.',
        )
      }
      chunks.push(bytes.subarray(start, end))
      start = end
    }
    return { chunks, grouped: region.paste || chunks.length > 1 }
  })
}

/**
 * The page's side of the terminals: bytes from the Rust pane host into each
 * pane's emulator (in order, acknowledged only after the emulator committed
 * them), keystrokes and emulator replies back (in chunks, each admitted with a
 * ticket and waited for, so a large paste keeps later typing behind it), and
 * the emulator's size into the pane. Pane ownership stays in Rust.
 *
 * The host stops reading a pane once a window of its output is owed an
 * acknowledgement, and its program then waits on its terminal. So each message
 * goes to the emulator as it comes, which parses as many as it holds at its
 * next turn, and one acknowledgement frees everything up to the last it drew.
 * A page throttled behind a hidden window or a locked screen gets a turn a
 * second or so, and one message a turn, each waiting on the acknowledgement of
 * the one before, held an agent that printed faster to the page's pace (T-454).
 */
export class TerminalLink {
  #invoke
  #registry
  #report
  /** By pane: the last message drawn, the last acknowledged, and whether an acknowledgement is on its way. */
  #acks = new Map()
  #inputs = new Map()
  #replies = new Map()
  #admissions = new Map()
  #sequences = new Map()
  #sizes = new Map()
  #retired = new Set()
  #pageId
  #nextGroup = 1

  constructor({ invoke, registry, report, pageId = crypto.randomUUID() }) {
    this.#invoke = invoke
    this.#registry = registry
    this.#report = report
    this.#pageId = pageId
  }

  /** One message from `subscribe_output`: bytes for one pane, in order. */
  output(message, emulatorFor) {
    if (message === null || typeof message !== 'object') return
    const key = paneKey(message)
    // A pane let go of while its program still runs is read all the same: what it prints
    // is acknowledged and drawn nowhere, or the host would stop reading it for good.
    if (this.#retired.has(key)) return this.#drawn(key, message)
    const emulator = emulatorFor(message)
    if (emulator === null) return
    emulator
      .write(new Uint8Array(message.bytes ?? []))
      .then(() => this.#drawn(key, message))
      .catch((cause) => this.#report(cause))
  }

  /**
   * A pane's output is drawn up to `seq`: acknowledged, one acknowledgement on its way at
   * a time, each for the last drawn when it goes.
   */
  #drawn(key, { id, generation, seq }) {
    const ack = this.#acks.get(key) ?? { id, generation, drawn: 0, sent: 0, sending: false }
    this.#acks.set(key, ack)
    ack.drawn = Math.max(ack.drawn, seq)
    if (!ack.sending) void this.#acknowledge(ack)
  }

  async #acknowledge(ack) {
    ack.sending = true
    try {
      while (ack.sent < ack.drawn) {
        const seq = ack.drawn
        await this.#invoke('pane_ack', { id: ack.id, generation: ack.generation, seq })
        ack.sent = seq
      }
    } catch (cause) {
      this.#report(cause)
    } finally {
      ack.sending = false
    }
  }

  /** What the human types into a pane. */
  input(pane, data) {
    return this.#stream('pane_input_enqueue', pane, data).catch((cause) => this.#report(cause))
  }

  /** What the emulator answers the program on its own (cursor reports and the like). */
  reply(pane, data) {
    return this.#stream('pane_reply_enqueue', pane, data).catch((cause) => this.#report(cause))
  }

  /** The emulator's size, sent until the live pane accepts it. */
  resize(pane, cols, rows) {
    const key = paneKey(pane)
    if (this.#retired.has(key)) return
    const size = this.#sizes.get(key) ?? { running: false, applied: null }
    size.desired = `${cols}x${rows}`
    size.request = { id: pane.id, generation: pane.generation, cols, rows }
    this.#sizes.set(key, size)
    void this.#syncSize(key)
  }

  /** A pane that is gone: its output acknowledged and drawn nowhere, and no more input or resizes for it. */
  retire(key) {
    this.#retired.add(key)
    this.#inputs.delete(key)
    this.#replies.delete(key)
    this.#admissions.delete(key)
    this.#sequences.delete(key)
    this.#sizes.delete(key)
    this.#registry.retire(key)
  }

  /** Whether a pane is gone for good: it gets no emulator again. */
  retired(key) {
    return this.#retired.has(key)
  }

  async #syncSize(key) {
    const size = this.#sizes.get(key)
    if (size === undefined || size.running || size.applied === size.desired) return
    size.running = true
    const { desired, request } = size
    try {
      const result = await this.#invoke('pane_resize', request)
      if (result?.ok === true) size.applied = desired
    } catch (cause) {
      this.#report(cause)
    } finally {
      size.running = false
      if (this.#sizes.get(key) === size && size.desired !== desired) void this.#syncSize(key)
    }
  }

  async #stream(command, pane, data) {
    const key = paneKey(pane)
    const regions = inputChunks(data)
    // Human inputs and replies keep their own order. A whole paste takes
    // the admission turn together; a reply can pass later waiting keys.
    const streams = command === 'pane_reply_enqueue' ? this.#replies : this.#inputs
    const previous = streams.get(key)
    const submit = async () => {
      for (const region of regions) {
        const groupId = region.grouped ? this.#nextGroup++ : null
        const send = async (held) => {
          for (let index = 0; index < region.chunks.length; index += 1) {
            if (this.#retired.has(key)) return false
            const group =
              groupId === null
                ? null
                : {
                    id: groupId,
                    start: index === 0,
                    end: index === region.chunks.length - 1,
                  }
            if (!(await this.#chunk(command, pane, region.chunks[index], group, held))) return false
          }
          return true
        }
        const sent = region.grouped
          ? await this.#exclusive(key, () => send(true))
          : await send(false)
        if (!sent) return
      }
    }
    const stream = previous ? previous.then(submit, submit) : submit()
    streams.set(key, stream)
    try {
      await stream
    } finally {
      if (streams.get(key) === stream) streams.delete(key)
    }
  }

  async #exclusive(key, send) {
    const previous = this.#admissions.get(key)
    const admission = previous ? previous.then(send, send) : send()
    this.#admissions.set(key, admission)
    try {
      return await admission
    } finally {
      if (this.#admissions.get(key) === admission) this.#admissions.delete(key)
    }
  }

  async #call(command, args, timeout) {
    let timer
    try {
      return await Promise.race([
        this.#invoke(command, args),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('pane input call timed out')), timeout)
        }),
      ])
    } finally {
      clearTimeout(timer)
    }
  }

  /** Find whether an uncertain request reached the host before trying it again. */
  async #position(pane) {
    let failures = 0
    const key = paneKey(pane)
    while (!this.#retired.has(key)) {
      try {
        const answer = await this.#call(
          'pane_input_position',
          {
            pageId: this.#pageId,
            id: pane.id,
            generation: pane.generation,
          },
          ADMISSION_TIMEOUT_MS,
        )
        if (answer?.ok === true && Number.isSafeInteger(answer.nextSequence)) return answer
        if (answer?.ok === false) return answer
      } catch {}
      failures += 1
      if (failures === 5)
        this.#report(new Error('Keep this window open while your typing reconnects.'))
      await pause(INPUT_RETRY_MS)
    }
    return null
  }

  async #admit(command, pane, bytes, group) {
    const key = paneKey(pane)
    const sequence = (this.#sequences.get(key) ?? 0) + 1
    let failures = 0
    while (!this.#retired.has(key)) {
      let answer
      try {
        answer = await this.#call(
          command,
          {
            pageId: this.#pageId,
            id: pane.id,
            generation: pane.generation,
            sequence,
            bytes: Array.from(bytes),
            ...(command === 'pane_input_enqueue' ? { group } : {}),
          },
          ADMISSION_TIMEOUT_MS,
        )
      } catch {
        // The request may have arrived. Query before any resend.
      }
      if (answer?.ok === true && typeof answer.ticket === 'string') {
        this.#sequences.set(key, sequence)
        return answer
      }
      if (
        answer?.ok === false &&
        answer.error !== 'pane-input-sequence-gap' &&
        answer.error !== 'pane-input-sequence-regression'
      )
        return answer
      const position = await this.#position(pane)
      if (position === null) return null
      if (position.ok !== true) return { ok: false, error: 'pane-input-uncertain' }
      if (position.nextSequence === sequence + 1 && typeof position.lastTicket === 'string') {
        this.#sequences.set(key, sequence)
        return { ok: true, ticket: position.lastTicket }
      }
      if (position.nextSequence !== sequence) {
        return { ok: false, error: 'pane-input-uncertain' }
      }
      // This number is not admitted. Submit it once, then ask again if its
      // answer is lost or it races the original request.
      failures += 1
      if (failures === 5)
        this.#report(new Error('Keep this window open while your typing reconnects.'))
      await pause(INPUT_RETRY_MS)
    }
    return null
  }

  async #chunk(command, pane, bytes, group, held) {
    const key = paneKey(pane)
    let admitted
    let full = 0
    do {
      admitted = held
        ? await this.#admit(command, pane, bytes, group)
        : await this.#exclusive(key, () => this.#admit(command, pane, bytes, group))
      if (admitted === null) return false
      if (
        admitted.error !== "the pane's input queue is full" &&
        admitted.error !== 'pane-input-ticket-capacity'
      )
        break
      full += 1
      if (full === 20)
        this.#report(
          new Error(
            'If typing stays stuck, check what appeared, restart the project, and retype what is missing.',
          ),
        )
      await pause(INPUT_RETRY_MS)
    } while (!this.#retired.has(key))
    if (this.#retired.has(key)) return false
    if (admitted.ok !== true || typeof admitted.ticket !== 'string') {
      const message =
        admitted.error === 'pane-input-uncertain'
          ? 'Check whether your last typing appeared, then restart the project before typing again.'
          : 'Typing did not reach this window. Restart the project and type it again.'
      this.#report(new Error(message))
      return false
    }
    // Completion and its acknowledgement are independent of the next
    // admission: a lost wait answer cannot hold later typing.
    void this.#collect(key, admitted.ticket)
    return true
  }

  async #collect(key, ticket) {
    let settled
    let failures = 0
    while (!this.#retired.has(key)) {
      try {
        settled = await this.#call(
          'pane_input_wait',
          { pageId: this.#pageId, ticket },
          WAIT_TIMEOUT_MS,
        )
        break
      } catch {
        failures += 1
        if (failures === 5)
          this.#report(new Error('Keep this window open while your typing is checked.'))
        await pause(INPUT_RETRY_MS)
      }
    }
    if (this.#retired.has(key)) return
    if (settled?.error === 'pane-input-stale-page') return
    void this.#forget(key, ticket)
    if (failures > 0 && settled?.error === 'pane-input-ticket-not-found') {
      this.#report(new Error('Check the window to see whether your last typing appeared.'))
      return
    }
    if (settled?.ok !== true) {
      this.#report(
        new Error(
          'Check whether your last typing appeared, then restart the project before typing again.',
        ),
      )
    }
  }

  async #forget(key, ticket) {
    while (!this.#retired.has(key)) {
      try {
        const answer = await this.#call(
          'pane_input_forget',
          { pageId: this.#pageId, ticket },
          ADMISSION_TIMEOUT_MS,
        )
        if (answer?.ok === true || answer?.error === 'pane-input-stale-page') return
      } catch {}
      await pause(INPUT_RETRY_MS)
    }
  }
}
