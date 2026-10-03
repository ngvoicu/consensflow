/**
 * Live: does a harness's window send what ConsensFlow pastes into it?
 *
 * Each harness opens in a window of the app's own pane host, started the way
 * the app starts it, in a folder of its own under the user's home, on its
 * cheap eval model. Three messages go in the way the daemon sends one
 * (`pane.write_paste`: a bracketed paste, then Enter once the window has
 * drawn it): one line, a few lines shaped like a ConsensFlow message, and one
 * of about 3,800 characters, which Devin and Claude fold into a placeholder.
 * Each asks for a sum the message does not hold, so its answer on the screen
 * means the window sent it, and the harness's own record must then hold the
 * message whole, its marks included (— “” → € …), as the app reads it there.
 * Each goes in as the app gives it to that window: Devin on Windows gets its
 * marks in ASCII. Nothing presses Enter a second time: a message left
 * waiting in the input is a failure, the one Devin showed on Windows.
 *
 *   npm run live:paste                      Devin
 *   npm run live:paste -- --harness claude --harness devin
 *   npm run live:paste -- --harness claude --long 8000 --long 16000
 *                                           long messages of those lengths, one of each
 *
 * Harnesses: the two the app pastes into, claude and devin (Codex, Pi and
 * OpenCode take their messages through their own queues). They run one
 * after another; the exit code is 1 when any message was not sent.
 */
import { parseArgs } from 'node:util'
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

const { values } = parseArgs({
  options: {
    harness: { type: 'string', multiple: true },
    long: { type: 'string', multiple: true },
  },
})
const harnesses = pastedHarnesses(values.harness ?? ['devin'])
/** The long messages' lengths, in characters. */
const LONG = (values.long ?? ['3700']).map(Number)
if (!LONG.every((length) => Number.isInteger(length) && length > 0)) {
  throw new Error('--long takes a length in characters')
}

const ask = ([a, b]) => `Reply with only the sum of ${a} and ${b}, in digits, and run no tools.`
const CASES = [
  { name: 'one line', sum: [1234, 4321], body: (line) => line },
  {
    name: 'a ConsensFlow message',
    sum: [2468, 1357],
    body: (line) =>
      [
        '[ConsensFlow m-1 · T-1 · result from @worker]',
        'The page is done — “Contact” moved → the footer, €0 spent…',
        '',
        'Files changed: index.html.',
        line,
      ].join('\n'),
  },
  ...LONG.map((length, at) => ({
    name: `a long message (${length} characters)`,
    sum: [3141 + at, 2718 + at],
    body: (line) => {
      const lines = ['[ConsensFlow m-2 · T-1 · result from @worker]']
      for (let n = 1; lines.join('\n').length < length; n += 1) {
        lines.push(`${n}. Section ${n} — headings, “links” → footer: all match…`)
      }
      return [...lines, line].join('\n')
    },
  })),
]

const app = await startLiveApp()
const results = []
try {
  for (const name of harnesses) {
    const window = await openWindow(app, name, { folder: 'paste', id: `paste-${name}` })
    if (window.trust !== null) process.stdout.write(`trust: ${window.trust}\n`)
    try {
      if (!(await window.still(READY_MS))) {
        results.push({ name, check: 'opens', ok: false, detail: lastLines(window.screen()) })
        continue
      }
      for (const check of CASES) {
        const answer = String(check.sum[0] + check.sum[1])
        const body = window.given(check.body(ask(check.sum)))
        const { written, shown, seconds } = await send(app, window, body, answer)
        // Its record holds what was pasted, whole: where the app looks for it.
        const whole = body.replace(/\r\n/g, '\n').trim()
        let users = []
        let kept = false
        for (let tries = 0; shown && !kept && tries < 20; tries += 1) {
          users = (await window.recorded()).filter((item) => item.role === 'user')
          kept = users.some((item) => item.text.replace(/\r\n/g, '\n').includes(whole))
          if (!kept) await sleep(500)
        }
        const escaped = (text) =>
          text.slice(0, 160).replace(/[^\x20-\x7e]/g, (c) => `\\u${c.charCodeAt(0).toString(16)}`)
        results.push({
          name,
          check: check.name,
          ok: written?.ok === true && shown && kept,
          detail: !shown
            ? `NOT SENT (${JSON.stringify(written)}): ${lastLines(window.screen())}`
            : kept
              ? `sent and recorded whole, answered in ${seconds} s`
              : `sent, but recorded otherwise: ${escaped(users.at(-1)?.text ?? '(no record found)')}`,
        })
        if (!shown) break
        await window.still(ANSWER_MS)
      }
    } finally {
      await window.kill()
    }
  }
} finally {
  await app.close()
}

for (const { name, check, ok, detail } of results) {
  process.stdout.write(`${ok ? 'ok  ' : 'FAIL'} ${name.padEnd(9)} ${check.padEnd(22)} ${detail}\n`)
}
process.exit(results.every((result) => result.ok) ? 0 : 1)
