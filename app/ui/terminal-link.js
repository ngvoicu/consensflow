import { paneKey } from './term.js'

// The largest paste: the host's own cap (`MAX_PAGE_INPUT_BYTES`), checked here
// too so that a larger one is refused before any of it is sent.
const MAX_PASTE_BYTES = 4 * 1024 * 1024
const ADMISSION_TIMEOUT_MS = 1500
const WAIT_TIMEOUT_MS = 30_000
const INPUT_RETRY_MS = 100
/** How long a failed acknowledgement or pull waits before it is tried again: then each second. */
const RETRY_MS = [250, 500]
const retryDelay = (failures) => RETRY_MS[failures] ?? 1000
const RESTART_INPUT_ADVICE =
  'Check whether your last typing appeared, then restart the project before typing again.'
const RECONNECTING =
  'ConsensFlow is reconnecting its terminals: what you type is not sent yet. Wait a moment, then type it again.'
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
  /** By pane: the last number issued to what was typed. */
  #issued = new Map()
  #sizes = new Map()
  #retired = new Set()
  #failedInputs = new Set()
  /** The ids the person dismissed on this page, never drawn again (the host's ids are never reused). */
  #dismissed = new Set()
  /** The dismissals the host has not acknowledged yet: asked again at each pull. */
  #unacknowledged = new Set()
  #notices
  #onGate
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
  /**
   * The host's epoch for this document, once its activation was answered; null until then.
   * Until then the terminals are read-only: nothing typed, pasted, dropped or answered is
   * numbered or sent, so no gesture looks accepted and then vanishes.
   */
  #epoch = null
  #gateTold = 0
  #named = new Map()
  #placed = null

  constructor({
    invoke,
    registry,
    report,
    pageId = crypto.randomUUID(),
    onTruncated = () => {},
    shown = () => true,
    notices = { draw: () => {} },
    onGate = () => {},
  }) {
    this.#invoke = invoke
    this.#registry = registry
    this.#report = report
    this.#pageId = pageId
    this.#onTruncated = onTruncated
    this.#shown = shown
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
        // xterm itself failed on the bytes (it stops drawing, as it does live), or the history
        // starts past the boundary: the window starts clean, goes live from the boundary and says
        // its history is cut. A failure does not stay across reloads. The host is told nothing of
        // the history.
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
    // A history that starts past the boundary leaves the live bytes between them with no owner:
    // the host never gives one, and none of it is taken (the window goes on as after a failure).
    if (piece.reset && piece.from > piece.boundary)
      throw new Error(`history from ${piece.from} starts past the boundary ${piece.boundary}`)
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
   * next offset. One that ends at or before it is already drawn (the offset is past the boundary
   * only by bytes the emulator has taken): dropped, and acknowledged. One that straddles it is
   * written from it. Where the host sent no offsets (no journal) the messages are written as they
   * come.
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
    return this.#send('pane_input_enqueue', pane, data)
  }

  /** What the emulator answers the program on its own (cursor reports and the like). */
  reply(pane, data) {
    return this.#send('pane_reply_enqueue', pane, data)
  }

  /**
   * What the host owes the person, as a pull of its list brought it (at activation, by a pull
   * or after its event): every outstanding notice, each by the host's id. Only such a list
   * draws rows, and every row on it is drawn until the person dismisses it: a refusal or a
   * wait's reply never draws one or takes one away. A notice's words are the host's, fixed
   * when it made it, and an id the person dismissed is not drawn again.
   */
  receive({ notices = [] } = {}) {
    const owed = notices.filter(
      (notice) => Number.isSafeInteger(notice?.id) && !this.#dismissed.has(notice.id),
    )
    this.#notices.draw(owed)
  }

  /** Ask the host what it owes the person. An idle page asks again on a timer. */
  async pull() {
    if (this.#epoch === null) return
    await this.#sendDismissals()
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
    if (answer?.ok === true) this.receive(answer)
  }

  /**
   * The person dismissed the notice with this id. It is gone from the page at once and never
   * drawn again; the host is told until it has heard (a dismissal of an id the host has since
   * replaced clears nothing of the new notice).
   */
  dismiss(id) {
    this.#dismissed.add(id)
    this.#unacknowledged.add(id)
    void this.#sendDismissals()
  }

  async #sendDismissals() {
    if (this.#epoch === null) return
    for (const id of [...this.#unacknowledged]) {
      try {
        const answer = await this.#call(
          'pane_lost_dismiss',
          { pageId: this.#pageId, epoch: this.#epoch, id },
          ADMISSION_TIMEOUT_MS,
        )
        if (typeof answer?.ok === 'boolean') this.#unacknowledged.delete(id)
      } catch {}
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
    const place = { pane, key, sequence, epoch: this.#epoch, used: false }
    void this.#tellPlace(place)
    return place
  }

  /**
   * The host holds the place from here: a reload before the path is typed is the host's to say
   * (it records the drop as a numbered loss with its notice), so the page keeps no marker of
   * its own. Asked until the host answers, or the place is used or let go.
   */
  async #tellPlace(place) {
    for (let tries = 0; tries < 20 && !place.used && !this.#retired.has(place.key); tries += 1) {
      try {
        const answer = await this.#call(
          'pane_input_place',
          {
            pageId: this.#pageId,
            epoch: place.epoch,
            id: place.pane.id,
            generation: place.pane.generation,
            sequence: place.sequence,
          },
          ADMISSION_TIMEOUT_MS,
        )
        if (typeof answer?.ok === 'boolean') return
      } catch {}
      await pause(INPUT_RETRY_MS)
    }
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
    this.#issued.delete(key)
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
   * it is pending.
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
        if (typeof answer?.ok === 'boolean' && answer.retry !== true) return
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
      void this.#collect(key, answer.ticket)
      return
    }
    // A call that ended unknown, or a refusal whose number the host could not keep, is skipped
    // until the host has spent its number. What the host recorded of it is told by its list:
    // this reply draws nothing.
    if (typeof answer?.ok !== 'boolean' || answer.spent === false) {
      await this.#skip(pane, sequence, epoch)
    }
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
      // A failed pane refuses later typing, even when this call wrote nothing. The notice of
      // it is the host's, from its list; this keeps the status line's advice.
      if (settled?.failedUntilRestart === true) this.#failedInputs.add(key)
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
