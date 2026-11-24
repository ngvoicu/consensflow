import { FitAddon, Terminal } from './vendor/xterm.js'

/**
 * How long a terminal's new size must hold before the terminal and the
 * program in it take it. Each resize makes the program redraw, and Claude
 * Code leaves behind a copy of what it showed at the old width: a divider
 * dragged across dozens of widths left dozens of copies of the chief's answer.
 */
const SETTLE_MS = 200

/** The pane host's bounds for a size (`MAX_TERMINAL_DIMENSION`): the page asks for none past it. */
export const MAX_PANE_DIMENSION = 4096

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
        // A character may occupy two terminal cells; its trailing cell is part of the
        // link and, at the right edge, determines whether its ending is ambiguous.
        const lastColumn = to.x + cells[end].getWidth() - 1
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
          .translateToString(true, lastColumn + 1)
          .trim()
        if (!after && lastColumn >= terminal.cols - PLAIN_LINK_EDGE_COLUMNS) continue
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
          range: { start: { x: from.x + 1, y: from.y }, end: { x: lastColumn + 1, y: to.y } },
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

/** `#rgb` or `#rrggbb` as its three channels, or null. */
function channels(colour) {
  const hex = /^#([\da-f]{3}|[\da-f]{6})$/i.exec(colour ?? '')?.[1]
  if (!hex) return null
  const full = hex.length === 3 ? [...hex].map((digit) => digit + digit).join('') : hex
  return [0, 2, 4].map((at) => Number.parseInt(full.slice(at, at + 2), 16))
}

/** WCAG's contrast of two colours, 1 to 21. */
function contrast(a, b) {
  const light = (colour) => {
    const [r, g, b] = colour.map((value) => {
      const share = value / 255
      return share <= 0.03928 ? share / 12.92 : ((share + 0.055) / 1.055) ** 2.4
    })
    return 0.2126 * r + 0.7152 * g + 0.0722 * b
  }
  const [high, low] = [light(a), light(b)].sort((x, y) => y - x)
  return (high + 0.05) / (low + 0.05)
}

/** Colour 16 to 255 of the 256 as xterm draws them: a 6×6×6 cube, then 24 greys. */
function extended(index) {
  if (index >= 232) return Array(3).fill(8 + (index - 232) * 10)
  const level = (step) => (step ? 55 + step * 40 : 0)
  const cube = index - 16
  return [Math.floor(cube / 36), Math.floor(cube / 6) % 6, cube % 6].map(level)
}

/**
 * A program's dim words (ESC[2m), drawn readable and still dim. xterm draws a dim word at half
 * its colour's opacity, over whatever is under it: the window's own words dim read at 4.4 to 1
 * on the well, and a colour dim as low as 2.3 (blue 2.7, grey 2.3), Claude Code's and Codex's
 * hints and footers among them. Here a dim word is its colour at `--term-dim` of its opacity,
 * or at the least share above that which reads at `--term-dim-contrast` to 1 on the well
 * (page.css): the same rule for the window's own words, the sixteen and the 256. Still partly
 * see-through, as xterm draws it, so over a ground of a program's own it is that colour fainter.
 *
 * What it does not reach, as xterm draws it: a colour too dark to read at the floor even whole
 * is drawn whole (xterm lifts one under half its own 4.5 to that half); a dim word in a colour of
 * the program's own (ESC[38;2;…m) xterm draws whole, not dim; and the window's own colours
 * swapped (ESC[7m) xterm draws in a colour of its own making, at its floor.
 *
 * The rules outweigh xterm's own (`.xterm-dom-renderer-owner-N .xterm-fg-N.xterm-dim`, made by its
 * DOM renderer from the theme) and yield to the colours xterm writes on a word itself.
 */
function dimWords(page, said) {
  if (typeof getComputedStyle !== 'function' || !page?.head || page.getElementById('term-dim')) {
    return
  }
  const style = getComputedStyle(page.documentElement)
  const share = Number.parseFloat(style.getPropertyValue('--term-dim')) || 0.5
  const floor = Number.parseFloat(style.getPropertyValue('--term-dim-contrast')) || 1
  const well = channels(said.background)
  const words = channels(said.foreground)
  // The sixteen as page.css says them, after the window's own colours in THEME. Without all of
  // them the page draws no dim word of its own: its rule for the window's own words would
  // outweigh xterm's for a colour it had not said.
  const sixteen = Object.keys(THEME)
    .slice(5)
    .map((key) => channels(said[key]))
  if (!well || !words || sixteen.some((colour) => !colour)) return
  // In 255ths, as a browser keeps a colour's opacity, and read at the floor both as the blend is
  // and in the whole steps of each channel the screen draws: not a rounding under it either way.
  const over = (colour, ground, alpha) =>
    colour.map((value, at) => value * alpha + ground[at] * (1 - alpha))
  const reads = (colour, ground, alpha) => {
    const blend = over(colour, ground, alpha)
    return Math.min(contrast(blend, ground), contrast(blend.map(Math.round), ground)) >= floor
  }
  const dim = (colour, ground) => {
    let step = Math.ceil(share * 255)
    while (step < 255 && !reads(colour, ground, step / 255)) step += 1
    return `rgba(${colour.join(', ')}, ${(step / 255).toFixed(4)})`
  }
  const rule = (classes, colour, ground = well) =>
    `.xterm .xterm-rows span.xterm-dim${classes} { color: ${dim(colour, ground)}; }`
  const rules = [rule('', words)]
  for (let index = 0; index < 256; index += 1) {
    rules.push(rule(`.xterm-fg-${index}`, index < 16 ? sixteen[index] : extended(index)))
  }
  // The window's own colours swapped: its ground's colour, on its words' colour.
  rules.push(rule('.xterm-fg-257', well, words))
  const sheet = page.createElement('style')
  sheet.id = 'term-dim'
  sheet.textContent = rules.join('\n')
  page.head.append(sheet)
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
    this.callbacks = { onReply, onResize }
    // Both OSC 8 and visible plain web addresses use the same click, hover and app opener.
    // OSC 8 takes priority and carries its destination whole, even when its text is wrapped.
    this.link = {
      activate: (event, uri) => {
        if (opensLink(event)) onLink(uri)
      },
      hover: (_event, uri) => {
        if (this.terminal.element) this.terminal.element.title = `${LINK_KEY}+click to open ${uri}`
      },
      leave: () => this.terminal.element?.removeAttribute('title'),
    }
    this.humanListeners = new Set()
    this.replaying = false
    this.#build()
    /** Whether the terminal has taken its host's size once. */
    this.sized = false
    this.settling = null
    /** The size the settle waits to take, "colsxrows". */
    this.pending = null
    this.resizeObserver = new ResizeObserver(() => this.fit())
    this.resizeObserver.observe(host)
    /** An unfinished UTF-8 suffix, at most three bytes, for the next output. */
    this.utf8Pending = null
    /** The replay's writes not yet done: an error of the page while they are fails them. */
    this.inflight = new Set()
    /** Whether anything was written to this xterm. */
    this.written = false
    this.failureWatch = (event) => {
      const failed = new Error(`xterm failed while a window's history was written: ${event.message}`)
      for (const fail of [...this.inflight]) fail(failed)
      this.inflight.clear()
    }
  }

  /** Make the xterm of this emulator, in its host: once, and again where a replay starts clean. */
  #build() {
    const colours = theme()
    this.terminal = new Terminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: 'ui-monospace, "SFMono-Regular", Menlo, Monaco, Consolas, monospace',
      fontSize: 12,
      lineHeight: 1.16,
      linkHandler: { ...this.link, allowNonHttpProtocols: false },
      // A program's words, in any colour on any background it asks for (ESC[41m and the
      // like), drawn at WCAG AA's 4.5 to 1 at least: the palette is bright for words on the
      // well, which no colour can be and still be dark enough under light words. xterm moves
      // the words' colour lighter or darker until they read; dim words, by its own rule, at half
      // (the page draws them brighter on the well: `dimWords`).
      minimumContrastRatio: 4.5,
      scrollback: 10_000,
      theme: colours,
    })
    this.terminal.registerLinkProvider(plainWebLinks(this.terminal, this.link))
    this.fitAddon = new FitAddon()
    this.terminal.loadAddon(this.fitAddon)
    this.terminal.open(this.host)
    dimWords(this.host.ownerDocument, colours)
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
        this.callbacks.onReply(data)
      }
    })
    this.resizeSubscription = this.terminal.onResize(({ cols, rows }) => {
      // The sizes a replay takes are the journal's own: the pane host has them.
      if (!this.replaying) this.callbacks.onResize(cols, rows)
    })
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

  /** Resolve once xterm has parsed the complete prefix; an unfinished suffix waits for more. */
  write(bytes) {
    let payload = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
    if (this.utf8Pending !== null) {
      const joined = new Uint8Array(this.utf8Pending.length + payload.length)
      joined.set(this.utf8Pending)
      joined.set(payload, this.utf8Pending.length)
      payload = joined
      this.utf8Pending = null
    }
    // xterm 6 loses a character when a write ends after its 0x80 second byte.
    // Leave a trailing incomplete sequence out of xterm until more bytes arrive.
    let start = payload.length
    while (start > 0 && payload.length - start < 3 && (payload[start - 1] & 0xc0) === 0x80)
      start -= 1
    if (start > 0) {
      const lead = payload[start - 1]
      // Include the malformed leads xterm itself treats as multi-byte starts: it
      // decides whether the completed sequence is valid when it parses it.
      const size =
        lead >= 0xc0 && lead <= 0xdf
          ? 2
          : lead >= 0xe0 && lead <= 0xef
            ? 3
            : lead >= 0xf0 && lead <= 0xf7
              ? 4
              : 0
      if (size && payload.length - start < size - 1) {
        this.utf8Pending = payload.slice(start - 1)
        payload = payload.subarray(0, start - 1)
      }
    }
    this.written = true
    return new Promise((resolve, reject) => {
      // Even an empty prefix must be queued behind earlier writes: callers can
      // acknowledge this message only once those earlier bytes have been parsed.
      // While a replay is written, a write xterm fails on never calls back: it is told as
      // the failure it is (`mute`), where live it stays as it was.
      if (this.replaying) this.inflight.add(reject)
      this.terminal.write(payload, () => {
        this.inflight.delete(reject)
        resolve()
      })
    })
  }

  /** How many bytes of an unfinished character it holds back for the next write (three at most). */
  get held() {
    return this.utf8Pending?.length ?? 0
  }

  /** The terminal's size now. */
  size() {
    return { cols: this.terminal.cols, rows: this.terminal.rows }
  }

  /**
   * A replay is written (`on`) or it is over. While it is written the terminal asks the program
   * nothing and takes no typing (`disableStdin`: a key, a paste, the page's own input, a cursor
   * report and a focus report, measured in Chromium and WebKit; the switch goes on before the
   * first write because xterm reports its focus by itself once a program sets 1004), takes no
   * fit, and tells nobody of the sizes it is given; a write xterm fails on is told as a
   * rejection. Over, it fits as a new window does.
   */
  mute(on) {
    if (on === this.replaying) return
    this.replaying = on
    this.terminal.options.disableStdin = on
    if (on) {
      window.addEventListener('error', this.failureWatch)
    } else {
      window.removeEventListener('error', this.failureWatch)
      this.inflight.clear()
      this.fit()
    }
  }

  /**
   * The terminal starts clean at `cols` by `rows`: a new xterm in the same place, with nothing
   * of the one before (the replay's start, or after xterm failed on a write). Nothing is
   * reported of the size.
   */
  restart(cols, rows) {
    const muted = this.replaying
    this.inflight.clear()
    this.utf8Pending = null
    clearTimeout(this.settling)
    this.settling = null
    this.pending = null
    // One nothing was written to is clean already.
    if (this.written) {
      this.#teardown()
      this.#build()
      this.written = false
    }
    this.sized = false
    this.replaying = true
    if (Number.isInteger(cols) && Number.isInteger(rows)) this.terminal.resize(cols, rows)
    this.replaying = muted
    this.terminal.options.disableStdin = muted
  }

  /** A size the journal holds, taken in order with the bytes (the host has it already). */
  resizeTo(cols, rows) {
    const was = this.replaying
    this.replaying = true
    try {
      this.terminal.resize(cols, rows)
    } finally {
      this.replaying = was
    }
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
    if (this.replaying) return
    if (this.host.clientWidth < 2 || this.host.clientHeight < 2) return
    if (!this.sized) {
      this.#fitNow()
      return
    }
    const size = this.#proposed()
    if (size === null) return
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

  /** The size the host's box holds, in whole cells and within the pane host's bounds: it refuses more. */
  #proposed() {
    const size = this.fitAddon.proposeDimensions()
    if (!Number.isInteger(size?.cols) || !Number.isInteger(size?.rows)) return null
    return {
      cols: Math.min(size.cols, MAX_PANE_DIMENSION),
      rows: Math.min(size.rows, MAX_PANE_DIMENSION),
    }
  }

  #fitNow() {
    this.settling = null
    this.pending = null
    if (this.replaying) return
    if (this.host.clientWidth < 2 || this.host.clientHeight < 2) return
    try {
      const size = this.#proposed()
      if (size !== null && (size.cols !== this.terminal.cols || size.rows !== this.terminal.rows)) {
        this.terminal._core?._renderService?.clear()
        this.terminal.resize(size.cols, size.rows)
      }
      this.sized = true
    } catch {
      // A card can move between the visible grid and the off-screen parking
      // lot during this frame. The next ResizeObserver delivery fits it.
    }
  }

  /** The xterm and what listens to it, taken down. */
  #teardown() {
    this.terminal.element?.removeEventListener('paste', this.imagePaste, true)
    this.terminal.textarea?.removeEventListener('blur', this.clearPaste)
    this.userInputSubscription.dispose()
    this.dataSubscription.dispose()
    this.resizeSubscription.dispose()
    // An xterm that failed on a write may not take down cleanly: it is left to go.
    try {
      this.terminal.dispose()
    } catch {}
  }

  dispose() {
    clearTimeout(this.settling)
    // There is no page-side EOF event. A suffix left at close cannot form a glyph.
    this.utf8Pending = null
    window.removeEventListener('error', this.failureWatch)
    this.inflight.clear()
    this.resizeObserver.disconnect()
    this.#teardown()
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
