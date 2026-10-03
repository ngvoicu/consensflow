/**
 * Live: does Codex's broker name the thread of a window opened again on it?
 *
 * A Codex window takes its messages through ConsensFlow's broker, which knows
 * the thread the window shows from the TUI's own requests; a window it never
 * names reads as starting, and nothing is delivered to it. A member's window
 * opened again on its conversation for a reopened task took the task and
 * asked the chief, and the chief's answer never reached it (2026-10-03, on
 * the Mac's Codex 0.159.2 and Windows' 0.160.0): their TUI resumes with its
 * workspace roots null, where the broker looked for a list. This opens a
 * Codex window as the daemon does, waits for
 * the broker to name the thread its first message starts, closes it, opens
 * one again on that thread with a message, as the daemon does for a reopened
 * task, and needs the broker to name it again.
 *
 *   npm run live:codex-resume
 */
import { randomUUID } from 'node:crypto'
import { HARNESSES } from '../../evals/plan.mjs'
import { answers } from '../../hosts/lib/completion.js'
import { codexAdapter } from '../../src/adapters/codex.js'
import { recordState } from '../../src/adapters/shared.js'
import { sessionState } from '../../src/channels/codex.js'
import { paneArgv } from '../../src/harnesses.js'
import {
  ANSWER_MS,
  ENV,
  lastLines,
  liveFolder,
  sleep,
  startLiveApp,
  windowEnv,
} from './live-window.mjs'

/** How long a window may take to start, and its broker to name its thread. */
const NAMED_MS = 120_000

const workspace = liveFolder('codex-resume')
const adapter = codexAdapter({ env: windowEnv(workspace) })
const request = (resume, message) => ({
  launchId: `live-${randomUUID()}`,
  role: 'worker',
  directory: workspace,
  resume,
  message,
  agent: { model: HARNESSES.codex.model, effort: 'low' },
  instructions: 'You are a live test window: answer in one word, and run no tools.',
})

const app = await startLiveApp()
/** Opens a prepared window and waits for its broker to name a thread: the thread, or null. */
async function named(plan, id) {
  const pane = { id, generation: 1 }
  const opened = await app.request('pane.open', {
    ...pane,
    cwd: workspace,
    argv: paneArgv(plan.argv, ENV),
    env: plan.env,
    dropEnv: plan.dropEnv,
    size: { rows: 40, cols: 120 },
  })
  if (opened?.ok !== true) throw new Error(`Codex did not open: ${JSON.stringify(opened)}`)
  const end = Date.now() + NAMED_MS
  let thread = null
  while (thread === null && Date.now() < end) {
    await sleep(500)
    thread = (await sessionState(plan.launch.channel))?.sessionId ?? null
  }
  return { pane, thread, screen: () => lastLines(app.output(pane.id)) }
}

/** Until the thread's own record holds an answer (its turn is over and saved), or ANSWER_MS passed. */
async function answered(thread) {
  const end = Date.now() + ANSWER_MS
  while (Date.now() < end) {
    const read = await answers('codex', thread, windowEnv(workspace)).catch(() => null)
    if (read && !read.unknown && recordState(read).items.some((item) => item.role === 'assistant'))
      return true
    await sleep(1_000)
  }
  return false
}

let outcome
try {
  const fresh = await named(await adapter.prepare(request(null, 'Reply with only: ok')), 'resume-a')
  const saved = fresh.thread !== null && (await answered(fresh.thread))
  await app.request('pane.kill', fresh.pane).catch(() => {})
  if (fresh.thread === null) {
    outcome = { ok: false, detail: `a fresh window was never named: ${fresh.screen()}` }
  } else if (!saved) {
    outcome = { ok: false, detail: `a fresh window never answered: ${fresh.screen()}` }
  } else {
    await sleep(2_000)
    // As the daemon opens it for a reopened task: on the thread, with the message.
    const again = await named(
      await adapter.prepare(request(fresh.thread, 'Reply with only: again')),
      'resume-b',
    )
    await app.request('pane.kill', again.pane).catch(() => {})
    outcome =
      again.thread === fresh.thread
        ? { ok: true, detail: `opened again on ${fresh.thread}, and named it` }
        : {
            ok: false,
            detail: `opened again on ${fresh.thread}, named ${again.thread ?? 'nothing'}: ${again.screen()}`,
          }
  }
} finally {
  await app.close()
}
process.stdout.write(
  `${outcome.ok ? 'ok  ' : 'FAIL'} codex     a window opened again on its thread: ${outcome.detail}\n`,
)
process.exit(outcome.ok ? 0 : 1)
