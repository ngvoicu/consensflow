// T-2126's gate: a randomized comparison of a reloaded page with a continuous xterm, in one
// browser, through the real host API (gate_bridge.rs), with a page that does what P5-C will do:
// replay the history muted, then take everything after on the live path.
//   node gate.mjs <webkit|chromium> <random cases> <first seed> [past-cap cases] [out.jsonl]
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { writeFileSync, appendFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { open, tree } from './lib.mjs'
import { randomCase, pastCapCase, namedCases, reviewCases } from './gen.mjs'
import { cases as review51 } from './cases51.mjs'

const [browserName = 'chromium', randomCount = '50', firstSeed = '1', pastCount = '0', out = ''] = process.argv.slice(2)
const hex = (bytes) => Buffer.from(bytes).toString('hex')
const unhex = (s) => [...Buffer.from(s, 'hex')]

function bridge() {
  const child = spawn(resolve(tree, 'app/src-tauri/target/debug/examples/gate_bridge'), [], { stdio: ['pipe', 'pipe', 'inherit'] })
  const waiting = new Map()
  let next = 0
  let onMessage = () => {}
  createInterface({ input: child.stdout }).on('line', (line) => {
    const v = JSON.parse(line)
    if (v.msg) return onMessage(v.msg)
    waiting.get(v.re)?.(v)
    waiting.delete(v.re)
  })
  return {
    set onMessage(f) { onMessage = f },
    call(cmd, window, args = {}) {
      const re = ++next
      return new Promise((done) => {
        waiting.set(re, done)
        child.stdin.write(JSON.stringify({ cmd, window, re, ...args }) + '\n')
      })
    },
    send(cmd, window, args = {}) { child.stdin.write(JSON.stringify({ cmd, window, re: ++next, ...args }) + '\n') },
    async close() { await this.call('quit', ''); child.stdin.end() },
  }
}

const { browser, page, ui } = await open(browserName)
const host = bridge()
const E = (fn, ...a) => page.evaluate(fn, ...a)
let windowNo = 0
const results = { cases: 0, failed: 0, compared: 0, reloads: 0, past: 0, answers: 0, truncated: 0, leaks: 0 }
const failures = []
// What the page asks of the app, answered by the real host (the bridge) as the app's commands do.
let appWindow = ''
await page.exposeFunction('hostCall', async (command, args) => {
  if (command === 'pane_history') return host.call('history', appWindow, { from: args.from })
  if (command === 'pane_ack') host.send('ack', appWindow, { seq: args.seq })
  else if (command === 'pane_resize')
    host.send('resize', appWindow, { rows: args.rows, cols: args.cols, at: args.at ?? null })
  return { ok: true }
})
const pageInvoke = () => async (command, args) => {
  const answer = await window.hostCall(command, args)
  if (command !== 'pane_history') return answer
  const fromHex = (hex) => Array.from({ length: hex.length >> 1 }, (_, i) => parseInt(hex.substr(i * 2, 2), 16))
  return {
    ok: true,
    reset: answer.reset,
    truncated: answer.truncated,
    from: answer.from,
    size: { rows: answer.size[0], cols: answer.size[1] },
    seed: fromHex(answer.seed),
    resizes: answer.resizes.map(([at, rows, cols]) => ({ at, rows, cols })),
    bytes: fromHex(answer.hex),
    boundary: answer.boundary,
    done: answer.done,
  }
}

async function runCase(c) {
  const win = 'w' + ++windowNo
  appWindow = win
  const fail = (what) => { throw Object.assign(new Error(what), { gate: true }) }
  await E(([c]) => { for (const n of Object.keys(P5.windows)) { try { P5.windows[n].dispose() } catch {} delete P5.windows[n] } P5.ref?.emulator.dispose(); P5.ref?.host.remove(); P5.ref = P5.reference('ref', c.cols, c.rows) }, [c])
  await host.call('open', win, { rows: c.rows, cols: c.cols })
  let total = 0 // the bytes the program has printed
  // the page: one window at a time, made when it subscribes, with the app's invoke
  let cur = null
  const pages = new Map()
  const early = new Map() // messages the host printed while it answered the subscription, before its reply named the page
  let chain = Promise.resolve()
  const deliver = (p, m) => {
    chain = chain.then(async () => {
      await p.created
      if (p.gone || p.held) return
      await E(([name, m]) => {
        const win = P5.windows[name]
        const fromHex = (hex) => Uint8Array.from({ length: hex.length >> 1 }, (_, i) => parseInt(hex.substr(i * 2, 2), 16))
        win.link.output({ id: win.pane.id, generation: win.pane.generation, seq: m.seq, offset: m.offset, bytes: fromHex(m.hex) }, () => win.emulator)
      }, [p.name, m])
    })
  }
  host.onMessage = (m) => {
    const p = pages.get(m.page)
    if (!p) { if (!early.has(m.page)) early.set(m.page, []); early.get(m.page).push(m); return }
    deliver(p, m)
  }
  const subscribe = async () => {
    const r = await host.call('subscribe', win)
    const p = { id: r.page, name: 'p' + r.page, boundary: r.boundary, gone: false, held: false }
    pages.set(r.page, p)
    // The page's window, as the app makes one, and its first act: ask for its history.
    p.created = E(([name]) => {
      const w = P5.window(name, { cols: 80, rows: 24, invoke: (command, args) => window.__pageInvoke(command, args) })
      w.start()
    }, [p.name])
    for (const m of early.get(r.page) ?? []) deliver(p, m)
    early.delete(r.page)
    return p
  }
  await E(`window.__pageInvoke = ${pageInvoke().toString()}`)
  cur = await subscribe()
  await cur.created
  await E(([name]) => P5.windows[name].replayed, [cur.name])
  const lostAnswers = new Set()
  const compare = async (label, p, past) => {
    results.compared++
    const [a, b, d] = await E(([name, past]) => {
      const w = P5.windows[name]
      const read = past ? P5.reliable : P5.state
      const a = read(P5.ref.emulator), b = read(w.emulator)
      const d = P5.diff(a, b)
      const sa = P5.sends(P5.ref.emulator), sb = P5.sends(w.emulator)
      if (sa !== sb) d.push(`paste: ${JSON.stringify(sa)} | ${JSON.stringify(sb)}`)
      return [a, b, d]
    }, [p.name, past])
    void a; void b
    return d
  }
  const differences = []
  const pageAnswers = []
  const settle = async () => {
    await chain
    if (cur && !cur.gone && !cur.held) await E(([name]) => P5.windows[name].quiet(), [cur.name])
  }
  const retire = async (p) => {
    const taken = await E(([name]) => {
      const w = P5.windows[name]
      const out = { answers: w.answers.slice(), leaks: w.leaks, reports: w.reports.slice() }
      w.dispose()
      delete P5.windows[name]
      return out
    }, [p.name])
    pageAnswers.push(...taken.answers)
    if (taken.leaks) differences.push(`${c.name}: ${taken.leaks} answers made while muted`)
    if (taken.reports.length) differences.push(`${c.name}: the page reported ${JSON.stringify(taken.reports)}`)
    results.leaks += taken.leaks
  }
  for (const op of c.ops) {
    if (op.emit || op.repeat) {
      const bytes = op.emit ?? op.repeat
      const count = op.repeat ? op.count : 1
      const len = bytes.length * count
      const before = await E(() => P5.ref.answers.length)
      await E(async ([b, n]) => {
        const emulator = P5.ref.emulator
        let left = n
        const per = Math.max(1, Math.floor(60000 / b.length))
        // xterm that fails on a write never calls back: the stream is the reference's failure, not the page's.
        const failed = new Promise((_, no) => { P5.ref.fail = (e) => no(new Error('xterm failed: ' + e.message)); })
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
        } finally { window.removeEventListener('error', listener) }
      }, [bytes, count])
      const after = await E(() => P5.ref.answers.length)
      total += len
      if (op.repeat) host.send('repeat', win, { hex: hex(bytes), count })
      else host.send('emit', win, { hex: hex(bytes) })
      await host.call('sync', win, { n: total })
      await settle()
      if (cur.held) for (let i = before; i < after; i++) lostAnswers.add(i) // handed to a page that left before it parsed them: main's limit 4
    } else if (op.resize) {
      await E(([s]) => P5.ref.emulator.terminal.resize(s[0], s[1]), [op.resize])
      if (cur && !cur.gone) {
        await E(([n, s]) => P5.windows[n].resize(s[0], s[1]), [cur.name, op.resize])
      } else host.send('resize', win, { rows: op.resize[1], cols: op.resize[0], at: null })
    } else if (op.gone) {
      await host.call('gone', win)
      cur.gone = true
    } else if (op.depart) {
      cur.held = true
    } else if (op.reload) {
      results.reloads++
      const old = cur
      if (old) await retire(old)
      cur = await subscribe()
      await cur.created
      await E(([name]) => P5.windows[name].replayed, [cur.name])
      await settle()
      const truncated = await E(([name]) => P5.windows[name].truncated, [cur.name])
      const d = await compare('reload', cur, c.past || truncated)
      if (truncated) results.truncated++
      if (d.length) differences.push(`after reload ${results.reloads}${truncated ? ' (past the cap)' : ''}: ${d.join(' ; ')}`)
    }
  }
  // the end: the page's state against the continuous xterm's, and the answers once each
  await settle()
  const truncatedEnd = await E(([name]) => P5.windows[name].truncated, [cur.name])
  const final = await compare('end', cur, c.past || truncatedEnd)
  if (final.length) differences.push(`at the end${truncatedEnd ? ' (past the cap)' : ''}: ${final.join(' ; ')}`)
  await retire(cur)
  const refAnswers = await E(() => P5.ref.answers.slice())
  const expected = refAnswers.filter((_, i) => !lostAnswers.has(i))
  if (JSON.stringify(expected) !== JSON.stringify(pageAnswers))
    differences.push(`answers: expected ${JSON.stringify(expected)} got ${JSON.stringify(pageAnswers)}`)
  results.answers += pageAnswers.length
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
    return await E(async ([c]) => {
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
    await host.call('close', 'w' + windowNo).catch?.(() => {})
    await E(() => { for (const n of Object.keys(P5.windows)) { try { P5.windows[n].dispose() } catch {} delete P5.windows[n] } })
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
