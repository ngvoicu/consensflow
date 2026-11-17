import { paneKey } from './term.js'

const INPUT_CHUNK_BYTES = 32 * 1024
const ADMISSION_TIMEOUT_MS = 1500
const WAIT_TIMEOUT_MS = 30_000
const INPUT_RETRY_MS = 100
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

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

  constructor({ invoke, registry, report }) {
    this.#invoke = invoke
    this.#registry = registry
    this.#report = report
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
    const bytes = new TextEncoder().encode(data)
    // Human input and emulator replies each keep their order. A reply must be
    // able to pass a paste whose completion depends on that very reply.
    const streams = command === 'pane_reply_enqueue' ? this.#replies : this.#inputs
    const previous = streams.get(key)
    const submit = async () => {
      for (let offset = 0; offset < bytes.length; offset += INPUT_CHUNK_BYTES) {
        if (this.#retired.has(key)) return
        const chunk = bytes.subarray(offset, offset + INPUT_CHUNK_BYTES)
        if (!(await this.#chunk(command, pane, chunk))) return
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

  /** The bridge may fail before admission, or lose the answer after admission. */
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

  /** Only one numbered admission for a pane is in doubt at a time. */
  async #admit(command, pane, bytes) {
    const key = paneKey(pane)
    const send = async () => {
      let sequence = (this.#sequences.get(key) ?? 0) + 1
      this.#sequences.set(key, sequence)
      let failures = 0
      while (!this.#retired.has(key)) {
        try {
          const answer = await this.#call(
            command,
            { id: pane.id, generation: pane.generation, sequence, bytes: Array.from(bytes) },
            ADMISSION_TIMEOUT_MS,
          )
          if (answer?.ok === true && typeof answer.ticket === 'string') return answer
          if (answer?.ok === false && typeof answer.error === 'string') {
            const mismatch =
              answer.error === 'pane-input-sequence-gap' ||
              answer.error === 'pane-input-sequence-regression'
            if (
              mismatch &&
              Number.isSafeInteger(answer.expectedSequence) &&
              answer.expectedSequence > 0 &&
              answer.expectedSequence !== sequence
            ) {
              sequence = answer.expectedSequence
              this.#sequences.set(key, sequence)
              continue
            }
            return answer
          }
          throw new Error('no admission answer')
        } catch {
          // Retrying the same number is safe even if the host admitted it:
          // it returns the same ticket for an identical request.
          failures += 1
          if (failures === 5) {
            this.#report(new Error('Keep this window open while your typing reconnects.'))
          }
          await pause(INPUT_RETRY_MS)
        }
      }
      return null
    }

    const previous = this.#admissions.get(key)
    const admission = previous ? previous.then(send, send) : send()
    this.#admissions.set(key, admission)
    try {
      return await admission
    } finally {
      if (this.#admissions.get(key) === admission) this.#admissions.delete(key)
    }
  }

  async #chunk(command, pane, bytes) {
    const key = paneKey(pane)
    let admitted
    let full = 0
    do {
      admitted = await this.#admit(command, pane, bytes)
      if (admitted === null) return false
      if (
        admitted.error !== "the pane's input queue is full" &&
        admitted.error !== 'pane-input-ticket-capacity'
      )
        break
      full += 1
      if (full === 20)
        this.#report(new Error('Keep this window open while your typing catches up.'))
      // A known refusal used its number but wrote nothing. Let replies through
      // while the queue makes room, then submit these same bytes under the next.
      await pause(INPUT_RETRY_MS)
    } while (!this.#retired.has(key))
    if (this.#retired.has(key)) return false
    if (admitted.ok !== true || typeof admitted.ticket !== 'string') {
      this.#report(new Error('Typing did not reach this window. Reopen it and type again.'))
      return false
    }

    let settled
    let failures = 0
    while (!this.#retired.has(key)) {
      try {
        settled = await this.#call('pane_input_wait', { ticket: admitted.ticket }, WAIT_TIMEOUT_MS)
        break
      } catch {
        failures += 1
        if (failures === 5) {
          this.#report(new Error('Keep this window open while your typing is checked.'))
        }
        await pause(INPUT_RETRY_MS)
      }
    }
    if (this.#retired.has(key)) return false
    // Keep the receipt until the page has seen it, then release it. Both the
    // wait and this acknowledgement can be repeated after a lost answer.
    void this.#forget(key, admitted.ticket)
    // A wait may have reached the host even if its answer did not. Never
    // resend admitted bytes on an unknown outcome: that could type them twice.
    if (failures > 0 && settled?.error === 'pane-input-ticket-not-found') {
      this.#report(new Error('Check the window to see whether your last typing appeared.'))
      return true
    }
    if (settled?.ok !== true) {
      this.#report(new Error('Check the window, then reopen it if your typing did not appear.'))
      return false
    }
    return true
  }

  async #forget(key, ticket) {
    while (!this.#retired.has(key)) {
      try {
        const answer = await this.#call('pane_input_forget', { ticket }, ADMISSION_TIMEOUT_MS)
        if (answer?.ok === true) return
      } catch {}
      await pause(INPUT_RETRY_MS)
    }
  }
}
