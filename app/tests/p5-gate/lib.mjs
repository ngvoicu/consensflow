// The gate's browser side: the shipped xterm (app/ui/vendor/xterm.js) in a page, with the
// helpers that read everything it keeps. From T-2096's lib.mjs, with named terminals, the carry
// rule (no write ends inside a character), answers recorded, and the muting a replay needs.
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
// The checkout this folder is in (app/tests/p5-gate/ -> the root); CF_TREE names another.
export const tree = process.env.CF_TREE ?? resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
const require = createRequire(resolve(tree, 'app/package.json'))
const pw = require('@playwright/test')

export async function open(browserName) {
  const browser = await pw[browserName].launch({ headless: true })
  const page = await browser.newPage()
  page.on('pageerror', (e) => console.error('PAGE ERROR', e.message))
  await page.setContent('<html><body></body></html>')
  await page.evaluate(async (xterm) => {
    const url = URL.createObjectURL(new Blob([xterm], { type: 'application/javascript' }))
    try { window.Term = (await import(url)).Terminal } finally { URL.revokeObjectURL(url) }
  }, readFileSync(resolve(tree, 'app/ui/vendor/xterm.js'), 'utf8'))
  await page.evaluate(() => {
    const G = (window.G = { terms: {}, carried: {}, answers: {}, leaks: {}, muted: {}, written: {}, errors: [] })
    window.addEventListener('error', (e) => G.errors.push(e.message))
    window.addEventListener('unhandledrejection', (e) => G.errors.push(String(e.reason)))
    G.reset = () => { for (const n of Object.keys(G.terms)) { try { G.terms[n].t.dispose() } catch {} G.terms[n].el.remove(); delete G.terms[n] }; G.errors.length = 0 }
    const fnv = (h, n) => Math.imul(h ^ (n >>> 0), 16777619) >>> 0
    G.make = (name, cols, rows) => {
      const t = new window.Term({ cols, rows, allowProposedApi: true, scrollback: 10000, convertEol: false })
      const el = document.body.appendChild(document.createElement('div'))
      t.open(el)
      G.terms[name] = { t, el }
      G.carried[name] = []
      G.answers[name] = []
      G.leaks[name] = 0
      G.muted[name] = false
      G.written[name] = 0
      t.onData((d) => { if (G.muted[name]) G.leaks[name]++; else G.answers[name].push(d) })
    }
    G.dispose = (name) => { const x = G.terms[name]; if (x) { x.t.dispose(); x.el.remove(); delete G.terms[name] } }
    G.mute = (name, on) => { G.muted[name] = on; G.terms[name].t.options.disableStdin = on }
    const plain = (t, data) => new Promise((done, fail) => {
      const timer = setTimeout(() => fail(new Error('xterm write did not finish: ' + (G.errors[0] ?? 'no page error'))), 20000)
      t.write(data, () => { clearTimeout(timer); done() })
    })
    const fromHex = (hex) => {
      const out = new Uint8Array(hex.length >> 1)
      for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16)
      return out
    }
    /** The rule of the page: no write ends inside a character; its unfinished tail (3 bytes at most) goes with the next write. Returns how many bytes are held back. */
    G.write = async (name, data) => {
      const t = G.terms[name].t
      const held = G.carried[name]
      let bytes
      if (held.length === 0) bytes = data instanceof Uint8Array ? data : Uint8Array.from(data)
      else bytes = Uint8Array.from([...held, ...data])
      G.carried[name] = []
      let i = bytes.length - 1, back = 0
      while (i >= 0 && back < 3 && (bytes[i] & 0xc0) === 0x80) { i--; back++ }
      if (i >= 0 && bytes[i] >= 0xc0) {
        const need = bytes[i] >= 0xf0 ? 3 : bytes[i] >= 0xe0 ? 2 : 1
        if (back < need) { G.carried[name] = Array.from(bytes.slice(i)); bytes = bytes.slice(0, i) }
      }
      if (bytes.length) await plain(t, bytes)
      return G.carried[name].length
    }
    /** The same for bytes given as hex, from byte `from` to byte `to`. */
    G.writeHex = (name, hex, from = 0, to = hex.length >> 1) => G.write(name, fromHex(hex.slice(from * 2, to * 2)))
    G.writeRepeat = async (name, data, count) => {
      let left = count
      const per = Math.max(1, Math.floor(60000 / data.length))
      while (left > 0) {
        const n = Math.min(per, left)
        const chunk = new Uint8Array(n * data.length)
        for (let k = 0; k < n; k++) chunk.set(data, k * data.length)
        await G.write(name, chunk)
        left -= n
      }
      return G.carried[name].length
    }
    G.resize = (name, cols, rows) => G.terms[name].t.resize(cols, rows)
    G.take = (name) => { const a = G.answers[name]; G.answers[name] = []; return a }
    G.sends = (name) => {
      const t = G.terms[name].t
      const sent = []
      const sub = t.onData((d) => sent.push(d))
      const was = G.muted[name]
      G.muted[name] = false
      const stdin = t.options.disableStdin
      t.options.disableStdin = false
      t.paste('A\nB')
      t.options.disableStdin = stdin
      G.muted[name] = was
      sub.dispose()
      return sent.join('')
    }
    G.state = (name) => {
      const t = G.terms[name].t
      const core = t._core, ih = core._inputHandler, bs = core._bufferService
      const buffer = (b) => {
        let h = 2166136261
        const wrapped = []
        for (let i = 0; i < b.lines.length; i++) {
          const line = b.lines.get(i)
          if (line.isWrapped) wrapped.push(i)
          const d = line._data
          for (let k = 0; k < d.length; k++) h = fnv(h, d[k])
          for (const k of Object.keys(line._combined)) { h = fnv(h, +k); for (const c of line._combined[k]) h = fnv(h, c.codePointAt(0)) }
          for (const k of Object.keys(line._extendedAttrs)) { h = fnv(h, +k); h = fnv(h, line._extendedAttrs[k].ext ?? 0); h = fnv(h, line._extendedAttrs[k].urlId ?? 0) }
          h = fnv(h, 0x9e3779b9)
        }
        return {
          lines: b.lines.length, cells: h, wrapped: wrapped.join(','), x: b.x, y: b.y, ybase: b.ybase,
          savedX: b.savedX, savedY: b.savedY, savedFg: b.savedCurAttrData.fg, savedBg: b.savedCurAttrData.bg,
          top: b.scrollTop, bottom: b.scrollBottom,
          tabs: Object.keys(b.tabs).filter((k) => b.tabs[k]).join(','),
        }
      }
      const text = []
      const a = t.buffer.active
      for (let i = 0; i < a.length; i++) text.push(a.getLine(i).translateToString(true))
      while (text.length && text[text.length - 1] === '') text.pop()
      const cs = core.coreService, ms = core.coreMouseService, ch = core._charsetService
      return {
        size: [t.cols, t.rows], active: a.type,
        normal: buffer(bs.buffers.normal), alt: buffer(bs.buffers.alt),
        modes: JSON.stringify(t.modes), dec: JSON.stringify(cs.decPrivateModes), core: JSON.stringify(cs.modes),
        hidden: cs.isCursorHidden, mouse: [ms._activeProtocol, ms._activeEncoding].join('/'),
        charset: [ch.glevel, ch._charsets.map((c) => (c ? Object.keys(c).length : 0)).join(','), ch.charset ? Object.keys(ch.charset).length : 0].join('/'),
        pen: [ih._curAttrData.fg, ih._curAttrData.bg, ih._curAttrData.extended?.ext ?? 0].join('/'),
        parser: ih._parser.currentState, join: ih._parser.precedingJoinState, interim: [...ih._utf8Decoder.interim].join(','),
        carried: G.carried[name].join(','), text,
      }
    }
    /** What the host tracks reliably, and so what a page that restarted at a mark must have right: the size, the modes, the margins of the buffer that shows, the cursor shown, which buffer shows, the mouse, and what a paste sends. */
    G.reliable = (name) => {
      const t = G.terms[name].t
      const core = t._core, bs = core._bufferService, cs = core.coreService, ms = core.coreMouseService
      const b = bs.buffer
      return {
        size: [t.cols, t.rows], active: t.buffer.active.type, margins: [b.scrollTop, b.scrollBottom],
        modes: JSON.stringify(t.modes), dec: JSON.stringify(cs.decPrivateModes), core: JSON.stringify(cs.modes),
        hidden: cs.isCursorHidden, mouse: [ms._activeProtocol, ms._activeEncoding].join('/'),
        carried: G.carried[name].join(','),
      }
    }
    G.diff = (a, b) => {
      const out = []
      for (const k of Object.keys(a)) {
        const x = JSON.stringify(a[k]), y = JSON.stringify(b[k])
        if (x !== y) out.push(k === 'text' ? 'text' : `${k}: ${x} | ${y}`)
      }
      return out
    }
  })
  return { browser, page }
}
