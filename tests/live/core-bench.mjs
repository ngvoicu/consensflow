#!/usr/bin/env node
/**
 * The live bench (VERIFY-BDC-08): the daemon, the real Rust
 * pane host and the REAL harness TUIs on cheap models, driven by code.
 *
 * The human gives the chief one task per worker: run `cf task add` for its tier;
 * the daemon picks the worker (the free one of that tier with the fewest tasks
 * so far, the earliest joined first), which the steps below lean on. The
 * chief (OpenCode on the free Muse Spark model by default; `--chief claude` for a
 * Claude Code chief on Sonnet) must run it itself; the daemon opens the worker's
 * window with the task, the worker must answer in full-permission mode, the
 * core must record the answer as the task's result and deliver it into the
 * chief's window, and the worker must read idle. Then the app restarts and the
 * project must come back on its chief's own conversation.
 *
 * State lives in a throwaway home; the harnesses use the real logins. Opt-in,
 * never part of `npm test`:
 *
 *   npm run bench:core [-- [--chief claude|opencode] opencode pi devin claude codex]
 */
import { mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { startIntegration } from '../integration/harness.mjs'
import { trustForClaude } from './trust-claude.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const DAEMON = join(HERE, 'core-live-daemon.mjs')
const H = process.env.HOME
const WORKSPACE = join(H, '.consensflow-candidate', 'bench', 'workspace')

// One cheap model per harness (brain: operations/test-models.md).
/** What each harness calls its question tool, for the worker's brief; none for Pi. */
const QUESTION_TOOL = {
  claude: 'AskUserQuestion tool',
  opencode: 'question tool',
  codex: 'request_user_input tool',
  devin: 'ask_user_question tool',
}

// Each harness's worker keeps a tier of its own: the daemon picks a member
// by tier alone, and a step that needs one harness's question tool must land
// on that harness. Critical work names a purpose; the flag carries it.
const AGENTS = {
  claude: {
    id: 'bench-claude',
    kind: 'claude-code',
    model: 'claude-sonnet-5',
    workTier: 'complex',
  },
  opencode: {
    id: 'bench-opencode',
    kind: 'opencode',
    model: 'opencode/muse-spark-1.3-contributor-free',
    workTier: 'light',
  },
  pi: {
    id: 'bench-pi',
    kind: 'pi',
    model: 'openrouter/meta/muse-spark-1.3',
    workTier: 'standard',
  },
  devin: { id: 'bench-devin', kind: 'devin', model: 'swe-1-6-slow', workTier: 'critical' },
  codex: { id: 'bench-codex', kind: 'codex', model: 'gpt-5.6-luna', workTier: 'critical' },
}
const tierFlag = (tier) =>
  tier === 'critical' ? '--tier critical --purpose hard-problem' : `--tier ${tier}`
const args = process.argv.slice(2)
const chiefAt = args.indexOf('--chief')
const CHIEF = chiefAt === -1 ? 'opencode' : args[chiefAt + 1]
const named =
  chiefAt === -1 ? args : args.filter((_arg, index) => index !== chiefAt && index !== chiefAt + 1)
const reviewerAt = named.indexOf('--reviewer')
const REVIEWER = reviewerAt === -1 ? 'devin' : named[reviewerAt + 1]
const workers =
  reviewerAt === -1
    ? named
    : named.filter((_arg, index) => index !== reviewerAt && index !== reviewerAt + 1)
const wanted = workers.length ? workers : ['opencode', 'pi', 'devin']
if (!AGENTS[REVIEWER]) throw new Error(`unsupported bench reviewer: ${REVIEWER}`)
if (!['claude', 'opencode'].includes(CHIEF)) throw new Error(`unsupported bench chief: ${CHIEF}`)

// A clean environment: never this shell's Claude session identity.
const ENV = {
  HOME: H,
  USER: process.env.USER,
  LOGNAME: process.env.USER,
  LANG: 'en_US.UTF-8',
  TERM: 'xterm-256color',
  PATH: [
    join(H, '.local', 'bin'),
    join(H, '.opencode', 'bin'),
    '/opt/homebrew/bin',
    '/usr/bin',
    '/bin',
    '/usr/sbin',
    '/sbin',
  ].join(':'),
  // Unset on purpose (null removes the harness's sandbox default): with it
  // set, Claude keeps its global config inside that directory instead of
  // `~/.claude.json`, finds no completed onboarding there, and opens on the
  // first-run dialog, never on a prompt. The app never sets it for panes.
  CLAUDE_CONFIG_DIR: null,
  CODEX_HOME: join(H, '.codex'),
  XDG_CONFIG_HOME: join(H, '.config'),
  ANTHROPIC_MODEL: 'claude-sonnet-5',
  OPENCODE_CONFIG_CONTENT: JSON.stringify({ model: 'opencode/muse-spark-1.3-contributor-free' }),
}

const report = []
const record = (check, ok, detail = {}) => {
  report.push({ check, ok, ...detail })
  process.stdout.write(`${ok ? 'PASS' : 'FAIL'} ${check} ${JSON.stringify(detail)}\n`)
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
async function until(predicate, timeoutMs, stepMs = 1000) {
  const end = Date.now() + timeoutMs
  while (Date.now() < end) {
    const value = await predicate()
    if (value) return value
    await sleep(stepMs)
  }
  return null
}

mkdirSync(WORKSPACE, { recursive: true })
// The bench measures delivery, not judgment: a chief left to guess its job
// explores for minutes after every result, and each delivery waits for that.
const BRIEF =
  'This folder is an automated ConsensFlow bench. Do exactly what each message asks and ' +
  'nothing more. When a ConsensFlow message arrives, reply with one line that names it ' +
  'and run no tools unless the message tells you to run a command.\n'
for (const name of ['AGENTS.md', 'CLAUDE.md']) writeFileSync(join(WORKSPACE, name), BRIEF)

const writeRoster = (home) =>
  writeFileSync(
    join(home, 'agents.json'),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        agents: [
          ...wanted.map((name) => AGENTS[name]),
          { ...AGENTS[REVIEWER], id: 'bench-reviewer' },
          // The chief runs on a saved agent: its harness's cheap model.
          { ...AGENTS[CHIEF], id: 'bench-chief' },
        ],
      },
      null,
      2,
    )}\n`,
  )

let app = await startIntegration({ daemon: DAEMON, fakeEnv: ENV })
process.stdout.write(
  `trust: ${await trustForClaude(app, WORKSPACE, join(H, '.local', 'bin', 'claude'))}\n`,
)
const root = app.root
try {
  writeRoster(app.env.CONSENSFLOW_HOME)
  // The baseline measures delivery alone; a review is its own scenario below.
  const opened = await app.requestNode('project.open', {
    directory: WORKSPACE,
    agent: 'bench-chief',
  })
  if (opened.ok !== true) throw new Error(`project.open: ${JSON.stringify(opened)}`)
  const project = opened.project.id
  // Each task names its worker's tier, and each bench worker has its own.
  const tiers = {}
  for (const name of wanted) {
    const added = await app.requestNode('member.add', { project, agent: AGENTS[name].id })
    if (added.ok !== true) throw new Error(`member.add ${name}: ${JSON.stringify(added)}`)
    tiers[name] = added.member.tier
  }
  const board = async () => (await app.requestNode('board.get', { project })).board
  // A member's work runs in a session of its own: its lane is its newest session's.
  const lane = async (handle) =>
    (await board()).lanes.findLast(
      (l) => l.participant.member === handle || l.participant.handle === handle,
    )
  /** The window a member's newest session runs in, for the pane host's output. */
  const paneOf = async (handle) =>
    `p${project}-${(await lane(handle))?.participant.handle ?? handle}`
  const inbox = async (participant) =>
    (await app.requestNode('inbox.get', { project, participant })).messages

  const chief = await until(async () => {
    const chiefLane = await lane('chief')
    return chiefLane?.activity?.state === 'idle' ? chiefLane : null
  }, 180_000)
  record('chief-ready', Boolean(chief), { chief: CHIEF, activity: (await lane('chief'))?.activity })

  for (const name of wanted) {
    const agent = AGENTS[name]
    const marker = `BENCH_OK_${name.toUpperCase()}`
    const started = Date.now()
    await app.tell(
      project,
      `Run exactly this command in your shell, then reply with one line:\ncf task add ${tierFlag(tiers[name])} "Reply with exactly: ${marker}"\nWhen its result arrives, do not accept it yet: reply with one line and wait for my next message.`,
      { idleMs: 300_000 },
    )
    const task = await until(
      async () => (await lane(agent.id))?.tasks.find((t) => t.requester === 'chief'),
      300_000,
    )
    record(`${name}-dispatched-by-chief`, Boolean(task), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(task ? { task: task.number } : { chiefActivity: (await lane('chief'))?.activity }),
    })
    if (!task) continue
    const done = await until(async () => {
      const current = (await lane(agent.id))?.tasks.find((t) => t.number === task.number)
      return current?.state === 'done' ? current : null
    }, 300_000)
    const result = done
      ? (await inbox('chief')).find((m) => m.kind === 'result' && m.taskNumber === task.number)
      : null
    record(`${name}-answered`, Boolean(done && result?.body.includes(marker)), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(done
        ? { result: result?.body.slice(0, 80) }
        : {
            worker: (await lane(agent.id))?.activity,
            state: (await lane(agent.id))?.tasks.find((t) => t.number === task.number)?.state,
            exits: app.exits.filter((exit) => exit.id === `p${project}-${agent.id}`),
            output: app.output(await paneOf(agent.id)).slice(-1500),
          }),
    })
    if (!result) continue
    const delivered = await until(
      async () => (await inbox('chief')).find((m) => m.id === result.id && m.state === 'delivered'),
      300_000,
    )
    record(`${name}-received-by-chief`, Boolean(delivered), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(delivered
        ? {}
        : {
            state: (await inbox('chief')).find((m) => m.id === result.id),
            chief: (await lane('chief'))?.activity,
          }),
    })
    // One task per session: the worker's window closes once its task is done.
    const closed = await until(
      async () => ((await lane(agent.id))?.activity?.state === 'closed' ? true : null),
      60_000,
    )
    record(
      `${name}-window-closed`,
      Boolean(closed),
      closed ? {} : { activity: (await lane(agent.id))?.activity },
    )
    // Continuation: the chief sends a follow-up to the window that did the
    // task; it comes back on its own conversation and answers again.
    {
      const again = `BENCH_AGAIN_${name.toUpperCase()}`
      const begun = Date.now()
      await app.tell(
        project,
        `Run exactly this command in your shell, then reply with one line:\ncf task add --after T-${task.number} "Reply with exactly: ${again}"`,
        { idleMs: 300_000 },
      )
      const follow = await until(
        async () =>
          (await board()).lanes
            .flatMap((l) => l.tasks)
            .find((t) => t.requester === 'chief' && t.number > task.number && t.pool === null),
        300_000,
      )
      record(`${name}-continued-in-same-window`, follow?.assignee === task.assignee, {
        seconds: Math.round((Date.now() - begun) / 1000),
        ...(follow
          ? { task: follow.number, window: follow.assignee }
          : { chief: (await lane('chief'))?.activity }),
      })
      if (follow) {
        const finished = await until(async () => {
          const current = (await board()).lanes
            .flatMap((l) => l.tasks)
            .find((t) => t.number === follow.number)
          return current?.state === 'done' ? current : null
        }, 300_000)
        const answered = finished
          ? (await inbox('chief')).find(
              (m) => m.kind === 'result' && m.taskNumber === follow.number,
            )
          : null
        record(`${name}-continued-answered`, Boolean(answered?.body.includes(again)), {
          seconds: Math.round((Date.now() - begun) / 1000),
          ...(answered
            ? { result: answered.body.slice(0, 80) }
            : {
                state: (await lane(agent.id))?.tasks.find((t) => t.number === follow.number)?.state,
                output: app.output(await paneOf(agent.id)).slice(-1200),
              }),
        })
      }
    }
  }

  // The question door, live: a worker asks through its harness's own question
  // tool; the door puts the question in the chief's inbox; the chief answers
  // with `cf answer`; the door hands the answer back and the worker finishes.
  for (const name of wanted.filter((candidate) => QUESTION_TOOL[candidate])) {
    const started = Date.now()
    const worker = AGENTS[name]
    await app.tell(
      project,
      `Run exactly this command in your shell, then reply with one line:\ncf task add ${tierFlag(tiers[name])} "Use your ${QUESTION_TOOL[name]} to ask me which colour I prefer, with the options red and blue. After I answer, reply with exactly one line: COLOUR=<the answer>"`,
      { idleMs: 300_000 },
    )
    const question = await until(
      async () =>
        (await inbox('chief')).find(
          (m) =>
            m.kind === 'question' &&
            m.sender !== null &&
            m.sender.startsWith(worker.id) &&
            m.questions !== null,
        ),
      300_000,
    )
    record(`${name}-question-asked`, Boolean(question), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(question
        ? { options: question.questions[0].options.map((o) => o.label) }
        : {
            chief: (await lane('chief'))?.activity,
            worker: (await lane(worker.id))?.activity,
            output: app.output(await paneOf(worker.id)).slice(-1200),
          }),
    })
    if (!question) continue
    const answer = await until(async () => {
      const found = (await inbox(question.sender)).find((m) => m.replyTo === question.id)
      return found?.state === 'read' ? found : null
    }, 300_000)
    record(`${name}-question-answered-by-chief`, Boolean(answer), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(answer
        ? { choices: answer.choices, from: answer.sender }
        : {
            chief: (await lane('chief'))?.activity,
            output: app.output(`p${project}-chief`).slice(-1200),
          }),
    })
    const done = await until(async () => {
      const current = (await lane(worker.id))?.tasks.find((t) => t.number === question.taskNumber)
      return current?.state === 'done' ? current : null
    }, 300_000)
    const result = done
      ? (await inbox('chief')).find(
          (m) => m.kind === 'result' && m.taskNumber === question.taskNumber,
        )
      : null
    const label = answer?.choices?.[0]?.[0]
    record(
      `${name}-question-returned-to-worker`,
      Boolean(
        result && label && result.body.toLowerCase().includes(`colour=${label.toLowerCase()}`),
      ),
      {
        seconds: Math.round((Date.now() - started) / 1000),
        ...(result
          ? { result: result.body.slice(0, 80), label }
          : {
              state: (await lane(worker.id))?.tasks.find((t) => t.number === question.taskNumber)
                ?.state,
              output: app.output(await paneOf(worker.id)).slice(-1200),
            }),
      },
    )
  }

  // A review, live: the chief puts it on the board for the reviewer's tier
  // like any task, the reviewer takes it in a session of its own, and its
  // findings come back to the chief as the result. Nothing is reviewed unless
  // the chief asks.
  {
    const started = Date.now()
    const marker = 'BENCH_REVIEW_OK'
    // Only the task the chief creates from here on counts, not the baseline's.
    const before = Math.max(
      0,
      ...(await board()).lanes.flatMap((l) => l.tasks.map((t) => t.number)),
    )
    const reviewer = await app.requestNode('member.add', {
      project,
      agent: 'bench-reviewer',
      roles: ['reviewer'],
    })
    record('review-setup', reviewer.ok === true, {
      reviewer: REVIEWER,
      ...(reviewer.ok ? { tier: reviewer.member.tier } : { error: reviewer.error }),
    })
    const tier = reviewer.member?.tier
    const flag =
      tier === 'critical' ? '--tier critical --purpose critical-review' : `--tier ${tier}`
    await app.tell(
      project,
      `Run exactly this command in your shell, then reply with one line:\ncf task add --review ${flag} "Review this one-line result of T-1 for spelling: BENCH_OK_PI. No files are involved. Reply with exactly: ${marker}"`,
      { idleMs: 300_000 },
    )
    const review = await until(
      async () =>
        (await board()).lanes
          .flatMap((l) => l.tasks)
          .find((t) => t.number > before && t.pool === 'reviewer' && t.state === 'done'),
      300_000,
    )
    record('review-finished', Boolean(review), {
      seconds: Math.round((Date.now() - started) / 1000),
      reviewer: review?.assignee ?? (await lane('bench-reviewer'))?.activity,
    })
    const delivered = review
      ? await until(
          async () =>
            (await inbox('chief')).find(
              (m) =>
                m.kind === 'result' && m.taskNumber === review.number && m.state === 'delivered',
            ),
          300_000,
        )
      : null
    record('review-received-by-chief', Boolean(delivered?.body.includes(marker)), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(delivered ? { findings: delivered.body.slice(0, 120) } : {}),
    })
  }

  // Restart: a new daemon and pane host over the same home, in the app's quit
  // order. The project must come back on its chief's own conversation.
  const chiefFrame = app.openFrames.find((frame) => frame.id === `p${project}-chief`)
  app.killDaemon()
  await until(() => app.daemonExited(), 10_000, 100)
  await app.close({ preserveRoot: true })
  app = await startIntegration({ daemon: DAEMON, fakeEnv: ENV, existingRoot: root })
  const back = await until(async () => {
    const projects = (await app.requestNode('projects.list', {})).projects
    return projects.find((s) => s.id === project)?.state === 'open'
  }, 120_000)
  const reopened = await until(
    () => app.openFrames.find((frame) => frame.id === `p${project}-chief`),
    60_000,
  )
  record('restart-restores-project', Boolean(back && reopened), {
    back: Boolean(back),
    before: chiefFrame?.argv?.slice(-4),
    after: reopened?.argv?.slice(-4),
  })
} finally {
  await app.close()
  const slug = WORKSPACE.replace(/[^A-Za-z0-9]/g, '-')
  rmSync(join(H, '.claude', 'projects', slug), { recursive: true, force: true })
  rmSync(join(H, '.pi', 'agent', 'sessions', `--${WORKSPACE.slice(1).replaceAll('/', '-')}--`), {
    recursive: true,
    force: true,
  })
}

const failed = report.filter((row) => !row.ok)
process.stdout.write(`\n${report.length - failed.length}/${report.length} checks passed\n`)
process.exit(failed.length === 0 ? 0 : 1)
