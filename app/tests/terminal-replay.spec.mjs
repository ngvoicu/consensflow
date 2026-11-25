import { expect, test } from '@playwright/test'

import { cases as review51 } from './p5-gate/cases51.mjs'
import { reviewCases } from './p5-gate/gen.mjs'
import { serveUi } from './serve-ui.mjs'

/**
 * P5-C: a reloaded page gets a window's terminal back from the pane host's journal (the window's
 * output and sizes), written muted, and then takes everything from the boundary live. The page's
 * own modules and the shipped xterm; the pane host is a stand-in in the page (`P5.fakeHost`)
 * that answers what the real one answers while its journal reaches the window's start. The real
 * host's rules, with the real page, are `app/tests/p5-gate/` (run by hand).
 */
const SCRIPT = new URL('./p5-gate/page.js', import.meta.url).pathname
let ui

test.setTimeout(60_000)
test.beforeAll(async () => {
  ui = await serveUi()
})
test.afterAll(() => ui?.close())

async function blankPage(page) {
  await page.route('**/p5-blank', (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/vendor/xterm.css"><body></body>',
    }),
  )
  await page.goto(`${ui.origin}/p5-blank`)
  await page.addScriptTag({ path: SCRIPT })
  await page.evaluate(() => window.P5.load())
}

const bytes = (text) => [...new TextEncoder().encode(text)]
const ESC = '\x1b'
const CSI = `${ESC}[`

test('a reloaded page equals a continuous xterm in every case the reviews ran', async ({ page }) => {
  await blankPage(page)
  const cases = reviewCases(review51)
  expect(cases).toHaveLength(51)
  const failures = []
  for (const [at, whole] of [
    ['whole', 1 << 20],
    ['pieces of 7 bytes', 7],
  ]) {
    for (const c of cases) {
      const differences = await page.evaluate(([c, pieceBytes]) => P5.runCase(c, { pieceBytes }), [
        c,
        whole,
      ])
      if (differences.length > 0) failures.push({ at, name: c.name, differences })
    }
  }
  expect(failures).toEqual([])
})

test('a character cut between messages is drawn whole, live and after a reload', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const out = {}
    for (const reload of [false, true]) {
      const host = P5.fakeHost({ cols: 30, rows: 8 })
      const name = `cut${reload}`
      let win = P5.window(name, { cols: 80, rows: 24, invoke: host.invoke, pageId: name })
      host.subscribe((m) => win.link.output(m, () => win.emulator))
      win.start()
      await win.replayed
      host.print([0x41, 0xe2, 0x80])
      await win.quiet()
      // The size the page takes now is journaled before the held bytes: the host reads `at` 1.
      await win.resize(31, 8)
      out.at = host.sizes.at(-1)
      if (reload) {
        win.dispose()
        win = P5.window(`${name}b`, { cols: 80, rows: 24, invoke: host.invoke, pageId: `${name}b` })
        host.subscribe((m) => win.link.output(m, () => win.emulator))
        win.start()
        await win.replayed
      }
      host.print([0x94])
      host.print([0x42])
      await win.quiet()
      out[reload ? 'reloaded' : 'live'] = P5.state(win.emulator).text
      out[`acks${reload}`] = host.acks.at(-1)
      win.dispose()
    }
    return out
  })
  expect(result.live).toEqual(['A—B'])
  expect(result.reloaded).toEqual(['A—B'])
  expect(result.at.at).toBe(1)
})

test('a reload keeps bracketed paste', async ({ page }) => {
  await blankPage(page)
  const sent = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    await P5.history(host, [...new TextEncoder().encode('\x1b[?2004hprompt')])
    let win = P5.window('paste', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'paste' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    win.start()
    await win.replayed
    return P5.sends(win.emulator)
  })
  expect(sent).toBe('\x1b[200~A\rB\x1b[201~')
})

test('a replay answers no question of the program and the first live question is answered once', async ({
  page,
}) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    // Questions in the history: a status report, a cursor report, device attributes, and a
    // program that asked for the focus reports.
    await P5.history(host, [...new TextEncoder().encode('\x1b[?1004h\x1b[5n\x1b[6n\x1b[c\x1b[?2004$phi')])
    // The page handed them live has gone: they were answered then or never.
    const win = P5.window('ask', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'ask' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    win.start()
    await win.replayed
    const afterReplay = win.answers.slice()
    const leaks = win.leaks
    host.print([...new TextEncoder().encode('\x1b[5n')])
    await win.quiet()
    return { afterReplay, leaks, afterLive: win.answers.slice(), muted: win.emulator.terminal.options.disableStdin }
  })
  expect(result.afterReplay).toEqual([])
  expect(result.leaks).toBe(0)
  expect(result.afterLive).toEqual(['\x1b[0n'])
  expect(result.muted).toBe(false)
})

test('nothing typed or pasted into a window while its history is written is sent', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    await P5.history(host, [...new TextEncoder().encode('\x1b[?2004hold text')])
    host.hold('pane_history')
    const win = P5.window('typing', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'typing' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    const typed = []
    win.emulator.onData((data) => typed.push(data))
    win.start()
    await P5.sleep(20)
    // Replaying: its history has not come, and the terminal takes no key, no paste, no input of
    // the page's own, and reports no focus.
    const during = win.emulator.terminal.options.disableStdin
    win.emulator.terminal.input('x')
    win.emulator.paste('p')
    win.emulator.terminal.textarea?.focus()
    win.emulator.terminal.textarea?.blur()
    host.release('pane_history')
    await win.replayed
    const duringTyped = typed.slice()
    win.emulator.terminal.input('y', true)
    win.emulator.paste('q')
    return { during, duringTyped, afterTyped: typed.slice(), after: win.emulator.terminal.options.disableStdin }
  })
  expect(result.during).toBe(true)
  expect(result.duringTyped).toEqual([])
  expect(result.after).toBe(false)
  // After it, the person's typing is sent as ever (the paste between the program's markers).
  expect(result.afterTyped).toContain('\x1b[200~q\x1b[201~')
})

test('two reloads in quick succession leave the second page with the window', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8, pieceBytes: 5 })
    await P5.history(host, [...new TextEncoder().encode('first line\r\nsecond line\r\n')])
    const first = P5.window('one', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'one' })
    host.subscribe((m) => first.link.output(m, () => first.emulator))
    first.start()
    await P5.sleep(5) // the first page is still pulling when the next subscribes
    host.stale.add('one')
    first.dispose()
    const second = P5.window('two', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'two' })
    host.subscribe((m) => second.link.output(m, () => second.emulator))
    second.start()
    host.print([...new TextEncoder().encode('third')])
    await second.replayed
    await second.quiet()
    return { text: P5.state(second.emulator).text, reports: [...first.reports, ...second.reports] }
  })
  expect(result.text).toEqual(['first line', 'second line', 'third'])
  expect(result.reports).toEqual([])
})

test('the page asks for no size the window refuses', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    const win = P5.window('big', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'big' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    win.start()
    await win.replayed
    const out = []
    for (const [cols, rows] of [
      [5000, 30],
      [60, 5000],
    ]) {
      const emulator = win.emulator
      emulator.host.style.width = '40000px'
      emulator.host.style.height = '40000px'
      emulator.fitAddon.proposeDimensions = () => ({ cols, rows })
      emulator.sized = false
      P5.XtermEmulator.prototype.fit.call(emulator)
      await P5.sleep(10)
      out.push([emulator.terminal.cols, emulator.terminal.rows])
    }
    return { out, asked: host.sizes.map((s) => [s.cols, s.rows]) }
  })
  expect(result.out).toEqual([
    [4096, 30],
    [60, 4096],
  ])
  expect(result.asked.flat().every((n) => n <= 4096)).toBe(true)
})

test('the end of a window: its history is pulled once more and let go, a later page has none', async ({
  page,
}) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    await P5.history(host, [...new TextEncoder().encode('last words\r\n')])
    host.ended = true
    const win = P5.window('ended', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'ended' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    win.start()
    await win.replayed
    const text = P5.state(win.emulator).text
    const done = host.done
    const consumed = host.calls.filter(([c]) => c === 'pane_history_done').map(([, a]) => a.consumed)
    // The host keeps no more of it: a page that comes later is told there is none, and goes live.
    const later = P5.window('later', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'later' })
    host.subscribe((m) => later.link.output(m, () => later.emulator))
    later.start()
    await later.replayed
    return { text, done, consumed, laterText: P5.state(later.emulator).text, reports: later.reports }
  })
  expect(result.text).toEqual(['last words'])
  expect(result.done).toBe(1)
  // All of the history: the offset the emulator has taken.
  expect(result.consumed).toEqual(['last words\r\n'.length])
  expect(result.laterText).toEqual([])
  expect(result.reports).toEqual([])
})

test('a pull that fails is tried again from the same offset, and a stale page stops', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8, pieceBytes: 6 })
    await P5.history(host, [...new TextEncoder().encode('0123456789ab')])
    host.failNext.set('pane_history', 2)
    const win = P5.window('retry', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'retry' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    const started = performance.now()
    win.start()
    await win.replayed
    const pulls = host.calls.filter(([c]) => c === 'pane_history').map(([, a]) => a.from)
    return { pulls, text: P5.state(win.emulator).text, took: performance.now() - started }
  })
  // 250 ms, then 500 ms, between the tries; then the second piece, from where the first ended.
  expect(result.pulls).toEqual([null, null, null, 6])
  expect(result.took).toBeGreaterThan(700)
  expect(result.text).toEqual(['0123456789ab'])
})

test('an acknowledgement that fails is tried again until it is taken', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    const win = P5.window('ack', { cols: 80, rows: 24, invoke: host.invoke, pageId: 'ack' })
    host.subscribe((m) => win.link.output(m, () => win.emulator))
    win.start()
    await win.replayed
    host.failNext.set('pane_ack', 1)
    const message = host.print([...new TextEncoder().encode('x')])
    await win.quiet()
    const early = host.acks.length
    await P5.sleep(600)
    return { early, acks: host.acks.slice(), seq: message.seq }
  })
  expect(result.early).toBe(0)
  expect(result.acks).toEqual([result.seq])
})

test('a window that xterm fails on is drawn clean from the boundary, marked truncated, every time', async ({
  page,
}) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const host = P5.fakeHost({ cols: 30, rows: 8 })
    // The smallest stream xterm 6.0.0 stops drawing on: the cursor in the last cell of a screen
    // that has not scrolled, then the erase above.
    // handed to an earlier page that has gone (the page that met it live is stuck, as on main)
    host.printed.push(...new TextEncoder().encode('before\x1b[8;30H\x1b[1J'))
    host.handed = host.printed.length
    host.seq = 1
    const out = []
    for (let reload = 0; reload < 2; reload++) {
      const win = P5.window(`stuck${reload}`, { cols: 80, rows: 24, invoke: host.invoke, pageId: `stuck${reload}` })
      host.subscribe((m) => win.link.output(m, () => win.emulator))
      win.start()
      // A replay left stuck never ends: the case is bounded, so it fails on its words.
      const ended = await Promise.race([win.replayed.then(() => true), P5.sleep(5000).then(() => false)])
      if (!ended) {
        out.push({ ended, text: [''], truncated: win.truncated, muted: true, acked: false })
        break
      }
      host.print([...new TextEncoder().encode(`live${reload}`)])
      await win.quiet()
      out.push({
        ended,
        done: win.calls.filter((c) => c === 'pane_history_done').length,
        text: P5.state(win.emulator).text,
        truncated: win.truncated,
        muted: win.emulator.terminal.options.disableStdin,
        acked: host.acks.at(-1) === host.seq,
      })
      win.dispose()
    }
    return out
  })
  for (const [at, one] of result.entries()) {
    expect(one.ended).toBe(true)
    expect(one.text.join('')).toContain(`live${at}`)
    // Nothing of the history is told of a window xterm failed on.
    expect(one.done).toBe(0)
    expect(one.truncated).toBe(true)
    expect(one.muted).toBe(false)
    expect(one.acked).toBe(true)
  }
})

/**
 * A history that begins past the boundary, which the host never gives, as the first piece or a
 * later one: the page takes none of it, starts clean at the boundary, draws the live bytes from
 * there and answers their question, says its history is cut, and tells the host nothing of the
 * history (T-2275: taken, it dropped delivered bytes and forgot an ended window's journal).
 */
for (const later of [false, true]) {
  test(`a history that begins past the boundary is not taken, as the ${later ? 'second' : 'first'} piece`, async ({
    page,
  }) => {
    await blankPage(page)
    const result = await page.evaluate(async (later) => {
      const text = (value) => [...new TextEncoder().encode(value)]
      const live = text('\x1b[5nTAIL')
      const past = {
        ok: true,
        reset: true,
        truncated: true,
        from: 14,
        size: { rows: 8, cols: 30 },
        seed: [],
        resizes: [],
        bytes: [],
        boundary: 6,
        done: true,
      }
      const pieces = later
        ? [
            { ...past, reset: true, truncated: false, from: 0, bytes: text('HEA'), done: false },
            past,
          ]
        : [past]
      const win = P5.window('past', {
        cols: 80,
        rows: 24,
        pageId: 'past',
        invoke: async (command) => {
          if (command === 'pane_history') return pieces.shift()
          return { ok: true }
        },
      })
      win.link.output({ id: 'w', generation: 1, seq: 2, offset: 6, bytes: Uint8Array.from(live) }, () => win.emulator)
      win.start()
      await win.replayed
      await win.quiet()
      await P5.sleep(20)
      return {
        text: P5.state(win.emulator).text,
        answers: win.answers.slice(),
        truncated: win.truncated,
        calls: win.calls.slice(),
      }
    }, later)
    expect(result.text).toEqual(['TAIL'])
    expect(result.answers).toEqual(['\x1b[0n'])
    expect(result.truncated).toBe(true)
    expect(result.calls).toContain('pane_ack')
    expect(result.calls).not.toContain('pane_history_done')
  })
}

test("the page reads the frame the app's pane_history command writes", async ({ page }) => {
  await blankPage(page)
  // The same header and bytes as `a_piece_of_history_is_framed_as_the_page_reads_it` in commands.rs.
  const header =
    '{"boundary":7,"done":true,"from":5,"ok":true,"reset":true,"resizes":[{"at":6,"cols":31,"rows":9}],"seedLength":2,"size":{"cols":30,"rows":8},"truncated":false}'
  const frame = await page.evaluate(async (header) => {
    const { historyFrame } = await import('/terminal-link.js')
    const head = new TextEncoder().encode(header)
    const out = new Uint8Array(4 + head.length + 4)
    new DataView(out.buffer).setUint32(0, head.length, true)
    out.set(head, 4)
    out.set([0x1b, 0x5b, 0x68, 0x69], 4 + head.length)
    const read = historyFrame(out.buffer)
    return { ...read, seed: [...read.seed], bytes: [...read.bytes] }
  }, header)
  expect(frame).toMatchObject({
    ok: true,
    reset: true,
    from: 5,
    boundary: 7,
    done: true,
    size: { rows: 8, cols: 30 },
    resizes: [{ at: 6, cols: 31, rows: 9 }],
    seed: [0x1b, 0x5b],
    bytes: [0x68, 0x69],
  })
})

/** Two windows whose history is `text`, replaying at once. */
const kept = '\x1b[?2004hKEPT'

test('an error of the page fails no replay: both windows keep their text and bracketed paste', async ({
  page,
}) => {
  await blankPage(page)
  const result = await page.evaluate(async (text) => {
    const data = [...new TextEncoder().encode(text)]
    const hosts = [P5.fakeHost({ cols: 30, rows: 8 }), P5.fakeHost({ cols: 30, rows: 8 })]
    for (const host of hosts) await P5.history(host, data)
    let writes = 0
    const windows = hosts.map((host, i) => {
      const win = P5.window(`good${i}`, { cols: 30, rows: 8, invoke: host.invoke, pageId: `good${i}` })
      host.subscribe((m) => win.link.output(m, () => win.emulator))
      const original = win.emulator.write.bind(win.emulator)
      win.emulator.write = (bytes) => {
        const wrote = original(bytes)
        // A board's own paint throws while both histories are being written.
        if (win.emulator.replaying && ++writes === 2)
          queueMicrotask(() => {
            throw new Error('unrelated board paint exception')
          })
        return wrote
      }
      return win
    })
    for (const win of windows) win.start()
    await Promise.all(windows.map((w) => w.replayed))
    const out = windows.map((win, i) => ({
      truncated: win.truncated,
      text: P5.state(win.emulator).text,
      paste: P5.sends(win.emulator),
      done: hosts[i].done,
    }))
    return { out, raised: P5.errors.slice() }
  }, kept)
  expect(result.raised.join()).toContain('unrelated board paint exception')
  for (const one of result.out) {
    expect(one.text).toEqual(['KEPT'])
    expect(one.paste).toBe('\x1b[200~A\rB\x1b[201~')
    expect(one.truncated).toBe(false)
  }
})

test('an xterm that fails leaves a healthy replay beside it as it was', async ({ page }) => {
  await blankPage(page)
  const result = await page.evaluate(async () => {
    const create = (name, history, sizes = 0) => {
      const data = [...new TextEncoder().encode(history)]
      // Each size at its offset is a write of its own, one after the other.
      const resizes = Array.from({ length: sizes }, (_, i) => ({
        at: Math.floor((data.length * (i + 1)) / (sizes + 1)),
        cols: 30,
        rows: 8,
      }))
      return P5.window(name, {
        cols: 30,
        rows: 8,
        pageId: name,
        invoke: async (command) =>
          command === 'pane_history'
            ? {
                ok: true,
                reset: true,
                truncated: false,
                from: 0,
                size: { cols: 30, rows: 8 },
                seed: [],
                resizes,
                bytes: data,
                boundary: data.length,
                done: true,
              }
            : { ok: true },
      })
    }
    const bad = create('bad', 'before\x1b[8;30H\x1b[1J')
    // The healthy one is still writing its history when the check runs, a write always pending:
    // a hundred writes one after another take far longer than the check waits.
    const good = create('healthy', `\x1b[?2004h${'\x1b[H'.repeat(100)}KEPT`, 100)
    bad.start()
    good.start()
    await Promise.race([
      Promise.all([bad.replayed, good.replayed]),
      P5.sleep(5000).then(() => {
        throw new Error('replay stuck')
      }),
    ])
    return {
      bad: { truncated: bad.truncated, calls: bad.calls.filter((c) => c === 'pane_history_done') },
      good: {
        truncated: good.truncated,
        text: P5.state(good.emulator).text,
        paste: P5.sends(good.emulator),
      },
    }
  })
  expect(result.bad.truncated).toBe(true)
  // The failed window says nothing of its history: the host keeps it.
  expect(result.bad.calls).toEqual([])
  expect(result.good.truncated).toBe(false)
  expect(result.good.text).toEqual(['KEPT'])
  expect(result.good.paste).toBe('\x1b[200~A\rB\x1b[201~')
})
