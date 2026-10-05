import { FitAddon, Terminal } from './vendor/xterm.js'

/**
 * How long a terminal's new size must hold before the terminal and the
 * program in it take it. Each resize makes the program redraw, and Claude
 * Code leaves behind a copy of what it showed at the old width: a divider
 * dragged across dozens of widths left dozens of copies of the chief's answer.
 */
const SETTLE_MS = 200

/** A Mac, where Cmd+C copies and Ctrl+C is always the program's. */
const MAC = /Macintosh|Mac OS X/.test(globalThis.navigator?.userAgent ?? '')

/**
 * The letter a key types for a shortcut: its own on a Latin layout, else the
 * one its place on the keyboard has (Ctrl+C on a Cyrillic layout is still
 * the C key).
 */
const shortcutLetter = (event) =>
  /^[a-z]$/i.test(event.key) ? event.key.toLowerCase() : event.code === 'KeyC' ? 'c' : null

/**
 * The page-side terminal contract. Pane ownership stays in Rust; this object
 * owns only one xterm parser, renderer and input subscription for one pane.
 *
 * @typedef {object} Emulator
 * @property {(bytes: Uint8Array) => Promise<void>} write
 * @property {(callback: (data: string) => void) => {dispose: () => void}} onData
 * @property {() => void} dispose
 */

export class XtermEmulator {
  constructor(host, { onReply = () => {}, onResize = () => {} } = {}) {
    this.host = host
    this.terminal = new Terminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: 'ui-monospace, "SFMono-Regular", Menlo, Monaco, Consolas, monospace',
      fontSize: 12,
      lineHeight: 1.16,
      scrollback: 10_000,
    })
    this.fitAddon = new FitAddon()
    this.terminal.loadAddon(this.fitAddon)
    this.terminal.open(host)
    this.terminal.attachCustomKeyEventHandler((event) => this.#programKey(event))
    this.humanDataPending = 0
    this.humanListeners = new Set()
    // xterm's public onData event erases the internal `wasUserInput` bit. In
    // v6, onUserInput fires immediately before the matching onData event. Keep
    // that one-shot signal so a queued parser write can never steal a real
    // keyboard, paste, or IME emission and misroute it as an emulator reply.
    const coreService = this.terminal._core?.coreService
    if (typeof coreService?.onUserInput !== 'function') {
      this.terminal.dispose()
      throw new Error('xterm 6 user-input routing signal is unavailable')
    }
    this.userInputSubscription = coreService.onUserInput(() => {
      this.humanDataPending += 1
    })
    this.dataSubscription = this.terminal.onData((data) => {
      if (this.humanDataPending > 0) {
        this.humanDataPending -= 1
        for (const listener of this.humanListeners) listener(data)
      } else {
        onReply(data)
      }
    })
    this.resizeSubscription = this.terminal.onResize(({ cols, rows }) => onResize(cols, rows))
    /** Whether the terminal has taken its host's size once. */
    this.sized = false
    this.settling = null
    /** The size the settle waits to take, "colsxrows". */
    this.pending = null
    this.resizeObserver = new ResizeObserver(() => this.fit())
    this.resizeObserver.observe(host)
  }

  /**
   * Whether xterm sends `event` to the program, and not when it copies.
   * Where Ctrl+C is the program's interrupt (Windows, Linux), Ctrl+C copies
   * the text selected instead, as Windows Terminal does, and the selection
   * goes, so the next one interrupts; Ctrl+Shift+C copies on every system.
   * The copy is the page's own, which xterm fills with the selection, as it
   * does for Cmd+C on a Mac.
   */
  #programKey(event) {
    if (event.type !== 'keydown' || !event.ctrlKey || event.altKey || event.metaKey) return true
    if (shortcutLetter(event) !== 'c') return true
    if (!event.shiftKey && (MAC || !this.terminal.hasSelection())) return true
    event.preventDefault()
    if (this.terminal.hasSelection()) {
      document.execCommand('copy')
      this.terminal.clearSelection()
    }
    return false
  }

  /** Resolve only after xterm's parser has committed these bytes. */
  write(bytes) {
    const payload = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
    return new Promise((resolve) => {
      this.terminal.write(payload, resolve)
    })
  }

  onData(callback) {
    this.humanListeners.add(callback)
    return { dispose: () => this.humanListeners.delete(callback) }
  }

  /** Take the host's size: the first at once, a later one once it holds still. */
  fit() {
    if (this.host.clientWidth < 2 || this.host.clientHeight < 2) return
    if (!this.sized) {
      this.#fitNow()
      return
    }
    const size = this.fitAddon.proposeDimensions()
    if (!Number.isInteger(size?.cols) || !Number.isInteger(size?.rows)) return
    // Every redraw asks again: the size already waited for keeps its wait,
    // so redraws faster than the settle cannot hold a resize off for good.
    const target = `${size.cols}x${size.rows}`
    if (target === this.pending) return
    clearTimeout(this.settling)
    this.pending = null
    // The size it has already: nothing to take.
    if (size.cols === this.terminal.cols && size.rows === this.terminal.rows) return
    this.pending = target
    this.settling = setTimeout(() => this.#fitNow(), SETTLE_MS)
  }

  #fitNow() {
    this.settling = null
    this.pending = null
    if (this.host.clientWidth < 2 || this.host.clientHeight < 2) return
    try {
      this.fitAddon.fit()
      this.sized = true
    } catch {
      // A card can move between the visible grid and the off-screen parking
      // lot during this frame. The next ResizeObserver delivery fits it.
    }
  }

  dispose() {
    clearTimeout(this.settling)
    this.resizeObserver.disconnect()
    this.userInputSubscription.dispose()
    this.dataSubscription.dispose()
    this.resizeSubscription.dispose()
    this.terminal.dispose()
  }
}

export function paneKey(pane) {
  return `${pane.id}:${pane.generation}`
}

export class EmulatorRegistry {
  constructor({
    onData,
    onReply,
    onResize,
    createEmulator = (host, callbacks) => new XtermEmulator(host, callbacks),
  }) {
    this.emulators = new Map()
    this.onData = onData
    this.onReply = onReply
    this.onResize = onResize
    this.createEmulator = createEmulator
  }

  ensure(pane, host) {
    const key = paneKey(pane)
    const existing = this.emulators.get(key)
    if (existing !== undefined) return existing.emulator

    const emulator = this.createEmulator(host, {
      onReply: (data) => this.onReply(pane, data),
      onResize: (cols, rows) => this.onResize(pane, cols, rows),
    })
    const input = emulator.onData((data) => this.onData(pane, data))
    this.emulators.set(key, { emulator, input })
    return emulator
  }

  get(pane) {
    return this.emulators.get(paneKey(pane))?.emulator ?? null
  }

  retire(key) {
    const entry = this.emulators.get(key)
    if (entry === undefined) return
    entry.input.dispose()
    entry.emulator.dispose()
    this.emulators.delete(key)
  }

  fit(pane) {
    const emulator = this.get(pane)
    if (emulator === null) return
    if (typeof emulator.fit === 'function') requestAnimationFrame(() => emulator.fit())
  }
}
