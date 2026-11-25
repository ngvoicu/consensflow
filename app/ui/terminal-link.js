import { paneKey } from './term.js'

const INPUT_CHUNK_BYTES = 32 * 1024
// The largest paste: the host's own cap (`MAX_PAGE_INPUT_BYTES`), checked here
// too so that a larger one is refused before any of it is sent.
const MAX_PASTE_BYTES = 4 * 1024 * 1024
const MAX_CONTROL_BYTES = 64 * 1024
const ADMISSION_TIMEOUT_MS = 1500
const WAIT_TIMEOUT_MS = 30_000
const INPUT_RETRY_MS = 100
/** How long a failed acknowledgement or pull waits before it is tried again: then each second. */
const RETRY_MS = [250, 500]
const retryDelay = (failures) => RETRY_MS[failures] ?? 1000
const RESTART_INPUT_ADVICE =
  'Check whether your last typing appeared, then restart the project before typing again.'
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/**
 * The pane host's answer to `pane_history`: four bytes (little endian) for the length of a header
 * of JSON, the header, the seed's bytes (`seedLength`), then the journal's. A refusal is a header
 * alone. Undefined for an answer that is not such a frame (an app that keeps no journals).
 */
export function historyFrame(answer) {
  const bytes =
    answer instanceof ArrayBuffer
      ? new Uint8Array(answer)
      : ArrayBuffer.isView(answer)
        ? new Uint8Array(answer.buffer, answer.byteOffset, answer.byteLength)
        : Array.isArray(answer) && answer.every((byte) => Number.isInteger(byte))
          ? Uint8Array.from(answer)
          : null
  if (bytes === null || bytes.length < 4) return undefined
  const length = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0, true)
  if (4 + length > bytes.length) return undefined
  let header
  try {
    header = JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + length)))
  } catch {
    return undefined
  }
  if (header === null || typeof header !== 'object') return undefined
  const seedEnd = 4 + length + (header.seedLength ?? 0)
  return { ...header, seed: bytes.subarray(4 + length, seedEnd), bytes: bytes.subarray(seedEnd) }
}

// Find escape sequences before choosing byte cuts, including OSC/DCS strings.
// A cut may be at either end of a sequence, never inside it.
/** A call's bytes as the base64 text both of Tauri's transports carry in its arguments. */
function base64Of(bytes) {
  let binary = ''
  for (let start = 0; start < bytes.length; start += 0x8000)
    binary += String.fromCharCode.apply(null, bytes.subarray(start, start + 0x8000))
  return btoa(binary)
}

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

  // A paste goes in as one call, whole with its markers, or not at all: a call
  // that is lost then loses the paste and never splits it. What is typed is cut
  // into calls at whole units (a character, an escape sequence).
  return regions.flatMap((region) => {
    if (region.paste) {
      if (region.end - region.start > MAX_PASTE_BYTES) {
        throw new Error(
          `This paste is too large to send (over ${MAX_PASTE_BYTES / (1024 * 1024)} MiB). Paste less of it.`,
        )
      }
      return [bytes.subarray(region.start, region.end)]
    }
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
    return chunks
  })
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
 *
 * A window's emulator starts with the window's history (P5-C): the page pulls the pane host's
 * journal of what the window printed and its sizes, writes it with the emulator muted (it asks
 * the program nothing and takes no typing), and then takes the live messages from the boundary
 * the host fixed for this page, each exactly once, in the order of their offsets. What the
 * window printed before the journal begins is not shown, and the card says so.
 *
 * No history is pulled before the page's subscription stands (the boundary is the
 * subscription's). Windows the person sees are drawn again first; the others, one at a time,
 * once the page has read its board and none of the shown ones is being drawn. A window is muted
 * from its making: one that waits its turn takes no key and tells nobody of a size, and what it
 * is sent waits, unacknowledged, for it.
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
  /** By pane: where its emulator stands in the window's output (`#begin`). */
  #windows = new Map()
  #onTruncated
  /** Whether the pane `id:generation` is one the person sees now. */
  #shown
  /** The page's output subscription stands: a history may be pulled. */
  #subscribed = false
  /** The page has read its board since the subscription: the windows not shown may begin. */
  #refreshed = false

  constructor({
    invoke,
    registry,
    report,
    pageId = crypto.randomUUID(),
    onTruncated = () => {},
    shown = () => true,
  }) {
    this.#invoke = invoke
    this.#registry = registry
    this.#report = report
    this.#pageId = pageId
    this.#onTruncated = onTruncated
    this.#shown = shown
  }

  /** The page's subscription to the output has answered: the shown windows are drawn again now. */
  subscribed() {
    this.#subscribed = true
    this.schedule()
  }

  /** The page has read its board since its subscription: a window not shown may now begin. */
  refreshed() {
    this.#refreshed = true
    this.schedule()
  }

  /** Whether the page's subscription stands. */
  get isSubscribed() {
    return this.#subscribed
  }

  /**
   * Begin the replays that may begin now: every window the person sees, at once; then, once the
   * board has been read, one of the others, where none is being drawn (one that waits to try a
   * pull again holds nobody back). Called whenever any of that may have changed.
   */
  schedule() {
    if (!this.#subscribed) return
    for (const window of this.#windows.values()) {
      if (!window.started && this.#shown(window.pane)) this.#start(window)
    }
    if (!this.#refreshed) return
    const drawing = [...this.#windows.values()].some(
      (window) => window.started && window.replaying && !window.retrying,
    )
    if (drawing) return
    const next = [...this.#windows.values()].find((window) => !window.started)
    if (next !== undefined) this.#start(next)
  }

  #start(window) {
    window.started = true
    void this.#replay(window)
  }

  /**
   * A pane's emulator is made: it is muted from now, it gets the window's history before
   * anything live, and the messages that come meanwhile wait. Once for a pane.
   */
  begin(pane, emulator = this.#registry.get(pane)) {
    const key = paneKey(pane)
    if (this.#windows.has(key) || this.#retired.has(key)) return
    if (emulator === null) return
    const window = {
      key,
      pane: { id: pane.id, generation: pane.generation },
      emulator,
      /** Until the history is in, live messages wait. */
      replaying: true,
      /** Whether its replay has begun, and whether it waits to try a pull again. */
      started: false,
      retrying: false,
      /** The last message the page was sent for it: what a window let go of is settled up to. */
      last: null,
      /** The offset of the next byte the emulator is handed; null until the history or the first message says. */
      next: null,
      /** The offset the emulator has parsed, not counting a character it holds back. */
      parsed: null,
      /** The live messages that wait, in the order they came. */
      inbox: [],
      boundary: null,
      truncated: false,
    }
    this.#windows.set(key, window)
    emulator.mute?.(true)
  }

  /** Pull the window's pieces, write them muted, and go live from the boundary. */
  async #replay(window) {
    const { emulator } = window
    let complete = false
    try {
      complete = await this.#rebuild(window)
    } catch (cause) {
      this.#report(cause)
    }
    // Let go of meanwhile: it is settled, and its emulator is gone.
    if (this.#retired.has(window.key)) return
    // Whatever happened, the window is not left muted and held: it goes on live.
    window.replaying = false
    emulator.mute?.(false)
    // Asked only with the whole of its history taken, and the offset it took: the host keeps an
    // ended window's journal unless that is its end.
    if (complete) void this.#historyDone(window, window.next)
    this.#pump(window)
    this.schedule()
  }

  /** Whether the whole history was written. */
  async #rebuild(window) {
    const { emulator } = window
    let from = null
    for (;;) {
      const piece = await this.#pull(window, from)
      if (piece === null) return false
      window.boundary = piece.boundary
      try {
        await this.#apply(window, piece)
      } catch (cause) {
        // xterm itself failed on the bytes (it stops drawing, as it does live): the window starts
        // clean, goes live from the boundary and says its history is cut. The failure does not
        // stay across reloads. The host is told nothing of the history.
        if (this.#retired.has(window.key)) return false
        const { cols, rows } = emulator.size?.() ?? { cols: undefined, rows: undefined }
        emulator.restart?.(cols, rows)
        emulator.mute?.(true)
        window.next = window.boundary
        window.parsed = window.boundary
        this.#truncated(window)
        return false
      }
      if (this.#retired.has(window.key)) return false
      if (piece.done) return true
      from = window.next
    }
  }

  /** Write one piece: the emulator starts clean where the piece says, takes its sizes at their offsets. */
  async #apply(window, piece) {
    const { emulator } = window
    if (piece.reset) {
      emulator.restart?.(piece.size.cols, piece.size.rows)
      emulator.mute?.(true)
      window.next = piece.from
      window.parsed = piece.from
      if (piece.seed.length > 0) await emulator.write(piece.seed)
      if (piece.truncated) this.#truncated(window)
    } else if (window.next !== piece.from) {
      // The host never gives a continuation that does not follow: start again from what it holds.
      throw new Error(`a piece of history from ${piece.from}, the window is at ${window.next}`)
    }
    let at = 0
    const bytes = piece.bytes
    for (const size of piece.resizes ?? []) {
      const upto = Math.min(Math.max(size.at - piece.from, at), bytes.length)
      if (upto > at) await emulator.write(bytes.subarray(at, upto))
      at = upto
      emulator.resizeTo?.(size.cols, size.rows)
    }
    if (at < bytes.length) await emulator.write(bytes.subarray(at))
    window.next = piece.from + bytes.length
    window.parsed = window.next - (emulator.held ?? 0)
  }

  #truncated(window) {
    if (window.truncated) return
    window.truncated = true
    this.#onTruncated(window.pane)
  }

  /**
   * One piece of the window's history from `from`. A call that fails and a refusal (the page's
   * subscription does not stand yet, `pane-not-subscribed`, among them) are tried again, 250 ms
   * then 500 ms then each second, from the same offset. Null where there is none to be had: this
   * page is no longer the current one (another has taken over, so it pulls nothing more), the
   * host keeps no journal of the window, the app keeps none at all, or the window was let go of.
   */
  async #pull(window, from) {
    for (let failures = 0; ; failures += 1) {
      if (this.#retired.has(window.key)) return null
      try {
        const answer = await this.#invoke('pane_history', {
          pageId: this.#pageId,
          ...window.pane,
          from,
        })
        // What was on its way to a window let go of is dropped.
        if (this.#retired.has(window.key)) return null
        const frame = historyFrame(answer)
        if (frame === undefined) return null
        if (frame.ok === true) return frame
        if (frame.error === 'pane-input-stale-page' || frame.error === 'pane-not-found') return null
      } catch (cause) {
        if (failures === 0) this.#report(cause)
      }
      window.retrying = true
      this.schedule()
      await pause(retryDelay(failures))
      window.retrying = false
    }
  }

  /** The whole history is in, up to `consumed`: an ended window's journal is let go, once. */
  async #historyDone(window, consumed) {
    try {
      await this.#invoke('pane_history_done', { pageId: this.#pageId, ...window.pane, consumed })
    } catch {}
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
    if (!this.#windows.has(key)) this.begin(message, emulator)
    const window = this.#windows.get(key)
    if (window === undefined) return
    window.last = { id: message.id, generation: message.generation, seq: message.seq }
    window.inbox.push(message)
    this.#pump(window)
    this.schedule()
  }

  /**
   * The live messages a window holds, written once its history is in: each at the emulator's
   * next offset. One that ends at or before it is below the history (the journal started again
   * past the boundary): dropped, and acknowledged. One that straddles it is written from it.
   * Where the host sent no offsets (no journal) the messages are written as they come.
   */
  #pump(window) {
    if (window.replaying || this.#retired.has(window.key)) return
    const { emulator } = window
    while (window.inbox.length > 0) {
      const message = window.inbox.shift()
      let bytes = new Uint8Array(message.bytes ?? [])
      const offset = message.offset
      let end
      if (offset !== undefined) {
        window.next ??= offset
        end = offset + bytes.length
        if (end <= window.next) {
          this.#drawn(window.key, message)
          continue
        }
        // One that starts past the emulator's offset is not promised: it is written whole.
        if (offset > window.next) window.next = offset
        bytes = bytes.subarray(window.next - offset)
        window.next = end
        window.parsed ??= offset
      }
      const wrote = emulator.write(bytes)
      const held = emulator.held ?? 0
      wrote
        .then(() => {
          if (end !== undefined) window.parsed = Math.max(window.parsed ?? 0, end - held)
          this.#drawn(window.key, message)
        })
        .catch((cause) => this.#report(cause))
    }
  }

  /**
   * A pane's output is drawn up to `seq`: acknowledged, one acknowledgement on its way at
   * a time, each for the last drawn when it goes.
   */
  #drawn(key, { id, generation, seq }) {
    const ack = this.#acks.get(key) ?? { key, id, generation, drawn: 0, sent: 0, sending: false }
    this.#acks.set(key, ack)
    ack.drawn = Math.max(ack.drawn, seq)
    if (!ack.sending) void this.#acknowledge(ack)
  }

  /**
   * An acknowledgement that fails, or is answered `ok: false` for a window still open, is tried
   * again, after 250 ms, 500 ms, then each second, until it is taken or the window is gone; the
   * host stops a window at its room until it is.
   */
  async #acknowledge(ack) {
    ack.sending = true
    let failures = 0
    try {
      while (ack.sent < ack.drawn) {
        const seq = ack.drawn
        try {
          const answer = await this.#invoke('pane_ack', {
            id: ack.id,
            generation: ack.generation,
            seq,
          })
          if (answer?.ok !== false) {
            ack.sent = seq
            failures = 0
            continue
          }
          if (answer.gone === true) return
        } catch (cause) {
          if (failures === 0) this.#report(cause)
        }
        await pause(retryDelay(failures))
        failures += 1
      }
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
    const window = this.#windows.get(key)
    // The history is being written, with the sizes the journal holds: none is the page's to say.
    if (window?.replaying) return
    const size = this.#sizes.get(key) ?? { running: false, applied: null }
    size.desired = `${cols}x${rows}`
    // `at`: the bytes of the window's output the emulator has parsed, where it took this size.
    size.request = {
      pageId: this.#pageId,
      id: pane.id,
      generation: pane.generation,
      cols,
      rows,
      ...(window?.parsed == null ? {} : { at: window.parsed }),
    }
    this.#sizes.set(key, size)
    void this.#syncSize(key)
  }

  /**
   * A pane that is gone: its output acknowledged and drawn nowhere, and no more input or resizes
   * for it. Every message the page was sent is settled now, drawn or not (the host cannot know
   * which delivered bytes a page will never draw), and each later one as it comes. It tells the
   * host nothing of the history: an ended window let go of keeps its journal.
   */
  retire(key) {
    const window = this.#windows.get(key)
    if (window !== undefined) {
      this.#windows.delete(key)
      if (window.last !== null) this.#drawn(key, window.last)
    }
    this.#retired.add(key)
    this.#inputs.delete(key)
    this.#sequences.delete(key)
    this.#sizes.delete(key)
    this.#failedInputs.delete(key)
    this.#registry.retire(key)
    this.schedule()
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
    const calls = inputChunks(data)
    // Keys and replies go in one order, one admission at a time: each call
    // finishes its admission before the next one starts.
    const streams = this.#inputs
    const previous = streams.get(key)
    const submit = async () => {
      for (const bytes of calls) {
        if (this.#retired.has(key)) return
        if (!(await this.#chunk(command, pane, bytes))) return
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
