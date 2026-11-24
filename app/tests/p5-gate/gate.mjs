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

const { browser, page } = await open(browserName)
const host = bridge()
const E = (fn, ...a) => page.evaluate(fn, ...a)
let windowNo = 0
const results = { cases: 0, failed: 0, compared: 0, reloads: 0, past: 0, answers: 0, truncated: 0 }
const failures = []

async function runCase(c) {
  const win = 'w' + ++windowNo
  const fail = (what) => { throw Object.assign(new Error(what), { gate: true }) }
  await E(([c]) => { G.dispose('ref'); G.make('ref', c.cols, c.rows) }, [c])
  await host.call('open', win, { rows: c.rows, cols: c.cols })
  let total = 0 // the bytes the program has printed
  let refSize = [c.cols, c.rows]
  // the page
  let cur = null
  const inboxes = new Map()
  const newPage = async (id, size) => {
    const name = 'p' + id
    const p = { id, name, next: 0, carried: 0, replaying: true, truncated: false, held: null, size, made: false, busy: Promise.resolve(), trace: [] }
    inboxes.set(id, p)
    if (early.has(id)) { p.inbox = early.get(id); early.delete(id) }
    return p
  }
  const ensureTerm = async (p, size) => {
    if (p.made) await E(([n]) => G.dispose(n), [p.name])
    await E(([n, c, r]) => G.make(n, c, r), [p.name, size[0], size[1]])
    p.made = true
  }
  const process = (p) => {
    p.busy = p.busy.then(async () => {
      try {
        while (!p.replaying && p.inbox?.length && p === cur && !p.held && !p.gone) {
          const m = p.inbox.shift()
          if (m.offset + m.len <= p.next) { host.send('ack', win, { seq: m.seq }); continue } // wholly below what the page has
          if (m.offset > p.next) throw new Error(`live message at ${m.offset}, the page is at ${p.next}`)
          // One that straddles the page's offset (the journal started again inside it, a mark a size made) is written from it.
          const hex = m.hex.slice((p.next - m.offset) * 2)
          const carried = await E(([n, h]) => G.writeHex(n, h), [p.name, hex])
          p.next += hex.length >> 1
          p.carried = carried
          host.send('ack', win, { seq: m.seq })
        }
      } catch (error) { p.error = error }
    })
    return p.busy
  }
  const early = new Map() // messages the host printed while it answered the subscription, before its reply named the page
  host.onMessage = (m) => {
    const p = inboxes.get(m.page)
    const item = { offset: m.offset, seq: m.seq, hex: m.hex, len: m.hex.length >> 1 }
    if (!p) { if (!early.has(m.page)) early.set(m.page, []); early.get(m.page).push(item); return }
    ;(p.inbox ??= []).push(item)
    void process(p)
  }
  const subscribe = async () => {
    const r = await host.call('subscribe', win)
    const p = await newPage(r.page, refSize)
    p.boundary = r.boundary
    return p
  }
  cur = await subscribe()
  await ensureTerm(cur, [c.cols, c.rows]); cur.replaying = false
  const refAnswersByOp = []
  const lostAnswers = new Set()
  const compare = async (label, p, past) => {
    results.compared++
    if (past) {
      const a = await E(() => G.reliable('ref')), b = await E(([n]) => G.reliable(n), [p.name])
      const d = await E(([a, b]) => G.diff(a, b), [a, b])
      const sa = await E(() => G.sends('ref')), sb = await E(([n]) => G.sends(n), [p.name])
      if (sa !== sb) d.push(`paste: ${JSON.stringify(sa)} | ${JSON.stringify(sb)}`)
      return d
    }
    const a = await E(() => G.state('ref')), b = await E(([n]) => G.state(n), [p.name])
    const d = await E(([a, b]) => G.diff(a, b), [a, b])
    const sa = await E(() => G.sends('ref')), sb = await E(([n]) => G.sends(n), [p.name])
    if (sa !== sb) d.push(`paste: ${JSON.stringify(sa)} | ${JSON.stringify(sb)}`)
    return d
  }
  const differences = []
  let truncatedNow = false
  const pageAnswers = []
  let opIndex = 0
  for (const op of c.ops) {
    opIndex++
    if (op.emit || op.repeat) {
      const bytes = op.emit ?? op.repeat
      const count = op.repeat ? op.count : 1
      const len = bytes.length * count
      const before = await E(() => G.answers.ref.length)
      if (op.repeat) await E(([b, n]) => G.writeRepeat('ref', b, n), [bytes, count])
      else await E(([b]) => G.write('ref', b), [bytes])
      const after = await E(() => G.answers.ref.length)
      const start = total
      total += len
      if (op.repeat) host.send('repeat', win, { hex: hex(bytes), count })
      else host.send('emit', win, { hex: hex(bytes) })
      await host.call('sync', win, { n: total })
      await cur.busy
      if (cur.error) throw cur.error
      if (cur.held) for (let i = before; i < after; i++) lostAnswers.add(i) // handed to a page that left before it parsed them: main's limit 4
      void start
    } else if (op.resize) {
      refSize = op.resize
      await E(([s]) => G.resize('ref', s[0], s[1]), [op.resize])
      if (cur && !cur.gone) {
        await E(([n, s]) => G.resize(n, s[0], s[1]), [cur.name, op.resize])
        host.send('resize', win, { rows: op.resize[1], cols: op.resize[0], at: cur.next - cur.carried })
      } else host.send('resize', win, { rows: op.resize[1], cols: op.resize[0], at: null })
    } else if (op.gone) {
      await host.call('gone', win)
      cur.gone = true
    } else if (op.depart) {
      cur.held = true
    } else if (op.reload) {
      results.reloads++
      const old = cur
      if (old) { old.replaying = true; pageAnswers.push(...(await E(([n]) => G.take(n), [old.name]))) ; await E(([n]) => G.dispose(n), [old.name]); old.made = false }
      const p = await subscribe()
      cur = p
      await ensureTerm(p, [c.cols, c.rows])
      await E(([n]) => G.mute(n, true), [p.name])
      let from = null
      for (;;) {
        const piece = await host.call('history', win, { from })
        if (piece.reset) {
          await ensureTerm(p, [piece.size[1], piece.size[0]])
          await E(([n]) => G.mute(n, true), [p.name])
          if (piece.seed) { await E(([n, b]) => G.write(n, b), [p.name, unhex(piece.seed)]) }
          p.next = piece.from; p.carried = 0
          if (piece.truncated) { p.truncated = true; truncatedNow = true }
        } else if (p.next !== piece.from) fail(`a continuation from ${piece.from}, the page is at ${p.next}`)
        const len = piece.hex.length >> 1
        let at = 0
        for (const [rat, rows, cols] of piece.resizes) {
          const upto = rat - piece.from
          if (upto > at) { p.carried = await E(([n, h, a, b]) => G.writeHex(n, h, a, b), [p.name, piece.hex, at, upto]); at = upto }
          await E(([n, c, r]) => G.resize(n, c, r), [p.name, cols, rows])
        }
        if (at < len) p.carried = await E(([n, h, a]) => G.writeHex(n, h, a), [p.name, piece.hex, at])
        p.next = piece.from + len
        if (piece.done) { p.boundaryDone = piece.boundary; break }
        from = p.next
      }
      // a restart past the boundary: the live messages below it are dropped by `process`
      await E(([n]) => G.mute(n, false), [p.name])
      const leaks = await E(([n]) => G.leaks[n], [p.name])
      if (leaks) differences.push(`${c.name}: ${leaks} answers made while muted`)
      p.replaying = false
      await process(p)
      if (p.error) throw p.error
      if (p.next !== total) fail(`the page is at ${p.next} after the reload, the window printed ${total}`)
      const sz = await E(([n]) => G.terms[n].t.cols + 'x' + G.terms[n].t.rows, [p.name])
      void sz
      const d = await compare('reload', p, c.past || p.truncated)
      if (p.truncated) results.truncated++
      if (d.length) differences.push(`after reload ${results.reloads}${p.truncated ? ' (past the cap)' : ''}: ${d.join(' ; ')}`)
    }
  }
  // the end: the page's state against the continuous xterm's, and the answers once each
  await cur.busy
  if (cur.error) throw cur.error
  pageAnswers.push(...(await E(([n]) => G.take(n), [cur.name])))
  const refAnswers = await E(() => G.answers.ref.slice())
  const expected = refAnswers.filter((_, i) => !lostAnswers.has(i))
  if (JSON.stringify(expected) !== JSON.stringify(pageAnswers))
    differences.push(`answers: expected ${JSON.stringify(expected)} got ${JSON.stringify(pageAnswers)}`)
  results.answers += pageAnswers.length
  const final = await compare('end', cur, c.past || cur.truncated)
  if (final.length) differences.push(`at the end${cur.truncated ? ' (past the cap)' : ''}: ${final.join(' ; ')}`)
  await E(([n]) => G.dispose(n), [cur.name])
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
    await E(([c]) => { G.reset(); G.make('solo', c.cols, c.rows) }, [c])
    for (const op of c.ops) {
      if (op.emit) await E(([b]) => G.write('solo', b), [op.emit])
      else if (op.repeat) await E(([b, n]) => G.writeRepeat('solo', b, n), [op.repeat, op.count])
      else if (op.resize) await E(([s]) => G.resize('solo', s[0], s[1]), [op.resize])
    }
    return null
  } catch (error) { return error.message } finally { await E(() => G.reset()) }
}
for (const c of all) {
  results.cases++
  let differences
  try { differences = await runCase(c) } catch (error) {
    differences = [`ERROR ${error.message}`]
    await host.call('close', 'w' + windowNo).catch?.(() => {})
    await E(() => G.reset())
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
