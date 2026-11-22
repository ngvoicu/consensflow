import { paneKey } from './term.js'

// The largest paste: the host's own cap (`MAX_PAGE_INPUT_BYTES`), checked here
// too so that a larger one is refused before any of it is sent.
const MAX_PASTE_BYTES = 4 * 1024 * 1024
const ADMISSION_TIMEOUT_MS = 1500
const WAIT_TIMEOUT_MS = 30_000
const INPUT_RETRY_MS = 100
const RESTART_INPUT_ADVICE =
  'Check whether your last typing appeared, then restart the project before typing again.'
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** A call's bytes as the base64 text both of Tauri's transports carry in its arguments. */
function base64Of(bytes) {
  let binary = ''
  for (let start = 0; start < bytes.length; start += 0x8000)
    binary += String.fromCharCode.apply(null, bytes.subarray(start, start + 0x8000))
  return btoa(binary)
}

/**
 * What the emulator emits at once is one call: a paste with its markers, typed
 * keys, a long control string. Over the host's cap it is refused whole, before
 * any of it is sent.
 */
function inputBytes(data) {
  const bytes = new TextEncoder().encode(data)
  if (bytes.length > MAX_PASTE_BYTES) {
    throw new Error(
      `This paste is too large to send (over ${MAX_PASTE_BYTES / (1024 * 1024)} MiB). Paste less of it.`,
    )
  }
  return bytes
}

/**
 * The page's side of the terminals: bytes from the Rust pane host into each
 * pane's emulator (in order, acknowledged only after the emulator committed
 * them), keystrokes and emulator replies back (one call each, a paste whole,
 * each admitted with a number and a ticket, in one order), and the emulator's
 * size into the pane. Pane ownership stays in Rust.
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
  #sequences = new Map()
  #sizes = new Map()
  #retired = new Set()
  #failedInputs = new Set()
  #pageId

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
    return this.#stream('pane_input_enqueue', pane, data).catch((cause) =>
      this.#reportInput(paneKey(pane), cause),
    )
  }

  /** What the emulator answers the program on its own (cursor reports and the like). */
  reply(pane, data) {
    return this.#stream('pane_reply_enqueue', pane, data).catch((cause) =>
      this.#reportInput(paneKey(pane), cause),
    )
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
    this.#sequences.delete(key)
    this.#sizes.delete(key)
    this.#failedInputs.delete(key)
    this.#registry.retire(key)
  }

  /** Whether a pane is gone for good: it gets no emulator again. */
  retired(key) {
    return this.#retired.has(key)
  }

  // The status line is shared by panes. Once any pane needs a restart,
  // later input notices must keep that advice until the pane is retired.
  #reportInput(key, cause, failed = false) {
    if (failed) this.#failedInputs.add(key)
    this.#report(this.#failedInputs.size > 0 ? new Error(RESTART_INPUT_ADVICE) : cause)
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
    const bytes = inputBytes(data)
    // Keys and replies go in one order, one admission at a time: each call
    // finishes its admission before the next one starts.
    const streams = this.#inputs
    const previous = streams.get(key)
    const submit = async () => {
      if (this.#retired.has(key)) return
      await this.#chunk(command, pane, bytes)
    }
    const stream = previous ? previous.then(submit, submit) : submit()
    streams.set(key, stream)
    try {
      await stream
    } finally {
      if (streams.get(key) === stream) streams.delete(key)
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

  /**
   * A call whose fate is unknown is never sent again: the host moves the pane's
   * count past its number, so the original either went in before this or is
   * refused when it comes late. The skip itself may be repeated.
   */
  async #skip(pane, sequence) {
    let failures = 0
    const key = paneKey(pane)
    while (!this.#retired.has(key)) {
      try {
        const answer = await this.#call(
          'pane_input_skip',
          {
            pageId: this.#pageId,
            id: pane.id,
            generation: pane.generation,
            sequence,
          },
          ADMISSION_TIMEOUT_MS,
        )
        if (typeof answer?.ok === 'boolean') return
      } catch {}
      failures += 1
      if (failures === 5)
        this.#reportInput(key, new Error('Keep this window open while your typing reconnects.'))
      await pause(INPUT_RETRY_MS)
    }
  }

  /**
   * One call per number, at most once. A refusal means nothing was written, so
   * the caller may retry the same number. A call that ends unknown (a timeout,
   * a lost answer) is skipped and its keys may be lost.
   */
  async #admit(command, pane, length, text) {
    const key = paneKey(pane)
    if (this.#retired.has(key)) return null
    const sequence = (this.#sequences.get(key) ?? 0) + 1
    let answer
    try {
      answer = await this.#call(
        command,
        {
          pageId: this.#pageId,
          id: pane.id,
          generation: pane.generation,
          sequence,
          length,
          bytes: text,
        },
        ADMISSION_TIMEOUT_MS,
      )
    } catch {}
    if (answer?.ok === true && typeof answer.ticket === 'string') {
      this.#sequences.set(key, sequence)
      return answer
    }
    if (answer?.ok === false) {
      if (answer.error === 'pane-input-sequence-stale') this.#sequences.set(key, sequence)
      return answer
    }
    this.#sequences.set(key, sequence)
    await this.#skip(pane, sequence)
    return this.#retired.has(key) ? null : { ok: false, error: 'pane-input-lost' }
  }

  async #chunk(command, pane, bytes) {
    const key = paneKey(pane)
    let admitted
    const text = base64Of(bytes)
    do {
      admitted = await this.#admit(command, pane, bytes.length, text)
      if (admitted === null) return false
      if (
        admitted.error !== "the pane's input queue is full" &&
        admitted.error !== 'pane-input-ticket-capacity'
      )
        break
      // A full queue is a paste still being written at the terminal's pace:
      // it says nothing until the host reports an uncertain write.
      await pause(INPUT_RETRY_MS)
    } while (!this.#retired.has(key))
    if (this.#retired.has(key)) return false
    if (admitted.ok !== true || typeof admitted.ticket !== 'string') {
      // Refused or lost before anything was written: this call's typing is
      // missing, and what follows goes on.
      this.#reportInput(
        key,
        new Error('Some of your typing may not have reached this window. Check what appeared.'),
      )
      return admitted.error !== 'pane-input-stale-page'
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
          this.#reportInput(key, new Error('Keep this window open while your typing is checked.'))
        await pause(INPUT_RETRY_MS)
      }
    }
    if (this.#retired.has(key)) return
    if (settled?.error === 'pane-input-stale-page') return
    void this.#forget(key, ticket)
    if (failures > 0 && settled?.error === 'pane-input-ticket-not-found') {
      this.#reportInput(
        key,
        new Error('Check the window to see whether your last typing appeared.'),
      )
      return
    }
    if (settled?.ok !== true) {
      // A failed pane refuses later typing, even when this call wrote nothing.
      this.#reportInput(
        key,
        new Error(
          settled?.uncertain === true
            ? RESTART_INPUT_ADVICE
            : 'Some of your typing may not have reached this window. Check what appeared.',
        ),
        settled?.failedUntilRestart === true,
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
