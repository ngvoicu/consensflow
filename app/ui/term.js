import { FitAddon, Terminal } from './vendor/xterm.js'

/**
 * The page-side terminal contract. Pane ownership stays in Rust; this object
 * owns only one xterm parser, renderer and input subscription for one pane.
 *
 * @typedef {object} Emulator
 * @property {(bytes: Uint8Array) => Promise<void>} write
 * @property {(callback: (data: string) => void) => {dispose: () => void}} onData
 * @property {(cols: number, rows: number) => void} resize
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
    this.resizeObserver = new ResizeObserver(() => this.fit())
    this.resizeObserver.observe(host)
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

  resize(cols, rows) {
    if (Number.isInteger(cols) && Number.isInteger(rows) && cols > 0 && rows > 0) {
      this.terminal.resize(cols, rows)
    }
  }

  fit() {
    if (this.host.clientWidth < 2 || this.host.clientHeight < 2) return
    try {
      this.fitAddon.fit()
    } catch {
      // A card can move between the visible grid and the off-screen parking
      // lot during this frame. The next ResizeObserver delivery fits it.
    }
  }

  dispose() {
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

  get(id, generation) {
    return this.emulators.get(`${id}:${generation}`)?.emulator ?? null
  }

  retire(key) {
    const entry = this.emulators.get(key)
    if (entry === undefined) return
    entry.input.dispose()
    entry.emulator.dispose()
    this.emulators.delete(key)
  }

  reconcile(liveKeys) {
    for (const key of this.emulators.keys()) {
      if (liveKeys.has(key)) continue
      this.retire(key)
    }
  }

  fit(id, generation) {
    const emulator = this.get(id, generation)
    if (emulator === null) return
    if (typeof emulator.fit === 'function') requestAnimationFrame(() => emulator.fit())
  }

  dispose() {
    this.reconcile(new Set())
  }
}
