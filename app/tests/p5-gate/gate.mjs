// P5's gate: a randomized comparison of a reloaded page with a continuous xterm, in one browser.
// The page under test is the shipped app page (app/ui: its own start, board view, terminals and
// link), loaded and reloaded as the app does it; only the app's bridge to it is stood in
// (`window.__TAURI__`): the daemon's board is one project with one chief's window, and the pane
// host's commands go to gate_bridge.rs, which answers them with the functions the app's own
// commands call (`cf_panes::page`) over a real pty, with the daemon's room. The continuous xterm
// is in a page of its own.
//
// What a case expects to be cut is worked out here from what it printed and sized, and the host's
// own frame says what was cut: never the page's notice, which must agree. Every piece the host
// gives holds exactly the printed bytes from its offset; and where a history is cut, the page's
// xterm is compared with one built here from the host's frame and the printed bytes. A size can be
// held on its way (a `hold` op) and let go at one of five points around a reload, and a second lane
// leaves while its program runs. The gate exits with a failure where any stream differed.
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
// The gate's own controls (selftest.mjs): a fault put into what the host or the page hands over,
// so that the gate can be shown to fail. `only` runs the cases whose names match.
const fault = process.env.CF_GATE_FAULT ?? ''
/** The longest any one wait of the gate lasts, in seconds; the host's own bound is the same. */
const WAIT = Number(process.env.CF_GATE_WAIT_S ?? 180)
/**
 * The gate ends with a failure and its cause where a wait of its own passes its bound, or the host
 * says its program stopped: it never waits on for someone outside to put it right.
 */
let stopped = false
function stop(cause) {
  if (stopped) return
  stopped = true
  console.log(`GATE STOPPED: ${cause}`)
  if (out) appendFileSync(out, JSON.stringify({ browser: browserName, name: 'the gate', differences: [cause] }) + '\n')
  try { hostChild?.kill('SIGKILL') } catch {}
  process.exit(1)
}
/** `promise`, or the gate's failure if it has not settled within the bound. */
function within(promise, what, seconds = WAIT) {
  let timer
  const late = new Promise((_, no) => {
    timer = setTimeout(() => stop(`${what} did not finish within ${seconds} s`), seconds * 1000)
  })
  return Promise.race([promise, late]).finally(() => clearTimeout(timer))
}
let hostChild = null
const only = process.env.CF_GATE_ONLY ? new RegExp(process.env.CF_GATE_ONLY) : null
const ROLES = JSON.parse(readFileSync(resolve(tree, 'app/tests/fixtures/roles.json'), 'utf8'))

function bridge() {
  const child = spawn(resolve(tree, 'app/src-tauri/target/debug/examples/gate_bridge'), [], {
    stdio: ['pipe', 'pipe', 'inherit'],
  })
  hostChild = child
  const waiting = new Map()
  let next = 0
  let onMessage = () => {}
  createInterface({ input: child.stdout }).on('line', (line) => {
    const v = JSON.parse(line)
    if (v.msg) return onMessage(v.msg)
    if (v.error) stop(`the host: ${v.error}`)
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
      return within(
        new Promise((done) => {
          waiting.set(re, done)
          child.stdin.write(JSON.stringify({ cmd, window, re, ...args }) + '\n')
        }),
        `the host's answer to ${cmd}`,
        WAIT + 10,
      )
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
/** A page's calls into the browser never wait without a bound. */
function bounded(page, name) {
  for (const method of ['evaluate', 'waitForFunction']) {
    const original = page[method].bind(page)
    page[method] = (...args) => within(original(...args), `${name}.${method}`)
  }
  return page
}
bounded(refPage, 'the reference page')
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
const MIB = 1 << 20

// ---- the app page -------------------------------------------------------------------------

/** What the app's `window.__TAURI__` is, in the page: the board stood in, the rest asked of node. */
function standIn(fault) {
  // The blank page between two cases is none of the app's.
  if (!location.protocol.startsWith('http')) return
  window.__pending = 0
  window.__pulled = 0
  window.__errors = []
  window.__listeners = {}
  window.__emus = []
  /** The chief's window's emulator: the one whose card says so. */
  Object.defineProperty(window, '__emu', {
    get: () =>
      window.__emus.find((e) => e.host.closest('.terminal-card')?.dataset.handle === 'chief')
        ?.emulator,
  })
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
      let text = new TextDecoder().decode(decode(args.bytes))
      // The controls: what the page sends is changed on its way out.
      if (fault === 'wrong-cpr-place')
        text = text.replace(/^\x1b\[(\??)(\d+);(\d+)R$/, (_, q, row, col) => `\x1b[${q}${Number(row) + 1};${col}R`)
      if (fault === 'private-cpr') text = text.replace(/^\x1b\[(\d+;\d+R)$/, '\x1b[?$1')
      const generation = args.generation + (fault === 'wrong-reply-generation' ? 1 : 0)
      return ask(command, {
        pageId: args.pageId,
        id: args.id,
        generation,
        sequence: args.sequence,
        text,
      })
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
    event: {
      listen: async (name, handler) => {
        window.__listeners[name] = handler
        return () => {}
      },
    },
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
        window.__emus.push({ host: hostElement, emulator })
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

const app = bounded(await browser.newPage(), 'the app page')
app.on('pageerror', (e) => console.error('PAGE ERROR', e.message))
await app.addInitScript({ path: pageScript })
await app.addInitScript(standIn, fault)

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
  const second = current.second
    ? [lane(3, 'zeus', 'worker', { id: current.second.win, generation: current.second.generation })]
    : []
  if (operation === 'projects.list') return { ok: true, projects: [project] }
  if (operation === 'board.get')
    return {
      ok: true,
      roles: ROLES,
      board: {
        project,
        open: [],
        lanes: [lane(1, 'human', 'human', null), lane(2, 'chief', 'chief', pane), ...second],
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
    // A size held for this point goes before this pull is answered.
    if (args.id === here.win && args.pageId === here.page) await here.atPull?.(args.from)
    let { hex: frame } = await host.call('history', here.win, {
      page: args.pageId,
      id: args.id,
      generation: args.generation,
      from: args.from,
    })
    if (fault === 'empty-frame') frame = emptied(frame)
    if (fault === 'short-boundary') frame = shortened(frame)
    if (args.id === here.win) here.piece(frame)
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
  if (command === 'pane_ack') {
    // The controls: a lane that has left has its acknowledgements lost, or sent for another generation.
    if (fault === 'no-retired-ack' && args.id === here.retired) return { ok: true }
    const { answer } = await host.call('ack', here.win, {
      id: args.id,
      generation: args.generation + (fault === 'wrong-last-ack' && args.id === here.retired && here.last ? 1 : 0),
      seq: args.seq,
    })
    // What the host took is what was acknowledged; one it refused is a failure of the page's or the gate's.
    if (answer.ok) here.acked.set(args.id, args.seq)
    else
      here.refused.push(
        `the acknowledgement of ${args.id} (generation ${args.generation}) up to ${args.seq} was refused: ${answer.error}`,
      )
    return answer
  }
  if (command === 'pane_resize') {
    const forward = async () => {
      const { answer } = await host.call('resize', here.win, {
        page: args.pageId,
        id: args.id,
        generation: args.generation,
        cols: args.cols,
        rows: args.rows,
        at: args.at ?? null,
      })
      if (answer.ok && args.id === here.win) here.sized(args.at, args.cols, args.rows)
      return answer
    }
    if (here.arm !== null && args.id === here.win) {
      const arm = here.arm
      here.arm = null
      return new Promise((answered) => {
        arm.captured(async () => {
          const answer = await forward()
          answered(answer)
          return answer
        })
      })
    }
    return forward()
  }
  if (command === 'pane_reply_enqueue') {
    // Admitted as the app's command admits it: a reply the host refuses is refused here.
    const { answer } = await host.call('reply', args.id, {
      page: args.pageId,
      id: args.id,
      generation: args.generation,
      sequence: args.sequence,
      hex: Buffer.from(args.text).toString('hex'),
    })
    if (answer.ok) (args.id === here.win ? here.answers : here.otherAnswers).push(args.text)
    else
      here.refused.push(
        `the reply ${JSON.stringify(args.text)} for ${args.id} (generation ${args.generation}, call ${args.sequence}) was refused: ${answer.error}`,
      )
    return answer
  }
  return { ok: true }
})

/** The control `empty-frame`: the host gives a history that starts at its boundary, whatever it holds. */
function emptied(frameHex) {
  const frame = Buffer.from(frameHex, 'hex')
  const length = frame.readUInt32LE(0)
  const header = JSON.parse(frame.subarray(4, 4 + length))
  if (!header.ok || !header.reset || header.boundary === 0) return frameHex
  const rewritten = Buffer.from(
    JSON.stringify({ ...header, from: header.boundary, seedLength: 0, resizes: [], done: true }),
  )
  const size = Buffer.alloc(4)
  size.writeUInt32LE(rewritten.length)
  return Buffer.concat([size, rewritten]).toString('hex')
}

/** The control `short-boundary`: the host ends its history, bytes and boundary, one byte before the bytes it handed to the page before. */
function shortened(frameHex) {
  const frame = Buffer.from(frameHex, 'hex')
  const length = frame.readUInt32LE(0)
  const header = JSON.parse(frame.subarray(4, 4 + length))
  if (!header.ok || !header.reset || header.boundary === 0) return frameHex
  const rewritten = Buffer.from(JSON.stringify({ ...header, boundary: header.boundary - 1 }))
  const size = Buffer.alloc(4)
  size.writeUInt32LE(rewritten.length)
  return Buffer.concat([size, rewritten, frame.subarray(4 + length, frame.length - 1)]).toString('hex')
}

/** A window of the case, as the page under test and the host see it. */
function caseState(win, generation, cols, rows) {
  const mine = {
    win,
    generation,
    page: null,
    second: null,
    retired: null,
    last: false,
    /** What the host refused of the page's calls (a reply, an acknowledgement). */
    refused: [],
    answers: [],
    otherAnswers: [],
    acked: new Map(),
    held: false,
    gone: false,
    arm: null,
    atPull: null,
    /** The first answer of a page whose history was cut: from it on a cursor's place is the
     * terminal's own (a cut history keeps no cursor, limit 4). */
    cutFrom: Infinity,
    /** How many answers the page had made when this load began. */
    loadStart: 0,
    /** What the program printed, in order, and how much. */
    chunks: [],
    total: 0,
    /** The sizes the host took, each where it was journaled, and how many differ from the one before. */
    sizes: [],
    size: [cols, rows],
    changes: 0,
    /** This load's pieces, as the host gave them. */
    pieces: [],
    problems: [],
    expectCut: () => {
      if (mine.total > 8.5 * MIB) return true
      return mine.total <= 8 * MIB && mine.changes <= 256 ? false : null
    },
    /** The journal's end when the last size that could start it again was taken (past 256 sizes, a size tries to). */
    startLimit: 0,
    sized: (at, cols, rows) => {
      if (mine.size[0] === cols && mine.size[1] === rows) return
      mine.size = [cols, rows]
      mine.changes++
      if (mine.changes > 256) mine.startLimit = mine.total
      mine.sizes.push({ at: at ?? mine.total, cols, rows })
    },
    printed: () => (mine.chunks.length > 1 ? (mine.chunks = [Buffer.concat(mine.chunks)]) : mine.chunks)[0] ?? Buffer.alloc(0),
    piece: (frameHex) => {
      const frame = Buffer.from(frameHex, 'hex')
      const length = frame.readUInt32LE(0)
      const header = JSON.parse(frame.subarray(4, 4 + length))
      if (!header.ok) return
      const seed = frame.subarray(4 + length, 4 + length + header.seedLength)
      const bytes = frame.subarray(4 + length + header.seedLength)
      const before = mine.pieces.at(-1)
      mine.pieces.push({ ...header, seed: [...seed], length: bytes.length })
      // One piece follows the last without a gap, and what the host calls the end of the history is its boundary.
      if (before && header.from !== before.from + before.length)
        mine.problems.push(`the piece from ${header.from} does not follow the one before it, which ends at ${before.from + before.length}`)
      if (header.done && header.from + bytes.length !== header.boundary)
        mine.problems.push(`the history ends at ${header.from + bytes.length}, not at its boundary ${header.boundary}`)
      // The retained bytes are the printed ones, whatever the page does with them.
      if (!mine.printed().subarray(header.from, header.from + bytes.length).equals(bytes))
        mine.problems.push(`the piece from ${header.from} is not what was printed there`)
      if (header.from > header.boundary)
        mine.problems.push(`a piece from ${header.from} past its boundary ${header.boundary}`)
      if (header.reset) {
        const cut = mine.expectCut()
        if (cut === false && (header.truncated || header.from !== 0))
          mine.problems.push(`the history is cut at ${header.from}, though all of it is promised`)
        if (cut === true && !header.truncated)
          mine.problems.push('the history is not cut, though more was printed than it holds')
        // Past 256 sizes the journal may start again, but never later than the end it had when the last size
        // that could start it was taken, and never at all before 8 MiB are printed otherwise.
        if (cut === null && mine.total <= 8 * MIB && header.from > mine.startLimit)
          mine.problems.push(`the history starts at ${header.from}, past ${mine.startLimit}: printed bytes the journal still holds are left out`)
        if (header.truncated && mine.changes <= 256 && header.boundary - header.from < Math.min(header.boundary, 4 * MIB))
          mine.problems.push(`only ${header.boundary - header.from} bytes are kept before ${header.boundary}`)
      }
    },
  }
  return mine
}

/** What reached a window's program's input, once it has as much as `length` bytes (or its bound has passed). */
async function programReceived(win, length) {
  let text = ''
  for (let i = 0; i < 1000; i++) {
    text = Buffer.from((await host.call('received', win)).hex, 'hex').toString()
    if (Buffer.byteLength(text) >= length) break
    await new Promise((done) => setTimeout(done, 5))
  }
  return text
}

/** The app page's replay of the window is over: it has pulled, and the emulator takes typing. */
const replayed = () =>
  app.waitForFunction(
    () => window.__pulled > 0 && window.__emu && !window.__emu.replaying,
    null,
    { timeout: WAIT * 1000 },
  )

async function runCase(c) {
  const win = 'w' + ++windowNo
  await R(([c]) => {
    P5.ref?.emulator.dispose()
    P5.ref?.host.remove()
    P5.ref = P5.reference('ref', c.cols, c.rows)
    P5.stream = []
  }, [c])
  const { generation } = await host.call('open', win, { rows: c.rows, cols: c.cols })
  const mine = caseState(win, generation, c.cols, c.rows)
  current = mine
  /** A size held on its way, and the point at which it is let go. */
  let holding = null
  const letGo = async () => {
    const { release, point } = holding
    holding = null
    const answer = await release()
    const taken = point === 'p1' || point === 'p2'
    if (taken ? !answer.ok : answer.error !== 'pane-input-stale-page')
      differences.push(`a size let go at ${point} was answered ${JSON.stringify(answer)}`)
  }
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
  const noticeShown = () =>
    app.evaluate(() => document.querySelector('#stage .terminal-host')?.dataset.truncated === 'true')
  /** Whether the host cut this load's history. */
  const hostCut = () => mine.pieces.some((piece) => piece.truncated)
  const load = async (reload, after) => {
    mine.pieces = []
    mine.loadStart = mine.answers.length
    // A size held for a point in the replay goes before a pull is answered: the first (p3), after
    // what the program printed for the new page, or the second (p4).
    mine.atPull = async (from) => {
      if (holding?.point === 'p3' && from === null) {
        mine.atPull = null
        if (after) await print({ emit: after }, false)
        await letGo()
      } else if (holding?.point === 'p4' && from !== null) {
        mine.atPull = null
        await letGo()
      }
    }
    mine.held = false
    mine.gone = false
    await host.call('navigate', '')
    mine.page = null
    if (reload) await app.reload()
    else await app.goto(`${ui.origin}/index.html`)
    await replayed()
    await settle()
    if (holding?.point === 'p5') {
      await letGo()
      await settle()
    }
  }
  /** The page's xterm against the continuous one, or (cut) against one built from the host's frame. */
  const compare = async (past) => {
    results.compared++
    const cut = hostCut()
    // The state of the page's xterm in its own page, and of the reference's in its.
    const readOf = (read) => {
      const emulator = window.__emu ?? P5.ref.emulator
      return { state: P5[read](emulator), sends: P5.sends(emulator) }
    }
    const read = past || cut ? 'reliable' : 'state'
    let a = await refPage.evaluate(readOf, read)
    let b = await app.evaluate(readOf, read)
    // The shipped xterm takes synchronized output (mode 2026) off by itself, a second after it
    // starts to hold a drawing back: the long-lived one may have done so and the replayed one not
    // yet. Where that mode differs, both are read again once the second is over.
    const synchronized = (side) => /synchronizedOutput\w*\\?":true/.test(JSON.stringify(side.state))
    if (synchronized(a) !== synchronized(b)) {
      await new Promise((done) => setTimeout(done, 1500))
      a = await refPage.evaluate(readOf, read)
      b = await app.evaluate(readOf, read)
    }
    const d = []
    // A cut history starts with the terminal's own reverse wraparound (mode 45), which no mark
    // keeps (limit 4): against the continuous xterm it is not compared.
    if (past || cut) {
      for (const side of [a, b]) {
        for (const key of ['modes', 'dec']) {
          side.state[key] = side.state[key].replace(/"reverseWraparound(Mode)?":(true|false),?/, '')
        }
      }
    }
    const diff = (one, other, words) => {
      for (const k of Object.keys(one)) {
        const x = JSON.stringify(one[k])
        const y = JSON.stringify(other[k])
        if (x !== y) d.push(words + (k === 'text' ? 'text' : `${k}: ${x} | ${y}`))
      }
    }
    diff(a.state, b.state, '')
    if (a.sends !== b.sends)
      d.push(`paste: ${JSON.stringify(a.sends)} | ${JSON.stringify(b.sends)}`)
    if (cut) {
      const built = await builtFromFrame()
      const full = built.state
      const page = await app.evaluate(() => P5.state(window.__emu))
      diff(full, page, 'against the retained bytes, ')
      // What the page answered since this load is what an xterm built as it was asks of the same
      // questions, cursor reports with their places.
      const answered = mine.answers.slice(mine.loadStart)
      if (JSON.stringify(built.owed) !== JSON.stringify(answered))
        d.push(`answers since this load: expected ${JSON.stringify(built.owed)} got ${JSON.stringify(answered)}`)
    }
    if ((await noticeShown()) !== cut)
      d.push(`the notice ${cut ? 'is missing from' : 'shows over'} a history the host ${cut ? 'cut' : 'did not cut'}`)
    const sizes = await host.call('size', win)
    const emulator = await app.evaluate(() => window.__emu.size())
    if (sizes.cols !== emulator.cols || sizes.rows !== emulator.rows)
      d.push(`the terminal is ${sizes.cols}x${sizes.rows}, the emulator ${emulator.cols}x${emulator.rows}`)
    return d
  }
  /**
   * An xterm built here as the page's must be where its history was cut: the host's frame (its
   * size, seed and the sizes it gave), then what the program printed from the piece's offset, with
   * the sizes the host has taken since at their offsets.
   */
  const builtFromFrame = async () => {
    const first = mine.pieces.find((piece) => piece.reset)
    const given = mine.pieces.flatMap((piece) => piece.resizes)
    const sizes = [...given.map((s) => ({ at: s.at, cols: s.cols, rows: s.rows }))]
    const boundary = mine.pieces.at(-1).boundary
    sizes.push(...mine.sizes.filter((s) => s.at > boundary))
    sizes.sort((x, y) => x.at - y.at)
    // The boundary is where the live part begins: what the xterm answers from there on is what the page owes.
    const live = sizes.findIndex((s) => s.at > boundary)
    sizes.splice(live < 0 ? sizes.length : live, 0, { at: boundary, mark: true })
    return refPage.evaluate(async ([from, size, seed, sizes]) => {
      P5.exp?.emulator.dispose()
      P5.exp?.host.remove()
      P5.exp = P5.reference('exp', size.cols, size.rows)
      let length = 0
      for (const chunk of P5.stream) length += chunk.length
      const whole = new Uint8Array(length)
      let offset = 0
      for (const chunk of P5.stream) {
        whole.set(chunk, offset)
        offset += chunk.length
      }
      P5.stream = [whole]
      const emulator = P5.exp.emulator
      if (seed.length) await emulator.write(Uint8Array.from(seed))
      let at = from
      let owed = 0
      for (const s of sizes) {
        const upto = Math.min(Math.max(s.at, at), length)
        if (upto > at) await emulator.write(whole.subarray(at, upto))
        at = upto
        if (s.mark) owed = P5.exp.answers.length
        else emulator.resizeTo(s.cols, s.rows)
      }
      if (at < length) await emulator.write(whole.subarray(at))
      return { state: P5.state(emulator), owed: P5.exp.answers.slice(owed) }
    }, [first.from, first.size, first.seed, sizes])
  }
  /** What the program prints, to the reference first; `settle`: waits for the page to draw it. */
  async function print(op, settled = true) {
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
          P5.stream.push(chunk)
          await Promise.race([emulator.write(chunk), failed])
          left -= k
        }
      } finally {
        window.removeEventListener('error', listener)
      }
    }, [bytes, count])
    const after = await R(() => P5.ref.answers.length)
    mine.chunks.push(Buffer.alloc(len, Buffer.from(bytes)))
    mine.total += len
    if (op.repeat) host.send('repeat', win, { hex: hex(bytes), count })
    else host.send('emit', win, { hex: hex(bytes) })
    await host.call('sync', win, { n: mine.total })
    if (settled) await settle()
    // Handed to a page that left before it parsed them: main's limit 4.
    if (mine.held) for (let i = before; i < after; i++) lostAnswers.add(i)
  }
  await load(false)
  for (const op of c.ops) {
    if (op.emit || op.repeat) {
      await print(op)
    } else if (op.resize) {
      await R(([s]) => P5.ref.emulator.terminal.resize(s[0], s[1]), [op.resize])
      if (!mine.gone && !mine.held) {
        await app.evaluate(([cols, rows]) => window.__emu.terminal.resize(cols, rows), op.resize)
        await app.evaluate(() => window.__quiet())
      } else {
        // The page is still the current one, with no sink: its size is taken at the journal's end.
        const { answer } = await host.call('resize', win, {
          page: mine.page,
          id: win,
          generation: mine.generation,
          cols: op.resize[0],
          rows: op.resize[1],
          at: null,
        })
        if (answer.ok) mine.sized(null, ...op.resize)
        else differences.push(`a size of the current page was refused: ${JSON.stringify(answer)}`)
      }
    } else if (op.hold) {
      const captured = new Promise((got) => {
        mine.arm = { captured: got }
      })
      await app.evaluate(([cols, rows]) => window.__emu.terminal.resize(cols, rows), op.hold)
      holding = { release: await captured, point: op.point }
      // Taken (let go before the page goes): the continuous xterm takes it here too.
      if (op.point === 'p1' || op.point === 'p2')
        await R(([s]) => P5.ref.emulator.terminal.resize(s[0], s[1]), [op.hold])
      if (op.point === 'p1') await letGo()
    } else if (op.gone) {
      await host.call('gone', win)
      mine.gone = true
    } else if (op.depart) {
      mine.held = true
    } else if (op.reload) {
      results.reloads++
      if (holding?.point === 'p2') await letGo()
      await retire()
      const before = mine.answers.length
      await load(true, op.after)
      if (hostCut()) {
        results.truncated++
        mine.cutFrom = Math.min(mine.cutFrom, before)
      }
      const d = await compare(c.past)
      if (d.length)
        differences.push(
          `after reload ${results.reloads}${hostCut() ? ' (past the cap)' : ''}: ${d.join(' ; ')}`,
        )
    }
  }
  // the end: the page's state against the continuous xterm's, and the answers once each
  await settle()
  const final = await compare(c.past)
  if (final.length)
    differences.push(`at the end${hostCut() ? ' (past the cap)' : ''}: ${final.join(' ; ')}`)
  if (holding !== null) differences.push(`a size held for ${holding.point} was never let go`)
  differences.push(...mine.problems, ...mine.refused)
  await retire()
  // What the program was given is what the host admitted for it, in order: nothing was admitted
  // that the program did not read, and nothing the program read was refused.
  const wanted = mine.answers.join('')
  const given = await programReceived(win, wanted.length)
  if (given !== wanted)
    differences.push(`the program received ${JSON.stringify(given)}, the host admitted ${JSON.stringify(wanted)}`)
  const refAnswers = await R(() => P5.ref.answers.slice())
  const placed = (answers) =>
    answers.map((answer, i) =>
      i >= mine.cutFrom ? answer.replace(/^\x1b\[(\??)[\d;]+R$/, '\x1b[$1a cursor report R') : answer,
    )
  const expected = refAnswers.filter((_, i) => !lostAnswers.has(i))
  if (JSON.stringify(placed(expected)) !== JSON.stringify(placed(mine.answers)))
    differences.push(
      `answers: expected ${JSON.stringify(expected)} got ${JSON.stringify(mine.answers)}`,
    )
  results.answers += mine.answers.length
  await app.goto('about:blank')
  await host.call('close', win)
  return differences
}

/**
 * A lane leaves while its program runs (the board stand-in takes it away): its status question gets
 * one reply while it is there and none once it has gone, what it prints is read (2 MiB, past the
 * room: the program is never held) and acknowledged, and the chief's window is untouched.
 */
async function laneCase() {
  const win = 'w' + ++windowNo
  const other = 'w' + ++windowNo
  const differences = []
  await R(() => {
    P5.ref?.emulator.dispose()
    P5.ref?.host.remove()
    P5.ref = P5.reference('ref', 80, 24)
    P5.stream = []
  })
  const mine = caseState(
    win,
    (await host.call('open', win, { rows: 24, cols: 80 })).generation,
    80,
    24,
  )
  current = mine
  const second = { win: other, generation: (await host.call('open', other, { rows: 24, cols: 80 })).generation }
  mine.second = second
  const lastSeq = new Map()
  host.onMessage = (m) => {
    if (m.page !== mine.page) return
    lastSeq.set(m.id, m.seq)
    app
      .evaluate((m) => {
        const bytes = Uint8Array.from({ length: m.hex.length >> 1 }, (_, i) => parseInt(m.hex.substr(i * 2, 2), 16))
        window.__output.onmessage({ id: m.id, generation: m.generation, seq: m.seq, offset: m.offset, bytes })
      }, m)
      .catch(() => {})
  }
  await host.call('navigate', '')
  await app.goto(`${ui.origin}/index.html`)
  await replayed()
  await app.waitForFunction(() => window.__emus.length === 2)
  const until = async (what, check) => {
    for (let i = 0; i < 2000; i++) {
      if (check()) return
      await new Promise((done) => setTimeout(done, 5))
    }
    differences.push(`waited for ${what}`)
  }
  let printed = 0
  const say = async (bytes, count = 1) => {
    printed += bytes.length * count
    host.send(count > 1 ? 'repeat' : 'emit', other, { hex: hex(bytes), count })
    await host.call('sync', other, { n: printed })
  }
  const ask = [...Buffer.from('\x1b[5n')]
  await say(ask)
  await until('the reply of the lane on the page', () => mine.otherAnswers.length === 1)
  // The lane leaves.
  mine.second = null
  mine.retired = other
  await app.evaluate(() => window.__listeners['state-changed']({ payload: { reason: 'task' } }))
  await app.waitForFunction(() => document.querySelector('#stage [data-handle="zeus"]') === null)
  await say([0x78], 2 * MIB)
  // The control `wrong-last-ack` acts on what the page acknowledges of this last question.
  mine.last = true
  await say(ask)
  await until('its output acknowledged', () => mine.acked.get(other) === lastSeq.get(other))
  await new Promise((done) => setTimeout(done, 2000))
  if (JSON.stringify(mine.otherAnswers) !== JSON.stringify(['\x1b[0n']))
    differences.push(`the lane that left answered: ${JSON.stringify(mine.otherAnswers)}`)
  // Its program read the one reply it was given while docked, and nothing after.
  const read = await programReceived(other, 4)
  if (read !== '\x1b[0n') differences.push(`the program of the lane that left received ${JSON.stringify(read)}`)
  differences.push(...mine.refused)
  await app.goto('about:blank')
  await host.call('close', other)
  await host.call('close', win)
  return differences
}

const all = []
for (const c of reviewCases(review51)) all.push(c)
for (const c of namedCases()) all.push(c)
const nFixed = all.length
if (only) all.splice(0, all.length, ...all.filter((c) => only.test(c.name)))
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
if (!only || only.test('a lane that leaves')) {
  results.cases++
  let differences
  try {
    differences = await laneCase()
  } catch (error) {
    differences = [`ERROR ${error.message}`]
  }
  if (differences.length) {
    results.failed++
    const row = { browser: browserName, name: 'a lane that leaves', differences }
    if (out) appendFileSync(out, JSON.stringify(row) + '\n')
    console.log(`FAIL a lane that leaves: ${differences.join(' | ').slice(0, 400)}`)
  }
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
process.exitCode = results.failed > 0 ? 1 : 0
