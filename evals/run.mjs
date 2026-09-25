#!/usr/bin/env node
/**
 * One eval run: a real chief on a toy project, with a real staff on a cheap
 * model and a scripted human, measured from the ledger at the end. See
 * evals/README.md. Spends real tokens; never part of a gate.
 *
 *   npm run eval -- --scenario six-decisions [--model claude-opus-5] [--repeat 1]
 *                   [--staff-model claude-haiku-4-5-20251001] [--timeout-min 40]
 */
import { execFileSync } from 'node:child_process'
import { cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import { startIntegration } from '../tests/integration/harness.mjs'
import { measure, verdict } from './measure.mjs'

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
    model: { type: 'string', default: 'claude-opus-5' },
    'staff-model': { type: 'string', default: 'claude-haiku-4-5-20251001' },
    repeat: { type: 'string', default: '1' },
    'timeout-min': { type: 'string', default: '40' },
  },
})
const scenario = (
  await import(pathToFileURL(join(HERE, 'scenarios', `${values.scenario}.mjs`)).href)
).default
const repeat = Number(values.repeat)
const timeoutMs = Number(values['timeout-min']) * 60_000

/** The bench's clean environment: the real logins, never this shell's session identity. */
const ENV = {
  HOME: H,
  USER: process.env.USER,
  LOGNAME: process.env.USER,
  LANG: 'en_US.UTF-8',
  TERM: 'xterm-256color',
  PATH: [
    join(H, '.local', 'bin'),
    '/opt/homebrew/bin',
    '/usr/bin',
    '/bin',
    '/usr/sbin',
    '/sbin',
  ].join(':'),
  // Unset on purpose (null removes the harness's sandbox default): with it set,
  // Claude finds no completed onboarding and opens on the first-run dialog.
  CLAUDE_CONFIG_DIR: null,
  // The chief has no model of its own in the roster: the harness's default is this.
  ANTHROPIC_MODEL: values.model,
}
// Two workers, so work that can run side by side has somewhere to run.
const STAFF = [
  { id: 'eval-worker', kind: 'claude-code', model: values['staff-model'], workTier: 'standard' },
  { id: 'eval-worker-2', kind: 'claude-code', model: values['staff-model'], workTier: 'standard' },
  { id: 'eval-advisor', kind: 'claude-code', model: values['staff-model'], workTier: 'standard' },
  { id: 'eval-reviewer', kind: 'claude-code', model: values['staff-model'], workTier: 'standard' },
]

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const stamp = () => new Date().toISOString().replace(/[:.]/g, '-')

async function run(index) {
  const started = Date.now()
  rmSync(WORKSPACE, { recursive: true, force: true })
  mkdirSync(WORKSPACE, { recursive: true })
  cpSync(join(HERE, 'fixtures', scenario.fixture), WORKSPACE, { recursive: true })
  process.stdout.write(
    `trust: ${execFileSync('python3', [TRUST, WORKSPACE], { encoding: 'utf8', env: { ...process.env, HOME: H } }).trim()}\n`,
  )
  const app = await startIntegration({ editor: EDITOR, fakeEnv: ENV })
  const log = []
  const note = (line) => {
    log.push({ at: Math.round((Date.now() - started) / 1000), line })
    process.stdout.write(`  [${Math.round((Date.now() - started) / 1000)}s] ${line}\n`)
  }
  let file = null
  try {
    writeFileSync(
      join(app.env.CONSENSFLOW_HOME, 'agents.json'),
      `${JSON.stringify({ schemaVersion: 1, agents: STAFF }, null, 2)}\n`,
    )
    file = join(app.env.CONSENSFLOW_HOME, 'consensflow.db')
    const opened = await app.requestNode('project.open', {
      directory: WORKSPACE,
      harness: 'claude-code',
      staff: [
        { agent: 'eval-worker', roles: ['worker'] },
        { agent: 'eval-worker-2', roles: ['worker'] },
        { agent: 'eval-advisor', roles: ['advisor'] },
        { agent: 'eval-reviewer', roles: ['reviewer'] },
      ],
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
    await app.waitFor(async () => (await chiefLane())?.activity?.state === 'idle', 180_000)
    note('chief ready; typing the prompt')
    await app.tell(project, scenario.prompt, { idleMs: 180_000 })

    const answered = new Set()
    let lastChange = Date.now()
    let signature = ''
    for (;;) {
      await sleep(5_000)
      for (const question of await questions()) {
        if (answered.has(question.id)) continue
        answered.add(question.id)
        const body = scenario.answer(question)
        await app.requestNode('message.answer', { question: question.id, body })
        note(
          `answered m-${question.id} (${question.body.split('\n')[0].slice(0, 70)}) with: ${body.split('\n')[0]}`,
        )
      }
      const current = await board()
      const chief = current.lanes.find((l) => l.participant.handle === 'chief')
      const tasks = current.lanes.flatMap((l) => l.tasks).concat(current.open)
      const now = JSON.stringify([
        chief?.activity?.state,
        tasks.map((t) => [t.number, t.state]),
        current.lanes.map((l) => [l.participant.handle, l.activity?.state]),
      ])
      if (now !== signature) {
        signature = now
        lastChange = Date.now()
        note(
          `chief ${chief?.activity?.state}; tasks ${tasks.map((t) => `T-${t.number}:${t.state}`).join(' ') || 'none'}`,
        )
      }
      const busy =
        chief?.activity?.state !== 'idle' ||
        tasks.some((t) => ['queued', 'working', 'waiting'].includes(t.state))
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
    await app.close({ preserveRoot: true })
  }
  const metrics = measure(file)
  const checks = verdict(scenario, metrics)
  const report = {
    scenario: scenario.id,
    model: values.model,
    staffModel: values['staff-model'],
    run: index,
    seconds: Math.round((Date.now() - started) / 1000),
    metrics,
    checks,
    log,
  }
  mkdirSync(REPORTS, { recursive: true })
  const out = join(REPORTS, `${stamp()}-${scenario.id}-${values.model}-${index}.json`)
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`)
  process.stdout.write(
    `\n${scenario.id} · chief ${values.model} · run ${index} · ${report.seconds}s\n`,
  )
  for (const check of checks)
    process.stdout.write(`  ${check.ok ? 'PASS' : 'FAIL'} ${check.name}\n`)
  process.stdout.write(
    `  tasks ${metrics.tasks.length} (parallel ${metrics.parallel}, advice ${metrics.advice}, reviews ${metrics.reviews}) · questions ${metrics.questionsToHuman.length} · notes ${metrics.notesToHuman.length} · chief edits ${metrics.chiefEdits} in ${metrics.chiefTurns} turns\n  report: ${out}\n`,
  )
  return checks.every((check) => check.ok)
}

let allOk = true
for (let index = 1; index <= repeat; index += 1) allOk = (await run(index)) && allOk
process.exit(allOk ? 0 : 1)
