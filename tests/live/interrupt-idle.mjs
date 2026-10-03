/**
 * Live: does a harness's interrupt, pressed at a window that is already idle,
 * leave the next message alone?
 *
 * The daemon interrupts only a window at work, but a turn can end in the
 * moment between its look and its keys. Two Escapes at an idle Devin open its
 * rewind, and the next message's Enter confirmed it, cutting the conversation
 * back (poker-lab, 2026-10-03); Devin's interrupt now ends with one Escape
 * more, which closes it. Each window here answers a first message, takes its
 * harness's interrupt as the daemon presses it (`pressInterrupt`), then a
 * second message: that one must be answered, and the first must still be in
 * the record.
 *
 *   npm run live:interrupt                      Devin
 *   npm run live:interrupt -- --harness claude --harness devin
 *
 * The exit code is 1 when a window lost its conversation or the message after.
 */
import { parseArgs } from 'node:util'
import { claudeCodeAdapter } from '../../src/adapters/claude-code.js'
import { devinAdapter } from '../../src/adapters/devin.js'
import { pressInterrupt } from '../../src/core/windows.js'
import {
  ANSWER_MS,
  ENV,
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
/** Each harness's interrupt, as its adapter tells the daemon to press it. */
const INTERRUPTS = {
  claude: claudeCodeAdapter({ env: ENV }).interrupt,
  devin: devinAdapter({ env: ENV }).interrupt,
}
const ask = (what, [a, b]) =>
  `${what}: reply with only the sum of ${a} and ${b}, in digits, and run no tools.`

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
      const first = await send(app, window, window.given(ask('First', [1111, 2222])), '3333')
      if (!first.shown) {
        results.push({
          name,
          ok: false,
          detail: `the first message was not answered: ${lastLines(window.screen())}`,
        })
        continue
      }
      await window.still(ANSWER_MS)
      await pressInterrupt(app, window.pane, INTERRUPTS[name])
      await sleep(2_000)
      const second = await send(app, window, window.given(ask('Second', [4444, 1111])), '5555')
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
