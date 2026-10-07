/**
 * Live: what a paste does to the text Claude puts back into its input box,
 * and what clears it first.
 *
 * Claude, interrupted before it wrote anything of its answer (an Escape within
 * the first seconds of a turn), puts the message it was given back into its
 * input box and writes no interrupt record. The daemon pastes into a window it
 * finds at rest, and a paste goes in where the cursor is: a message pasted
 * into that box would be sent with the old one in front of it. Each strategy
 * here is a window of its own: a first message, an Escape at once as the daemon
 * presses one, the keys of the strategy once Claude reads idle, then a second
 * message pasted the way the daemon pastes (`pane.write_paste`). What Claude's
 * record holds of the second one says whether the old text went with it. When
 * that was right, the same keys are pressed at the empty box of a window at
 * rest, before a third message, which must arrive alone and be answered: a key
 * that harms an empty prompt (a second Escape opens Claude's rewind) is no way
 * to clear one.
 *
 *   npm run live:input-box                        every strategy
 *   npm run live:input-box -- --strategy ctrl-c   one
 *   npm run live:input-box -- --require ctrl-c    the exit code is 1 unless it cleared
 *
 * The strategies are keys written into the pane as the daemon writes its own
 * (`pane.input`), `none` being what a paste does with nothing pressed first.
 * Claude on its cheap eval model, in a folder of its own, with no MCP server
 * and no browser; each window is killed when its strategy is done.
 *
 * Found with Claude 2.1.292 (2026-10-07): `none` pastes the second message
 * after the first, as one message; `ctrl-u` clears the cursor's line only,
 * leaving the first line of a two-line message; `esc-esc` clears the text and
 * then, at an empty box, opens Claude's rewind, which swallows the next paste;
 * `ctrl-c` clears it whole and is harmless at an empty box. It is the key the
 * daemon presses (`CLEAR_INPUT`, crates/cf-harness/src/claude/stopped.rs), so
 * with no `--require` the exit code asks for it.
 */
import { parseArgs } from 'node:util'
import { ANSWER_MS, lastLines, openWindow, READY_MS, sleep, startLiveApp } from './live-window.mjs'
import { claudeRecord, claudeStatus, seconds, until } from './receipt-rig.mjs'

/** Keys are bytes; a number is a pause in milliseconds. */
const STRATEGIES = {
  none: [],
  'ctrl-u': [[0x15]],
  'ctrl-c': [[0x03]],
  'esc-esc': [[0x1b], 300, [0x1b]],
}

const { values } = parseArgs({
  options: {
    strategy: { type: 'string', multiple: true },
    require: { type: 'string', multiple: true },
  },
})
const CHOSEN = values.strategy ?? Object.keys(STRATEGIES)
/** The daemon's own key must clear the box, where it is tried. */
const REQUIRED = values.require ?? CHOSEN.filter((name) => name === 'ctrl-c')
for (const name of [...CHOSEN, ...REQUIRED]) {
  if (!STRATEGIES[name]) throw new Error(`no such strategy: ${name}`)
}

/** A message shaped like the daemon's, which asks for one word back. */
const message = (n, word) =>
  `[ConsensFlow m-${n} · T-1 · task from @chief]\nReply with exactly one line: ${word}`
const IDLE_MS = 30_000

/** The user's records of the conversation, oldest first, as text. */
const users = (session) => claudeRecord(session).filter((record) => record.type === 'user')

/** Presses `steps` into the pane: keys, and the pauses between them. */
async function press(app, window, steps) {
  for (const step of steps) {
    if (typeof step === 'number') await sleep(step)
    else await app.request('pane.input', { ...window.pane, bytes: step })
  }
}

/**
 * Pastes `word`'s message and says what arrived: the text of the user's record
 * that holds the word, and whether the window answered it.
 */
async function pasteAndRead(app, window, n, word) {
  await app.request('pane.write_paste', { ...window.pane, body: window.given(message(n, word)) })
  const arrived = await until(
    () => users(window.session).find((record) => record.text.includes(`m-${n} `)),
    60_000,
    250,
  )
  const answered = await until(
    () =>
      claudeRecord(window.session).find(
        (record) => record.type === 'assistant' && record.text.includes(word),
      ),
    ANSWER_MS,
    500,
  )
  return { text: arrived?.text ?? null, answered: Boolean(answered) }
}

/** Whether `read.text` is the message `n` alone: the old messages are not in it. */
const alone = (read, n) =>
  Boolean(read.text?.includes(`m-${n} `)) &&
  [...read.text.matchAll(/m-(\d+) /g)].every(([, found]) => Number(found) === n)

async function strategy(app, name) {
  const steps = STRATEGIES[name]
  const window = await openWindow(app, 'claude', { folder: 'input-box', id: `input-box-${name}` })
  const lines = []
  const say = (line) => lines.push(line)
  try {
    if (window.trust !== null) say(`trust: ${window.trust}`)
    if (!(await window.still(READY_MS))) {
      say(`FAIL it did not open: ${lastLines(window.screen())}`)
      return { name, ok: false, lines }
    }
    const status = claudeStatus(window.session)
    await app.request('pane.write_paste', {
      ...window.pane,
      body: window.given(message(1, 'FIRST')),
    })
    const pressed = Date.now()
    await app.request('pane.input', { ...window.pane, bytes: [27] })
    const idle = await until(
      () => status.samples.find((sample) => sample.at > pressed && sample.status === 'idle'),
      IDLE_MS,
      100,
    )
    await sleep(2_000)
    status.stop()
    const records = claudeRecord(window.session)
    const wrote = records.filter((record) => record.type === 'assistant').length
    say(
      `Escape at once: Claude read idle ${idle ? `${seconds(idle.at - pressed)} s after it, its status changed at +${seconds(idle.changedAt - pressed)} s` : 'never'}; its record holds ${wrote} assistant records and ${users(window.session).length} user record: ${JSON.stringify(users(window.session).at(-1)?.text.slice(0, 60))}`,
    )
    say(`its screen: ${lastLines(window.screen())}`)
    if (!idle || wrote > 0) {
      say(
        'FAIL Claude did not stop before it wrote anything: this is not the shape the strategy is for',
      )
      return { name, ok: false, lines }
    }
    await press(app, window, steps)
    await sleep(700)
    say(`after ${name}: ${lastLines(window.screen())}`)
    const second = await pasteAndRead(app, window, 2, 'SECOND')
    const clean = alone(second, 2)
    say(
      `${clean ? 'ok  ' : 'also'} the second message arrived ${clean ? 'alone' : 'with the first'}${second.answered ? ', and was answered' : ', and was not answered'}: ${JSON.stringify(second.text?.slice(0, 160) ?? null)}`,
    )
    let harmless = true
    if (clean && second.answered) {
      await window.still(ANSWER_MS)
      await press(app, window, steps)
      await sleep(700)
      const third = await pasteAndRead(app, window, 3, 'THIRD')
      harmless = alone(third, 3) && third.answered
      say(
        `${harmless ? 'ok  ' : 'FAIL'} the same keys at the empty box of a window at rest: the third message ${harmless ? 'arrived alone and was answered' : `did not: ${JSON.stringify(third.text?.slice(0, 160) ?? null)}; its screen: ${lastLines(window.screen())}`}`,
      )
    }
    return { name, ok: clean && second.answered && harmless, lines }
  } finally {
    await window.kill()
  }
}

const app = await startLiveApp()
const results = []
try {
  for (const name of CHOSEN) results.push(await strategy(app, name))
} finally {
  await app.close()
}

for (const { name, ok, lines } of results) {
  process.stdout.write(
    `\n== ${name}: ${ok ? 'clears the box and is harmless at an empty one' : 'does not'}\n`,
  )
  for (const line of lines) process.stdout.write(`   ${line}\n`)
}
process.exit(REQUIRED.every((name) => results.find((result) => result.name === name)?.ok) ? 0 : 1)
