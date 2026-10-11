/**
 * Live: a chief the app's quit cut in a turn, through the product.
 *
 * A real Claude Code is the chief of a project on the native daemon, and its one
 * member is the stand-in (no model). The human asks the chief to run a long
 * command; the app quits with the chief inside it (the daemon dies, then the
 * pane host, which ends every window); the app starts again on the same home,
 * and the daemon resumes the project: the chief's window opens on `--resume` of
 * the conversation the quit cut. Claude resumed waits at its prompt, status
 * idle, and does not go on with the turn (`npm run live:resume-cut`), so the
 * daemon has to read that window at rest: the lane reads idle, and what
 * waits for the chief goes in. A task is then given to the member, as the chief
 * would give it, and its result must reach the chief, which answers it.
 *
 *   npm run live:restart-cut                     Claude is the chief
 *   npm run live:restart-cut -- --chief pi       Pi is
 *
 * Needs `npm run build:bridge` and `npm run build:cf`. The exit code is 1 when
 * the chief's lane never read idle after the restart, or the result of the task
 * was not delivered to it.
 */
import { parseArgs } from 'node:util'
import { windowText } from '../rust-harness.mjs'
import { lastLines } from './live-window.mjs'
import { openRig, processesWith, seconds, sleep, until, versionOf } from './receipt-rig.mjs'

const { values } = parseArgs({ options: { chief: { type: 'string', default: 'claude' } } })
if (!['claude', 'pi'].includes(values.chief)) throw new Error(`no such chief: ${values.chief}`)

const MARK = 'cf-restart-cut-long-command'
const LONG = `node -e "setTimeout(() => {}, 300000)" ${MARK}`
const ASK = `Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n${LONG}`
/** How long the chief may take to be at its command, to read idle after the restart, and to be given a result. */
const WORK_MS = 180_000
const IDLE_MS = 60_000
const DELIVERED_MS = 180_000

function endCommand() {
  for (const found of processesWith(MARK)) {
    try {
      process.kill(found.pid, 'SIGKILL')
    } catch {}
  }
}

const rig = await openRig({ folder: 'restart-cut', workers: ['fake'], chief: values.chief })
const lines = []
const say = (text) => lines.push(text)
let ok = false
try {
  const chief = async () => rig.lane('chief')
  const ready = await until(async () => (await chief())?.activity?.state === 'idle', WORK_MS, 500)
  if (!ready)
    throw new Error(`the chief never read idle: ${JSON.stringify((await chief())?.activity)}`)
  say(`trust: ${rig.trust ?? 'none'}`)

  // The human asks the chief to run a long command.
  const { pane } = await chief()
  await rig.app.request('pane.write_paste', { ...pane, body: await windowText(ASK) })
  const atCommand = await until(() => processesWith(MARK).length > 0, WORK_MS, 200)
  if (!atCommand) {
    throw new Error(`the chief never ran its command: ${lastLines(rig.app.output(pane.id))}`)
  }
  await sleep(2_000)
  say(`before the quit: the chief's lane reads ${(await chief()).activity.state}, its command runs`)

  // The app's quit, and the app started again.
  const quitAt = Date.now()
  await rig.restart()
  endCommand()
  say(`quit and started again in ${seconds(Date.now() - quitAt)} s`)

  // The resumed chief: the lane, sampled until it reads idle or the wait is out.
  const samples = []
  const startedAt = Date.now()
  const idle = await until(
    async () => {
      const state = (await chief())?.activity?.state ?? 'no lane'
      if (samples.at(-1)?.state !== state) samples.push({ state, at: Date.now() })
      return state === 'idle'
    },
    IDLE_MS,
    500,
  )
  say(
    `after the restart the chief's lane reads: ${samples.map((sample) => `${sample.state} from +${seconds(sample.at - startedAt)} s`).join(' → ')}`,
  )
  if (!idle) {
    const reading = await chief()
    say(`it never read idle in ${IDLE_MS / 1000} s: ${JSON.stringify(reading?.activity)}`)
  }

  // A result for it: the member's task, given as the chief would give it.
  const number = await rig.give('fake', 'Reply with exactly: DONE')
  const given = Date.now()
  const delivered = await until(
    async () =>
      (await rig.inbox('chief')).find(
        (message) =>
          message.kind === 'result' &&
          message.taskNumber === number &&
          message.state === 'delivered',
      ),
    DELIVERED_MS,
    500,
  )
  const result = (await rig.inbox('chief')).find(
    (message) => message.kind === 'result' && message.taskNumber === number,
  )
  say(
    delivered
      ? `T-${number}'s result was delivered to the chief ${seconds(Date.now() - given)} s after the task was given`
      : `T-${number}'s result was never delivered in ${DELIVERED_MS / 1000} s: its state is ${result?.state ?? 'none'}; the chief's lane reads ${JSON.stringify((await chief())?.activity)}`,
  )
  const screen = rig.app.output((await chief()).pane.id)
  say(`the chief's screen: ${lastLines(screen)}`)
  ok = Boolean(idle && delivered)
} catch (cause) {
  say(`FAIL ${cause.message}`)
} finally {
  endCommand()
  await rig.close().catch(() => {})
}

process.stdout.write(`${values.chief}: ${versionOf(values.chief)}\n`)
for (const text of lines) process.stdout.write(`   ${text}\n`)
process.stdout.write(
  `${ok ? 'ok' : 'FAIL'}: ${ok ? 'a chief the quit cut reads at rest and is given what waits for it' : 'the chief the quit cut was not given what waited for it'}\n`,
)
process.exit(ok ? 0 : 1)
