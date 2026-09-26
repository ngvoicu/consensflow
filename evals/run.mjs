#!/usr/bin/env node
/**
 * One eval run: a real chief on a toy project, with a real staff on cheap
 * models and a scripted human, measured from the ledger at the end. See
 * evals/README.md. Spends real tokens; never part of a gate.
 *
 *   npm run eval -- --scenario six-decisions [--chief claude] [--staff claude,codex]
 *                   [--model …] [--claude-staff-model …] [--repeat 1] [--timeout-min 40]
 *
 * `--model` is the chief's: Opus for Claude Code and the cheap model for OpenCode
 * unless given; Codex, Pi and Devin run their own default and ignore it.
 */
import { execFileSync } from 'node:child_process'
import { chmodSync, cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import { startIntegration } from '../tests/integration/harness.mjs'
import { measure, mechanics, verdict } from './measure.mjs'
import {
  answerFor,
  chiefEnvironment,
  claudeProjectKey,
  codexIsolation,
  HARNESSES,
  lastLines,
  realOnPath,
  staffFor,
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
  },
})
const scenario = (
  await import(pathToFileURL(join(HERE, 'scenarios', `${values.scenario}.mjs`)).href)
).default
const chief = values.chief
const staffHarnesses = (values.staff ?? chief).split(',').map((s) => s.trim())
const repeat = Number(values.repeat)
const timeoutMs = Number(values['timeout-min']) * 60_000
const { agents, staff } = staffFor(staffHarnesses, { claude: values['claude-staff-model'] })
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
mkdirSync(ISOLATED_BIN, { recursive: true })
wrapper('claude', realOnPath('claude', process.env.PATH ?? ''), [
  '--strict-mcp-config',
  '--no-chrome',
])
const realCodex = realOnPath('codex', process.env.PATH ?? '')
// Codex also opens on an update prompt whenever a newer release exists
// (seen 2026-09-26 with 0.157.0 out), and a chief started without a first
// message waits on it for good: the eval turns the startup check off.
// With its MCP servers off, Codex runs commands through its own shell, which
// applies the user's shell_environment_policy; a policy of inherit = "core"
// (this Mac's, 2026-09-26) strips every ConsensFlow variable, so cf in the
// window can reach nothing. The eval environment is ours and holds no
// secret, so Codex windows pass it on whole. The model is the chief's, or
// the cheap one; a member's own --model still wins.
const codexModel = chief === 'codex' ? chiefSetup.model : HARNESSES.codex.model
wrapper(
  'codex',
  realCodex,
  [
    '-c',
    'check_for_update_on_startup=false',
    '-c',
    'shell_environment_policy.inherit="all"',
    '-c',
    'shell_environment_policy.ignore_default_excludes=true',
    '-c',
    `model=${JSON.stringify(codexModel)}`,
  ].concat(
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
  ...chiefSetup.env,
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
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

async function run(index) {
  const started = Date.now()
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
  let boardTasks = 0
  const refused = []
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
    const questions = async () =>
      (await app.requestNode('inbox.get', { project, participant: 'human' })).messages.filter(
        (m) => m.kind === 'question' && m.sender === 'chief' && m.state !== 'answered',
      )
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

    const answered = new Set()
    let lastChange = Date.now()
    let signature = ''
    for (;;) {
      await sleep(5_000)
      for (const question of await questions()) {
        if (answered.has(question.id)) continue
        answered.add(question.id)
        const answer = answerFor(scenario, question)
        const reply = await app.requestNode('message.answer', { question: question.id, ...answer })
        const shown = answer.body ?? answer.choices.map((picks) => picks.join(', ')).join(' / ')
        const subject = question.body.split('\n')[0].slice(0, 70)
        if (reply?.ok === false || reply?.message === undefined) {
          // A refused answer leaves the asker waiting for good: the report says so.
          refused.push({ question: question.id, error: reply?.error ?? 'no reply' })
          note(`the owner's answer to m-${question.id} (${subject}) was REFUSED: ${reply?.error}`)
        } else {
          await app.requestNode('message.read', { message: question.id })
          note(`answered m-${question.id} (${subject}) with: ${shown}`)
        }
      }
      // With the gate on, the owner approves every message as the board's For you does.
      for (const waiting of (await board()).gated ?? []) {
        const reply = await app.requestNode('message.approve', { message: waiting.id })
        if (reply?.ok === false) {
          refused.push({ approve: waiting.id, error: reply.error })
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
      // A chief that stops in its terminal, asking or proposing there, with
      // nothing ever put on the board, hears the owner there too (twice at
      // most), so the run sees what it does next; the report counts these,
      // because they are what the board was for.
      if (
        scenario.nudge !== undefined &&
        terminalAnswers < 2 &&
        !busy &&
        tasks.length === 0 &&
        answered.size === 0 &&
        Date.now() - lastChange > NUDGE_AFTER_MS
      ) {
        terminalAnswers += 1
        note(`the chief stopped in its terminal; the owner typed there: ${scenario.nudge}`)
        await settled(() => app.output(pane.id).length)
        await say(scenario.nudge)
        lastChange = Date.now()
        continue
      }
      if (!busy && Date.now() - lastChange > scenario.quietMs) {
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
  })
  const checks = verdict(scenario, metrics)
  const plumbing = mechanics(metrics, boardTasks)
  if (values.gate)
    plumbing.push({
      name: `the gate held the messages for the owner's approval (${approvals} approved)`,
      ok: approvals > 0 && refused.every((r) => r.approve === undefined),
    })
  const report = {
    scenario: scenario.id,
    chief,
    model: chiefSetup.model,
    staff: staffHarnesses,
    staffModels: Object.fromEntries(agents.map((a) => [a.id, a.model])),
    run: index,
    seconds: Math.round((Date.now() - started) / 1000),
    metrics,
    checks,
    mechanics: plumbing,
    chiefScreen: screen,
    terminalAnswers,
    refusedAnswers: refused,
    gate: values.gate,
    approvals,
    log,
  }
  mkdirSync(REPORTS, { recursive: true })
  const out = join(
    REPORTS,
    `${stamp()}-${scenario.id}-chief-${chief}-staff-${staffHarnesses.join('+')}-${index}.json`,
  )
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`)
  process.stdout.write(
    `\n${scenario.id} · chief ${chief} (${chiefSetup.model}) · staff ${staffHarnesses.join('+')} · run ${index} · ${report.seconds}s\n`,
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
    `  tasks ${metrics.tasks.length} (parallel ${metrics.parallel}, advice ${metrics.advice}, reviews ${metrics.reviews}) · questions ${metrics.questionsToHuman.length} · notes ${metrics.notesToHuman.length} · chief edits ${metrics.chiefEdits ?? '?'} in ${metrics.chiefTurns} turns · files changed ${metrics.filesChanged.length} · answered in the terminal ${terminalAnswers}\n  report: ${out}\n`,
  )
  return checks.every((check) => check.ok) && plumbing.every((check) => check.ok)
}

let allOk = true
for (let index = 1; index <= repeat; index += 1) allOk = (await run(index)) && allOk
process.exit(allOk ? 0 : 1)
