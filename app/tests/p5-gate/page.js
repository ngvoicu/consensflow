// The browser side of P5-C's checks: the page's own modules (`/term.js`, `/terminal-link.js`, the
// shipped xterm) as a window's page runs them, with what it takes to read every part of what
// xterm keeps. A classic script, added to a page served from app/ui (`serveUi`), by the page
// suite (a stand-in for the pane host) and by the randomized gate (the real pane host).
//
//   await P5.load()
//   const win = P5.window(name, { cols, rows, invoke })   // a page's window, as the app makes one
//   win.start()                                           // asks for the window's history, muted
//   win.link.output(message, () => win.emulator)          // a live message
//
// `invoke(command, args)` is the app: `pane_history` answers the host's piece as an object
// ({ ok, reset, truncated, from, size: { rows, cols }, seed, resizes, bytes, boundary, done },
// seed and bytes as number arrays); this script frames it as the app's command does.
;(() => {
  const P5 = { windows: {}, errors: [] }
  window.P5 = P5
  window.addEventListener('error', (event) => P5.errors.push(event.message))
  window.addEventListener('unhandledrejection', (event) => P5.errors.push(String(event.reason)))
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms))

  P5.load = async () => {
    const [term, link] = await Promise.all([import('/term.js'), import('/terminal-link.js')])
    P5.XtermEmulator = term.XtermEmulator
    P5.TerminalLink = link.TerminalLink
    P5.historyFrame = link.historyFrame
  }

  /** The frame `pane_history` answers (`history_frame` in commands.rs), as an ArrayBuffer. */
  P5.frame = (piece) => {
    const seed = Uint8Array.from(piece.seed ?? [])
    const bytes = Uint8Array.from(piece.bytes ?? [])
    const header = new TextEncoder().encode(
      JSON.stringify(
        piece.ok === false
          ? { ok: false, error: piece.error }
          : {
              ok: true,
              reset: piece.reset,
              truncated: piece.truncated,
              from: piece.from,
              size: piece.size,
              seedLength: seed.length,
              resizes: piece.resizes,
              boundary: piece.boundary,
              done: piece.done,
            },
      ),
    )
    const out = new Uint8Array(4 + header.length + seed.length + bytes.length)
    new DataView(out.buffer).setUint32(0, header.length, true)
    out.set(header, 4)
    out.set(seed, 4 + header.length)
    out.set(bytes, 4 + header.length + seed.length)
    return out.buffer
  }

  const fnv = (h, n) => Math.imul(h ^ (n >>> 0), 16777619) >>> 0

  /** Every part of what xterm keeps (cells, cursors, margins, tab stops, modes, pen, parser). */
  P5.state = (emulator) => {
    const t = emulator.terminal
    const core = t._core
    const ih = core._inputHandler
    const bs = core._bufferService
    const buffer = (b) => {
      let h = 2166136261
      const wrapped = []
      for (let i = 0; i < b.lines.length; i++) {
        const line = b.lines.get(i)
        if (line.isWrapped) wrapped.push(i)
        const d = line._data
        for (let k = 0; k < d.length; k++) h = fnv(h, d[k])
        for (const k of Object.keys(line._combined)) {
          h = fnv(h, +k)
          for (const c of line._combined[k]) h = fnv(h, c.codePointAt(0))
        }
        for (const k of Object.keys(line._extendedAttrs)) {
          h = fnv(h, +k)
          h = fnv(h, line._extendedAttrs[k].ext ?? 0)
          h = fnv(h, line._extendedAttrs[k].urlId ?? 0)
        }
        h = fnv(h, 0x9e3779b9)
      }
      return {
        lines: b.lines.length,
        cells: h,
        wrapped: wrapped.join(','),
        x: b.x,
        y: b.y,
        ybase: b.ybase,
        savedX: b.savedX,
        savedY: b.savedY,
        savedFg: b.savedCurAttrData.fg,
        savedBg: b.savedCurAttrData.bg,
        top: b.scrollTop,
        bottom: b.scrollBottom,
        tabs: Object.keys(b.tabs)
          .filter((k) => b.tabs[k])
          .join(','),
      }
    }
    const text = []
    const a = t.buffer.active
    for (let i = 0; i < a.length; i++) text.push(a.getLine(i).translateToString(true))
    while (text.length && text[text.length - 1] === '') text.pop()
    const cs = core.coreService
    const ms = core.coreMouseService
    const ch = core._charsetService
    return {
      size: [t.cols, t.rows],
      active: a.type,
      normal: buffer(bs.buffers.normal),
      alt: buffer(bs.buffers.alt),
      modes: JSON.stringify(t.modes),
      dec: JSON.stringify(cs.decPrivateModes),
      core: JSON.stringify(cs.modes),
      hidden: cs.isCursorHidden,
      mouse: [ms._activeProtocol, ms._activeEncoding].join('/'),
      charset: [
        ch.glevel,
        ch._charsets.map((c) => (c ? Object.keys(c).length : 0)).join(','),
        ch.charset ? Object.keys(ch.charset).length : 0,
      ].join('/'),
      pen: [ih._curAttrData.fg, ih._curAttrData.bg, ih._curAttrData.extended?.ext ?? 0].join('/'),
      parser: ih._parser.currentState,
      join: ih._parser.precedingJoinState,
      interim: [...ih._utf8Decoder.interim].join(','),
      carried: (emulator.utf8Pending ?? []).join(','),
      text,
    }
  }

  /** What the host tracks reliably, and so what a page that restarted at a mark must have right. */
  P5.reliable = (emulator) => {
    const t = emulator.terminal
    const core = t._core
    const bs = core._bufferService
    const cs = core.coreService
    const ms = core.coreMouseService
    const b = bs.buffer
    return {
      size: [t.cols, t.rows],
      active: t.buffer.active.type,
      margins: [b.scrollTop, b.scrollBottom],
      marginsNormal: [bs.buffers.normal.scrollTop, bs.buffers.normal.scrollBottom],
      marginsAlt: [bs.buffers.alt.scrollTop, bs.buffers.alt.scrollBottom],
      modes: JSON.stringify(t.modes),
      dec: JSON.stringify(cs.decPrivateModes),
      core: JSON.stringify(cs.modes),
      hidden: cs.isCursorHidden,
      mouse: [ms._activeProtocol, ms._activeEncoding].join('/'),
      carried: (emulator.utf8Pending ?? []).join(','),
    }
  }

  P5.diff = (a, b) => {
    const out = []
    for (const k of Object.keys(a)) {
      const x = JSON.stringify(a[k])
      const y = JSON.stringify(b[k])
      if (x !== y) out.push(k === 'text' ? 'text' : `${k}: ${x} | ${y}`)
    }
    return out
  }

  /** What the emulator sends for a paste of two lines, as the person's own: brackets where the program asked. */
  P5.sends = (emulator) => {
    const sent = []
    const sub = emulator.onData((data) => sent.push(data))
    const stdin = emulator.terminal.options.disableStdin
    emulator.terminal.options.disableStdin = false
    emulator.paste('A\nB')
    emulator.terminal.options.disableStdin = stdin
    sub.dispose()
    return sent.join('')
  }

  /** An emulator with no link and nothing sizing it: the continuous xterm a replay is compared with. */
  P5.reference = (name, cols, rows) => {
    const host = document.body.appendChild(document.createElement('div'))
    const ref = { name, host, answers: [] }
    ref.emulator = new P5.XtermEmulator(host, { onReply: (data) => ref.answers.push(data) })
    ref.emulator.resizeObserver.disconnect()
    clearTimeout(ref.emulator.settling)
    ref.emulator.fit = () => {}
    ref.emulator.resizeTo(cols, rows)
    return ref
  }

  /**
   * One page's window, as the app makes it: the real emulator and the real link, the app's
   * answers from `invoke`. `replayed` settles once the link has gone live (it unmutes the
   * emulator then, whether it wrote all of the history or none).
   */
  P5.window = (
    name,
    { cols, rows, invoke, pageId = `page-${name}`, pane = { id: 'w', generation: 1 } },
  ) => {
    const host = document.body.appendChild(document.createElement('div'))
    const win = {
      name,
      host,
      pane,
      answers: [],
      leaks: 0,
      truncated: false,
      reports: [],
      writing: 0,
      calls: [],
      resizing: [],
    }
    let markReplayed
    win.replayed = new Promise((done) => {
      markReplayed = done
    }).then(() => win.quiet())
    win.emulator = new P5.XtermEmulator(host, {
      onReply: (data) => {
        win.answers.push(data)
        if (win.emulator.replaying) win.leaks += 1
      },
      onResize: (c, r) => win.link.resize(pane, c, r),
    })
    win.emulator.resizeObserver.disconnect()
    clearTimeout(win.emulator.settling)
    // No layout here: the size is the one the journal gives (a test that fits asks for it).
    win.emulator.fit = () => {}
    win.emulator.resizeTo(cols, rows)
    const mute = win.emulator.mute.bind(win.emulator)
    win.emulator.mute = (on) => {
      mute(on)
      if (!on) markReplayed()
    }
    const write = win.emulator.write.bind(win.emulator)
    win.emulator.write = (bytes) => {
      win.writing += 1
      return write(bytes).finally(() => {
        win.writing -= 1
      })
    }
    const app = async (command, args) => {
      win.calls.push(command)
      if (command === 'pane_history') {
        const piece = await invoke(command, args)
        return P5.frame(piece)
      }
      const promise = Promise.resolve(invoke(command, args))
      if (command === 'pane_resize') win.resizing.push(promise)
      return await promise
    }
    win.link = new P5.TerminalLink({
      invoke: app,
      registry: { get: () => win.emulator, retire: () => {} },
      report: (cause) => win.reports.push(String(cause?.message ?? cause)),
      pageId,
      onTruncated: () => {
        win.truncated = true
      },
    })
    // The page's subscription stands before a window's history is asked for.
    win.start = () => {
      win.link.begin(pane, win.emulator)
      win.link.subscribed()
    }
    /** Every write handed to the emulator is parsed. */
    win.quiet = async () => {
      while (win.writing > 0) await sleep(1)
    }
    /** The emulator takes a size, as xterm does for a person's drag; the app has heard of it once this settles. */
    win.resize = async (cols, rows) => {
      win.resizing.length = 0
      win.emulator.terminal.resize(cols, rows)
      await Promise.all(win.resizing)
    }
    win.dispose = () => {
      win.emulator.dispose()
      host.remove()
    }
    P5.windows[name] = win
    return win
  }
  P5.sleep = sleep

  /**
   * What a window printed before the page that is about to subscribe: an earlier page was handed
   * it (so it is history for the next, not parked) and has gone.
   */
  P5.history = async (host, bytes) => {
    const earlier = P5.window(`earlier-${host.seq}`, {
      cols: 80,
      rows: 24,
      invoke: host.invoke,
      pageId: `earlier-${host.seq}`,
    })
    host.subscribe((m) => earlier.link.output(m, () => earlier.emulator))
    earlier.start()
    await earlier.replayed
    host.print(bytes)
    await earlier.quiet()
    earlier.dispose()
    host.acks.length = 0
    host.done = 0
    host.calls.length = 0
  }

  P5.disposeAll = () => {
    for (const name of Object.keys(P5.windows)) {
      try {
        P5.windows[name].dispose()
      } catch {}
      delete P5.windows[name]
    }
  }

  /**
   * A case (the gate's ops: `emit`, `resize`, `reload`) through the fake host and the real page,
   * against a continuous xterm: what differs, in words, after each reload and at the end.
   */
  P5.runCase = async (c, { pieceBytes } = {}) => {
    const ref = P5.reference('ref', c.cols, c.rows)
    const host = P5.fakeHost({ cols: c.cols, rows: c.rows, pieceBytes })
    const differences = []
    const answers = []
    let count = 0
    let cur = null
    const subscribe = async () => {
      const name = `p${++count}`
      const win = P5.window(name, { cols: 80, rows: 24, invoke: host.invoke, pageId: name })
      host.subscribe((message) => win.link.output(message, () => win.emulator))
      win.start()
      await win.replayed
      await win.quiet()
      return win
    }
    const compare = (label) => {
      const d = P5.diff(P5.state(ref.emulator), P5.state(cur.emulator))
      const sa = P5.sends(ref.emulator)
      const sb = P5.sends(cur.emulator)
      if (sa !== sb) d.push(`paste: ${JSON.stringify(sa)} | ${JSON.stringify(sb)}`)
      if (cur.leaks) d.push(`${cur.leaks} answers made while muted`)
      if (cur.reports.length) d.push(`reported ${JSON.stringify(cur.reports)}`)
      if (d.length) differences.push(`${label}: ${d.join(' ; ')}`)
    }
    cur = await subscribe()
    for (const op of c.ops) {
      if (op.emit) {
        await ref.emulator.write(Uint8Array.from(op.emit))
        host.print(op.emit)
        await cur.quiet()
      } else if (op.resize) {
        ref.emulator.terminal.resize(op.resize[0], op.resize[1])
        await cur.resize(op.resize[0], op.resize[1])
      } else if (op.reload) {
        answers.push(...cur.answers)
        cur.dispose()
        cur = await subscribe()
        compare('after a reload')
      }
    }
    await cur.quiet()
    answers.push(...cur.answers)
    compare('at the end')
    if (JSON.stringify(ref.answers) !== JSON.stringify(answers))
      differences.push(
        `answers: expected ${JSON.stringify(ref.answers)} got ${JSON.stringify(answers)}`,
      )
    cur.dispose()
    ref.emulator.dispose()
    ref.host.remove()
    return differences
  }

  /**
   * A stand-in for the pane host and the app's commands, in the page, for one window: what the
   * program printed (`print`), whether it has `ended`, the sizes the page's emulator took, the pages that subscribed and
   * the boundary each was given, and `pane_history`'s pieces from that. It keeps the whole
   * journal (no cap), so it answers what the real host answers while the journal reaches the
   * window's start; the real host's rules are the randomized gate's. `pieceBytes` makes pieces
   * smaller than 1 MiB. `hold(command)` makes the next such call wait for `release()`.
   */
  P5.fakeHost = ({ cols, rows, pieceBytes = 1 << 20 }) => {
    const host = {
      printed: [],
      sizes: [],
      first: { cols, rows },
      pages: [],
      current: null,
      handed: 0,
      parked: [],
      seq: 0,
      acks: [],
      calls: [],
      held: new Map(),
      failNext: new Map(),
      done: 0,
      stale: new Set(),
    }
    const messageOf = (bytes) => {
      const message = {
        id: 'w',
        generation: 1,
        seq: ++host.seq,
        offset: host.printed.length,
        bytes,
      }
      host.printed.push(...bytes)
      return message
    }
    /** What the window prints: handed to the current page's sink where there is one, parked otherwise. */
    host.print = (bytes) => {
      const message = messageOf(Uint8Array.from(bytes))
      if (host.current !== null && !host.current.gone) {
        host.handed = message.offset + message.bytes.length
        host.current.sink(message)
      } else host.parked.push(message)
      return message
    }
    /** A page subscribes: its boundary is what was handed to pages, what was parked is its first live. */
    host.subscribe = (sink) => {
      const page = { sink, boundary: host.handed, gone: false }
      host.pages.push(page)
      host.current = page
      const parked = host.parked
      host.parked = []
      for (const message of parked) {
        host.handed = message.offset + message.bytes.length
        sink(message)
      }
      return page
    }
    host.release = (command) => {
      host.held.get(command)?.release()
      host.held.delete(command)
    }
    host.hold = (command) => {
      let release
      const waiting = new Promise((done) => (release = done))
      host.held.set(command, { waiting, release })
    }
    host.size = () => host.sizes.at(-1) ?? host.first
    host.invoke = async (command, args) => {
      host.calls.push([command, args])
      const held = host.held.get(command)
      if (held) await held.waiting
      const failing = host.failNext.get(command) ?? 0
      if (failing > 0) {
        host.failNext.set(command, failing - 1)
        throw new Error(`${command} failed`)
      }
      if (command === 'pane_ack') {
        host.acks.push(args.seq)
        return { ok: true }
      }
      if (command === 'pane_resize') {
        const at = Math.min(args.at ?? host.printed.length, host.printed.length)
        host.sizes.push({ at, rows: args.rows, cols: args.cols })
        return { ok: true }
      }
      if (command === 'pane_history_done') {
        host.done += 1
        // An ended window's journal is let go only where the page took all of it.
        if (host.ended && args.consumed === host.printed.length) host.gone = true
        return { ok: host.gone === true }
      }
      if (command === 'pane_history') {
        if (host.stale.has(args.pageId)) return { ok: false, error: 'pane-input-stale-page' }
        if (host.gone) return { ok: false, error: 'pane-not-found' }
        const page = host.current
        const from = args.from ?? 0
        const end = Math.min(from + pieceBytes, page.boundary)
        const reset = args.from == null
        return {
          ok: true,
          reset,
          truncated: false,
          from,
          size: reset ? host.first : { rows: 0, cols: 0 },
          seed: [],
          resizes: host.sizes
            .filter(
              (size) =>
                size.at <= end &&
                (reset ? size.at >= 0 : size.at > from) &&
                size.at <= page.boundary,
            )
            .map((size) => ({ at: size.at, rows: size.rows, cols: size.cols })),
          bytes: host.printed.slice(from, end),
          boundary: page.boundary,
          done: end >= page.boundary,
        }
      }
      return { ok: true }
    }
    return host
  }
})()
