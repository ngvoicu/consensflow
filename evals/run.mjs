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
import { chmodSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import { answers } from '../hosts/lib/completion.js'
import { interactiveStart } from '../hosts/lib/windows.js'
import { recordState } from '../src/adapters/shared.js'
import { consoleText } from '../src/console-text.js'
import { onWindows, runnable } from '../src/harnesses.js'
import { startIntegration } from '../tests/integration/harness.mjs'
import { trustForClaude } from '../tests/live/trust-claude.mjs'
import { askingTurnEnd, bareMetrics, findSession } from './bare.mjs'
import {
  changed,
  chiefOpenQuestion,
  devinChiefQuestions,
  measure,
  mechanics,
  verdict,
} from './measure.mjs'
import {
  chiefEnvironment,
  claudeProjectKey,
  codexIsolation,
  devinPickerAnswers,
  HARNESSES,
  lastLines,
  liveEnvironment,
  realOnPath,
  staffFor,
  terminalAnswer,
} from './plan.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const DAEMON = join(HERE, '..', 'tests', 'live', 'core-live-daemon.mjs')
// A Windows terminal names no HOME; the user's profile is the home there.
const H = process.env.HOME ?? homedir()
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
    // A scenario's { switch: true } step moves the chief to this harness, on
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
      `${scenario.id} switches the chief: --switch-to names a --staff harness other than the chief's`,
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
/** The chief's agent, which the daemon opens no project without: the chief's harness, model and effort. */
const chiefAgent = {
  id: `eval-${chief}-chief`,
  kind: HARNESSES[chief].kind,
  model: chiefSetup.model,
  workTier: 'standard',
  ...(chiefEffort == null ? {} : { [chief === 'pi' ? 'thinking' : 'effort']: chiefEffort }),
}

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
const WINDOWS = process.platform === 'win32'
const wrapper = (name, real, flags) => {
  if (WINDOWS) {
    // A window opens on a .cmd only in npm's shape, node and a script: the
    // script starts the real program with these flags and every argument it
    // was given, in the same console, and ends as it ends. Ctrl+C is the
    // program's to answer, not the script's.
    const relay = join(ISOLATED_BIN, `${name}.mjs`)
    const harnesses = pathToFileURL(join(HERE, '..', 'src', 'harnesses.js')).href
    writeFileSync(
      relay,
      [
        '// Written by evals/run.mjs: eval windows reach no MCP server and no browser.',
        "import { spawn } from 'node:child_process'",
        `import { runnable } from ${JSON.stringify(harnesses)}`,
        `const { file, args, options } = runnable(${JSON.stringify(real)}, [...${JSON.stringify(flags)}, ...process.argv.slice(2)])`,
        "for (const signal of ['SIGINT', 'SIGBREAK']) process.on(signal, () => {})",
        "const child = spawn(file, args, { ...options, stdio: 'inherit' })",
        "child.on('error', (error) => { console.error(error.message); process.exit(1) })",
        "child.on('exit', (code) => process.exit(code ?? 1))",
        '',
      ].join('\n'),
    )
    writeFileSync(
      join(ISOLATED_BIN, `${name}.cmd`),
      `@echo off\r\nREM Written by evals/run.mjs: eval windows reach no MCP server and no browser.\r\n"${process.execPath}" "${relay}" %*\r\n`,
    )
    return
  }
  const file = join(ISOLATED_BIN, name)
  const args = [real, ...flags].map((arg) => JSON.stringify(arg)).join(' ')
  writeFileSync(
    file,
    `#!/bin/sh\n# Written by evals/run.mjs: eval windows reach no MCP server and no browser.\nexec ${args} "$@"\n`,
  )
  chmodSync(file, 0o755)
}
/** Whether a harness has a window in this run, as the chief or on the staff. */
const inRun = (name) => chief === name || staffHarnesses.includes(name)
// Fresh each run: a wrapper written for another chief must not outlive its run.
rmSync(ISOLATED_BIN, { recursive: true, force: true })
mkdirSync(ISOLATED_BIN, { recursive: true })
// The chief's effort goes first; a member's own, later on its command line, wins.
if (inRun('claude')) {
  wrapper('claude', realOnPath('claude', process.env.PATH ?? ''), [
    '--strict-mcp-config',
    '--no-chrome',
    ...(chief === 'claude' ? ['--effort', values.effort] : []),
  ])
}
if (chief === 'pi' && WINDOWS)
  throw new Error("the Pi chief's wrapper is sh: run Pi chiefs on the Mac")
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
const realCodex = inRun('codex') ? realOnPath('codex', process.env.PATH ?? '') : null
// Codex also opens on an update prompt whenever a newer release exists
// (seen 2026-09-26 with 0.157.0 out), and a chief started without a first
// message waits on it for good: the eval turns the startup check off.
// The chief's model, or the cheap one; a member's own --model still wins.
// The product itself now skips Codex's update prompt, keeps ConsensFlow's
// variables for Codex's commands and isolates members; the eval adds only
// what is eval-only: the chief isolated too, and the model.
const codexModel = chief === 'codex' ? chiefSetup.model : HARNESSES.codex.model
if (realCodex !== null) {
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
            (() => {
              const list = runnable(realCodex, ['mcp', 'list', '--json'])
              return execFileSync(list.file, list.args, {
                ...list.options,
                encoding: 'utf8',
                timeout: 30_000,
              })
            })(),
          ),
        ),
      ),
  )
}

const ENV = {
  ...liveEnvironment({ home: H, bin: ISOLATED_BIN }),
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
/** How long the owner waits after each key in a dialog, and after each answer, for the window to draw it. */
const KEY_PAUSE_MS = 600
const QUESTION_PAUSE_MS = 2_000
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
}

/** The fresh workspace trusted for Claude, in a pane of the run's own host, when Claude is in the run. */
async function trustWorkspace(app) {
  if (!inRun('claude')) return
  const trust = await trustForClaude(app, WORKSPACE, realOnPath('claude', process.env.PATH ?? ''))
  process.stdout.write(`trust: ${trust}\n`)
}

async function run(index) {
  const started = Date.now()
  freshWorkspace()
  const app = await startIntegration({ daemon: DAEMON, fakeEnv: ENV })
  // Which daemon the run is on (CONSENSFLOW_TEST_DAEMON, tests/choice.mjs): what
  // the Windows matrix reads back to say what it ran against.
  process.stdout.write(`daemon: ${app.daemon.kind} (${app.daemon.runtime})\n`)
  await trustWorkspace(app)
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
      `${JSON.stringify({ schemaVersion: 1, agents: [chiefAgent, ...agents] }, null, 2)}\n`,
    )
    file = join(app.env.CONSENSFLOW_HOME, 'consensflow.db')
    const opened = await app.requestNode('project.open', {
      directory: WORKSPACE,
      agent: chiefAgent.id,
      staff,
      ...(values.gate ? { gate: true } : {}),
    })
    if (opened.ok !== true) throw new Error(`project.open: ${JSON.stringify(opened)}`)
    const project = opened.project.id
    const board = async () => (await app.requestNode('board.get', { project })).board
    const chiefLane = async () =>
      (await board()).lanes.find((l) => l.participant.handle === 'chief')
    /** A wait that, when it gives up, says what the chief's window was doing and showed. */
    const waitForChief = async (what, predicate, ms) => {
      try {
        await app.waitFor(predicate, ms)
      } catch (cause) {
        const lane = await chiefLane().catch(() => null)
        // A window that closed already: the last one opened for the chief, as it was started.
        const opened = app.openFrames.findLast((frame) => /^p\d+-chief-|-chief$/.test(frame.id))
        const pane = lane?.pane ?? opened ?? null
        const shown = pane ? lastLines(app.output(pane.id)).slice(-12) : []
        const started = opened
          ? ` started (${opened.argv.join(' ').length} characters) as ${opened.argv
              .map((arg) => (arg.length > 100 ? `${arg.slice(0, 100)}…(${arg.length})` : arg))
              .join(' ')};`
          : ''
        note(
          `gave up waiting for ${what}: chief ${JSON.stringify(lane?.activity ?? null)};${started} its screen: ${shown.join(' ⏎ ')}`,
        )
        throw cause
      }
    }
    await waitForChief(
      'the chief to be ready',
      async () => (await chiefLane())?.activity?.state === 'idle',
      240_000,
    )
    pane = (await chiefLane()).pane
    await settled(() => app.output(pane.id).length)
    note(`chief (${chief}) ready; typing the prompt`)
    /** Whether the chief's window went to work after `at`, as the daemon's trace has it. */
    const workedSince = (at) => {
      let trace = ''
      try {
        trace = readFileSync(join(app.env.CONSENSFLOW_HOME, 'events.jsonl'), 'utf8')
      } catch {
        return false
      }
      return trace.split('\n').some((line) => {
        if (!line.includes('"window.activity"') || !line.includes('"working"')) return false
        const event = JSON.parse(line)
        return event.participant === 'chief' && event.state === 'working' && event.at >= at
      })
    }
    /** Type into the chief's terminal as the owner would, Enter pressed again while the window keeps the text. */
    const say = async (text) => {
      await app.waitFor(async () => (await chiefLane())?.activity?.state === 'idle', 240_000)
      const typed = new Date().toISOString()
      await app.tell(project, text, { idleMs: 240_000 })
      // Devin takes a pasted prompt into its box and waits for an Enter of its
      // own; Codex on Windows once took the second Enter as a new line too. A
      // window that went to work took the text, however soon it was done.
      for (let more = 0; more < 3; more += 1) {
        await sleep(5_000)
        if (workedSince(typed) || (await chiefLane())?.activity?.state !== 'idle') return
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
      // the owner answers it there. A Devin chief's question the scenario
      // has an answer for gets it typed into the dialog's Other; any other,
      // and any other harness's dialog, Enter: its first option.
      if (
        lane?.activity?.state === 'waiting' &&
        pickerAnswers < MAX_TERMINAL_REPLIES &&
        Date.now() - lastChange > PICKER_AFTER_MS
      ) {
        pickerAnswers += 1
        note(
          `the chief's question dialog waits. Its screen: ${lastLines(app.output(pane.id)).slice(-6).join(' ⏎ ')}`,
        )
        const questions = chief === 'devin' ? devinChiefQuestions(file, ENV) : null
        const replies =
          questions === null
            ? [{ question: null, answer: null, keys: [[13]] }]
            : devinPickerAnswers(scenario, questions, onWindows(ENV) ? consoleText : undefined)
        for (const { question, answer, keys } of replies) {
          note(
            answer === null
              ? `the owner took the first option there${question ? ` for "${question}"` : ''}`
              : `the owner answered "${question}" there: ${answer}`,
          )
          for (const bytes of keys) {
            await app.request('pane.input', { id: pane.id, generation: pane.generation, bytes })
            await sleep(KEY_PAUSE_MS)
          }
          await sleep(QUESTION_PAUSE_MS)
        }
        lastChange = Date.now()
        continue
      }
      // A chief that asked in its terminal hears the owner there once the
      // board is quiet: its newest question since the owner last typed
      // answered from the scenario's answers, once, the same way in every arm.
      if (
        !busy &&
        terminalReplies < MAX_TERMINAL_REPLIES &&
        Date.now() - lastChange > FOLLOW_UP_AFTER_MS
      ) {
        const end = chiefOpenQuestion(file)
        if (end !== undefined && !repliedTo.has(end.id)) {
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
          // The owner switches the chief from its card; the new window
          // takes the handoff first, then the owner goes on in it.
          const agent = `eval-${values['switch-to']}-worker`
          const before = pane.generation
          note(`the owner switches the chief to ${values['switch-to']} (${agent})`)
          const reply = await app.requestNode('chief.switch', { project, agent, when: 'turn' })
          if (reply?.ok === false) throw new Error(`chief.switch: ${JSON.stringify(reply)}`)
          await waitForChief(
            "the new chief's window",
            async () => {
              const current = (await chiefLane())?.pane
              return current !== null && current !== undefined && current.generation !== before
            },
            240_000,
          )
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
        // A harness's record keeps only what it finished: a member's turn
        // that never ends shows only on its screen.
        for (const member of current.lanes) {
          if (member.participant.role === 'chief' || member.activity?.state !== 'working') continue
          if (!member.pane) continue
          note(
            `@${member.participant.handle} still works. Its screen: ${lastLines(app.output(member.pane.id)).slice(-20).join(' ⏎ ')}`,
          )
        }
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
  const app = await startIntegration({ daemon: DAEMON, fakeEnv: ENV })
  // Which daemon the run is on (CONSENSFLOW_TEST_DAEMON, tests/choice.mjs): what
  // the Windows matrix reads back to say what it ran against.
  process.stdout.write(`daemon: ${app.daemon.kind} (${app.daemon.runtime})\n`)
  await trustWorkspace(app)
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
      // Devin takes a pasted prompt into its box and waits for an Enter of its
      // own; Codex on Windows once took the second Enter as a new line too.
      for (let more = 0; more < 3 && ((await record())?.items.length ?? 0) === before; more += 1) {
        await app.request('pane.input', { ...pane, bytes: [13] })
        note('Enter pressed again: the window had not taken the text')
        await sleep(5_000)
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
