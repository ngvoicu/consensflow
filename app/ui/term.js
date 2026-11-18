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

/** Windows' Ctrl+V is a paste shortcut, not a control character for the program. */
const WINDOWS = /Windows/.test(globalThis.navigator?.userAgent ?? '')

/** The key a click must hold to open a link, as VS Code's terminal asks: Cmd on a Mac, Ctrl elsewhere. */
const LINK_KEY = MAC ? 'Cmd' : 'Ctrl'

/**
 * Whether `event`, a click on a link, is the one that opens it: the primary button with the
 * system's key held, so a click into a pane to type or select never opens one, and nor does
 * the other system's key (Ctrl+click is a Mac's secondary click).
 */
const opensLink = (event) => event.button === 0 && (MAC ? event.metaKey : event.ctrlKey)

// Pi's recorded output uses one padding column; leave three more for the other harnesses.
const PLAIN_LINK_EDGE_COLUMNS = 4

/**
 * Find visible plain addresses in the terminal's buffer. Join only rows xterm marks as a
 * soft wrap, and return the same whole address from each row it crosses. An address ending
 * in the edge zone with nothing visible after it is left unlinked: a harness might continue it
 * on a hard row. Rows with table borders are left alone as well.
 * Screen layouts can still hide a continuation behind another column (the stated limit).
 */
function plainWebLinks(terminal, link) {
  return {
    provideLinks(y, callback) {
      const buffer = terminal.buffer.active
      let first = y - 1
      let last = first
      if (!buffer.getLine(first)) return callback(undefined)
      // A program can fill the scrollback with one wrapped line. Bound work per pointer move,
      // and decline the whole logical line rather than inspecting only a shortened piece.
      const tooLong = () => (last - first + 1) * terminal.cols > 8192
      while (first > 0 && buffer.getLine(first)?.isWrapped) {
        first--
        if (tooLong()) return callback(undefined)
      }
      while (buffer.getLine(last + 1)?.isWrapped) {
        last++
        if (tooLong()) return callback(undefined)
      }
      let text = ''
      const places = []
      const cells = []
      for (let row = first; row <= last; row++) {
        const line = buffer.getLine(row)
        for (let x = 0; x < terminal.cols; x++) {
          const cell = line.getCell(x)
          if (!cell || cell.getWidth() === 0) continue
          const chars = cell.getChars() || ' '
          for (let i = 0; i < chars.length; i++) {
            places.push({ x, y: row + 1 })
            cells.push(cell)
          }
          text += chars
        }
      }
      if (/[│┃|]/u.test(text)) return callback(undefined)
      const links = []
      for (const match of text.matchAll(/https?:\/\/[^\s<>"`]+/gi)) {
        let url = match[0]
        const start = match.index
        const before = text[start - 1]
        // Claude and Pi write a Markdown link as `words (address)`. Accept that wrapper
        // only when its closing parenthesis is present; a cut address is not it.
        if (before === '(') {
          const opens = (url.match(/\(/g) || []).length
          const closes = (url.match(/\)/g) || []).length
          if (!url.endsWith(')') || closes !== opens + 1) continue
          url = url.slice(0, -1)
        }
        const end = start + url.length - 1
        const from = places[start]
        const to = places[end]
        if (from.y > y || to.y < y) continue
        // Reject an address embedded in a path, another scheme or a larger word, and any
        // token we would have to guess how to trim. URL() alone silently accepts backslashes
        // and malformed percent escapes; do not let it repair what the program printed.
        if (before && !/[\s([{<'"=]/u.test(before)) continue
        if (url.length > 2048) continue
        // A harness may hard-wrap just inside the right edge; if visible text follows on
        // the same row it has ended the address, even inside the zone.
        const after = buffer
          .getLine(to.y - 1)
          .translateToString(true, to.x + 1)
          .trim()
        if (!after && to.x >= terminal.cols - PLAIN_LINK_EDGE_COLUMNS) continue
        if (
          url.includes('\\') ||
          [...url].some((c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127)
        )
          continue
        if (/%(?![0-9a-f]{2})/i.test(url)) continue
        if (/[.,;:!?\]}']$/u.test(url)) continue
        if ((url.match(/\(/g) || []).length !== (url.match(/\)/g) || []).length) continue
        // OSC 8 cells belong only to xterm's higher-priority provider, even if their label
        // itself resembles a web address and their hidden destination was refused.
        if (
          cells
            .slice(start, end + 1)
            .some(
              (cell) =>
                cell.isInvisible() ||
                typeof cell.extended?.urlId !== 'number' ||
                cell.extended.urlId,
            )
        )
          continue
        try {
          const parsed = new URL(url)
          if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname) continue
        } catch {
          continue
        }
        links.push({
          range: { start: { x: from.x + 1, y: from.y }, end: { x: to.x + 1, y: to.y } },
          text: url,
          ...link,
        })
      }
      callback(links.length ? links : undefined)
    },
  }
}

/** xterm's colour for each of the page's `--term-*` variables (page.css). */
const THEME = {
  background: 'background',
  foreground: 'foreground',
  cursor: 'cursor',
  cursorAccent: 'background',
  selectionBackground: 'selection',
  black: 'black',
  red: 'red',
  green: 'green',
  yellow: 'yellow',
  blue: 'blue',
  magenta: 'magenta',
  cyan: 'cyan',
  white: 'white',
  brightBlack: 'bright-black',
  brightRed: 'bright-red',
  brightGreen: 'bright-green',
  brightYellow: 'bright-yellow',
  brightBlue: 'bright-blue',
  brightMagenta: 'bright-magenta',
  brightCyan: 'bright-cyan',
  brightWhite: 'bright-white',
}

/** The terminals' colours, the page's: what page.css does not say stays xterm's own. */
function theme() {
  if (typeof getComputedStyle !== 'function' || !globalThis.document?.documentElement) return {}
  const style = getComputedStyle(document.documentElement)
  const said = {}
  for (const [key, name] of Object.entries(THEME)) {
    const value = style.getPropertyValue(`--term-${name}`).trim()
    if (value) said[key] = value
  }
  return said
}

/**
 * What a window is sent for Shift+Enter (T-1330, T-1336, T-1348): a line feed, Ctrl+J, **one byte,
 * in every harness on a Mac and on Windows.** xterm.js 6 has neither the kitty keyboard protocol
 * nor modifyOtherKeys, and sends the plain Enter's CR for Shift+Enter, so a message was sent where
 * the human asked for a new line, in every harness. A window is not asked what it takes; the page
 * sends the key every harness lists, as the byte a terminal writes for it.
 *
 * **A pane's key never starts with Escape.** A program takes a lone Escape for "stop", and a read
 * that ends between the two bytes of a write reads the Escape alone, then the rest: a pty whose
 * input queue is nearly full (a window behind a long paste, slow to read: on a Mac, Codex read
 * `ESC CR` as Escape and Enter, which stops a turn and sends the message), the console host's
 * 4,096-byte read on Windows. Only a single byte cannot be split.
 *
 * - **A Mac:** every harness lists Ctrl+J as a new line (Claude Code's `chat:newline`, Codex's and
 *   Muse's keymaps, Devin's `ctrl-j`, Pi's `tui.input.newLine`), and a program in raw mode reads it
 *   as the key it is.
 * - **Windows, Claude Code and Pi:** they set the console's virtual-terminal input, which hands the
 *   byte on whole, so it is their new line there too.
 * - **Windows, Codex, Devin and Muse:** they read the console's key events, which make a line feed
 *   the Enter key with Control, bound to nothing: **Shift+Enter does nothing there, safely** (it
 *   used to send the message). Their new line is Alt+Enter, `ESC CR`, which a read can split into
 *   the Escape key and then Enter; and the kitty protocol's `CSI 13;2u` reaches them as seven key
 *   events, which Codex's own reader rebuilds as Shift+Enter, except where the console host's
 *   read ends after `ESC [` (it reads Alt+[ and types `13;2u`: measured, T-1348). A guard in the
 *   pane host that waited for an empty pipe before an Escape-led write was built and taken out
 *   (T-1336, T-1348): it waits for ever on a host whose pending read is smaller than the pipe's
 *   quota.
 *
 * Alt+Enter and Ctrl+J typed by the human stay xterm's.
 */
const SHIFT_ENTER = '\n'

/** Whether `event` is Shift+Enter alone, typed (not an input method choosing a candidate). */
const isShiftEnter = (event) =>
  event.key === 'Enter' &&
  event.shiftKey &&
  !event.ctrlKey &&
  !event.altKey &&
  !event.metaKey &&
  !event.isComposing

/**
 * The letter a key types for a shortcut: its own on a Latin layout, else the
 * one its place on the keyboard has (Ctrl+C on a Cyrillic layout is still
 * the C key).
 */
const shortcutLetter = (event) =>
  /^[a-z]$/i.test(event.key)
    ? event.key.toLowerCase()
    : event.code === 'KeyC'
      ? 'c'
      : event.code === 'KeyV'
        ? 'v'
        : null

/**
 * The page-side terminal contract. Pane ownership stays in Rust; this object
 * owns only one xterm parser, renderer and input subscription for one pane.
 *
 * @typedef {object} Emulator
 * @property {(bytes: Uint8Array) => Promise<void>} write
 * @property {(callback: (data: string) => void) => {dispose: () => void}} onData
 * @property {(text: string) => void} paste
 * @property {() => void} dispose
 */

export class XtermEmulator {
  constructor(host, { onReply = () => {}, onResize = () => {}, onLink = () => {} } = {}) {
    this.host = host
    // Both OSC 8 and visible plain web addresses use the same click, hover and app opener.
    // OSC 8 takes priority and carries its destination whole, even when its text is wrapped.
    const link = {
      activate: (event, uri) => {
        if (opensLink(event)) onLink(uri)
      },
      hover: (_event, uri) => {
        if (this.terminal.element) this.terminal.element.title = `${LINK_KEY}+click to open ${uri}`
      },
      leave: () => this.terminal.element?.removeAttribute('title'),
    }
    this.terminal = new Terminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: 'ui-monospace, "SFMono-Regular", Menlo, Monaco, Consolas, monospace',
      fontSize: 12,
      lineHeight: 1.16,
      linkHandler: { ...link, allowNonHttpProtocols: false },
      // A program's words, in any colour on any background it asks for (ESC[41m and the
      // like), drawn at WCAG AA's 4.5 to 1 at least: the palette is bright for words on the
      // well, which no colour can be and still be dark enough under light words. xterm moves
      // the words' colour lighter or darker until they read; dim words, by its own rule, at half.
      minimumContrastRatio: 4.5,
      scrollback: 10_000,
      theme: theme(),
    })
    this.terminal.registerLinkProvider(plainWebLinks(this.terminal, link))
    this.fitAddon = new FitAddon()
    this.terminal.loadAddon(this.fitAddon)
    this.terminal.open(host)
    this.terminal.attachCustomKeyEventHandler((event) => this.#programKey(event))
    this.awaitingPaste = false
    this.clearPaste = () => {
      this.awaitingPaste = false
    }
    this.imagePaste = (event) => {
      const fromShortcut = this.awaitingPaste
      this.awaitingPaste = false
      const data = event.clipboardData
      if (
        !fromShortcut ||
        !data ||
        data.getData('text/plain') ||
        !Array.from(data.items).some(
          (item) => item.kind === 'file' && item.type.startsWith('image/'),
        )
      )
        return
      // Before the Windows shortcut was passed to the browser, the harness received Ctrl+V
      // itself. Keep that image-only path for harnesses that read the clipboard for images.
      event.preventDefault()
      event.stopImmediatePropagation()
      this.terminal.input('\x16')
    }
    if (WINDOWS) {
      this.terminal.element?.addEventListener('paste', this.imagePaste, true)
      this.terminal.textarea?.addEventListener('blur', this.clearPaste)
    }
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
   * Whether xterm sends `event` to the program, and not when it copies; and what Shift+Enter
   * is sent as, which xterm cannot say. Where Ctrl+C is the program's interrupt (Windows, Linux), Ctrl+C copies
   * the text selected instead, as Windows Terminal does, and the selection
   * goes, so the next one interrupts; Ctrl+Shift+C copies on every system.
   * The copy is the page's own, which xterm fills with the selection, as it
   * does for Cmd+C on a Mac.
   */
  #programKey(event) {
    if (WINDOWS && event.type === 'keyup') this.awaitingPaste = false
    if (isShiftEnter(event)) {
      // Typed as a key press the window hears (the human's), once, on the key going down; the
      // press xterm would make of the rest of it (the keypress after the keydown) is none.
      if (event.type === 'keydown') {
        event.preventDefault()
        this.terminal.input(SHIFT_ENTER)
      }
      return false
    }
    if (event.type !== 'keydown' || !event.ctrlKey || event.altKey || event.metaKey) return true
    if (WINDOWS && !event.shiftKey && shortcutLetter(event) === 'v') {
      // Let the browser fire its paste event: xterm will paste its text, with the program's
      // bracketed-paste mode. Returning false stops xterm's raw Ctrl+V without cancelling the key.
      this.awaitingPaste = true
      return false
    }
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

  /**
   * Types `text` as a paste: xterm's own, which the human's paste event takes
   * too, so it reaches `onData` as theirs does, between the markers where
   * the program asked for them.
   */
  paste(text) {
    this.terminal.paste(text)
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
    this.terminal.element?.removeEventListener('paste', this.imagePaste, true)
    this.terminal.textarea?.removeEventListener('blur', this.clearPaste)
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
    onLink = () => {},
    createEmulator = (host, callbacks) => new XtermEmulator(host, callbacks),
  }) {
    this.emulators = new Map()
    this.onData = onData
    this.onReply = onReply
    this.onResize = onResize
    this.onLink = onLink
    this.createEmulator = createEmulator
  }

  ensure(pane, host) {
    const key = paneKey(pane)
    const existing = this.emulators.get(key)
    if (existing !== undefined) return existing.emulator

    const emulator = this.createEmulator(host, {
      onReply: (data) => this.onReply(pane, data),
      onResize: (cols, rows) => this.onResize(pane, cols, rows),
      onLink: (uri) => this.onLink(uri),
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
