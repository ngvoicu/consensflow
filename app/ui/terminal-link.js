import { paneKey } from './term.js'

// The largest paste: the host's own cap (`MAX_PAGE_INPUT_BYTES`), checked here
// too so that a larger one is refused before any of it is sent.
const MAX_PASTE_BYTES = 4 * 1024 * 1024
const ADMISSION_TIMEOUT_MS = 1500
const WAIT_TIMEOUT_MS = 30_000
const INPUT_RETRY_MS = 100
const RESTART_INPUT_ADVICE =
  'Check whether your last typing appeared, then restart the project before typing again.'
const LOST_TYPING = 'Some of your typing may not have reached this window. Check what appeared.'
const RECONNECTING =
  'ConsensFlow is reconnecting its terminals: what you type is not sent yet. Wait a moment, then type it again.'
// The host's own words for a window that holds all the input it takes (`InputError::queue_full`).
const WINDOW_FULL = "the pane's input queue is full"
const WINDOW_FULL_ADVICE =
  'This window is still taking in what you sent before, so your last typing was not sent. Wait a moment, then type it again.'
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
 * each numbered when it is typed and handed to the host at once; the host
 * keeps the window's order by the numbers), and the emulator's size into the
 * pane. Pane ownership stays in Rust.
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
  /** By pane: the last number issued to what was typed. */
  #issued = new Map()
  #sizes = new Map()
  #retired = new Set()
  #failedInputs = new Set()
  /** By window: the latest loss version this page has shown (the host's own numbers). */
  #shown = new Map()
  /** The last version of the count past the host's bound that this page has shown. */
  #overflowShown = 0
  #notices
  #onGate
  #pageId
  /**
   * The host's epoch for this document, once its activation was answered; null until then.
   * Until then the terminals are read-only: nothing typed, pasted, dropped or answered is
   * numbered or sent, so no gesture looks accepted and then vanishes.
   */
  #epoch = null
  #gateTold = 0
  #wanted = new Map()
  #wantedOverflow = null
  #flushing = false
  #named = new Map()
  #placed = null

  constructor({
    invoke,
    registry,
    report,
    pageId = crypto.randomUUID(),
    notices = { show: () => false, showOverflow: () => false },
    onGate = () => {},
  }) {
    this.#invoke = invoke
    this.#registry = registry
    this.#report = report
    this.#pageId = pageId
    this.#notices = notices
    this.#onGate = onGate
    this.#onGate(false)
  }

  /** This document's activation was answered with its epoch: the terminals take input. */
  open(epoch) {
    this.#epoch = epoch
    this.#onGate(true)
    for (const [key, name] of this.#named) {
      const at = key.lastIndexOf(':')
      this.#sendName({ id: key.slice(0, at), generation: Number(key.slice(at + 1)) }, name)
    }
  }

  /** Whether the terminals take input: this document's epoch is acknowledged. */
  get ready() {
    return this.#epoch !== null
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
    return this.#send('pane_input_enqueue', pane, data)
  }

  /** What the emulator answers the program on its own (cursor reports and the like). */
  reply(pane, data) {
    return this.#send('pane_reply_enqueue', pane, data)
  }

  /**
   * What the host owes the person, as it came at activation, by a pull or with a refusal:
   * `lost` (each `{ id, generation, loss, closed }`, the host's version of the window's loss) and
   * `overflow` (`{ version, count }`, the losses past the host's bound). Each version is shown
   * once on this page, as a row of its own naming its window; the host is told it was shown
   * only after the row is there, and is told again for a version already known, until it
   * has cleared it (an answer can be lost).
   */
  async receive({ lost = [], overflow = null } = {}) {
    const confirm = []
    for (const notice of lost) {
      const { id, generation, loss } = notice
      if (!Number.isSafeInteger(loss)) continue
      const key = paneKey({ id, generation })
      if (loss > (this.#shown.get(key) ?? 0)) {
        // Shown means a person can see it: a row that is out of view or covered is not, and
        // is asked for again at the next pull.
        const seen = this.#notices.show({
          pane: { id, generation },
          loss,
          closed: notice.closed === true || this.#retired.has(key),
          name: notice.name ?? null,
          text: this.#failedInputs.has(key) ? RESTART_INPUT_ADVICE : LOST_TYPING,
        })
        if (!seen) continue
        this.#shown.set(key, loss)
      }
      confirm.push({ id, generation, loss })
    }
    let overflowShown = null
    if (overflow !== null && Number.isSafeInteger(overflow?.version)) {
      if (
        overflow.version > this.#overflowShown &&
        this.#notices.showOverflow({ version: overflow.version, count: overflow.count })
      ) {
        this.#overflowShown = overflow.version
      }
      if (overflow.version <= this.#overflowShown) {
        overflowShown = { version: overflow.version, count: overflow.count }
      }
    }
    this.#confirm(confirm, overflowShown)
  }

  /** Ask the host what it owes the person. An idle page asks again on a timer. */
  async pull() {
    if (this.#epoch === null) return
    let answer
    try {
      answer = await this.#call(
        'pane_lost_pull',
        { pageId: this.#pageId, epoch: this.#epoch },
        ADMISSION_TIMEOUT_MS,
      )
    } catch {
      return
    }
    if (answer?.ok === true) await this.receive(answer)
  }

  /**
   * The person has been shown these (`{ id, generation, loss }`) and the count of the overflow:
   * the host may forget exactly those versions. One confirmation is in flight at a time, and
   * its wait is bounded: an answer that never comes holds neither a pull nor the next
   * confirmation. The host keeps what it was not told, and the next pull asks again.
   */
  #confirm(keys, overflow) {
    for (const each of keys) this.#wanted.set(paneKey(each), each)
    if (overflow !== null) this.#wantedOverflow = overflow
    void this.#flushConfirmations()
  }

  async #flushConfirmations() {
    if (this.#flushing) return
    this.#flushing = true
    try {
      while (this.#wanted.size > 0 || this.#wantedOverflow !== null) {
        const keys = [...this.#wanted.values()]
        const overflow = this.#wantedOverflow
        this.#wanted.clear()
        this.#wantedOverflow = null
        try {
          await this.#call(
            'pane_lost_told',
            { pageId: this.#pageId, epoch: this.#epoch, keys, overflow },
            ADMISSION_TIMEOUT_MS,
          )
        } catch {}
      }
    } finally {
      this.#flushing = false
    }
  }

  /** A refusal, a failed wait or a skip said the window's typing may be missing, with the host's number. */
  #noticed(pane, key, loss, text) {
    if (this.#retired.has(key)) return
    const numbered = Number.isSafeInteger(loss)
    // A version the person has seen (and perhaps dismissed) is not shown again.
    if (numbered && loss <= (this.#shown.get(key) ?? 0)) {
      this.#confirm([{ id: pane.id, generation: pane.generation, loss }], null)
      return
    }
    const seen = this.#notices.show({ pane, loss, closed: false, name: null, text })
    if (numbered && seen) {
      this.#shown.set(key, loss)
      this.#confirm([{ id: pane.id, generation: pane.generation, loss }], null)
    }
  }

  /**
   * A place among what is typed, taken where a gesture is made (a file dropped, whose path is
   * looked up before it is typed): the next number of the window, held for it. Null where the
   * terminals are read-only (the person is told) or the window is gone.
   */
  holdPlace(pane) {
    const key = paneKey(pane)
    if (this.#retired.has(key)) return null
    if (this.#epoch === null) {
      this.#gated()
      return null
    }
    const sequence = (this.#issued.get(key) ?? 0) + 1
    this.#issued.set(key, sequence)
    return { pane, key, sequence, epoch: this.#epoch, used: false }
  }

  /** Types what `action` pastes in `place`: its input takes the number the place holds. */
  withPlace(place, action) {
    place.used = true
    this.#placed = place
    try {
      action()
    } finally {
      const left = this.#placed
      this.#placed = null
      // Nothing was sent in it: its number is let go, and nothing is told, for nothing was typed.
      if (left !== null) void this.#release(left)
    }
  }

  /** A place no one used (the drop was refused, or the window closed): its number is let go. */
  releasePlace(place) {
    if (!place.used) {
      place.used = true
      void this.#release(place)
    }
  }

  async #release(place) {
    const key = place.key
    while (!this.#retired.has(key)) {
      try {
        const answer = await this.#call(
          'pane_input_release',
          {
            pageId: this.#pageId,
            epoch: place.epoch,
            id: place.pane.id,
            generation: place.pane.generation,
            sequence: place.sequence,
          },
          ADMISSION_TIMEOUT_MS,
        )
        if (typeof answer?.ok === 'boolean' && answer.retry !== true) return
      } catch {}
      await pause(INPUT_RETRY_MS)
    }
  }

  /** What a window is called, told to the host for the notice of a window that has closed. */
  nameWindow(pane, name) {
    const key = paneKey(pane)
    if (this.#named.get(key) === name) return
    this.#named.set(key, name)
    if (this.#epoch !== null) this.#sendName(pane, name)
  }

  #sendName(pane, name) {
    void this.#call(
      'pane_window_name',
      {
        pageId: this.#pageId,
        epoch: this.#epoch,
        id: pane.id,
        generation: pane.generation,
        name,
      },
      ADMISSION_TIMEOUT_MS,
    ).catch(() => {})
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
    this.#issued.delete(key)
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

  /**
   * What was typed gets its number now, and its call goes at once: nothing
   * waits in the page for a call before it, so a reload loses none of it. The
   * host writes a window's calls in the order of their numbers, and only of the epoch it
   * gave this document: until that is acknowledged nothing is numbered or sent, and the
   * person is told so.
   */
  async #send(command, pane, data) {
    const key = paneKey(pane)
    if (this.#retired.has(key)) return
    if (this.#epoch === null) {
      this.#gated()
      return
    }
    let epoch = this.#epoch
    let bytes
    try {
      bytes = inputBytes(data)
    } catch (cause) {
      // Too large: nothing is sent, and no number is spent.
      this.#reportInput(key, cause)
      return
    }
    // A drop's input takes the place it held where it was dropped.
    const place = this.#placed?.key === key ? this.#placed : null
    let sequence
    if (place !== null) {
      this.#placed = null
      sequence = place.sequence
      epoch = place.epoch
    } else {
      sequence = (this.#issued.get(key) ?? 0) + 1
      this.#issued.set(key, sequence)
    }
    try {
      await this.#hand(command, pane, key, sequence, bytes, epoch)
    } catch (cause) {
      this.#reportInput(key, cause)
    }
  }

  /** A gesture while the terminals are read-only: said, at most once a second. */
  #gated() {
    const now = Date.now()
    if (now - this.#gateTold < 1000) return
    this.#gateTold = now
    this.#report(new Error(RECONNECTING))
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
   * refused when it comes late. The skip itself is repeated until the host says it
   * is final: a number it cannot spend yet is asked again (`retry`), and the person is told
   * it is pending. Answers the number of the loss the host recorded for it.
   */
  async #skip(pane, sequence, epoch) {
    let failures = 0
    const key = paneKey(pane)
    while (!this.#retired.has(key)) {
      try {
        const answer = await this.#call(
          'pane_input_skip',
          {
            pageId: this.#pageId,
            epoch,
            id: pane.id,
            generation: pane.generation,
            sequence,
          },
          ADMISSION_TIMEOUT_MS,
        )
        if (typeof answer?.ok === 'boolean' && answer.retry !== true) return answer.loss
      } catch {}
      failures += 1
      if (failures === 5)
        this.#reportInput(key, new Error('Keep this window open while your typing reconnects.'))
      await pause(INPUT_RETRY_MS)
    }
  }

  /**
   * One call per number, at most once. A refusal is final for its number:
   * nothing was written, the person is told, and the numbers after it go on.
   * A call that ends unknown (a timeout, a lost answer) is skipped and its keys
   * may be lost; the notice says "may".
   */
  async #hand(command, pane, key, sequence, bytes, epoch) {
    let answer
    try {
      answer = await this.#call(
        command,
        {
          pageId: this.#pageId,
          epoch,
          id: pane.id,
          generation: pane.generation,
          sequence,
          length: bytes.length,
          bytes: base64Of(bytes),
        },
        ADMISSION_TIMEOUT_MS,
      )
    } catch {}
    if (this.#retired.has(key)) return
    if (answer?.ok === true && typeof answer.ticket === 'string') {
      // Completion and its acknowledgement are independent of what is typed
      // next: a lost wait answer cannot hold later typing.
      void this.#collect(pane, key, answer.ticket)
      return
    }
    let loss = answer?.loss
    // A call that ended unknown, or a refusal whose number the host could not keep, is skipped
    // until the host has spent its number.
    if (typeof answer?.ok !== 'boolean' || answer.spent === false) {
      loss = await this.#skip(pane, sequence, epoch)
    }
    if (this.#retired.has(key)) return
    this.#noticed(pane, key, loss, answer?.error === WINDOW_FULL ? WINDOW_FULL_ADVICE : LOST_TYPING)
  }

  async #collect(pane, key, ticket) {
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
      if (settled?.failedUntilRestart === true) this.#failedInputs.add(key)
      this.#noticed(
        pane,
        key,
        settled?.loss,
        this.#failedInputs.has(key)
          ? RESTART_INPUT_ADVICE
          : settled?.uncertain === true
            ? 'Some of your typing may have reached this window, and some may not. Check what appeared.'
            : LOST_TYPING,
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
