/**
 * Live: what a harness does when it is resumed on a conversation the app's quit
 * cut in the middle of a turn.
 *
 * The app's quit ends every window's process with its turn at work, and the
 * daemon that starts again opens the chief's window on the harness's own
 * `resume` of the conversation the quit cut: a record whose last turn has no
 * end. The daemon reads a window at rest from what the harness says of it and
 * what its record says of the turn, so what the resumed harness does with a turn
 * it finds cut (go on with it, close it itself, or wait at its prompt and leave
 * the record as it was) is what that reading has to agree with.
 *
 * Each case is a window of its own on a message that has the harness work, a
 * kill of its process with the turn at work (`pane.kill`, which the pane host
 * does to each window as the app quits), and then a window on `resume` of the
 * conversation, opened with the app's own arguments, and watched: the harness's
 * own status where it keeps one (Claude's), what it writes to its record, its
 * screen, and how the daemon's record reader reads the record. At the end a
 * second message is pasted, to show that the window takes it.
 *
 *   command   the message has the harness run a long command; the window is
 *             killed once the command runs: the record ends in the tool's call
 *   early     the window is killed as soon as its turn is at work and has said
 *             nothing (Claude's status reads busy, or the record holds the
 *             message and no more): the record ends in the user's message
 *
 *   npm run live:resume-cut                           Claude, both cases
 *   npm run live:resume-cut -- --case command
 *   npm run live:resume-cut -- --harness codex --harness devin --harness pi --harness opencode
 *   npm run live:resume-cut -- --model haiku          another Claude model (the default is sonnet)
 *
 * The other harnesses run on their cheap eval models, plain windows in a
 * folder of their own: what is read of them is their record and their screen,
 * which is what the daemon reads of a Codex, Pi or Devin window (OpenCode's
 * server says more, and Pi's extension). `--keep <folder>` copies each case's
 * Claude transcript there, as the cut left it and as it ended. Claude has no
 * MCP server and no browser. Needs `npm run build:bridge` and `npm run
 * build:cf`. The exit code is 1 only when a case could not be run (a window
 * that did not open, or a turn that was never at work): what a harness does is
 * the result, and it is said.
 */
import { copyFileSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { HARNESSES } from '../../evals/plan.mjs'
import { readRecord } from '../rust-harness.mjs'
import {
  ANSWER_MS,
  lastLines,
  openWindow,
  READY_MS,
  RECORD_ENV,
  sleep,
  startLiveApp,
} from './live-window.mjs'
import {
  claudeRecord,
  claudeStatus,
  claudeTranscript,
  forgetClaudeFolder,
  processesWith,
  runs,
  seconds,
  until,
  versionOf,
} from './receipt-rig.mjs'

const { values } = parseArgs({
  options: {
    harness: { type: 'string', multiple: true },
    case: { type: 'string', multiple: true },
    model: { type: 'string', default: 'sonnet' },
    keep: { type: 'string' },
  },
})
const NAMES = values.harness ?? ['claude']
for (const name of NAMES) {
  if (!(name in HARNESSES)) throw new Error(`no such harness: ${name}`)
}
const CASES = values.case ?? ['command', 'early']
for (const name of CASES) {
  if (!['command', 'early'].includes(name)) throw new Error(`no such case: ${name}`)
}
const FOLDER = 'resume-cut'
/** How long a turn may take to be at work, and the resumed window to be watched. */
const WORK_MS = 180_000
const WATCH_MS = 45_000
const MARK = 'cf-resume-cut-long-command'
const LONG = `node -e "setTimeout(() => {}, 300000)" ${MARK}`

const MESSAGES = {
  command: `Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n${LONG}`,
  early: 'Count from 1 to 40, one number to a line, and then reply with exactly one line: COUNTED',
}
const SECOND = 'Reply with exactly one line: AFTER'

/** The agent a harness's windows run: Claude's model is the one named, the others' their cheap eval model. */
const agentOf = (name) =>
  name === 'claude' ? { kind: 'claude-code', model: values.model } : HARNESSES[name]

/** An item of a record as a line: who it is from, and what it says. */
const line = (item) =>
  `${item.role}: ${item.text.replace(/\s+/g, ' ').slice(0, 140) || '(no text)'}`

/** How the daemon's own reader reads the record of `session`, as a line. */
async function reading(kind, session) {
  const read = await readRecord(kind, session, RECORD_ENV).catch(() => null)
  if (read === null) return "the daemon's reader: no record"
  const last = read.items.at(-1)
  return `the daemon's reader: ${read.settlement}, ${read.settled ? 'at rest' : 'not at rest'}, ${read.items.length} items, last ${JSON.stringify(last ? line(last).slice(0, 80) : null)}`
}

/** The last items of the record that say something, as a line. */
async function tail(kind, session, last = 3) {
  const read = await readRecord(kind, session, RECORD_ENV).catch(() => null)
  const said = (read?.items ?? []).filter((item) => item.text !== '')
  return `${read?.items.length ?? 0} items; the last: ${said.slice(-last).map(line).join(' | ')}`
}

/** Copies Claude's transcript as it is now into `--keep`'s folder, as `<case>-<when>-<session>.jsonl`. */
function keep(name, when, session) {
  const file = claudeTranscript(session)
  if (!values.keep || file === null) return
  mkdirSync(values.keep, { recursive: true })
  copyFileSync(file, join(values.keep, `${name}-${when}-${session}.jsonl`))
}

/** Ends what a case left: its long command, which a killed window does not take with it. */
function endCommand() {
  for (const found of processesWith(MARK)) {
    try {
      process.kill(found.pid, 'SIGKILL')
    } catch {}
  }
}

async function run(app, harness, name) {
  const lines = []
  const say = (text) => lines.push(text)
  const agent = agentOf(harness)
  const { kind } = agent
  const cut = await openWindow(app, harness, {
    folder: FOLDER,
    id: `resume-cut-${harness}-${name}`,
    agent,
  })
  let resumed = null
  let session = cut.session
  try {
    if (!(await cut.still(READY_MS))) {
      say(`FAIL the window did not open: ${lastLines(cut.screen())}`)
      return { harness, name, ok: false, lines }
    }
    const first = harness === 'claude' ? claudeStatus(session) : null
    await app.request('pane.write_paste', { ...cut.pane, body: await cut.given(MESSAGES[name]) })
    // `early`: the message is in the record and nothing of the answer is: Claude
    // says so by its status, the others by what their record holds.
    const unanswered = async () => {
      const items = await cut.recorded()
      const at = items.findLastIndex(
        (item) => item.role === 'user' && item.text.includes(MESSAGES.early),
      )
      return at !== -1 && items.slice(at + 1).every((item) => item.role === 'custom')
    }
    const atWork = await until(
      async () => {
        if (name === 'command') return processesWith(MARK).length > 0
        return first === null
          ? await unanswered()
          : first.samples.some((sample) => sample.status === 'busy')
      },
      WORK_MS,
      100,
    )
    first?.stop()
    if (!atWork) {
      say(`FAIL the turn was never at work in ${WORK_MS / 1000} s: ${lastLines(cut.screen())}`)
      return { harness, name, ok: false, lines }
    }
    if (name === 'command') await sleep(2_000)
    // The harnesses that name their own conversation have named it by now.
    session = (await cut.conversation()) ?? session
    if (session === null) {
      say(`FAIL no conversation was recorded: ${lastLines(cut.screen())}`)
      return { harness, name, ok: false, lines }
    }

    // The quit: the window's process ends with its turn at work.
    await app.request('pane.kill', cut.pane)
    await until(() => cut.closed(), 30_000, 100)
    endCommand()
    await sleep(1_000)
    say(`cut: ${await tail(kind, session)}`)
    say(await reading(kind, session))
    keep(name, 'cut', session)

    // The daemon that starts again: the window on the conversation, as the app opens it.
    const before = (await cut.recorded()).length
    resumed = await openWindow(app, harness, {
      folder: FOLDER,
      id: `resume-cut-${harness}-${name}-again`,
      agent,
      resume: session,
    })
    const status = harness === 'claude' ? claudeStatus(session, { liveOnly: true }) : null
    const opened = Date.now()
    const drew = await resumed.still(READY_MS)
    say(
      `resumed: the window ${drew ? 'drew its screen' : 'did not hold still'} ${seconds(Date.now() - opened)} s after it opened`,
    )
    await sleep(WATCH_MS)
    status?.stop()
    if (status !== null) say(`its status: ${runs(status.samples, opened).join(' → ')}`)
    const after = await resumed.recorded()
    say(
      after.length === before
        ? `its record: unchanged, ${before} items`
        : `its record: ${before} → ${after.length} items; the new ones: ${after.slice(before).map(line).join(' | ')}`,
    )
    say(await reading(kind, session))
    say(`its screen: ${lastLines(resumed.screen())}`)

    // A message as the daemon pastes one: does the window take it, and alone?
    await app.request('pane.write_paste', { ...resumed.pane, body: await resumed.given(SECOND) })
    const pasted = Date.now()
    const arrived = await until(
      async () =>
        (await resumed.recorded()).findLast(
          (item) => item.role === 'user' && item.text.includes('AFTER'),
        ),
      60_000,
      500,
    )
    const answered = await until(
      async () =>
        (await resumed.recorded()).find(
          (item) => item.role === 'assistant' && item.text.trim() === 'AFTER',
        ),
      ANSWER_MS,
      500,
    )
    say(
      `a second message pasted: ${arrived ? `it arrived as ${JSON.stringify(arrived.text.slice(0, 120))}` : 'it never arrived'}${answered ? `, and was answered in ${seconds(Date.now() - pasted)} s` : ', and was not answered'}`,
    )
    // The turn that message began ended as any does: the record is at rest.
    await sleep(3_000)
    say(await reading(kind, session))
    if (harness === 'claude') {
      const closed = claudeRecord(session).find(
        (item) => item.type === 'assistant' && item.raw.includes('"model":"<synthetic>"'),
      )
      say(
        closed
          ? `what Claude wrote of the cut turn, once the second message came: ${closed.type}: ${closed.text}`
          : 'Claude wrote nothing of the cut turn',
      )
      keep(name, 'resumed', session)
    }
    return { harness, name, ok: true, lines }
  } finally {
    await cut.kill()
    await resumed?.kill()
    endCommand()
  }
}

const app = await startLiveApp()
const results = []
try {
  for (const harness of NAMES) {
    for (const name of CASES) results.push(await run(app, harness, name))
  }
} finally {
  await app.close()
  forgetClaudeFolder(join(process.env.HOME ?? '', '.consensflow-candidate', 'live', FOLDER))
}

for (const harness of NAMES) {
  process.stdout.write(
    `${harness}: ${versionOf(harness)}${harness === 'claude' ? `, model ${values.model}` : `, model ${HARNESSES[harness].model}`}\n`,
  )
}
for (const { harness, name, ok, lines } of results) {
  process.stdout.write(`\n== ${harness} ${name}${ok ? '' : ' (not run)'}\n`)
  for (const text of lines) process.stdout.write(`   ${text}\n`)
}
process.exit(results.every((result) => result.ok) ? 0 : 1)
