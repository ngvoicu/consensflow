// P5's gate: a randomized comparison of a reloaded page with a continuous xterm, in one browser.
// The page under test is the shipped app page (app/ui: its own start, board view, terminals and
// link), loaded and reloaded as the app does it; only the app's bridge to it is stood in
// (`window.__TAURI__`): the daemon's board is one project with one chief's window, and the pane
// host's commands go to gate_bridge.rs, which answers them with the functions the app's own
// commands call (`cf_panes::page`) over a real pty, with the daemon's room. The continuous xterm
// is in a page of its own.
//   node gate.mjs <webkit|chromium> <random cases> <first seed> [past-cap cases] [out.jsonl]
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { writeFileSync, appendFileSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { open, pageScript, tree } from './lib.mjs'
import { randomCase, pastCapCase, namedCases, reviewCases } from './gen.mjs'
import { cases as review51 } from './cases51.mjs'

const [browserName = 'chromium', randomCount = '50', firstSeed = '1', pastCount = '0', out = ''] =
  process.argv.slice(2)
const hex = (bytes) => Buffer.from(bytes).toString('hex')
const ROLES = JSON.parse(readFileSync(resolve(tree, 'app/tests/fixtures/roles.json'), 'utf8'))

function bridge() {
  const child = spawn(resolve(tree, 'app/src-tauri/target/debug/examples/gate_bridge'), [], {
    stdio: ['pipe', 'pipe', 'inherit'],
  })
  const waiting = new Map()
  let next = 0
  let onMessage = () => {}
  createInterface({ input: child.stdout }).on('line', (line) => {
    const v = JSON.parse(line)
    if (v.msg) return onMessage(v.msg)
    waiting.get(v.re)?.(v)
    waiting.delete(v.re)
  })
  child.on('exit', (code) => {
    if (code) {
      console.error(`gate_bridge ended with ${code}`)
      process.exit(1)
    }
  })
  return {
    set onMessage(f) {
      onMessage = f
    },
    call(cmd, window, args = {}) {
      const re = ++next
      return new Promise((done) => {
        waiting.set(re, done)
        child.stdin.write(JSON.stringify({ cmd, window, re, ...args }) + '\n')
      })
    },
    send(cmd, window, args = {}) {
      child.stdin.write(JSON.stringify({ cmd, window, re: ++next, ...args }) + '\n')
    },
    async close() {
      await this.call('quit', '')
      child.stdin.end()
    },
  }
}

const { browser, page: refPage, ui } = await open(browserName)
const host = bridge()
const R = (fn, ...a) => refPage.evaluate(fn, ...a)
let windowNo = 0
const results = {
  cases: 0,
  failed: 0,
  compared: 0,
  reloads: 0,
  answers: 0,
  truncated: 0,
  pulls: 0,
  done: 0,
  forgotten: 0,
}
const failures = []

// ---- the app page -------------------------------------------------------------------------

/** What the app's `window.__TAURI__` is, in the page: the board stood in, the rest asked of node. */
function standIn() {
  // The blank page between two cases is none of the app's.
  if (!location.protocol.startsWith('http')) return
  window.__pending = 0
  window.__pulled = 0
  window.__errors = []
  window.addEventListener('error', (event) => window.__errors.push(event.message))
  window.addEventListener('unhandledrejection', (event) =>
    window.__errors.push(String(event.reason)),
  )
  // Neither xterm here holds the keyboard, as the continuous one does not: a focus report is then
  // the program's setting alone (the board focuses the chief's terminal, and a program that asked
  // for focus reports would hear it from one of them only). The page suite holds that a muted
  // xterm reports none.
  const focus = HTMLTextAreaElement.prototype.focus
  HTMLTextAreaElement.prototype.focus = function (...args) {
    if (!this.classList.contains('xterm-helper-textarea')) focus.apply(this, args)
  }
  // The product's fit asks the box for a size; here it asks for the size the xterm has, so the
  // sizes are the case's (a drag is the case's own resize of the xterm) and the fit's own paths
  // run unchanged. The modules are the page's own, imported once it has loaded them (no card, and
  // so no terminal, comes before the page has asked the app anything).
  document.addEventListener('DOMContentLoaded', async () => {
    // Whole URLs: WebKit resolves a relative one in an injected script against its own scripts.
    const [{ FitAddon }, { XtermEmulator }] = await Promise.all([
      import(`${location.origin}/vendor/xterm.js`),
      import(`${location.origin}/term.js`),
    ])
    FitAddon.prototype.proposeDimensions = function () {
      return { cols: this._terminal.cols, rows: this._terminal.rows }
    }
    window.__XtermEmulator = XtermEmulator
  })
  const decode = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0))
  const ask = async (command, args) => {
    window.__pending += 1
    try {
      return await window.gateCall(command, args)
    } finally {
      window.__pending -= 1
    }
  }
  const invoke = async (command, args) => {
    if (command === 'daemon_request') return window.gateBoard(args.operation)
    if (command === 'subscribe_output') {
      window.__output = args.onOutput
      return ask(command, { pageId: args.pageId })
    }
    if (command.startsWith('update_')) return { ok: false, error: 'not in the gate' }
    if (command === 'pane_history') {
      window.__pulled += 1
      const frame = await ask(command, args)
      return Uint8Array.from({ length: frame.length >> 1 }, (_, i) =>
        parseInt(frame.substr(i * 2, 2), 16),
      ).buffer
    }
    if (command === 'pane_reply_enqueue') {
      const text = new TextDecoder().decode(decode(args.bytes))
      return ask(command, { pageId: args.pageId, text })
    }
    if (command === 'pane_input_enqueue') return { ok: true, ticket: 't' }
    if (command.startsWith('pane_')) return ask(command, args)
    return { ok: true }
  }
  class Channel {
    onmessage = null
  }
  window.__TAURI__ = {
    core: { invoke, Channel },
    event: { listen: async () => () => {} },
    test: {
      createEmulator: (hostElement, callbacks) => {
        const emulator = new window.__XtermEmulator(hostElement, callbacks)
        window.__writing = 0
        const write = emulator.write.bind(emulator)
        emulator.write = (bytes) => {
          window.__writing += 1
          return write(bytes).finally(() => {
            window.__writing -= 1
          })
        }
        window.__emu = emulator
        return emulator
      },
    },
  }
  window.__quiet = () =>
    new Promise((done) => {
      let still = 0
      const tick = () => {
        still = window.__writing === 0 && window.__pending === 0 ? still + 1 : 0
        if (still >= 3) done()
        else setTimeout(tick, 1)
      }
      tick()
    })
}

const app = await browser.newPage()
app.on('pageerror', (e) => console.error('PAGE ERROR', e.message))
await app.addInitScript({ path: pageScript })
await app.addInitScript(standIn)

/** The case's window, as the page under test and the host see it. */
let current = null
await app.exposeFunction('gateBoard', (operation) => {
  const project = { id: 1, name: 'gate', directory: '/gate', state: 'open' }
  const lane = (id, handle, role, pane) => ({
    participant: {
      id,
      projectId: 1,
      handle,
      role,
      roles: [role],
      harness: role === 'human' ? null : 'claude-code',
      agent: null,
      designer: false,
      tier: null,
      outUntil: null,
      memberId: null,
      member: null,
      session: null,
    },
    tasks: [],
    activity: { state: pane ? 'working' : 'closed' },
    pane,
  })
  const pane = { id: current.win, generation: current.generation }
  if (operation === 'projects.list') return { ok: true, projects: [project] }
  if (operation === 'board.get')
    return {
      ok: true,
      roles: ROLES,
      board: {
        project,
        open: [],
        lanes: [lane(1, 'human', 'human', null), lane(2, 'chief', 'chief', pane)],
      },
    }
  if (operation === 'inbox.get') return { ok: true, messages: [], total: 0, shown: 0 }
  if (operation === 'harnesses.missing') return { ok: true, missing: [], roles: ROLES }
  if (operation === 'specs.get')
    return {
      ok: true,
      app: { on: false },
      applied: { folder: '.specs', variant: 'core' },
      specmint: { version: '3.0.0', variants: [] },
    }
  return { ok: true }
})
await app.exposeFunction('gateCall', async (command, args) => {
  const here = current
  if (command === 'pane_page_prepare')
    return (await host.call('page_prepare', '', { page: args.pageId }))
  if (command === 'pane_page_activate')
    return (await host.call('page_activate', '', { page: args.pageId }))
  if (command === 'subscribe_output') {
    here.page = args.pageId
    return (await host.call('subscribe', here.win, { page: args.pageId })).answer
  }
  if (command === 'pane_history') {
    results.pulls++
    const { hex: frame } = await host.call('history', here.win, {
      page: args.pageId,
      id: args.id,
      generation: args.generation,
      from: args.from,
    })
    return frame
  }
  if (command === 'pane_history_done') {
    results.done++
    const { answer } = await host.call('done', here.win, {
      page: args.pageId,
      id: args.id,
      generation: args.generation,
      consumed: args.consumed,
    })
    if (answer.ok) results.forgotten++
    return answer
  }
  if (command === 'pane_ack')
    return (await host.call('ack', here.win, {
      id: args.id,
      generation: args.generation,
      seq: args.seq,
    })).answer
  if (command === 'pane_resize')
    return (await host.call('resize', here.win, {
      page: args.pageId,
      id: args.id,
      generation: args.generation,
      cols: args.cols,
      rows: args.rows,
      at: args.at ?? null,
    })).answer
  if (command === 'pane_reply_enqueue') {
    here.answers.push(args.text)
    return { ok: true, ticket: 't' }
  }
  return { ok: true }
})

/** The app page's replay of the window is over: it has pulled, and the emulator takes typing. */
const replayed = () =>
  app.waitForFunction(
    () => window.__emu && window.__pulled > 0 && !window.__emu.replaying,
    null,
    { timeout: 120_000 },
  )

async function runCase(c) {
  const win = 'w' + ++windowNo
  await R(([c]) => {
    P5.ref?.emulator.dispose()
    P5.ref?.host.remove()
    P5.ref = P5.reference('ref', c.cols, c.rows)
  }, [c])
  const { generation } = await host.call('open', win, { rows: c.rows, cols: c.cols })
  const mine = { win, generation, page: null, answers: [], held: false, gone: false }
  current = mine
  let total = 0 // the bytes the program has printed
  let chain = Promise.resolve()
  host.onMessage = (m) => {
    // A page that has left is handed nothing; one that is about to leave keeps what it is sent.
    chain = chain.then(async () => {
      if (m.page !== mine.page || mine.held || m.id !== win) return
      await app.evaluate((m) => {
        const bytes = Uint8Array.from({ length: m.hex.length >> 1 }, (_, i) =>
          parseInt(m.hex.substr(i * 2, 2), 16),
        )
        window.__output.onmessage({
          id: m.id,
          generation: m.generation,
          seq: m.seq,
          offset: m.offset,
          bytes,
        })
      }, m)
    })
  }
  const differences = []
  const lostAnswers = new Set()
  const settle = async () => {
    await chain
    if (!mine.held && !mine.gone) {
      await app.evaluate(() => window.__quiet())
      await chain
      await app.evaluate(() => window.__quiet())
    }
  }
  /** What the departing page leaves: its errors, and what the human would read in its status line. */
  const retire = async () => {
    const left = await app.evaluate(() => ({
      errors: window.__errors.slice(),
      status: document.querySelector('#status')?.textContent ?? '',
    }))
    if (left.errors.length) differences.push(`the page raised ${JSON.stringify(left.errors)}`)
    if (left.status) differences.push(`the page said ${JSON.stringify(left.status)}`)
  }
  const isTruncated = () =>
    app.evaluate(() => document.querySelector('#stage .terminal-host')?.dataset.truncated === 'true')
  const load = async (reload) => {
    mine.held = false
    mine.gone = false
    await host.call('navigate', '')
    mine.page = null
    if (reload) await app.reload()
    else await app.goto(`${ui.origin}/index.html`)
    await replayed()
    await settle()
  }
  const compare = async (label, past) => {
    results.compared++
    // The state of the page's xterm in its own page, and of the reference's in its.
    const readOf = (read) => {
      const emulator = window.__emu ?? P5.ref.emulator
      return { state: P5[read](emulator), sends: P5.sends(emulator) }
    }
    const read = past ? 'reliable' : 'state'
    const a = await refPage.evaluate(readOf, read)
    const b = await app.evaluate(readOf, read)
    const d = []
    for (const k of Object.keys(a.state)) {
      const x = JSON.stringify(a.state[k])
      const y = JSON.stringify(b.state[k])
      if (x !== y) d.push(k === 'text' ? 'text' : `${k}: ${x} | ${y}`)
    }
    if (a.sends !== b.sends)
      d.push(`paste: ${JSON.stringify(a.sends)} | ${JSON.stringify(b.sends)}`)
    return d
  }
  await load(false)
  for (const op of c.ops) {
    if (op.emit || op.repeat) {
      const bytes = op.emit ?? op.repeat
      const count = op.repeat ? op.count : 1
      const len = bytes.length * count
      const before = await R(() => P5.ref.answers.length)
      await R(async ([b, n]) => {
        const emulator = P5.ref.emulator
        let left = n
        const per = Math.max(1, Math.floor(60000 / b.length))
        // xterm that fails on a write never calls back: the stream is the reference's failure, not the page's.
        const failed = new Promise((_, no) => {
          P5.ref.fail = (e) => no(new Error('xterm failed: ' + e.message))
        })
        const listener = (e) => P5.ref.fail(e)
        window.addEventListener('error', listener)
        try {
          while (left > 0) {
            const k = Math.min(per, left)
            const chunk = new Uint8Array(k * b.length)
            for (let i = 0; i < k; i++) chunk.set(b, i * b.length)
            await Promise.race([emulator.write(chunk), failed])
            left -= k
          }
        } finally {
          window.removeEventListener('error', listener)
        }
      }, [bytes, count])
      const after = await R(() => P5.ref.answers.length)
      total += len
      if (op.repeat) host.send('repeat', win, { hex: hex(bytes), count })
      else host.send('emit', win, { hex: hex(bytes) })
      await host.call('sync', win, { n: total })
      await settle()
      // Handed to a page that left before it parsed them: main's limit 4.
      if (mine.held) for (let i = before; i < after; i++) lostAnswers.add(i)
    } else if (op.resize) {
      await R(([s]) => P5.ref.emulator.terminal.resize(s[0], s[1]), [op.resize])
      if (!mine.gone && !mine.held) {
        await app.evaluate(([cols, rows]) => window.__emu.terminal.resize(cols, rows), op.resize)
        await app.evaluate(() => window.__quiet())
      } else
        await host.call('resize', win, {
          page: null,
          id: win,
          generation: mine.generation,
          cols: op.resize[0],
          rows: op.resize[1],
          at: null,
        })
    } else if (op.gone) {
      await host.call('gone', win)
      mine.gone = true
    } else if (op.depart) {
      mine.held = true
    } else if (op.reload) {
      results.reloads++
      await retire()
      await load(true)
      const truncated = await isTruncated()
      const d = await compare('reload', c.past || truncated)
      if (truncated) results.truncated++
      if (d.length)
        differences.push(
          `after reload ${results.reloads}${truncated ? ' (past the cap)' : ''}: ${d.join(' ; ')}`,
        )
    }
  }
  // the end: the page's state against the continuous xterm's, and the answers once each
  await settle()
  const truncatedEnd = await isTruncated()
  const final = await compare('end', c.past || truncatedEnd)
  if (final.length)
    differences.push(`at the end${truncatedEnd ? ' (past the cap)' : ''}: ${final.join(' ; ')}`)
  await retire()
  const refAnswers = await R(() => P5.ref.answers.slice())
  const expected = refAnswers.filter((_, i) => !lostAnswers.has(i))
  if (JSON.stringify(expected) !== JSON.stringify(mine.answers))
    differences.push(
      `answers: expected ${JSON.stringify(expected)} got ${JSON.stringify(mine.answers)}`,
    )
  results.answers += mine.answers.length
  await app.goto('about:blank')
  await host.call('close', win)
  return differences
}

const all = []
for (const c of reviewCases(review51)) all.push(c)
for (const c of namedCases()) all.push(c)
const nFixed = all.length
for (let i = 0; i < Number(randomCount); i++) all.push(randomCase(Number(firstSeed) + i))
for (let i = 0; i < Number(pastCount); i++) all.push(pastCapCase(Number(firstSeed) + 100000 + i))
if (out) writeFileSync(out, '')
const started = Date.now()
/** The stream through one xterm alone, no host: where xterm itself fails on a stream, that is not the host's difference. */
async function refAlone(c) {
  try {
    return await R(async ([c]) => {
      const solo = P5.reference('solo', c.cols, c.rows)
      const errors = []
      const listener = (e) => errors.push(e.message)
      window.addEventListener('error', listener)
      try {
        for (const op of c.ops) {
          if (op.emit || op.repeat) {
            const b = op.emit ?? op.repeat
            const n = op.repeat ? op.count : 1
            const chunk = new Uint8Array(b.length * n)
            for (let i = 0; i < n; i++) chunk.set(b, i * b.length)
            await Promise.race([solo.emulator.write(chunk), new Promise((_, no) => { const t = setInterval(() => { if (errors.length) { clearInterval(t); no(new Error(errors[0])) } }, 5) })])
          } else if (op.resize) solo.emulator.terminal.resize(op.resize[0], op.resize[1])
        }
        return null
      } catch (error) { return error.message } finally { window.removeEventListener('error', listener); solo.emulator.dispose(); solo.host.remove() }
    }, [c])
  } catch (error) { return error.message }
}
for (const c of all) {
  results.cases++
  let differences
  try { differences = await runCase(c) } catch (error) {
    differences = [`ERROR ${error.message}`]
    await app.goto('about:blank')
    await host.call('close', 'w' + windowNo)
    const alone = await refAlone(c)
    if (alone) { results.xtermFailed = (results.xtermFailed ?? 0) + 1; differences = []; console.log(`XTERM FAILS ALONE ${c.name} seed ${c.seed}: ${alone.slice(0, 200)}`); if (out) appendFileSync(out + '.xterm', JSON.stringify({ name: c.name, seed: c.seed, alone }) + '\n') }
  }
  if (differences.length) {
    results.failed++
    const row = { browser: browserName, name: c.name, seed: c.seed, past: !!c.past, differences }
    failures.push(row)
    if (out) appendFileSync(out, JSON.stringify(row) + '\n')
    console.log(`FAIL ${c.name} seed ${c.seed}: ${differences.join(' | ').slice(0, 400)}`)
  }
  if (results.cases % 50 === 0) console.log(`... ${results.cases}/${all.length} cases, ${results.failed} failed, ${Math.round((Date.now() - started) / 1000)} s`)
}
console.log(JSON.stringify({ browser: browserName, fixed: nFixed, random: Number(randomCount), pastCap: Number(pastCount), firstSeed: Number(firstSeed), ...results }))
await host.close()
await browser.close()
await ui.close()
