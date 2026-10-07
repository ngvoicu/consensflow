/**
 * Live: a question a member asks through its harness's door, answered from the
 * board, is received there, once (the receipt and stop design, §1.2, §1.6,
 * §2.5, §2.7).
 *
 * Each harness whose member asks through a door opens as a member of a project
 * on the native daemon, one after another: Claude Code through its hook
 * (`cf hook claude`), Codex through the broker, OpenCode through its extension
 * (`hosts/lib/question-door.js`), Devin through its hook (`cf hook devin`). Pi
 * has no question tool, so no door. The member is given a task that has it ask
 * which colour to use; the check, as the chief, answers it from the board with
 * `cf answer`, and shows from what the daemon wrote that the answer was
 *
 *   - claimed by the door (`delivery.claimed`),
 *   - acknowledged `received: true` (`message.read` with the receipt
 *     `{"door": true}`: only an acknowledged claim leaves that proof),
 *   - never pasted (no `delivery.begun` or `delivery.confirmed` for it, no
 *     attempts, no second answer),
 *
 * and that its task went from waiting back to working, and the result carries
 * the answer. An answer the door fails to acknowledge comes back as text: the
 * paste is the failure this check tells apart from a receipt, whichever
 * harness's door it is.
 *
 * Claude's run also reads the window's status the way the daemon reads it
 * (`~/.claude/sessions/<pid>.json`) over the whole wait of the hook, which
 * `--hold` stretches: if it ever read idle there, the daemon's look at rest
 * would let an answer be pasted into a window that is mid-hook.
 *
 *   npm run live:door                       Claude, Codex, OpenCode, Devin, and Pi's line
 *   npm run live:door -- --harness opencode --hold 10
 *   npm run live:hook-status                Claude's status over a 90 s wait
 *   npm run live:door -- --harness fake     the rig's stand-in Claude: no model
 *
 * Needs `npm run build:bridge` and `npm run build:cf`. The exit code is 1 when
 * a door did not receive its answer as it should have.
 */
import { parseArgs } from 'node:util'
import { lastLines } from './live-window.mjs'
import {
  claudeStatus,
  forgetClaudeFolder,
  openRig,
  questionBrief,
  runs,
  seconds,
  sessionOf,
  sleep,
  until,
  versionOf,
} from './receipt-rig.mjs'

const { values } = parseArgs({
  options: {
    harness: { type: 'string', multiple: true },
    hold: { type: 'string' },
    model: { type: 'string', multiple: true },
    answer: { type: 'string', default: 'blue' },
  },
})
const HARNESSES = values.harness ?? ['claude', 'codex', 'opencode', 'devin', 'pi']
/** Which have no door, and why: said, not run. */
const NO_DOOR = { pi: 'Pi has no question tool, so it has no door' }
/** How long the chief takes to answer, so the door is mid-wait when the answer comes. */
const holdFor = (name) => Number(values.hold ?? (name === 'claude' ? 20 : 8)) * 1000
const MODELS = Object.fromEntries((values.model ?? []).map((pair) => pair.split('=')))
/** How long a member may take to ask, and to finish once its answer is in. */
const ASK_MS = 420_000
const DONE_MS = 300_000
const ENDED = ['done', 'accepted', 'failed', 'cancelled']

const when = (text) => Date.parse(text)

/** What the daemon wrote of the answer and its task: whether it was received at the door, once. */
function judge(events, thread, question, answer) {
  const about = (kind) =>
    events.filter((event) => event.kind === kind && event.data?.message === answer.id)
  const [claimed, read, begun, confirmed, unclaimed] = [
    'delivery.claimed',
    'message.read',
    'delivery.begun',
    'delivery.confirmed',
    'delivery.unclaimed',
  ].map(about)
  const number = thread.number
  const moves = events
    .filter((event) => event.kind === 'task.state' && event.data?.task === number)
    .map((event) => event.data.to)
  const answers = thread.messages.filter(
    (message) => message.kind === 'answer' && message.replyTo === question.id,
  )
  const asked = thread.messages.filter((message) => message.kind === 'question')
  const result = thread.messages.findLast((message) => message.kind === 'result')
  const waited = moves.indexOf('waiting')
  const resumed = waited === -1 ? -1 : moves.indexOf('working', waited)
  const [claim, receipt] = [claimed[0], read[0]]
  const plural = (count, word) => `${count} ${word}${count === 1 ? '' : 's'}`
  return {
    receivedAt: receipt ? when(receipt.at) : null,
    // What happened to the claim before the receipt: seen, not a failure, for the answer was not pasted.
    notes: unclaimed.map(
      (event) =>
        `the claim on m-${answer.id} was given up ${seconds(when(event.at) - when((claimed.findLast((found) => found.n < event.n) ?? claim).at))} s after it was made (event ${event.n}: "${event.data.because}")${receipt ? `, and the door's receipt still made it read ${seconds(when(receipt.at) - when(event.at))} s later (event ${receipt.n})` : ''}`,
    ),
    checks: [
      {
        name: 'claimed',
        ok: claimed.length >= 1,
        detail: claim
          ? `m-${answer.id} claimed by the door ${seconds(when(claim.at) - when(answer.createdAt))} s after it was given (event ${claim.n}${claimed.length > 1 ? `; claimed ${claimed.length} times` : ''})`
          : `no delivery.claimed event for m-${answer.id}`,
      },
      {
        name: 'acknowledged received: true',
        ok:
          answer.state === 'read' &&
          answer.receipt?.door === true &&
          read.length === 1 &&
          receipt.data.receipt?.door === true,
        detail: `m-${answer.id} is ${answer.state}, receipt ${JSON.stringify(answer.receipt)}, ${plural(read.length, 'message.read event')}${receipt && claim ? ` (event ${receipt.n}, ${seconds(when(receipt.at) - when(claim.at))} s after the claim)` : ''}`,
      },
      {
        name: 'never pasted',
        ok:
          begun.length === 0 &&
          confirmed.length === 0 &&
          answer.attempts === 0 &&
          answers.length === 1,
        detail: `m-${answer.id}: ${begun.length} delivery.begun, ${confirmed.length} delivery.confirmed, attempts ${answer.attempts}; ${plural(answers.length, 'answer')} to m-${question.id}; ${plural(asked.length, 'question')} on T-${number}`,
      },
      {
        name: 'waiting to working',
        ok: waited !== -1 && resumed !== -1,
        detail: `T-${number}: ${moves.join(' → ')}`,
      },
      {
        name: 'result carries the answer',
        ok: Boolean(result?.body.toLowerCase().includes(values.answer)),
        detail: result ? JSON.stringify(result.body.slice(0, 100)) : `no result on T-${number}`,
      },
    ],
  }
}

async function run(name) {
  const rig = await openRig({ folder: `door-${name}`, workers: [name], model: MODELS })
  const lines = []
  try {
    if (rig.trust) lines.push(`trust: ${rig.trust}`)
    const number = await rig.give(name, questionBrief(name))
    const asked = (thread) => thread.messages.find((message) => message.kind === 'question')
    // The question, or the end of the task without one: a member that answers on its own
    // never used its door, and waiting for it would only run out the clock.
    const gone = (thread) => (ENDED.includes(thread.state) ? thread : null)
    const first = await until(
      async () => {
        const thread = await rig.thread(number)
        return asked(thread) ?? gone(thread)
      },
      ASK_MS,
      500,
    )
    const question = first?.kind === 'question' ? first : null
    const window = await rig.windowOf(name)
    if (!question) {
      const thread = await rig.thread(number)
      const result = thread.messages.findLast((message) => message.kind === 'result')
      lines.push(
        first
          ? `FAIL T-${number} ended (${thread.state}) and the member never put a question on the board; its result: ${JSON.stringify(result?.body.slice(0, 300))}`
          : `FAIL the member never put its question on the board in ${ASK_MS / 1000} s`,
      )
      lines.push(`     its window: ${lastLines(rig.app.output(window?.pane?.id ?? ''))}`)
      return { ok: false, lines }
    }
    const session = sessionOf(window.open)
    const status = name === 'claude' ? claudeStatus(session) : null
    await sleep(holdFor(name))
    const given = await rig.asChief(['answer', `m-${question.id}`, values.answer])
    if (given.code !== 0) {
      status?.stop()
      lines.push(`FAIL cf answer: ${JSON.stringify(given)}`)
      return { ok: false, lines }
    }
    // Its receipt (or the paste that stands in for it), then the member's result.
    const finished = await until(
      async () => ENDED.includes((await rig.thread(number)).state),
      DONE_MS,
      500,
    )
    status?.stop()
    const thread = await rig.thread(number)
    const answer = thread.messages.find(
      (message) => message.kind === 'answer' && message.replyTo === question.id,
    )
    if (!answer) {
      lines.push('FAIL no answer in the thread after cf answer')
      return { ok: false, lines }
    }
    const events = rig.events()
    const verdict = judge(events, thread, question, answer)
    lines.push(
      `question m-${question.id} on T-${number} put at ${question.createdAt.slice(11, 23)}, answered "${values.answer}" ${seconds(when(answer.createdAt) - when(question.createdAt))} s later; T-${number} ${finished ? 'ended' : `still ${thread.state} after ${DONE_MS / 1000} s`}`,
    )
    for (const check of verdict.checks) {
      lines.push(`${check.ok ? 'ok  ' : 'FAIL'} ${check.name}: ${check.detail}`)
    }
    for (const note of verdict.notes) lines.push(`note ${note}`)
    let ok = verdict.checks.every((check) => check.ok)
    lines.push('     the sequence the daemon wrote (events.jsonl):')
    for (const event of events) {
      const ours =
        [answer.id, question.id].includes(event.data?.message) ||
        (event.kind === 'task.state' && event.data?.task === number) ||
        (event.kind.startsWith('window.') && event.participant === window.handle) ||
        event.kind.startsWith('delivery.held')
      if (ours) {
        const said = event.data ?? {
          state: event.state,
          reason: event.reason,
          message: event.message,
        }
        lines.push(
          `       ${event.n} ${event.at.slice(11, 23)} ${event.kind} ${JSON.stringify(said)}`,
        )
      }
    }
    if (!ok) lines.push(`     the member's window: ${lastLines(rig.app.output(window.pane.id))}`)
    if (status) {
      // The hook's whole wait: from its question to the answer's receipt, or to the paste that replaced it.
      const from = when(question.createdAt)
      const to = verdict.receivedAt ?? Date.now()
      const wait = status.samples.filter((sample) => sample.at >= from && sample.at <= to)
      const rest = wait.filter((sample) => sample.status === 'idle' || sample.status === 'shell')
      lines.push(
        `${rest.length === 0 ? 'ok  ' : 'FAIL'} claude status over the hook's wait (${seconds(to - from)} s, ${wait.length} reads of sessions/<pid>.json, session ${session}): ${rest.length === 0 ? 'never idle' : `idle in ${rest.length} reads`}`,
      )
      for (const line of runs(wait, from)) lines.push(`       ${line}`)
      // What the daemon itself saw of the window over the same wait: its own trace.
      const saw = events.filter(
        (event) =>
          event.kind === 'window.activity' &&
          event.participant === window.handle &&
          when(event.at) >= from - 60_000 &&
          when(event.at) <= to,
      )
      lines.push(
        `     the daemon's reading of the window (window.activity): ${saw.map((event) => `${event.state} at ${seconds(when(event.at) - from)} s`).join(', ') || 'none'}`,
      )
      if (rest.length > 0) ok = false
    }
    return { ok, lines }
  } finally {
    await rig.close()
    if (name === 'claude') forgetClaudeFolder(rig.workspace)
  }
}

const results = []
for (const name of HARNESSES) {
  const before = versionOf(name)
  let result
  if (NO_DOOR[name]) result = { ok: true, lines: [`left out: ${NO_DOOR[name]}`] }
  else {
    try {
      result = await run(name)
    } catch (cause) {
      result = { ok: false, lines: [`FAIL ${cause.stack ?? cause}`] }
    }
  }
  // A harness may update itself while it runs: the version is named before and after.
  const after = versionOf(name)
  const version = after === before ? before : `${before}, then ${after}`
  results.push({ name, version, ...result })
  process.stdout.write(`\n== ${name} ${version}: ${result.ok ? 'ok' : 'FAILED'}\n`)
  for (const line of result.lines) process.stdout.write(`   ${line}\n`)
}
process.stdout.write(
  `\n${results.map((result) => `${result.ok ? 'ok  ' : 'FAIL'} ${result.name} ${result.version}`).join('\n')}\n`,
)
process.exit(results.every((result) => result.ok) ? 0 : 1)
