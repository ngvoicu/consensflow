/**
 * The rig of the live receipt and stop checks (`npm run live:door`,
 * `npm run live:stops`): the native daemon this checkout builds, the real Rust
 * pane host, and a real harness's window as a member of a project, driven by
 * code. The checks play the chief with its own token, so no chief's model is
 * in the loop: the chief's window is the rig's stand-in Claude, which only
 * notes what it is sent, and a question is answered, a task given, paused or
 * resumed at the moment a check chooses. What a check shows, it shows from what
 * the daemon wrote (its event file: every ledger event and every change of a
 * window's activity, as it happens), from the board's own reads, and from the
 * harness's own record and status.
 *
 * Needs `npm run build:bridge` and `npm run build:cf` first. State lives in a
 * throwaway home; the harnesses use the real logins. macOS and Linux only: the
 * stand-in's shim is a shell script, and processes are read with `ps`.
 */
import { execFile, execFileSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { liveEnvironment, realOnPath } from '../../evals/plan.mjs'
import { startIntegration } from '../integration/harness.mjs'
import { readRecord } from '../rust-harness.mjs'
import { AGENTS, QUESTION_TOOL, tierFlag } from './bench-agents.mjs'
import { CF, traceOf, useNativeDaemon, useNodeDaemon } from './native-daemon.mjs'
import { trustForClaude } from './trust-claude.mjs'

const REPO = fileURLToPath(new URL('../..', import.meta.url))
const FAKE_AGENT = join(REPO, 'tests', 'integration', 'fake-agent.mjs')
export const HOME = process.env.HOME ?? homedir()
/** Where a harness looks for its records: the live environment without what it removes. */
const RECORD_ENV = Object.fromEntries(
  Object.entries(liveEnvironment({ home: HOME })).filter(([, value]) => value !== null),
)

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The first truthy value `predicate` gives within `ms`, or null. */
export async function until(predicate, ms, step = 500) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    const value = await predicate()
    if (value) return value
    await sleep(step)
  }
  return null
}

/** The version a harness's own CLI reports, for the line that quotes a result. */
export function versionOf(harness) {
  const command = {
    claude: 'claude',
    codex: 'codex',
    opencode: 'opencode',
    pi: 'pi',
    devin: 'devin',
  }[harness]
  if (command === undefined) return 'stand-in'
  try {
    const file = realOnPath(command, liveEnvironment({ home: HOME }).PATH)
    return execFileSync(file, ['--version'], { encoding: 'utf8', timeout: 30_000 })
      .trim()
      .split('\n')[0]
  } catch (cause) {
    return `unknown (${cause.message.split('\n')[0]})`
  }
}

/** What a harness's member is told to do to ask its question, and how the stand-in is told to. */
export function questionBrief(harness) {
  if (harness === 'fake') {
    const questions = [
      {
        question: 'Which colour?',
        header: 'Colour',
        options: [{ label: 'red' }, { label: 'blue' }],
        multiSelect: false,
      },
    ]
    return `ASK ${JSON.stringify(questions)}`
  }
  return `Use your ${QUESTION_TOOL[harness]} to ask which colour to use, with the options red and blue. Do not pick one yourself: ask, and wait for the answer. Once it is answered, reply with exactly one line: COLOUR=<the answer>`
}

export const seconds = (ms) => (ms / 1000).toFixed(1)

/** The session a window opened on, from its open request's command line. */
export function sessionOf(open) {
  const flag = open.argv.findIndex((arg) => arg === '--session-id' || arg === '--resume')
  return flag === -1 ? null : open.argv[flag + 1]
}

/**
 * Claude's own status of a session, read as the daemon reads it (the
 * `sessions/<pid>.json` file that names the session) every 150 ms, until
 * stopped: when each read was made, the `status` word (`busy`, `idle`,
 * `waiting`), what it waits for, and when Claude says the status last
 * changed (`statusUpdatedAt`, which the daemon's look reads an interrupt's
 * effect by).
 */
export function claudeStatus(session) {
  const folder = join(HOME, '.claude', 'sessions')
  const samples = []
  const read = () => {
    try {
      for (const file of readdirSync(folder)) {
        if (!/^\d+\.json$/.test(file)) continue
        let row
        try {
          row = JSON.parse(readFileSync(join(folder, file), 'utf8'))
        } catch {
          continue
        }
        if (row.sessionId === session) return row
      }
    } catch {}
    return null
  }
  const timer = setInterval(() => {
    const row = read()
    samples.push({
      at: Date.now(),
      status: row?.status ?? null,
      waitingFor: row?.waitingFor ?? null,
      changedAt: row?.statusUpdatedAt ?? null,
    })
  }, 150)
  return { samples, stop: () => clearInterval(timer) }
}

/** Runs of samples that read alike, each as a line: `busy 0.0–12.3 s (82 reads)`. */
export function runs(samples, from) {
  const found = []
  for (const sample of samples) {
    const word = `${sample.status ?? 'no status file'}${sample.waitingFor ? ` (${sample.waitingFor})` : ''}`
    const last = found.at(-1)
    if (last?.word === word) {
      last.to = sample.at
      last.count += 1
    } else found.push({ word, from: sample.at, to: sample.at, count: 1 })
  }
  return found.map(
    (run) =>
      `${run.word} ${seconds(run.from - from)}–${seconds(run.to - from)} s (${run.count} reads)`,
  )
}

/**
 * How the daemon's own record reader (`cf_harness::records`, asked through
 * `tests/rust-harness.mjs`) reads the transcript of a session: whether the
 * turn is settled, its settlement, its last item. What it says is what the
 * daemon's look at the window will say of its record. `env` says where Claude
 * keeps it: the live environment's, unless a test gives another.
 */
export async function claudeSettlement(session, env = RECORD_ENV) {
  const read = await readRecord('claude-code', session, env).catch(() => null)
  if (read === null) return null
  const last = read.items.at(-1)
  return {
    settled: read.settled,
    settlement: read.settlement,
    items: read.items.length,
    last: last ? `${last.role}: ${last.text.replace(/\s+/g, ' ').slice(0, 60)}` : null,
  }
}

/** Where Claude keeps the transcript of a session (under `~/.claude/projects`), or null. */
export function claudeTranscript(session) {
  const root = join(HOME, '.claude', 'projects')
  try {
    for (const folder of readdirSync(root)) {
      const candidate = join(root, folder, `${session}.jsonl`)
      if (existsSync(candidate)) return candidate
    }
  } catch {}
  return null
}

/**
 * What Claude kept of a conversation: the records of its transcript, oldest
 * first, each as `{at, type, text, raw}`; the text is what the record says (a
 * message's words, a tool's call, a tool's result), `raw` the start of its
 * line, and `[]` where there is no transcript yet.
 */
export function claudeRecord(session) {
  const file = claudeTranscript(session)
  if (file === null) return []
  const shown = (part) => {
    if (part.type === 'text') return part.text
    if (part.type === 'tool_use') return `tool_use ${part.name} ${JSON.stringify(part.input)}`
    if (part.type === 'tool_result') {
      const said = typeof part.content === 'string' ? part.content : JSON.stringify(part.content)
      return `tool_result${part.is_error ? ' (error)' : ''} ${said}`
    }
    return part.type
  }
  const items = []
  for (const line of readFileSync(file, 'utf8').split('\n')) {
    if (line === '') continue
    let record
    try {
      record = JSON.parse(line)
    } catch {
      continue
    }
    const content = record.message?.content
    const parts =
      typeof content === 'string'
        ? [{ type: 'text', text: content }]
        : Array.isArray(content)
          ? content
          : []
    items.push({
      at: Date.parse(record.timestamp),
      type: `${record.message?.role ?? record.type}${record.subtype ? `/${record.subtype}` : ''}`,
      text: parts.map(shown).join(' ⏎ '),
      raw: line.slice(0, 400),
    })
  }
  return items
}

/** The ids of the processes whose command line holds `text`, from `ps`. */
export function processesWith(text) {
  const listed = execFileSync('ps', ['-axo', 'pid=,ppid=,command='], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  })
  return listed
    .split('\n')
    .map((line) => /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(line))
    .filter((found) => found?.[3].includes(text) && !/\bps -axo\b/.test(found[3]))
    .map((found) => ({ pid: Number(found[1]), ppid: Number(found[2]), command: found[3] }))
}

/**
 * What the project's folder tells every window: the bench's rule (do what each
 * message asks and nothing more), but a message that names one of the window's
 * tools is asking for it. The bench's own words ("run no tools unless the
 * message tells you to run a command") had Claude decline to ask its question
 * with the tool the message named, for a tool is not a command.
 */
const WORKSPACE_BRIEF =
  'This folder is an automated ConsensFlow test. Do exactly what each message asks and ' +
  'nothing more. When a ConsensFlow message tells you to run a command or to use one of ' +
  'your tools, do it; otherwise run no tools, and reply with one line.\n'

/**
 * What a window's `claude` runs. The chief's window is the rig's stand-in
 * Claude (`tests/integration/fake-agent.mjs`): no model, no MCP server, and it
 * only notes what it is sent, so nothing answers a question or takes a task
 * but the check, which plays the chief with the chief's own token. The
 * stand-in takes the place of Claude on PATH, for the windows whose agent's
 * model is `fake` or `fake-chief` alone; every other window `exec`s the real
 * Claude Code, the same process under the pane, with the arguments it was given.
 * The stand-in writes its record and status where the daemon looks for
 * Claude's own, `~/.claude`.
 */
const SHIM = `#!/bin/sh
for arg in "$@"; do
  case "$arg" in
    fake | fake-chief) CLAUDE_CONFIG_DIR="$HOME/.claude" exec "$CF_TEST_NODE" "$CF_TEST_HARNESS" "$@" ;;
  esac
done
exec "$CF_REAL_CLAUDE" "$@"
`

/**
 * A project on the native daemon: its chief's window (the stand-in), and one
 * member for each of `workers` (harness names of the bench's agents, or `fake`
 * for the stand-in Claude that asks through the hook when its brief says
 * `ASK …`). `files` are written into the project's folder before anything
 * opens (a path under it, and its text): a Claude window reads the settings
 * there.
 */
export async function openRig({ folder, workers, model = {}, files = {}, daemon = 'native' }) {
  // The native daemon, which this is for; Node's only for a baseline of a case beside it.
  if (daemon === 'node') useNodeDaemon()
  else useNativeDaemon()
  const root = mkdtempSync(join(tmpdir(), 'consensflow-receipt-'))
  const home = join(root, 'consensflow')
  mkdirSync(home, { recursive: true })
  const live = liveEnvironment({ home: HOME })
  const realClaude = realOnPath('claude', live.PATH)
  const shims = join(root, 'shim-bin')
  mkdirSync(shims)
  writeFileSync(join(shims, 'claude'), SHIM, { mode: 0o755 })
  const env = {
    ...live,
    ANTHROPIC_MODEL: AGENTS.claude.model,
    OPENCODE_CONFIG_CONTENT: JSON.stringify({ model: model.opencode ?? AGENTS.opencode.model }),
    PATH: `${shims}:${live.PATH}`,
    CF_TEST_HARNESS: FAKE_AGENT,
    CF_REAL_CLAUDE: realClaude,
  }
  const agent = (name) => ({ ...AGENTS[name], ...(model[name] ? { model: model[name] } : {}) })
  const saved = [
    { id: 'chief', kind: 'claude-code', model: 'fake-chief' },
    ...workers.map((name) =>
      name === 'fake' ? { id: 'worker', kind: 'claude-code', model: 'fake' } : agent(name),
    ),
  ]
  writeFileSync(
    join(home, 'agents.json'),
    `${JSON.stringify({ schemaVersion: 1, agents: saved }, null, 2)}\n`,
  )
  const workspace = join(HOME, '.consensflow-candidate', 'live', folder)
  mkdirSync(workspace, { recursive: true })
  for (const name of ['AGENTS.md', 'CLAUDE.md'])
    writeFileSync(join(workspace, name), WORKSPACE_BRIEF)
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(workspace, path)), { recursive: true })
    writeFileSync(join(workspace, path), text)
  }

  const app = await startIntegration({ fakeEnv: env, existingRoot: root })
  const events = traceOf(home)
  const closing = []
  try {
    const trust = workers.includes('claude')
      ? await trustForClaude(app, workspace, realClaude)
      : null
    const opened = await app.requestNode('project.open', { directory: workspace, agent: 'chief' })
    if (opened.ok !== true) throw new Error(`project.open: ${JSON.stringify(opened)}`)
    const project = opened.project.id

    const board = async () => (await app.requestNode('board.get', { project })).board
    // A member's work runs in a session of its own: its lane is its newest session's.
    const lane = async (handle) =>
      (await board()).lanes.findLast(
        (candidate) =>
          candidate.participant.member === handle || candidate.participant.handle === handle,
      )
    const inbox = async (participant) =>
      (await app.requestNode('inbox.get', { project, participant })).messages
    /** A task as the board reads it, with its whole thread: each message's state and receipt. */
    const thread = async (number) =>
      (await app.requestNode('task.get', { project, task: number })).task

    const ready = await until(
      async () => (await lane('chief'))?.activity?.state === 'idle',
      180_000,
    )
    if (!ready)
      throw new Error(
        `the chief's window never went idle: ${JSON.stringify((await lane('chief'))?.activity)}`,
      )
    const chiefPane = `p${project}-chief`
    const members = {}
    for (const name of workers) {
      const agentId = name === 'fake' ? 'worker' : AGENTS[name].id
      const added = await app.requestNode('member.add', { project, agent: agentId })
      if (added.ok !== true) throw new Error(`member.add ${name}: ${JSON.stringify(added)}`)
      members[name] = { agent: agentId, tier: added.member.tier }
    }

    /** `cf` as the chief runs it: the chief's own window's environment and token. */
    const asChief = (words) =>
      new Promise((resolve) => {
        const frame = app.openFrames.findLast((open) => open.id === chiefPane)
        const child = execFile(
          CF,
          words,
          {
            env: {
              ...Object.fromEntries(Object.entries(env).filter(([, v]) => v !== null)),
              ...frame.env,
            },
            timeout: 60_000,
          },
          (error, stdout, stderr) =>
            resolve({
              code: error ? (error.code ?? 1) : 0,
              stdout: stdout.trim(),
              stderr: stderr.trim(),
            }),
        )
        child.stdin.end()
      })

    /** A task for `name`'s member, given by the chief: its number. */
    async function give(name, brief) {
      const flag = tierFlag(members[name].tier)
      const added = await asChief(['task', 'add', ...flag.split(' '), '--json', brief])
      let number = null
      try {
        const said = JSON.parse(added.stdout)
        number = said.task?.number ?? said.number ?? null
      } catch {}
      if (number === null) throw new Error(`cf task add: ${JSON.stringify(added)}`)
      return number
    }

    /** A member's window: its lane's participant and the pane the daemon opened it in. */
    async function windowOf(name) {
      const found = await lane(members[name].agent)
      if (!found?.pane) return null
      const open = app.openFrames.findLast((frame) => frame.id === found.pane.id)
      return {
        handle: found.participant.handle,
        pane: found.pane,
        open,
        activity: found.activity,
        lane: found,
      }
    }

    /** The daemon's own presses into a pane: when each Escape was seen sent. */
    function watchEscapes(paneId) {
      const seen = []
      let counted = 0
      const timer = setInterval(() => {
        for (; counted < app.nodeFrames.length; counted += 1) {
          const frame = app.nodeFrames[counted]
          if (
            frame.op === 'pane.input' &&
            frame.body?.id === paneId &&
            JSON.stringify(frame.body.bytes) === '[27]'
          ) {
            seen.push(Date.now())
          }
        }
      }, 25)
      closing.push(() => clearInterval(timer))
      return { seen, stop: () => clearInterval(timer) }
    }

    return {
      app,
      env,
      root,
      home,
      project,
      workspace,
      trust,
      members,
      board,
      lane,
      inbox,
      thread,
      events,
      asChief,
      give,
      windowOf,
      watchEscapes,
      close: shutDown,
    }
  } catch (cause) {
    await shutDown().catch(() => {})
    throw cause
  }

  /** Ends the daemon and its windows, and takes the stand-ins' records out of Claude's folder. */
  async function shutDown() {
    for (const stop of closing) stop()
    const stood = app.processes().map((found) => found.sessionId)
    await app.close()
    for (const session of stood) {
      rmSync(join(HOME, '.claude', 'projects', 'integration', `${session}.jsonl`), { force: true })
    }
    try {
      rmdirSync(join(HOME, '.claude', 'projects', 'integration'))
    } catch {}
  }
}

/** Removes what a Claude window kept of a folder, as the bench does (its `memory/` is read by the next run). */
export function forgetClaudeFolder(folder) {
  const slug = folder.replace(/[^A-Za-z0-9]/g, '-')
  rmSync(join(HOME, '.claude', 'projects', slug), { recursive: true, force: true })
}
