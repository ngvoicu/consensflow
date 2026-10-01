#!/usr/bin/env node
/**
 * One eval run: a real chief on a toy project, with a real staff on cheap
 * models and a scripted human, measured from the ledger at the end. See
 * evals/README.md. Spends real tokens; never part of a gate.
 *
 *   npm run eval -- --scenario six-decisions [--chief claude] [--staff claude,codex]
 *                   [--model …] [--claude-staff-model …] [--repeat 1] [--timeout-min 40]
 *                   [--effort high] [--staff-effort medium]
 *
 * `--model` is the chief's: Opus for Claude Code and the cheap model for OpenCode
 * unless given; Codex, Pi and Devin run their own default and ignore it.
 * `--effort` is the chief's reasoning level (Claude, Codex, Pi; OpenCode's
 * window and Devin have no switch for it), `--staff-effort` every member's.
 */
import { execFileSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { chmodSync, cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import { answers } from '../hosts/lib/completion.js'
import { interactiveStart } from '../hosts/lib/windows.js'
import { recordState } from '../src/adapters/shared.js'
import { startIntegration } from '../tests/integration/harness.mjs'
import { askingTurnEnd, bareMetrics, findSession } from './bare.mjs'
import { changed, chiefTurnEnd, countQuestions, measure, mechanics, verdict } from './measure.mjs'
import {
  chiefEnvironment,
  claudeProjectKey,
  codexIsolation,
  HARNESSES,
  lastLines,
  realOnPath,
  staffFor,
  terminalAnswer,
} from './plan.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const EDITOR = join(HERE, '..', 'tests', 'live', 'core-live-editor.mjs')
const TRUST = join(HERE, '..', 'tests', 'live', 'trust-claude-folder.py')
const H = process.env.HOME
// One fixed workspace, trusted once per run: Claude asks about an unknown folder.
const WORKSPACE = join(H, '.consensflow-candidate', 'evals', 'workspace')
const REPORTS = join(HERE, 'reports')

const { values } = parseArgs({
  options: {
    scenario: { type: 'string', default: 'six-decisions' },
    chief: { type: 'string', default: 'claude' },
    staff: { type: 'string' },
    model: { type: 'string' },
    gate: { type: 'boolean', default: false },
    'claude-staff-model': { type: 'string', default: HARNESSES.claude.model },
    repeat: { type: 'string', default: '1' },
    'timeout-min': { type: 'string', default: '40' },
    effort: { type: 'string', default: 'high' },
    'staff-effort': { type: 'string', default: 'medium' },
    // card: ConsensFlow's chief card. nocard: ConsensFlow with a one-line
    // card that names no board. bare: the harness alone, no ConsensFlow.
    arm: { type: 'string', default: 'card' },
    // A scenario's { switch: true } step moves the lead to this harness, on
    // its staff agent's model (so the harness must be in --staff).
    'switch-to': { type: 'string' },
  },
})
if (!['card', 'nocard', 'bare'].includes(values.arm)) throw new Error(`no such arm: ${values.arm}`)
const scenario = (
  await import(pathToFileURL(join(HERE, 'scenarios', `${values.scenario}.mjs`)).href)
).default
const chief = values.chief
const staffHarnesses = (values.staff ?? chief).split(',').map((s) => s.trim())
if ((scenario.followUps ?? []).some((step) => step?.switch === true)) {
  const target = values['switch-to']
  if (target === undefined || !staffHarnesses.includes(target) || target === chief) {
    throw new Error(
      `${scenario.id} switches the lead: --switch-to names a --staff harness other than the chief's`,
    )
  }
}
const repeat = Number(values.repeat)
const timeoutMs = Number(values['timeout-min']) * 60_000
const { agents, staff } = staffFor(
  staffHarnesses,
  { claude: values['claude-staff-model'] },
  values['staff-effort'],
)
/** The chief's effort as it reached the chief: null where its harness has no switch for it. */
const chiefEffort = ['claude', 'codex', 'pi'].includes(chief) ? values.effort : null
const chiefSetup = chiefEnvironment(chief, values.model)

/** The bench's clean environment: the real logins, never this shell's session identity. */
/**
 * Every Claude and Codex window in a run starts through a wrapper, first on
 * the daemon's PATH, that shuts out MCP servers, connectors and the browser.
 * The windows run in full-permission mode on the user's own setup, which
 * reaches their browser, screen and accounts: on 2026-09-26 an eval reviewer
 * called the Claude in Chrome tools, and Codex's setup gained browser and
 * computer-use servers the same day. A scripted run must reach none of them.
 */
const ISOLATED_BIN = join(H, '.consensflow-candidate', 'evals', 'bin')
const wrapper = (name, real, flags) => {
  const file = join(ISOLATED_BIN, name)
  const args = [real, ...flags].map((arg) => JSON.stringify(arg)).join(' ')
  writeFileSync(
    file,
    `#!/bin/sh\n# Written by evals/run.mjs: eval windows reach no MCP server and no browser.\nexec ${args} "$@"\n`,
  )
  chmodSync(file, 0o755)
}
// Fresh each run: a wrapper written for another chief must not outlive its run.
rmSync(ISOLATED_BIN, { recursive: true, force: true })
mkdirSync(ISOLATED_BIN, { recursive: true })
// The chief's effort goes first; a member's own, later on its command line, wins.
wrapper('claude', realOnPath('claude', process.env.PATH ?? ''), [
  '--strict-mcp-config',
  '--no-chrome',
  ...(chief === 'claude' ? ['--effort', values.effort] : []),
])
if (chief === 'pi') {
  // The chief's model and thinking level, for a window only (Pi's own
  // subcommands take neither) and only where the window names none: a
  // member's roster agent passes its own, and it must not depend on which of
  // two flags Pi keeps.
  const realPi = JSON.stringify(realOnPath('pi', process.env.PATH ?? ''))
  const file = join(ISOLATED_BIN, 'pi')
  writeFileSync(
    file,
    [
      '#!/bin/sh',
      "# Written by evals/run.mjs: the Pi chief's model and thinking level.",
      'case "$1" in',
      `  -*|'') ;;`,
      `  *) exec ${realPi} "$@" ;;`,
      'esac',
      `case " $* " in *" --model "*) ;; *) set -- --model ${JSON.stringify(chiefSetup.model)} "$@" ;; esac`,
      `case " $* " in *" --thinking "*) ;; *) set -- --thinking ${JSON.stringify(values.effort)} "$@" ;; esac`,
      `exec ${realPi} "$@"`,
      '',
    ].join('\n'),
  )
  chmodSync(file, 0o755)
}
const realCodex = realOnPath('codex', process.env.PATH ?? '')
// Codex also opens on an update prompt whenever a newer release exists
// (seen 2026-09-26 with 0.157.0 out), and a chief started without a first
// message waits on it for good: the eval turns the startup check off.
// The chief's model, or the cheap one; a member's own --model still wins.
// The product itself now skips Codex's update prompt, keeps ConsensFlow's
// variables for Codex's commands and isolates members; the eval adds only
// what is eval-only: the chief isolated too, and the model.
const codexModel = chief === 'codex' ? chiefSetup.model : HARNESSES.codex.model
wrapper(
  'codex',
  realCodex,
  ['-c', `model=${JSON.stringify(codexModel)}`]
    .concat(
      chief === 'codex' ? ['-c', `model_reasoning_effort=${JSON.stringify(values.effort)}`] : [],
    )
    .concat(
      codexIsolation(
        JSON.parse(
          execFileSync(realCodex, ['mcp', 'list', '--json'], { encoding: 'utf8', timeout: 30_000 }),
        ),
      ),
    ),
)

const ENV = {
  HOME: H,
  USER: process.env.USER,
  LOGNAME: process.env.USER,
  LANG: 'en_US.UTF-8',
  TERM: 'xterm-256color',
  PATH: [
    ISOLATED_BIN,
    join(H, '.local', 'bin'),
    join(H, '.opencode', 'bin'),
    join(H, '.codex', 'bin'),
    join(H, '.pi', 'bin'),
    '/opt/homebrew/bin',
    '/usr/bin',
    '/bin',
    '/usr/sbin',
    '/sbin',
  ].join(':'),
  // Unset on purpose (null removes the harness's sandbox default): with it set,
  // Claude finds no completed onboarding and opens on the first-run dialog.
  CLAUDE_CONFIG_DIR: null,
  CODEX_HOME: join(H, '.codex'),
  XDG_CONFIG_HOME: join(H, '.config'),
  ...(values.arm === 'nocard'
    ? { CONSENSFLOW_EVAL_CHIEF_CARD: join(HERE, 'fixtures', 'no-card.md') }
    : {}),
  ...chiefSetup.env,
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
/** How long the chief and the board stay quiet before the owner sends the next message of a conversation. */
const FOLLOW_UP_AFTER_MS = 20_000
/** How long a chief's question dialog holds its window before the owner answers it. */
const PICKER_AFTER_MS = 10_000
/** How many times at most the owner answers questions a chief left in its terminal. */
const MAX_TERMINAL_REPLIES = 8
/** How long a chief may sit idle with nothing on the board before the owner answers it in its terminal. */
const NUDGE_AFTER_MS = 60_000

/**
 * Wait until a window's output has grown and then held still for `stillMs`:
 * the harness has drawn its prompt. A window can read idle the moment it
 * opens (Pi and Devin do), and text typed before the prompt is lost.
 */
async function settled(size, { stillMs = 3_000, capMs = 60_000 } = {}) {
  const started = Date.now()
  let last = size()
  let since = started
  for (;;) {
    await sleep(500)
    const now = size()
    if (now !== last) {
      last = now
      since = Date.now()
    } else if (now > 0 && Date.now() - since >= stillMs) return
    if (Date.now() - started > capMs) return
  }
}
const stamp = () => new Date().toISOString().replace(/[:.]/g, '-')

/** The scenario's fixture in a fresh workspace, which Claude trusts, with nothing it remembered from the last run. */
function freshWorkspace() {
  rmSync(WORKSPACE, { recursive: true, force: true })
  mkdirSync(WORKSPACE, { recursive: true })
  // A fresh run: nothing Claude remembered about this folder from the last one.
  rmSync(join(H, '.claude', 'projects', claudeProjectKey(WORKSPACE), 'memory'), {
    recursive: true,
    force: true,
  })
  cpSync(join(HERE, 'fixtures', scenario.fixture), WORKSPACE, { recursive: true })
  if (chief === 'claude' || staffHarnesses.includes('claude')) {
    process.stdout.write(
      `trust: ${execFileSync('python3', [TRUST, WORKSPACE], { encoding: 'utf8', env: { ...process.env, HOME: H } }).trim()}\n`,
    )
  }
}

async function run(index) {
  const started = Date.now()
  freshWorkspace()
  const app = await startIntegration({ editor: EDITOR, fakeEnv: ENV })
  const log = []
  const note = (line) => {
    const at = Math.round((Date.now() - started) / 1000)
    log.push({ at, line })
    process.stdout.write(`  [${at}s] ${line}\n`)
  }
  let file = null
  let pane = null
  let screen = []
  let terminalAnswers = 0
  let terminalReplies = 0
  let pickerAnswers = 0
  const repliedTo = new Set()
  let followUpsSent = 0
  let boardTasks = 0
  const refusedApprovals = []
  let approvals = 0
  try {
    writeFileSync(
      join(app.env.CONSENSFLOW_HOME, 'agents.json'),
      `${JSON.stringify({ schemaVersion: 1, agents }, null, 2)}\n`,
    )
    file = join(app.env.CONSENSFLOW_HOME, 'consensflow.db')
    const opened = await app.requestNode('project.open', {
      directory: WORKSPACE,
      harness: HARNESSES[chief].kind,
      staff,
      ...(values.gate ? { gate: true } : {}),
    })
    if (opened.ok !== true) throw new Error(`project.open: ${JSON.stringify(opened)}`)
    const project = opened.project.id
    const board = async () => (await app.requestNode('board.get', { project })).board
    const chiefLane = async () =>
      (await board()).lanes.find((l) => l.participant.handle === 'chief')
    await app.waitFor(async () => (await chiefLane())?.activity?.state === 'idle', 240_000)
    pane = (await chiefLane()).pane
    await settled(() => app.output(pane.id).length)
    note(`chief (${chief}) ready; typing the prompt`)
    /** Type into the chief's terminal as the owner would, Enter pressed again if the window kept the text. */
    const say = async (text) => {
      await app.tell(project, text, { idleMs: 240_000 })
      await sleep(5_000)
      if ((await chiefLane())?.activity?.state === 'idle') {
        // Devin takes a pasted prompt into its box and waits for an Enter of its own.
        await app.request('pane.input', { id: pane.id, generation: pane.generation, bytes: [13] })
        note('Enter pressed again: the window had not taken the text')
      }
    }
    await say(scenario.prompt)
    // A conversation: the owner's next message once the chief is done with
    // the last one (idle, nothing running on the board, a moment of quiet).
    const followUps = [...(scenario.followUps ?? [])]

    let lastChange = Date.now()
    let signature = ''
    for (;;) {
      await sleep(5_000)
      // With the gate on, the owner approves every message as the board's For you does.
      for (const waiting of (await board()).gated ?? []) {
        const reply = await app.requestNode('message.approve', { message: waiting.id })
        if (reply?.ok === false) {
          refusedApprovals.push({ message: waiting.id, error: reply.error })
          note(`approving m-${waiting.id} was REFUSED: ${reply.error}`)
        } else approvals += 1
      }
      const current = await board()
      const lane = current.lanes.find((l) => l.participant.handle === 'chief')
      const tasks = current.lanes.flatMap((l) => l.tasks).concat(current.open)
      boardTasks = tasks.length
      const now = JSON.stringify([
        lane?.activity?.state,
        tasks.map((t) => [t.number, t.state]),
        current.lanes.map((l) => [l.participant.handle, l.activity?.state]),
      ])
      if (now !== signature) {
        signature = now
        lastChange = Date.now()
        note(
          `chief ${lane?.activity?.state}; tasks ${tasks.map((t) => `T-${t.number}:${t.state}`).join(' ') || 'none'}`,
        )
      }
      const busy =
        lane?.activity?.state !== 'idle' ||
        tasks.some((t) => ['queued', 'working', 'waiting'].includes(t.state))
      // A chief's own question dialog holds its window, waiting for input:
      // the owner answers it there, Enter taking its first option.
      if (
        lane?.activity?.state === 'waiting' &&
        pickerAnswers < MAX_TERMINAL_REPLIES &&
        Date.now() - lastChange > PICKER_AFTER_MS
      ) {
        pickerAnswers += 1
        note(
          `the chief's question dialog waits; the owner pressed Enter there. Its screen: ${lastLines(app.output(pane.id)).slice(-6).join(' ⏎ ')}`,
        )
        await app.request('pane.input', { id: pane.id, generation: pane.generation, bytes: [13] })
        lastChange = Date.now()
        continue
      }
      // A chief that ended its turn asking in its terminal hears the owner
      // there: its questions answered from the scenario's answers, once per
      // turn, the same way in every arm.
      if (
        !busy &&
        terminalReplies < MAX_TERMINAL_REPLIES &&
        Date.now() - lastChange > FOLLOW_UP_AFTER_MS
      ) {
        const end = chiefTurnEnd(file)
        if (end !== undefined && !repliedTo.has(end.id) && countQuestions(end.text) > 0) {
          repliedTo.add(end.id)
          terminalReplies += 1
          const reply = terminalAnswer(scenario, end.text)
          note(`the chief asked in its terminal; the owner answered there: ${reply}`)
          await say(reply)
          lastChange = Date.now()
          continue
        }
      }
      // A chief that stops in its terminal without asking anything, with
      // nothing put on the board, hears the owner there too (twice at most),
      // so the run sees what it does next; the report counts these nudges
      // apart from the answers to its questions.
      if (
        scenario.nudge !== undefined &&
        terminalAnswers < 2 &&
        terminalReplies === 0 &&
        pickerAnswers === 0 &&
        !busy &&
        tasks.length === 0 &&
        Date.now() - lastChange > NUDGE_AFTER_MS
      ) {
        terminalAnswers += 1
        note(`the chief stopped in its terminal; the owner typed there: ${scenario.nudge}`)
        await settled(() => app.output(pane.id).length)
        await say(scenario.nudge)
        lastChange = Date.now()
        continue
      }
      if (followUps.length > 0 && !busy && Date.now() - lastChange > FOLLOW_UP_AFTER_MS) {
        const next = followUps.shift()
        if (next.switch === true) {
          // The owner switches the lead from the chief's row; the new window
          // takes the handoff first, then the owner goes on in it.
          const agent = `eval-${values['switch-to']}-worker`
          const before = pane.generation
          note(`the owner switches the lead to ${values['switch-to']} (${agent})`)
          const reply = await app.requestNode('chief.switch', { project, agent, when: 'turn' })
          if (reply?.ok === false) throw new Error(`chief.switch: ${JSON.stringify(reply)}`)
          await app.waitFor(async () => {
            const lead = (await chiefLane())?.pane
            return lead !== null && lead !== undefined && lead.generation !== before
          }, 240_000)
          pane = (await chiefLane()).pane
          await settled(() => app.output(pane.id).length)
          lastChange = Date.now()
          continue
        }
        followUpsSent += 1
        note(`the owner's next message (${followUpsSent}): ${next.slice(0, 80)}`)
        await settled(() => app.output(pane.id).length)
        await say(next)
        lastChange = Date.now()
        continue
      }
      if (followUps.length === 0 && !busy && Date.now() - lastChange > scenario.quietMs) {
        note('quiet: the run is over')
        break
      }
      if (Date.now() - started > timeoutMs) {
        note('time is up')
        break
      }
    }
  } finally {
    if (pane !== null) screen = lastLines(app.output(pane.id))
    await app.close({ preserveRoot: true })
  }
  const metrics = measure(file, {
    fixture: join(HERE, 'fixtures', scenario.fixture),
    workspace: WORKSPACE,
    pickers: pickerAnswers,
  })
  const checks = verdict(scenario, metrics)
  const plumbing = mechanics(metrics, boardTasks)
  if (values.gate)
    plumbing.push({
      name: `the gate held the messages for the owner's approval (${approvals} approved)`,
      ok: approvals > 0 && refusedApprovals.length === 0,
    })
  const report = {
    scenario: scenario.id,
    chief,
    model: chiefSetup.model,
    effort: chiefEffort,
    staffEffort: values['staff-effort'],
    staff: staffHarnesses,
    switchTo: values['switch-to'] ?? null,
    staffModels: Object.fromEntries(agents.map((a) => [a.id, a.model])),
    run: index,
    seconds: Math.round((Date.now() - started) / 1000),
    metrics,
    checks,
    mechanics: plumbing,
    chiefScreen: screen,
    arm: values.arm,
    terminalAnswers,
    terminalReplies,
    pickerAnswers,
    followUpsSent,
    refusedApprovals,
    gate: values.gate,
    approvals,
    log,
  }
  mkdirSync(REPORTS, { recursive: true })
  const out = join(
    REPORTS,
    `${stamp()}-${scenario.id}-chief-${chief}${values.arm === 'card' ? '' : `-${values.arm}`}-staff-${staffHarnesses.join('+')}-${index}.json`,
  )
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`)
  process.stdout.write(
    `\n${scenario.id} · chief ${chief}${values.arm === 'card' ? '' : ` [${values.arm}]`} (${chiefSetup.model}, effort ${chiefEffort ?? 'its default'}) · staff effort ${values['staff-effort']} · staff ${staffHarnesses.join('+')} · run ${index} · ${report.seconds}s\n`,
  )
  for (const check of checks)
    process.stdout.write(`  ${check.ok ? 'PASS' : 'FAIL'} ${check.name}\n`)
  process.stdout.write('  the plumbing:\n')
  for (const check of plumbing)
    process.stdout.write(`  ${check.ok ? 'PASS' : 'FAIL'} ${check.name}\n`)
  if (metrics.chiefTurns === 0)
    process.stdout.write(
      `  the chief's screen ended with:\n${screen
        .slice(-8)
        .map((l) => `    ${l}`)
        .join('\n')}\n`,
    )
  process.stdout.write(
    `  tasks ${metrics.tasks.length} (parallel ${metrics.parallel}, advice ${metrics.advice}, reviews ${metrics.reviews}) · notes ${metrics.notesToHuman.length} · chief edits ${metrics.chiefEdits ?? '?'} in ${metrics.chiefTurns} turns · files changed ${metrics.filesChanged.length} · answered in the terminal ${terminalAnswers} (nudges) + ${terminalReplies} (its questions)\n  the owner was asked in the terminal: ${metrics.ownerQuestions.questions} questions in ${metrics.ownerQuestions.turnsAsking} turns, ${metrics.ownerQuestions.pickers} in its question dialog · on the board ${metrics.questionsOnBoard} · cf ask refused ${metrics.askRefused}\n  members asked the chief: ${metrics.memberQuestionsBy.map((r) => `${r.role}/${r.harness} ${r.asked} (answered ${r.answered}, delivered ${r.delivered}, longest ${r.longest})`).join(' · ') || 'nothing'}\n  report: ${out}\n`,
  )
  return checks.every((check) => check.ok) && plumbing.every((check) => check.ok)
}

/**
 * How long a bare window's screen and record both hold still before it
 * counts as idle whatever its record says: a working harness keeps drawing
 * (a spinner, a timer) or adding to its record, and without ConsensFlow
 * Devin's and OpenCode's records never say a turn is over.
 */
const SCREEN_IDLE_MS = 30_000
/** Where a harness looks for its native records, without the eval's removals (null means unset). */
const RECORD_ENV = Object.fromEntries(Object.entries(ENV).filter(([, value]) => value !== null))

/**
 * The chief as a bare harness: no ConsensFlow project, the harness alone in
 * a window of the same pane host, started through the same wrappers (model,
 * effort, no MCP), its state read from its own record. The owner types the
 * prompt and the follow-ups, answers what it asks at the end of a turn, and
 * nudges it once or twice when it stops without asking, as in the other arms.
 */
async function runBare(index) {
  const started = Date.now()
  freshWorkspace()
  const app = await startIntegration({ editor: EDITOR, fakeEnv: ENV })
  const log = []
  const note = (line) => {
    const at = Math.round((Date.now() - started) / 1000)
    log.push({ at, line })
    process.stdout.write(`  [${at}s] ${line}\n`)
  }
  const kind = HARNESSES[chief].kind
  // Claude and Pi open on an id they are given; the others name theirs in their stores.
  let session = kind === 'claude-code' || kind === 'pi' ? randomUUID() : null
  const start = interactiveStart({ kind }, session, null)
  const executable = ['claude', 'codex', 'pi'].includes(start.command)
    ? join(ISOLATED_BIN, start.command)
    : realOnPath(start.command, ENV.PATH)
  const pane = { id: 'bare-chief', generation: 1 }
  let screen = []
  let items = []
  let terminalAnswers = 0
  let terminalReplies = 0
  let followUpsSent = 0
  const repliedTo = new Set()
  const ends = new Set()
  try {
    const opened = await app.request('pane.open', {
      ...pane,
      launch: `bare-${index}`,
      cwd: WORKSPACE,
      argv: [executable, ...start.args],
      env: {},
      dropEnv: start.dropEnv,
    })
    if (opened?.ok !== true) throw new Error(`pane.open: ${JSON.stringify(opened)}`)
    const record = async () => {
      session ??= findSession(kind, {
        workspace: WORKSPACE,
        since: started,
        home: H,
        env: RECORD_ENV,
      })
      if (session === null) return null
      const read = await answers(kind, session, RECORD_ENV).catch(() => null)
      return read === null || read.unknown ? null : recordState(read)
    }
    const type = async (text) => {
      await settled(() => app.output(pane.id).length)
      const before = (await record())?.items.length ?? 0
      await app.request('pane.input', {
        ...pane,
        bytes: [...Buffer.from(`\u001b[200~${text}\u001b[201~\r`)],
      })
      await sleep(8_000)
      // Devin takes a pasted prompt into its box and waits for an Enter of its own.
      if (((await record())?.items.length ?? 0) === before) {
        await app.request('pane.input', { ...pane, bytes: [13] })
        note('Enter pressed again: the window had not taken the text')
      }
    }
    await settled(() => app.output(pane.id).length)
    note(`chief (${chief}, bare) ready; typing the prompt`)
    await type(scenario.prompt)
    const followUps = [...(scenario.followUps ?? [])]
    let lastChange = Date.now()
    let signature = ''
    let screenAt = { length: -1, at: Date.now() }
    let recordAt = { shape: '', at: Date.now() }
    for (;;) {
      await sleep(5_000)
      const state = await record()
      items = state?.items ?? items
      const printed = app.output(pane.id).length
      if (printed !== screenAt.length) screenAt = { length: printed, at: Date.now() }
      const grown = JSON.stringify([items.length, items.at(-1)?.id, items.at(-1)?.text?.length])
      if (grown !== recordAt.shape) recordAt = { shape: grown, at: Date.now() }
      const moving = Date.now() - Math.max(screenAt.at, recordAt.at) < SCREEN_IDLE_MS
      const busy = (state === null || !state.settled) && moving
      // At rest, its newest message ended the turn.
      const newest = items.filter((item) => item.role === 'assistant').at(-1)
      if (!busy && newest !== undefined) ends.add(newest.id)
      const now = JSON.stringify([items.length, items.at(-1)?.id, items.at(-1)?.text?.length, busy])
      if (now !== signature) {
        signature = now
        lastChange = Date.now()
        note(`chief ${busy ? 'working' : 'idle'}; ${items.length} items in its record`)
      }
      const quiet = Date.now() - lastChange
      if (!busy && terminalReplies < MAX_TERMINAL_REPLIES && quiet > FOLLOW_UP_AFTER_MS) {
        const end = askingTurnEnd(items, ends)
        if (end !== undefined && !repliedTo.has(end.id)) {
          repliedTo.add(end.id)
          terminalReplies += 1
          const reply = terminalAnswer(scenario, end.text)
          note(`the chief asked in its terminal; the owner answered there: ${reply}`)
          await type(reply)
          lastChange = Date.now()
          continue
        }
      }
      if (
        scenario.nudge !== undefined &&
        terminalAnswers < 2 &&
        terminalReplies === 0 &&
        !busy &&
        quiet > NUDGE_AFTER_MS
      ) {
        terminalAnswers += 1
        note(`the chief stopped in its terminal; the owner typed there: ${scenario.nudge}`)
        await type(scenario.nudge)
        lastChange = Date.now()
        continue
      }
      if (followUps.length > 0 && !busy && quiet > FOLLOW_UP_AFTER_MS) {
        const next = followUps.shift()
        followUpsSent += 1
        note(`the owner's next message (${followUpsSent}): ${next.slice(0, 80)}`)
        await type(next)
        lastChange = Date.now()
        continue
      }
      if (followUps.length === 0 && !busy && quiet > scenario.quietMs) {
        note('quiet: the run is over')
        break
      }
      if (Date.now() - started > timeoutMs) {
        note('time is up')
        break
      }
    }
  } finally {
    screen = lastLines(app.output(pane.id))
    await app.close({ preserveRoot: true })
  }
  const metrics = bareMetrics(items, {
    filesChanged: changed(join(HERE, 'fixtures', scenario.fixture), WORKSPACE),
    ends,
  })
  const checks = verdict(scenario, metrics)
  const report = {
    scenario: scenario.id,
    chief,
    model: chiefSetup.model,
    effort: chiefEffort,
    arm: 'bare',
    session,
    run: index,
    seconds: Math.round((Date.now() - started) / 1000),
    metrics,
    checks,
    chiefScreen: screen,
    terminalAnswers,
    terminalReplies,
    followUpsSent,
    log,
  }
  mkdirSync(REPORTS, { recursive: true })
  const out = join(REPORTS, `${stamp()}-${scenario.id}-chief-${chief}-bare-${index}.json`)
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`)
  const asked = metrics.ownerQuestions
  process.stdout.write(
    `\n${scenario.id} · chief ${chief} [bare] (${chiefSetup.model}, effort ${chiefEffort ?? 'its default'}) · run ${index} · ${report.seconds}s\n` +
      checks.map((check) => `  ${check.ok ? 'PASS' : 'FAIL'} ${check.name}\n`).join('') +
      `  turns ${metrics.chiefTurns} · files changed ${metrics.filesChanged.length} · answered in the terminal ${terminalAnswers} (nudges) + ${terminalReplies} (its questions)\n` +
      `  the owner was asked in the terminal: ${asked.questions} questions in ${asked.turnsAsking} turns\n  report: ${out}\n`,
  )
  return checks.every((check) => check.ok)
}

let allOk = true
for (let index = 1; index <= repeat; index += 1)
  allOk = (await (values.arm === 'bare' ? runBare(index) : run(index))) && allOk
process.exit(allOk ? 0 : 1)
