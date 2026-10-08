/**
 * Live: does Codex's broker name the thread of a window opened again on it?
 *
 * A Codex window takes its messages through ConsensFlow's broker, which knows
 * the thread the window shows from the TUI's own requests; a window it never
 * names reads as starting, and nothing is delivered to it. A member's window
 * opened again on its conversation for a reopened task took the task and
 * asked the chief, and the chief's answer never reached it (2026-10-03, on
 * the Mac's Codex 0.159.2 and Windows' 0.160.0): their TUI resumes with its
 * workspace roots null, where the broker looked for a list.
 *
 * This is the daemon's own path to it, on the rig of the live receipt checks
 * (`npm run live:door`): a Codex member of a project on the native daemon is
 * given a task and ends it, and the daemon closes its window with the task.
 * Then a follow-up for the same window (`cf task add --after`) has the daemon
 * open the window again on its conversation, with the follow-up as its first
 * message. It needs the window to open on the thread the first one ran in
 * (`codex resume <thread>`), the follow-up to reach it, and its result to come
 * back: a broker that does not name the thread again leaves the follow-up
 * undelivered.
 *
 *   npm run live:codex-resume
 *
 * Needs `npm run build:bridge` and `npm run build:cf`. The exit code is 1 when
 * the window opened again did not take its message.
 */
import { lastLines } from './live-window.mjs'
import { openRig, seconds, sleep, until } from './receipt-rig.mjs'

/** How long a member may take to end a task, and a window to close once it has. */
const DONE_MS = 300_000
const CLOSED_MS = 60_000
const ENDED = ['done', 'accepted', 'failed', 'cancelled']

const rig = await openRig({ folder: 'codex-resume', workers: ['codex'] })
let outcome
try {
  // A fresh window: the task opens one, and ends with the member's result.
  const first = await rig.give('codex', 'Reply with only: ok')
  const opened = await until(() => rig.windowOf('codex'), DONE_MS, 500)
  const ended = await until(
    async () => ENDED.includes((await rig.thread(first)).state),
    DONE_MS,
    500,
  )
  if (!opened || !ended) {
    outcome = {
      ok: false,
      detail: `a fresh window never ${opened ? 'ended its task' : 'opened'} in ${DONE_MS / 1000} s: ${lastLines(rig.app.output(opened?.pane?.id ?? ''))}`,
    }
  } else {
    // The daemon closes a window with its task; one it left open is closed here, so that
    // the follow-up has to open it again.
    const exited = () =>
      rig.app.exits.some(
        (exit) => exit.id === opened.pane.id && exit.generation === opened.pane.generation,
      )
    if (!(await until(exited, CLOSED_MS, 500))) {
      await rig.app.request('pane.kill', opened.pane).catch(() => {})
      await until(exited, CLOSED_MS, 500)
    }
    await sleep(2_000)
    const named = rig
      .events()
      .filter((event) => event.kind === 'conversation.bound')
      .map((event) => event.data.nativeSession)
    // As the daemon opens it for a reopened task: on the thread, with the message.
    const started = Date.now()
    const openedBefore = rig.app.openFrames.length
    const follow = await rig.asChief([
      'task',
      'add',
      '--after',
      `T-${first}`,
      '--json',
      'Reply with only: again',
    ])
    const number = JSON.parse(follow.stdout || 'null')?.task?.number
    if (follow.code !== 0 || number === undefined) {
      outcome = { ok: false, detail: `cf task add --after T-${first}: ${JSON.stringify(follow)}` }
    } else {
      const done = await until(
        async () => ENDED.includes((await rig.thread(number)).state),
        DONE_MS,
        500,
      )
      // The window the follow-up opened is the same session's: its pane has the same name.
      const again = rig.app.openFrames
        .slice(openedBefore)
        .find((frame) => frame.id === opened.pane.id)
      const argv = again?.argv ?? []
      const thread = argv[argv.indexOf('resume') + 1]
      const result = (await rig.thread(number)).messages.findLast((m) => m.kind === 'result')
      if (!argv.includes('resume') || !named.includes(thread)) {
        outcome = {
          ok: false,
          detail: `the window was not opened again on a thread the first one ran in (named: ${named.join(', ') || 'none'}): ${argv.join(' ') || 'no window opened'}`,
        }
      } else if (!done || !result?.body.toLowerCase().includes('again')) {
        outcome = {
          ok: false,
          detail: `opened again on ${thread}, but its message was not taken and answered in ${DONE_MS / 1000} s (T-${number}: ${(await rig.thread(number)).state}): ${lastLines(rig.app.output(opened.pane.id))}`,
        }
      } else {
        outcome = {
          ok: true,
          detail: `opened again on ${thread}, took its message and answered in ${seconds(Date.now() - started)} s`,
        }
      }
    }
  }
} finally {
  await rig.close()
}
process.stdout.write(
  `${outcome.ok ? 'ok  ' : 'FAIL'} codex     a window opened again on its thread: ${outcome.detail}\n`,
)
process.exit(outcome.ok ? 0 : 1)
