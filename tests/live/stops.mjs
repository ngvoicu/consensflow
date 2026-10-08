/**
 * Live: a stop, as the daemon presses it (the receipt and stop design, §1.3,
 * §5.6). A pause of a task asks a stop of its window; at a window at work the
 * daemon presses the harness's interrupt (Escape) into its pane, again three
 * seconds later, three rounds in all, and a window that reads at rest has paid
 * the stop with no key. Each case opens a real member on the native daemon,
 * gives it a task, pauses the task as the human's Pause does (`task.pause`),
 * and shows what the daemon pressed (every `pane.input` of an Escape it sent),
 * what the command, the hook and the turn did, and what the window's own
 * status and record say, beside the daemon's own reading of the window.
 *
 * Claude Code's cases:
 *   command   Escape into Claude inside a long command: the command and the
 *             turn stop, and nothing more is pressed once it is at rest.
 *   early     Escape right after the task's words went in, before Claude's
 *             first output. Claude writes no record of such a stop and puts
 *             the words back in its input box, and out of the conversation it
 *             answers from; the daemon reads the window at rest by its own
 *             press, and the ledger keeps the brief for the window again.
 *             Then the resume of the task: its words must land as a message
 *             of their own, not after the old text, carry the brief, and the
 *             task must end with its result, which holds the brief's answer.
 *   hook      Escape at its question hook (the member's question waits on the
 *             board): what happens to the hook's wait and the turn; then the
 *             answer the chief gives late, and the resume that carries it.
 *   escape    The same hook, with no pause: one Escape into the pane, as the
 *             daemon presses it, and what that does to the hook, the turn and
 *             the question the board still holds.
 *   slowhook  Escape while a hook of the project's own, that ignores the signals
 *             Claude ends a hook with, outlives Claude's turn (`--hook` says
 *             which: Stop, PreToolUse or UserPromptSubmit): the stop the daemon
 *             could take for ignored. When Claude stopped, the resume of the
 *             task, and its result (the hook runs again in the turn after it).
 * Any harness (`--harness`):
 *   stuck     A window that may not stop, pressed until the stop is exhausted
 *             (the human and the requester are told): a command that ignores
 *             INT, TERM and HUP is run, then the task paused. When the harness
 *             stops anyway, it says so, and at which press.
 *
 *   npm run live:stops                          every case, `stuck` on OpenCode
 *   npm run live:stops -- --case command --case hook
 *   npm run live:stops -- --case stuck --harness codex
 *
 * `--keep <folder>` copies each Claude transcript there. Needs `npm run
 * build:bridge` and `npm run build:cf`. The exit code is 1 when a case showed
 * what the design says it must not.
 */
import { copyFileSync, mkdirSync } from 'node:fs'
import { basename, join } from 'node:path'
import { parseArgs } from 'node:util'
import { lastLines } from './live-window.mjs'
import {
  claudeRecord,
  claudeSettlement,
  claudeStatus,
  claudeTranscript,
  forgetClaudeFolder,
  openRig,
  processesWith,
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
    case: { type: 'string', multiple: true },
    hook: { type: 'string', default: 'stop' },
    harness: { type: 'string', default: 'opencode' },
    // A folder to copy each watched window's Claude transcript into, for a look at what it wrote.
    keep: { type: 'string' },
  },
})
const CASES = values.case ?? ['command', 'early', 'hook', 'escape', 'slowhook', 'stuck']
/** How long a member may take to reach its command or ask, and a stop to take hold. */
const WORK_MS = 300_000
const STOP_MS = 40_000
/** How long the words that resume a task may take to go into its window. */
const RESUME_MS = 90_000
/** How long the task may then take to end with its result (a hook of the project's may run 100 s of it). */
const RESULT_MS = 420_000
/** How long the stop of a window that may not stop is watched. */
const STUCK_WATCH_MS = 150_000
/** How long a window at rest is watched for a press it should not get. */
const REST_MS = 12_000
const ENDED = ['done', 'accepted', 'failed', 'cancelled']
const MARK = 'cf-stops-live-long-command'
const LONG = `node -e "setTimeout(() => {}, 300000)" ${MARK}`

const at = (stamp, from) => `+${seconds(stamp - from)} s`

/** The conversations watched, to keep a copy of each transcript (`--keep`) before the rig clears them. */
const watchedSessions = []

/** Ends the rig; first the transcripts of the windows it watched are copied where `--keep` says, if it does. */
async function closeRig(rig) {
  for (const [label, session] of watchedSessions.splice(0)) {
    const file = claudeTranscript(session)
    if (values.keep && file !== null) {
      mkdirSync(values.keep, { recursive: true })
      copyFileSync(file, join(values.keep, `${label}-${session}.jsonl`))
    }
  }
  await rig.close()
  forgetClaudeFolder(rig.workspace)
}

/** A member's window and what is watched of it from here on: its status, and the Escapes the daemon presses. */
async function watch(rig, name) {
  const window = await rig.windowOf(name)
  const session = sessionOf(window.open)
  watchedSessions.push([basename(rig.workspace), session])
  return {
    window,
    session,
    status: claudeStatus(session),
    escapes: rig.watchEscapes(window.pane.id),
  }
}

/** The human's Pause of the task, and when it was made. */
async function pause(rig, number) {
  const made = Date.now()
  const paused = await rig.app.requestNode('task.pause', { project: rig.project, task: number })
  return { made, paused }
}

/**
 * The last `last` records of the window's conversation, as lines with their
 * time relative to `from`; the first line says how many records it has.
 */
function recordSince(session, from, last = 14) {
  const items = claudeRecord(session)
  return [
    `${items.length} records in the transcript of ${session}`,
    ...items
      .slice(-last)
      .map(
        (item) =>
          `${at(item.at, from)} ${item.type}: ${item.text.replace(/\s+/g, ' ').slice(0, 220)}`,
      ),
  ]
}

/**
 * How the daemon's record reader (`cf_harness::records`, before the Claude
 * adapter reads the window by more than its record) reads the window's
 * transcript now. The native daemon's look at the window needs Claude's status
 * idle and the record settled, as this reader says, but it departs from it in
 * two cases, and reads the window at rest where this one says "not settled":
 * an interrupt record that names no message (the `slowhook` case), and a turn
 * the daemon's own press stopped before Claude wrote a word (the `early` case,
 * which has no record of the stop). The daemon's own reading is the
 * `window.activity` line of each case.
 */
async function settlementLine(session) {
  const read = await claudeSettlement(session)
  return read === null
    ? 'the record reader finds no transcript'
    : `the record reader (cf_harness::records, before the adapter's own look) reads its transcript as ${read.settled ? 'SETTLED' : 'NOT settled'} (settlement ${read.settlement}; ${read.items} items, the last ${JSON.stringify(read.last)})`
}

/** What a window's screen shows now, its last lines. */
const screenOf = (rig, window) => lastLines(rig.app.output(window.pane.id))

/**
 * The long command's own processes: the `node` it runs, not the window whose
 * launch text (the task, as an argument of Claude) names it too.
 */
const commandProcesses = () =>
  processesWith(MARK).filter((found) => /^(\S*\/)?node\s+-e\s/.test(found.command))

/** Processes as `pid (parent) command`. */
const shown = (found) => found.map((one) => `${one.pid} (${one.ppid}) ${one.command.slice(0, 110)}`)

/** The last records the transcript holds, each with the start of its line: what Claude wrote of a stop. */
const rawTail = (session, from, last = 6) =>
  claudeRecord(session)
    .slice(-last)
    .map((item) => `${at(item.at, from)} ${item.raw.replace(/\s+/g, ' ').slice(0, 260)}`)

/** The window's interrupt, as Claude's record writes it, if it wrote one since `from`. */
const interrupted = (session, from) =>
  claudeRecord(session).find(
    (item) => item.at >= from - 500 && /Request interrupted by user/.test(item.text),
  )

/** `unstopped` on the member's lane, as the board shows it. */
async function unstopped(rig, name) {
  return (await rig.lane(rig.members[name].agent))?.unstopped ?? null
}

/** The stop's own events: its pause, and the daemon's notes that it was ignored. */
function stopEvents(rig, number) {
  return rig
    .events()
    .filter(
      (event) =>
        (event.kind === 'task.state' && event.data?.task === number) ||
        event.kind === 'message.sent' ||
        event.kind.startsWith('delivery.') ||
        event.kind === 'message.carried',
    )
    .map((event) => {
      // A window's own lines (delivery.held) carry their fields beside the kind, a ledger event's in data.
      const { n, at: when, kind, data, project: _project, ...rest } = event
      return `${n} ${when.slice(11, 23)} ${kind} ${JSON.stringify(data ?? rest)}`
    })
}

/**
 * A Claude member given `brief`, paused once `ready` says it is where the
 * case wants it: what the daemon pressed, whether the turn and the command
 * stopped, what Claude's record and status say of it, and whether the daemon
 * says (to the board, the human, the requester) that the window did not stop.
 * `ready(rig, number, lines)` waits for that place and returns whether it was
 * reached. The design (§1.3): a window that stopped is paid at rest with no
 * key, and is never reported as one that did not. `recordless` is a stop
 * before a word of Claude's answer, which it writes no interrupt record of.
 */
async function stopOf(label, brief, ready, { command, after = null, recordless = false }) {
  const rig = await openRig({
    folder: `stops-${label}`,
    workers: ['claude'],
  })
  const lines = []
  try {
    const number = await rig.give('claude', brief)
    if (!(await ready(rig, number, lines))) return { ok: false, lines }
    const watched = await watch(rig, 'claude')
    const { made, paused } = await pause(rig, number)
    lines.push(
      `task.pause ${paused.ok ? `made T-${number} ${paused.task.state}` : `refused: ${JSON.stringify(paused)}`}`,
    )
    let goneAt = null
    const screens = []
    const idleAt = await until(
      () => {
        if (command && goneAt === null && commandProcesses().length === 0) goneAt = Date.now()
        return watched.status.samples.find((s) => s.at > made && s.status === 'idle')?.at ?? null
      },
      STOP_MS,
      100,
    )
    await sleep(1_500)
    screens.push(`at ${at(Date.now(), made)}: ${screenOf(rig, watched.window)}`)
    if (command) {
      await until(() => goneAt !== null || commandProcesses().length === 0, 5_000, 100)
      goneAt ??= commandProcesses().length === 0 ? Date.now() : null
    }
    await sleep(REST_MS)
    screens.push(`at ${at(Date.now(), made)}: ${screenOf(rig, watched.window)}`)
    watched.status.stop()
    watched.escapes.stop()
    const presses = watched.escapes.seen
    // A press more than a look after the window read idle is one at a window that had stopped.
    const lateKeys = presses.filter((press) => idleAt !== null && press > idleAt + 1_500)
    const ended = interrupted(watched.session, made)
    // Claude's own words of the turn it was stopped in: none, where it stopped before the first.
    const spoke = claudeRecord(watched.session).filter(
      (item) => item.type === 'assistant' && item.at >= made - 500,
    )
    // When the daemon first read the window at rest after the pause: where the stop was paid.
    const restedAt = rig
      .events()
      .find(
        (event) =>
          event.kind === 'window.activity' &&
          event.participant === watched.window.handle &&
          event.state === 'idle' &&
          Date.parse(event.at) > made,
      )
    const stuck = await unstopped(rig, 'claude')
    const told = (await rig.inbox(undefined)).filter((message) =>
      /did not stop for T-/.test(message.body),
    )
    const task = await rig.thread(number)
    const checks = [
      [
        'the daemon pressed Escape',
        presses.length >= 1,
        `${presses.length} Escape${presses.length === 1 ? '' : 's'}: ${presses.map((press) => at(press, made)).join(', ') || 'none'}`,
      ],
      ...(command
        ? [
            [
              'the command stopped',
              goneAt !== null,
              goneAt ? `its processes were gone at ${at(goneAt, made)}` : 'still running',
            ],
          ]
        : []),
      [
        'the turn stopped',
        idleAt !== null,
        idleAt ? `status read idle at ${at(idleAt, made)}` : `no idle in ${STOP_MS / 1000} s`,
      ],
      recordless
        ? [
            'Claude stopped before a word of its answer',
            spoke.length === 0,
            spoke.length === 0
              ? `its record holds no word of its answer and ${ended ? `an interrupt record ("${ended.text.slice(0, 60)}")` : 'no interrupt record'}: it says nothing of the stop`
              : `${spoke.length} records of its answer: ${JSON.stringify(spoke[0].text.slice(0, 80))}`,
          ]
        : [
            'the record says it was interrupted',
            Boolean(ended),
            ended
              ? `"${ended.text.slice(0, 80)}" at ${at(ended.at, made)}`
              : 'no interrupt record in its transcript',
          ],
      [
        'the daemon read the window at rest, and paid the stop there',
        Boolean(restedAt),
        restedAt
          ? `its reading of the window turned idle at ${at(Date.parse(restedAt.at), made)}`
          : 'its reading of the window never turned idle',
      ],
      [
        'nothing is pressed once it has stopped',
        lateKeys.length === 0,
        `${lateKeys.length} Escapes more than a look after the status read idle`,
      ],
      [
        'it is not said to have ignored the stop',
        stuck === null && told.length === 0,
        `unstopped on its lane: ${JSON.stringify(stuck)}; notes to the human: ${told.length}${told[0] ? ` ("${told[0].body.slice(0, 90)}…")` : ''}`,
      ],
      ['the task is paused', task.state === 'paused', `T-${number} is ${task.state}`],
    ]
    for (const [name, ok, detail] of checks)
      lines.push(`${ok ? 'ok  ' : 'FAIL'} ${name}: ${detail}`)
    lines.push(
      `     status over the stop: ${runs(
        watched.status.samples.filter((s) => s.at >= made - 2_000 && s.at <= made + 8_000),
        made,
      ).join('; ')}`,
    )
    const saw = rig
      .events()
      .filter(
        (event) =>
          event.kind === 'window.activity' &&
          event.participant === watched.window.handle &&
          Date.parse(event.at) >= made - 3_000,
      )
    lines.push(
      `     the daemon's reading of the window (window.activity) from just before: ${saw.map((event) => `${event.state} at ${at(Date.parse(event.at), made)}`).join(', ') || 'no change'}`,
    )
    if (command)
      lines.push(
        `     the command's processes now: ${shown(commandProcesses()).join('; ') || 'none'}`,
      )
    for (const screen of screens) lines.push(`     its screen ${screen}`)
    lines.push(`     ${await settlementLine(watched.session)}`)
    const kept = claudeRecord(watched.session)
    const kinds = {}
    for (const item of kept) kinds[item.type] = (kinds[item.type] ?? 0) + 1
    lines.push(
      `     its transcript holds ${kept.length} records: ${Object.entries(kinds)
        .map(([type, count]) => `${type} ×${count}`)
        .join(
          ', ',
        )}; the last user message: ${JSON.stringify(kept.findLast((item) => item.type === 'user')?.text.slice(0, 80) ?? null)}`,
    )
    lines.push('     the last records of its transcript (the start of each line):')
    for (const line of rawTail(watched.session, made)) lines.push(`       ${line}`)
    // Where the case goes on: the stop's consequence for the task.
    const goes = after ? await after(rig, number, lines, watched) : true
    lines.push('     what the daemon wrote:')
    for (const line of stopEvents(rig, number)) lines.push(`       ${line}`)
    return { ok: checks.every(([, ok]) => ok) && goes, lines }
  } finally {
    await closeRig(rig)
  }
}

/** Escape into Claude inside a long command: it is paused once the command runs. */
const commandCase = () =>
  stopOf(
    'command',
    `Run exactly this one shell command, then stop: ${LONG}`,
    async (_rig, number, lines) => {
      const running = await until(() => commandProcesses().length > 0, WORK_MS, 250)
      if (!running) {
        lines.push(`FAIL the member never ran the command in ${WORK_MS / 1000} s`)
        return false
      }
      await sleep(2_000)
      lines.push(
        `T-${number}: the member is inside \`${LONG}\` (${shown(commandProcesses()).join('; ')})`,
      )
      return true
    },
    { command: true },
  )

/**
 * The human resumes the paused task: its words (the resume carrier) must go
 * into the window once it reads at rest, within a minute and a half; when they
 * did not, why, from what the daemon said of the message it held. They must
 * land as a message of their own, not after the text a window that stopped
 * before its first word puts back in its input box, and the task must end with
 * its result. `expect` says more of a window that took its brief back out of
 * its conversation when it stopped: the words that resume the task carry the
 * `brief`, and the result holds the `answer` to it. Returns whether all of it
 * happened.
 */
async function resumeAfter(rig, number, lines, watched, expect = {}) {
  const resumedAt = Date.now()
  const resumed = await rig.app.requestNode('task.resume', { project: rig.project, task: number })
  lines.push(
    `task.resume ${resumed.ok ? `made T-${number} ${resumed.task.state}` : `refused: ${JSON.stringify(resumed)}`}`,
  )
  const since = (event) => Date.parse(event.at) >= resumedAt
  const pasted = await until(
    () => rig.events().find((event) => event.kind === 'delivery.begun' && since(event)),
    RESUME_MS,
    500,
  )
  const held = rig.events().filter((event) => event.kind === 'delivery.held' && since(event))
  const reasons = [...new Set(held.map((event) => event.reason))]
  const lane = await rig.lane(rig.members.claude.agent)
  lines.push(
    `${pasted ? 'ok  ' : 'FAIL'} the resume words went into the window: ${pasted ? `delivery.begun of m-${pasted.data.message} at +${seconds(Date.parse(pasted.at) - resumedAt)} s after the resume` : `nothing was pasted in ${RESUME_MS / 1000} s after the resume`}; its lane: activity ${JSON.stringify(lane?.activity)}, unstopped ${JSON.stringify(lane?.unstopped ?? null)}, holding ${lane?.holding}${reasons.length ? `; held because: ${reasons.join(' | ')}` : ''}`,
  )
  if (!pasted) return false
  // What Claude's record holds of them: one message, with no other message's header in it.
  const marker = `[ConsensFlow m-${pasted.data.message} `
  const record = await until(
    () =>
      claudeRecord(watched.session).find(
        (item) => item.type === 'user' && item.text.includes(marker),
      ),
    60_000,
    250,
  )
  const headers = record?.text.match(/\[ConsensFlow m-\d+ /g) ?? []
  const alone = Boolean(record) && headers.length === 1
  lines.push(
    `${alone ? 'ok  ' : 'FAIL'} the resume words landed as a message of their own: ${record ? JSON.stringify(record.text.slice(0, 300)) : 'its record never held them'}`,
  )
  // A brief the window took back is not in the conversation the resume is answered in: it is in the resume.
  const carried = expect.brief === undefined || Boolean(record?.text.includes(expect.brief))
  if (expect.brief !== undefined) {
    lines.push(
      `${carried ? 'ok  ' : 'FAIL'} the resume carried the task's brief: ${carried ? 'its message holds the brief whole' : 'its message does not hold it'}`,
    )
  }
  const finished = await until(
    async () => ENDED.includes((await rig.thread(number)).state),
    RESULT_MS,
    1_000,
  )
  const thread = await rig.thread(number)
  const result = thread.messages.findLast((message) => message.kind === 'result')
  const done = Boolean(finished) && ['done', 'accepted'].includes(thread.state) && Boolean(result)
  lines.push(
    `${done ? 'ok  ' : 'FAIL'} the task ended with its result: T-${number} is ${thread.state}${result ? `, its result ${JSON.stringify(result.body.slice(0, 200))}` : ' and holds none'}`,
  )
  const answered = expect.answer === undefined || Boolean(result?.body.includes(expect.answer))
  if (expect.answer !== undefined) {
    lines.push(
      `${answered ? 'ok  ' : 'FAIL'} the result carries the task's own answer: ${answered ? JSON.stringify(expect.answer) : `${JSON.stringify(expect.answer)} is not in it: the worker did not have its brief`}`,
    )
  }
  return alone && carried && done && answered
}

/** What the brief of the `early` case asks, and what only a worker that has the brief can answer. */
const EARLY_ANSWER = 'EARLY-4117'
const EARLY_BRIEF = `Reply with exactly one line: ${EARLY_ANSWER}`

/** Escape into Claude right after its task's words went in: paused as soon as the task is working. */
const earlyCase = () =>
  stopOf(
    'early',
    EARLY_BRIEF,
    async (rig, number, lines) => {
      const working = await until(
        async () => (await rig.thread(number)).state === 'working',
        WORK_MS,
        50,
      )
      if (!working) lines.push('FAIL the task never began')
      else lines.push(`T-${number} is working: its first turn has just begun`)
      return Boolean(working)
    },
    {
      command: false,
      recordless: true,
      after: (rig, number, lines, watched) =>
        resumeAfter(rig, number, lines, watched, { brief: EARLY_BRIEF, answer: EARLY_ANSWER }),
    },
  )

/** A member's question waits on the board: its hook polls. Returns what to watch, with the question. */
async function askingMember(rig, lines) {
  const asked = (thread) => thread.messages.find((message) => message.kind === 'question')
  // A model may answer on its own and never ask: that task ends, and another is given, twice at most.
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    const number = await rig.give('claude', questionBrief('claude'))
    const first = await until(
      async () => {
        const thread = await rig.thread(number)
        return asked(thread) ?? (ENDED.includes(thread.state) ? thread : null)
      },
      WORK_MS,
      500,
    )
    if (first?.kind !== 'question') {
      const thread = await rig.thread(number)
      const result = thread.messages.findLast((message) => message.kind === 'result')
      lines.push(
        first
          ? `T-${number} ended (${thread.state}) and the member never asked (attempt ${attempt}); its result: ${JSON.stringify(result?.body.slice(0, 200))}`
          : `T-${number}: the member never asked in ${WORK_MS / 1000} s (attempt ${attempt})`,
      )
      if (!first) return null
      continue
    }
    const watched = await watch(rig, 'claude')
    await sleep(3_000)
    const hooks = processesWith('hook claude')
    lines.push(
      `T-${number}: m-${first.id} is on the board and the hook waits for it (${hooks.length ? `process ${hooks.map((found) => found.pid).join(', ')}` : 'no hook process found'})`,
    )
    return { number, question: first, watched, hooks }
  }
  return null
}

/** What the hook and the turn did after the stop, as lines; whether the window stopped. */
async function afterStop(asked, made, lines) {
  const { watched, number } = asked
  let hookGoneAt = null
  const idleAt = await until(
    () => {
      if (hookGoneAt === null && processesWith('hook claude').length === 0) hookGoneAt = Date.now()
      return watched.status.samples.find((s) => s.at > made && s.status === 'idle')?.at ?? null
    },
    STOP_MS,
    100,
  )
  const pressed = [...watched.escapes.seen]
  await sleep(REST_MS)
  watched.status.stop()
  watched.escapes.stop()
  const record = recordSince(watched.session, made)
  const heard = claudeRecord(watched.session).some((item) =>
    item.text.includes(`T-${number} was stopped, so m-${asked.question.id} is not answered here`),
  )
  const ended = interrupted(watched.session, made)
  lines.push(
    `     Escapes the daemon pressed: ${pressed.length} (${pressed.map((press) => at(press, made)).join(', ') || 'none'}); ${watched.escapes.seen.length - pressed.length} more in the ${REST_MS / 1000} s after`,
  )
  lines.push(
    `     the hook: ${hookGoneAt ? `its process was gone at ${at(hookGoneAt, made)}` : 'still running'}; the model ${heard ? 'was told "T-n was stopped, so m-q is not answered here…"' : 'was not told the door is closed'}`,
  )
  lines.push(
    `     the turn: ${idleAt ? `status read idle at ${at(idleAt, made)}` : `never read idle in ${STOP_MS / 1000} s`}; the record ${ended ? `holds "${ended.text.slice(0, 60)}" at ${at(ended.at, made)}` : 'holds no interrupt'}`,
  )
  lines.push(
    `     status over the stop: ${runs(
      watched.status.samples.filter((s) => s.at >= made - 2_000 && s.at <= made + 10_000),
      made,
    ).join('; ')}`,
  )
  lines.push(`     ${await settlementLine(watched.session)}`)
  lines.push('     its record from the stop on:')
  for (const line of record) lines.push(`       ${line}`)
  return { idleAt, hookGoneAt, pressed, heard, ended }
}

async function hookCase() {
  const rig = await openRig({ folder: 'stops-hook', workers: ['claude'] })
  const lines = []
  try {
    const asked = await askingMember(rig, lines)
    if (!asked) return { ok: false, lines }
    const { made, paused } = await pause(rig, asked.number)
    lines.push(
      `task.pause ${paused.ok ? `made T-${asked.number} ${paused.task.state}` : `refused: ${JSON.stringify(paused)}`}`,
    )
    const seen = await afterStop(asked, made, lines)
    const task = await rig.thread(asked.number)
    // The chief answers late: the door is shut, so the answer waits for the task to go on.
    const given = await rig.asChief(['answer', `m-${asked.question.id}`, 'blue'])
    await sleep(3_000)
    const late = (await rig.thread(asked.number)).messages.find(
      (message) => message.kind === 'answer' && message.replyTo === asked.question.id,
    )
    lines.push(
      `     the chief answered ${given.code === 0 ? 'late' : `(cf answer failed: ${given.stderr})`}: m-${late?.id} is ${late?.state} while T-${asked.number} is ${(await rig.thread(asked.number)).state}`,
    )
    const resumed = await rig.app.requestNode('task.resume', {
      project: rig.project,
      task: asked.number,
    })
    lines.push(
      `task.resume ${resumed.ok ? 'made it queued again' : `refused: ${JSON.stringify(resumed)}`}`,
    )
    const finished = await until(
      async () =>
        ['done', 'accepted', 'failed', 'cancelled'].includes(
          (await rig.thread(asked.number)).state,
        ),
      WORK_MS,
      500,
    )
    const thread = await rig.thread(asked.number)
    const answer = thread.messages.find((message) => message.kind === 'answer')
    const result = thread.messages.findLast((message) => message.kind === 'result')
    const checks = [
      [
        'the task is paused',
        task.state === 'paused',
        `T-${asked.number} was ${task.state} after the stop`,
      ],
      [
        'the turn stopped',
        seen.idleAt !== null,
        seen.idleAt ? 'status read idle' : 'it never read idle',
      ],
      [
        'the late answer came with the resume',
        finished !== null &&
          answer?.state !== 'queued' &&
          Boolean(result?.body.toLowerCase().includes('blue')),
        `m-${answer?.id} ${answer?.state} ${JSON.stringify(answer?.receipt)}; T-${asked.number} ${thread.state}; result ${JSON.stringify(result?.body.slice(0, 80))}`,
      ],
    ]
    for (const [name, ok, detail] of checks)
      lines.push(`${ok ? 'ok  ' : 'FAIL'} ${name}: ${detail}`)
    lines.push('     what the daemon wrote:')
    for (const line of stopEvents(rig, asked.number)) lines.push(`       ${line}`)
    return { ok: checks.every(([, ok]) => ok), lines }
  } finally {
    await closeRig(rig)
  }
}

async function escapeCase() {
  const rig = await openRig({ folder: 'stops-escape', workers: ['claude'] })
  const lines = []
  try {
    const asked = await askingMember(rig, lines)
    if (!asked) return { ok: false, lines }
    // One Escape into the pane, as the daemon presses it: no pause, so the door is open.
    const made = Date.now()
    await rig.app.request('pane.input', { ...asked.watched.window.pane, bytes: [27] })
    lines.push('one Escape into the pane (task not paused, door open)')
    await afterStop(asked, made, lines)
    const thread = await rig.thread(asked.number)
    lines.push(
      `     T-${asked.number} is ${thread.state}; the question m-${asked.question.id} is ${thread.messages.find((m) => m.id === asked.question.id)?.state}; no answer was given yet`,
    )
    // The chief answers later. If the Escape ended the hook, no door waits for the answer.
    const given = await rig.asChief(['answer', `m-${asked.question.id}`, 'blue'])
    lines.push(
      `the chief answered from the board${given.code === 0 ? '' : ` (cf answer failed: ${given.stderr})`}`,
    )
    const finished = await until(
      async () => ENDED.includes((await rig.thread(asked.number)).state),
      WORK_MS,
      500,
    )
    const after = await rig.thread(asked.number)
    const answer = after.messages.find((message) => message.kind === 'answer')
    const result = after.messages.findLast((message) => message.kind === 'result')
    const about = rig.events().filter((event) => answer && event.data?.message === answer.id)
    const checks = [
      [
        'the answer is not lost',
        Boolean(finished) &&
          ['read', 'delivered'].includes(answer?.state) &&
          Boolean(result?.body.toLowerCase().includes('blue')),
        `m-${answer?.id} is ${answer?.state}, receipt ${JSON.stringify(answer?.receipt)}, attempts ${answer?.attempts} (${about.map((event) => event.kind).join(', ')}); T-${asked.number} is ${after.state}; result ${JSON.stringify(result?.body.slice(0, 80))}`,
      ],
    ]
    for (const [name, ok, detail] of checks)
      lines.push(`${ok ? 'ok  ' : 'FAIL'} ${name}: ${detail}`)
    lines.push('     what the daemon wrote:')
    for (const line of stopEvents(rig, asked.number)) lines.push(`       ${line}`)
    // What an Escape does at an open door is recorded above, not judged: the answer must not be lost.
    return { ok: checks.every(([, ok]) => ok), lines }
  } finally {
    await closeRig(rig)
  }
}

/**
 * What keeps a real Claude window at work through its presses: a hook of the
 * project's own (`.claude/settings.local.json`, which Claude reads beside the
 * settings the daemon launches it with) that ignores the signals Claude ends a
 * hook with and runs for `IGNORE_S` seconds. Where the hook runs decides what
 * Claude is doing meanwhile; `--hook` picks it.
 */
const IGNORE_S = 100
const HOOK_MARK = 'cf-stops-live-ignoring-hook'
const SLOW_HOOK = `trap '' INT TERM HUP; sleep ${IGNORE_S}; : ${HOOK_MARK}`
const HOOKS = {
  stop: { event: 'Stop', brief: 'Reply with exactly one line: DONE' },
  pretool: {
    event: 'PreToolUse',
    matcher: 'Bash',
    brief: 'Run exactly this one shell command, then stop: echo hello',
  },
  // Claude writes a prompt's record after its UserPromptSubmit hooks: a hook of a hundred seconds keeps
  // the resume's record out for longer than the daemon waits for it, which sends it again. The stop is
  // what this variant shows.
  prompt: {
    event: 'UserPromptSubmit',
    brief: 'Reply with exactly one line: DONE',
    resumes: false,
  },
}

async function ignoredCase() {
  const hook = HOOKS[values.hook]
  if (!hook) throw new Error(`no such hook: ${values.hook}`)
  const settings = {
    hooks: {
      [hook.event]: [
        {
          ...(hook.matcher ? { matcher: hook.matcher } : {}),
          hooks: [{ type: 'command', command: SLOW_HOOK, timeout: IGNORE_S + 120 }],
        },
      ],
    },
  }
  const rig = await openRig({
    folder: `stops-ignored-${values.hook}`,
    workers: ['claude'],
    files: { '.claude/settings.local.json': JSON.stringify(settings) },
  })
  const lines = []
  try {
    const number = await rig.give('claude', hook.brief)
    const running = await until(() => processesWith(HOOK_MARK).length > 0, WORK_MS, 250)
    if (!running) {
      lines.push(
        `FAIL the ${hook.event} hook that ignores signals never ran in ${WORK_MS / 1000} s`,
      )
      return { ok: false, lines }
    }
    const watched = await watch(rig, 'claude')
    const agent = rig.members.claude.agent
    await sleep(1_000)
    const { made, paused } = await pause(rig, number)
    lines.push(
      `T-${number}: the member is inside a ${hook.event} hook that ignores INT, TERM and HUP for ${IGNORE_S} s (process ${processesWith(
        HOOK_MARK,
      )
        .map((found) => found.pid)
        .join(
          ', ',
        )}); task.pause ${paused.ok ? `made T-${number} ${paused.task.state}` : `refused: ${JSON.stringify(paused)}`}`,
    )
    // The board's own word on it: `unstopped` on the lane, once every round was ignored.
    let flagged = null
    let cleared = null
    const end = made + (IGNORE_S + 40) * 1000
    while (Date.now() < end) {
      const flag = (await rig.lane(agent))?.unstopped ?? null
      if (flag && flagged === null) flagged = { at: Date.now(), flag }
      if (!flag && flagged && cleared === null) cleared = Date.now()
      const idle = watched.status.samples.findLast((sample) => sample.at > made)
      // A window that stopped and was never said to have ignored the stop has nothing more to show.
      const stopped = flagged === null && idle?.status === 'idle' && Date.now() > made + 18_000
      if ((cleared !== null && idle?.status === 'idle') || stopped) break
      await sleep(500)
    }
    await sleep(REST_MS / 2)
    watched.status.stop()
    watched.escapes.stop()
    const presses = watched.escapes.seen
    const hookAlive = processesWith(HOOK_MARK).length > 0
    const human = (await rig.inbox(undefined)).filter((message) =>
      /did not stop for T-/.test(message.body),
    )
    const requester = (await rig.inbox('chief')).filter((message) =>
      /did not stop/.test(message.body),
    )
    const quiet = watched.status.samples.filter((sample) => sample.at > made)
    const idleAt = quiet.find((sample) => sample.status === 'idle')?.at ?? null
    const interrupt = interrupted(watched.session, made)
    const gaps = presses.slice(1).map((press, at) => seconds(press - presses[at]))
    // Did Claude stop at the first Escape? Its status says so, whatever the hook it left behind does.
    // Where the hook keeps the first message from being written (UserPromptSubmit), the first Escape
    // comes when the message is, a hundred seconds after the pause: the moment is the Escape's.
    const stoppedAtOnce = idleAt !== null && presses.length > 0 && idleAt - presses[0] < 5_000
    const lateKeys = presses.filter((press) => idleAt !== null && press > idleAt + 1_500)
    const paneOpen = (await rig.lane(agent))?.pane != null
    const told = `${human.length} note${human.length === 1 ? '' : 's'} to the human${human[0] ? ` (${JSON.stringify(human[0].body)})` : ''} and ${requester.length} to the requester${requester[0] ? ` (${JSON.stringify(requester[0].body)})` : ''}`
    const how = `${presses.length} Escapes (${presses.map((press) => at(press, made)).join(', ')}; gaps ${gaps.join(', ')} s); unstopped ${flagged ? `${JSON.stringify(flagged.flag)} on its lane at ${at(flagged.at, made)}${cleared ? `, gone at ${at(cleared, made)}` : ', still there when it was last looked at'}` : 'never on its lane'}; its pane ${paneOpen ? 'is still open' : 'is gone'}; the hook ${hookAlive ? 'still runs' : 'ended'}`
    const checks = stoppedAtOnce
      ? [
          // Claude did stop: what the design promises of a window that stopped.
          [
            'Claude stopped at the first Escape',
            true,
            `status read idle ${at(idleAt, made)}; its record ${interrupt ? `holds "${interrupt.text.slice(0, 60)}" at ${at(interrupt.at, made)}` : 'holds no interrupt'}; the ${hook.event} hook it was in is still running`,
          ],
          [
            'nothing is pressed once it has stopped',
            lateKeys.length === 0,
            `${lateKeys.length} Escapes more than a look after the status read idle`,
          ],
          [
            'it is not said to have ignored the stop',
            flagged === null && human.length === 0 && requester.length === 0,
            `${told}`,
          ],
        ]
      : [
          // The window kept working: the exhausted stop of §1.3.
          [
            'the window kept working through its presses',
            presses.length >= 3,
            `the status first read idle ${idleAt ? at(idleAt, made) : 'never'}`,
          ],
          ['the board says it did not stop', flagged !== null, `${how}`],
          ['the human is told, once', human.length === 1, `${told}`],
          ['the requester is told, once', requester.length === 1, `${told}`],
          [
            'the window is not killed',
            paneOpen,
            `its pane ${paneOpen ? 'is still open' : 'is gone'}`,
          ],
        ]
    for (const [name, ok, detail] of checks)
      lines.push(`${ok ? 'ok  ' : 'FAIL'} ${name}: ${detail}`)
    lines.push(`     the exhausted stop of §1.3, as it ran: ${how}`)
    lines.push(`     ${await settlementLine(watched.session)}`)
    lines.push(
      `     status over the stop: ${runs(
        watched.status.samples.filter((s) => s.at >= made - 2_000),
        made,
      ).join('; ')}`,
    )
    // A window that stopped is paid at rest: the task goes on, and ends with its result.
    const goes =
      stoppedAtOnce && hook.resumes !== false && checks.every(([, ok]) => ok)
        ? await resumeAfter(rig, number, lines, watched)
        : true
    lines.push('     what the daemon wrote:')
    for (const line of stopEvents(rig, number)) lines.push(`       ${line}`)
    return { ok: checks.every(([, ok]) => ok) && goes, lines }
  } finally {
    await closeRig(rig)
  }
}

/**
 * A window that may keep working through its presses, on any harness: given a
 * command that ignores the signals a harness ends a command with (INT, TERM,
 * HUP) and runs for `STUCK_S` seconds, then paused. Whether the harness ends
 * its turn anyway (it kills what it cannot signal, or lets go of it) or
 * waits for the command is what is found out. The daemon's own view is the
 * evidence (its lane's activity, `unstopped`, the notes), beside what was
 * pressed; the harness's own record is not read. A window that stops is an
 * answer, as one that does not: the leftovers are killed at the end.
 */
const STUCK_S = 200
const STUCK_SCRIPT = 'ignore-signals-cf-stops-live.sh'
const STUCK_FILES = { [STUCK_SCRIPT]: `#!/bin/sh\ntrap '' INT TERM HUP\nsleep ${STUCK_S}\n` }
const STUCK_BRIEF = `Run exactly this one shell command, then stop: sh ./${STUCK_SCRIPT}`

/** The command's own processes: the script and its sleep, not the harness whose launch text names the script. */
function stuckProcesses() {
  const scripts = processesWith(STUCK_SCRIPT).filter((found) =>
    new RegExp(`^(\\S*/)?sh\\s+\\./${STUCK_SCRIPT}$`).test(found.command),
  )
  const sleeps = processesWith(`sleep ${STUCK_S}`).filter(
    (found) =>
      scripts.some((script) => script.pid === found.ppid) &&
      new RegExp(`^(\\S*/)?sleep\\s+${STUCK_S}$`).test(found.command),
  )
  return [...scripts, ...sleeps]
}

async function stuckCase() {
  const name = values.harness
  const rig = await openRig({
    folder: `stops-stuck-${name}`,
    workers: [name],
    files: STUCK_FILES,
  })
  const lines = []
  try {
    const agent = rig.members[name].agent
    const number = await rig.give(name, STUCK_BRIEF)
    const state = async () => (await rig.lane(agent))?.activity?.state
    const running = await until(() => stuckProcesses().length > 0, WORK_MS, 250)
    if (!running) {
      lines.push(`FAIL the member never ran the command in ${WORK_MS / 1000} s`)
      lines.push(`     its screen: ${screenOf(rig, (await rig.windowOf(name)) ?? { pane: {} })}`)
      return { ok: false, lines }
    }
    const window = await rig.windowOf(name)
    const escapes = rig.watchEscapes(window.pane.id)
    await sleep(3_000)
    lines.push(
      `T-${number}: ${name} is inside \`sh ./${STUCK_SCRIPT}\`, which ignores INT, TERM and HUP for ${STUCK_S} s (${shown(stuckProcesses()).join('; ')}); its activity: ${await state()}`,
    )
    const { made, paused } = await pause(rig, number)
    if (!paused.ok) {
      lines.push(`FAIL task.pause refused: ${JSON.stringify(paused)}`)
      return { ok: false, lines }
    }
    lines.push(`task.pause made T-${number} ${paused.task.state}`)
    // The lane as the board shows it, each change with its time, and when the command went.
    const seen = []
    let last = null
    let goneAt = null
    let quietSince = null
    const end = made + STUCK_WATCH_MS
    while (Date.now() < end) {
      const lane = await rig.lane(agent)
      const word = `${lane?.activity?.state}${lane?.unstopped ? ` unstopped ${JSON.stringify(lane.unstopped)}` : ''}`
      if (word !== last) seen.push(`${word} at ${at(Date.now(), made)}`)
      last = word
      if (goneAt === null && stuckProcesses().length === 0) goneAt = Date.now()
      const rest = ['idle', 'closed'].includes(lane?.activity?.state) && !lane?.unstopped
      quietSince = rest ? (quietSince ?? Date.now()) : null
      // A window at rest for a while after the stop has stopped; one never at rest is watched to the end.
      if (quietSince !== null && Date.now() - quietSince > 15_000) break
      await sleep(500)
    }
    escapes.stop()
    const presses = escapes.seen
    const exhausted = seen.some((word) => word.includes('unstopped'))
    const human = (await rig.inbox(undefined)).filter((message) =>
      /did not stop for T-/.test(message.body),
    )
    const requester = (await rig.inbox('chief')).filter((message) =>
      /did not stop/.test(message.body),
    )
    const gaps = presses.slice(1).map((press, index) => seconds(press - presses[index]))
    lines.push(
      `${presses.length} Escapes: ${presses.map((press) => at(press, made)).join(', ') || 'none'}${gaps.length ? ` (gaps ${gaps.join(', ')} s)` : ''}; the lane over the stop: ${seen.join(', ')}`,
    )
    lines.push(
      `the command: ${goneAt ? `gone at ${at(goneAt, made)}` : `still running: ${shown(stuckProcesses()).join('; ')}`}`,
    )
    lines.push(`its screen now: ${screenOf(rig, window)}`)
    if (exhausted) {
      lines.push(
        `${name} kept working through its presses (the exhausted stop of §1.3): ${human.length} note to the human (${JSON.stringify(human[0]?.body)}), ${requester.length} to the requester (${JSON.stringify(requester[0]?.body)})`,
      )
    } else {
      lines.push(
        `${name} stopped: its lane read at rest ${seen.find((word) => word.startsWith('idle') || word.startsWith('closed')) ?? 'never'} after ${presses.length} press${presses.length === 1 ? '' : 'es'}; it could not be made to ignore them with a command that ignores signals`,
      )
    }
    lines.push('     what the daemon wrote:')
    for (const line of stopEvents(rig, number)) lines.push(`       ${line}`)
    // Either end is an answer: a window that ignores (told to the human and the requester) or one that stops.
    return { ok: exhausted ? human.length === 1 && requester.length === 1 : true, lines }
  } finally {
    // What ignores the signals is ended for good, and with it the window.
    for (const found of stuckProcesses()) {
      try {
        process.kill(found.pid, 'SIGKILL')
      } catch {}
    }
    await rig.close()
  }
}

const RUN = {
  command: commandCase,
  early: earlyCase,
  hook: hookCase,
  escape: escapeCase,
  slowhook: ignoredCase,
  stuck: stuckCase,
}
const results = []
for (const name of CASES) {
  if (!RUN[name]) throw new Error(`no such case: ${name}`)
  // Every case is Claude's but the stuck one, which runs on the harness `--harness` names.
  const harness = name === 'stuck' ? values.harness : 'claude'
  const before = versionOf(harness)
  let result
  try {
    result = await RUN[name]()
  } catch (cause) {
    result = { ok: false, lines: [`FAIL ${cause.stack ?? cause}`] }
  }
  // A harness may update itself while it runs: the version is named before and after.
  const after = versionOf(harness)
  const version = after === before ? before : `${before}, then ${after}`
  results.push({ name, version, ...result })
  process.stdout.write(
    `\n== ${harness} ${version}, ${name} (native daemon): ${result.ok ? 'ok' : 'FAILED'}\n`,
  )
  for (const line of result.lines) process.stdout.write(`   ${line}\n`)
}
process.stdout.write(
  `\n${results.map((result) => `${result.ok ? 'ok  ' : 'FAIL'} ${result.name} ${result.version}`).join('\n')}\n`,
)
process.exit(results.every((result) => result.ok) ? 0 : 1)
