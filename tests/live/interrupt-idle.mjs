/**
 * Live: does a harness's interrupt, pressed at a window that is already idle,
 * leave the next message alone?
 *
 * The daemon interrupts only a window at work, but a turn can end in the
 * moment between its look and its keys. Two Escapes at an idle Devin open its
 * rewind, and the next message's Enter confirmed it, cutting the conversation
 * back (poker-lab, 2026-10-03); Devin's interrupt now ends with one Escape
 * more, which closes it. Each window here answers a first message, takes its
 * harness's interrupt as the daemon presses it (`pressInterrupt`, the keys
 * its adapter says), then a second message: that one must be answered, and
 * the first must still be in the record.
 *
 *   npm run live:interrupt                      Devin
 *   npm run live:interrupt -- --harness claude --harness devin
 *
 * The exit code is 1 when a window lost its conversation or the message after.
 *
 * What the daemon's interrupt does to a window at work (Escape into a long
 * command, at a question hook, a stop that is ignored) is `npm run live:stops`.
 */
import { parseArgs } from 'node:util'
import { interruptOf } from '../rust-harness.mjs'
import {
  ANSWER_MS,
  lastLines,
  openWindow,
  pastedHarnesses,
  READY_MS,
  send,
  sleep,
  startLiveApp,
} from './live-window.mjs'

const { values } = parseArgs({ options: { harness: { type: 'string', multiple: true } } })
const harnesses = pastedHarnesses(values.harness ?? ['devin'])
/** The kind of each harness the app pastes into, which its adapter is asked for the interrupt by. */
const KINDS = { claude: 'claude-code', devin: 'devin' }
const ask = (what, [a, b]) =>
  `${what}: reply with only the sum of ${a} and ${b}, in digits, and run no tools.`

/** The key that interrupts a turn, and the gap of a double press: the daemon's (crates/cf-engine/src/windows.rs). */
const ESCAPE = 27
const DOUBLE_PRESS_MS = 150

/**
 * A harness's interrupt as keys into its window, as the daemon presses it
 * (`press_interrupt`): Escape as many times in a row as it asks for and,
 * where those presses open a dialog at a turn that ended just before them
 * (Devin's rewind), one more after a pause, which closes it and is nothing
 * anywhere else.
 */
async function pressInterrupt(host, pane, { presses, closeAfterMs }) {
  const pressEscape = () => host.request('pane.input', { ...pane, bytes: [ESCAPE] }).catch(() => {})
  for (let press = 0; press < presses; press += 1) {
    if (press > 0) await sleep(DOUBLE_PRESS_MS)
    await pressEscape()
  }
  if (closeAfterMs !== null) {
    await sleep(closeAfterMs)
    await pressEscape()
  }
}

const app = await startLiveApp()
const results = []
try {
  for (const name of harnesses) {
    const window = await openWindow(app, name, { folder: 'interrupt', id: `interrupt-${name}` })
    if (window.trust !== null) process.stdout.write(`trust: ${window.trust}\n`)
    try {
      if (!(await window.still(READY_MS))) {
        results.push({ name, ok: false, detail: `did not open: ${lastLines(window.screen())}` })
        continue
      }
      const first = await send(app, window, await window.given(ask('First', [1111, 2222])), '3333')
      if (!first.shown) {
        results.push({
          name,
          ok: false,
          detail: `the first message was not answered: ${lastLines(window.screen())}`,
        })
        continue
      }
      await window.still(ANSWER_MS)
      await pressInterrupt(app, window.pane, await interruptOf(KINDS[name]))
      await sleep(2_000)
      const second = await send(
        app,
        window,
        await window.given(ask('Second', [4444, 1111])),
        '5555',
      )
      const users = (await window.recorded())
        .filter((item) => item.role === 'user')
        .map((item) => item.text)
      const whole = users.some((text) => text.includes('First:'))
      results.push({
        name,
        ok: second.shown && whole,
        detail: !second.shown
          ? `the message after it was not answered: ${lastLines(window.screen())}`
          : whole
            ? `the message after it answered in ${second.seconds} s, the conversation whole`
            : 'the message after it was answered, but the first is gone: the conversation was cut back',
      })
    } finally {
      await window.kill()
    }
  }
} finally {
  await app.close()
}

for (const { name, ok, detail } of results) {
  process.stdout.write(
    `${ok ? 'ok  ' : 'FAIL'} ${name.padEnd(9)} interrupt at an idle window: ${detail}\n`,
  )
}
process.exit(results.every((result) => result.ok) ? 0 : 1)
