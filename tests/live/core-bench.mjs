#!/usr/bin/env node
/**
 * The live bench for the new core (VERIFY-BDC-08): its daemon, the real Rust
 * pane host and the REAL harness TUIs on cheap models, driven by code.
 *
 * The human gives the lead one task per worker: run `cf task add` for its tier;
 * the daemon picks the worker (the free one of that tier with the fewest tasks
 * so far, the earliest joined first), which the steps below lean on. The
 * lead (OpenCode on the free Muse Spark model by default; `--lead claude` for a
 * Claude Code lead on Sonnet) must run it itself; the core opens the worker's
 * window with the task, the worker must answer in full-permission mode, the
 * core must record the answer as the task's result and deliver it into the
 * lead's window, and the worker must read idle. Then the app restarts and the
 * project must come back on its lead's own conversation.
 *
 * State lives in a throwaway home; the harnesses use the real logins. Opt-in,
 * never part of `npm test`:
 *
 *   npm run bench:core [-- [--lead claude|opencode] opencode pi devin claude codex]
 */
import { execFileSync } from 'node:child_process'
import { mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { startIntegration } from '../integration/harness.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const EDITOR = join(HERE, 'core-live-editor.mjs')
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

const AGENTS = {
  claude: { id: 'bench-claude', kind: 'claude-code', model: 'claude-sonnet-5' },
  opencode: {
    id: 'bench-opencode',
    kind: 'opencode',
    model: 'opencode/muse-spark-1.3-contributor-free',
  },
  pi: { id: 'bench-pi', kind: 'pi', model: 'opencode-go/muse-spark-1.3-contributor' },
  devin: { id: 'bench-devin', kind: 'devin', model: 'swe-1-6-slow' },
  codex: { id: 'bench-codex', kind: 'codex', model: 'gpt-5.6-luna' },
}
const args = process.argv.slice(2)
const leadAt = args.indexOf('--lead')
const LEAD = leadAt === -1 ? 'opencode' : args[leadAt + 1]
const named =
  leadAt === -1 ? args : args.filter((_arg, index) => index !== leadAt && index !== leadAt + 1)
const reviewerAt = named.indexOf('--reviewer')
const REVIEWER = reviewerAt === -1 ? 'devin' : named[reviewerAt + 1]
const workers =
  reviewerAt === -1
    ? named
    : named.filter((_arg, index) => index !== reviewerAt && index !== reviewerAt + 1)
const wanted = workers.length ? workers : ['opencode', 'pi', 'devin']
if (!AGENTS[REVIEWER]) throw new Error(`unsupported bench reviewer: ${REVIEWER}`)
const LEAD_KIND = { claude: 'claude-code', opencode: 'opencode' }[LEAD]
if (!LEAD_KIND) throw new Error(`unsupported bench lead: ${LEAD}`)

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
// The bench measures delivery, not judgment: a lead left to guess its job
// explores for minutes after every result, and each delivery waits for that.
const BRIEF =
  'This folder is an automated ConsensFlow bench. Do exactly what each message asks and ' +
  'nothing more. When a ConsensFlow message arrives, reply with one line that names it ' +
  'and run no tools unless the message tells you to run a command.\n'
for (const name of ['AGENTS.md', 'CLAUDE.md']) writeFileSync(join(WORKSPACE, name), BRIEF)
process.stdout.write(
  `trust: ${execFileSync('python3', [join(HERE, 'trust-claude-folder.py'), WORKSPACE], { encoding: 'utf8' }).trim()}\n`,
)

const writeRoster = (home) =>
  writeFileSync(
    join(home, 'agents.json'),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        agents: [
          ...wanted.map((name) => AGENTS[name]),
          { ...AGENTS[REVIEWER], id: 'bench-reviewer' },
        ],
      },
      null,
      2,
    )}\n`,
  )

let app = await startIntegration({ editor: EDITOR, fakeEnv: ENV })
const root = app.root
try {
  writeRoster(app.env.CONSENSFLOW_HOME)
  // The baseline measures delivery alone; the review gate is its own scenario below.
  const opened = await app.requestNode('project.open', {
    directory: WORKSPACE,
    harness: LEAD_KIND,
    review: 'none',
  })
  if (opened.ok !== true) throw new Error(`project.open: ${JSON.stringify(opened)}`)
  const project = opened.project.id
  // Each task names its worker's tier. Two bench workers of one tier are told
  // apart by the daemon's own rule, fewest tasks taken then join order: the
  // per-worker steps run in join order, one task at a time, so each round
  // goes around the tier's workers in that order; a scenario that cannot
  // count on a balanced round follows the task to whichever worker got it.
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

  const lead = await until(async () => {
    const leadLane = await lane('lead')
    return leadLane?.activity?.state === 'idle' ? leadLane : null
  }, 180_000)
  record('lead-ready', Boolean(lead), { lead: LEAD, activity: (await lane('lead'))?.activity })

  for (const name of wanted) {
    const agent = AGENTS[name]
    const marker = `BENCH_OK_${name.toUpperCase()}`
    const started = Date.now()
    await app.requestNode('task.add', {
      project,
      to: 'lead',
      body: `Run exactly this command in your shell, then reply with one line:\ncf task add --tier ${tiers[name]} "Reply with exactly: ${marker}"`,
    })
    const task = await until(
      async () => (await lane(agent.id))?.tasks.find((t) => t.requester === 'lead'),
      300_000,
    )
    record(`${name}-dispatched-by-lead`, Boolean(task), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(task ? { task: task.number } : { leadActivity: (await lane('lead'))?.activity }),
    })
    if (!task) continue
    const done = await until(async () => {
      const current = (await lane(agent.id))?.tasks.find((t) => t.number === task.number)
      return current?.state === 'done' ? current : null
    }, 300_000)
    const result = done
      ? (await inbox('lead')).find((m) => m.kind === 'result' && m.taskNumber === task.number)
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
      async () => (await inbox('lead')).find((m) => m.id === result.id && m.state === 'delivered'),
      300_000,
    )
    record(`${name}-received-by-lead`, Boolean(delivered), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(delivered
        ? {}
        : {
            state: (await inbox('lead')).find((m) => m.id === result.id),
            lead: (await lane('lead'))?.activity,
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
    // Continuation: the lead sends a follow-up to the window that did the
    // task; it comes back on its own conversation and answers again.
    {
      const again = `BENCH_AGAIN_${name.toUpperCase()}`
      const begun = Date.now()
      await app.requestNode('task.add', {
        project,
        to: 'lead',
        body: `Run exactly this command in your shell, then reply with one line:\ncf task add --after T-${task.number} "Reply with exactly: ${again}"`,
      })
      const follow = await until(
        async () =>
          (await board()).lanes
            .flatMap((l) => l.tasks)
            .find((t) => t.requester === 'lead' && t.number > task.number && t.pool === null),
        300_000,
      )
      record(`${name}-continued-in-same-window`, follow?.assignee === task.assignee, {
        seconds: Math.round((Date.now() - begun) / 1000),
        ...(follow
          ? { task: follow.number, window: follow.assignee }
          : { lead: (await lane('lead'))?.activity }),
      })
      if (follow) {
        const finished = await until(async () => {
          const current = (await board()).lanes
            .flatMap((l) => l.tasks)
            .find((t) => t.number === follow.number)
          return current?.state === 'done' ? current : null
        }, 300_000)
        const answered = finished
          ? (await inbox('lead')).find((m) => m.kind === 'result' && m.taskNumber === follow.number)
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
  // tool; the door puts the question in the lead's inbox; the lead answers
  // with `cf answer`; the door hands the answer back and the worker finishes.
  for (const name of wanted.filter((candidate) => QUESTION_TOOL[candidate])) {
    const started = Date.now()
    const worker = AGENTS[name]
    // A lead may put the worker's question to the human, whose preference it
    // is; the bench answers as the human would, on the board, and says so.
    const humanAnswers = []
    const answerAsHuman = async () => {
      for (const m of await inbox('human')) {
        if (m.kind !== 'question' || m.state !== 'queued') continue
        const answered = await app.requestNode('message.answer', {
          question: m.id,
          ...(m.questions
            ? { choices: m.questions.map((q) => [q.options[0]?.label ?? 'blue']) }
            : { body: 'blue' }),
        })
        await app.requestNode('message.read', { message: m.id })
        humanAnswers.push({
          id: m.id,
          from: m.sender,
          options: m.questions !== null,
          ok: answered.ok,
        })
      }
    }
    await app.requestNode('task.add', {
      project,
      to: 'lead',
      body: `Run exactly this command in your shell, then reply with one line:\ncf task add --tier ${tiers[name]} "Use your ${QUESTION_TOOL[name]} to ask me which colour I prefer, with the options red and blue. After I answer, reply with exactly one line: COLOUR=<the answer>"`,
    })
    const question = await until(
      async () =>
        (await inbox('lead')).find(
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
            lead: (await lane('lead'))?.activity,
            worker: (await lane(worker.id))?.activity,
            output: app.output(await paneOf(worker.id)).slice(-1200),
          }),
    })
    if (!question) continue
    const answer = await until(async () => {
      await answerAsHuman()
      const found = (await inbox(question.sender)).find((m) => m.replyTo === question.id)
      return found?.state === 'read' ? found : null
    }, 300_000)
    if (humanAnswers.length > 0) {
      record(
        `${name}-question-forwarded-to-human`,
        humanAnswers.every((a) => a.ok),
        {
          answered: humanAnswers,
        },
      )
    }
    record(`${name}-question-answered-by-lead`, Boolean(answer), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(answer
        ? { choices: answer.choices, from: answer.sender }
        : {
            lead: (await lane('lead'))?.activity,
            output: app.output(`p${project}-lead`).slice(-1200),
          }),
    })
    const done = await until(async () => {
      await answerAsHuman()
      const current = (await lane(worker.id))?.tasks.find((t) => t.number === question.taskNumber)
      return current?.state === 'done' ? current : null
    }, 300_000)
    const result = done
      ? (await inbox('lead')).find(
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

  // The review gate, live: a reviewer on another model judges one worker's
  // result before the lead sees it. The daemon picks the worker (the one
  // with the fewest tasks so far, so not necessarily the first) and the
  // reviewer; if every worker shares the reviewer's model the work goes on
  // unreviewed, and the check says so.
  {
    const started = Date.now()
    const marker = 'BENCH_REVIEW_OK'
    // Only the task the lead creates from here on counts, not the baseline's.
    const before = Math.max(
      0,
      ...(await board()).lanes.flatMap((l) => l.tasks.map((t) => t.number)),
    )
    // The reviewer first: a policy with nobody to review is refused.
    const reviewer = await app.requestNode('member.add', {
      project,
      agent: 'bench-reviewer',
      roles: ['reviewer'],
    })
    const policy = await app.requestNode('project.review', { project, review: 'members' })
    record('review-setup', policy.ok === true && reviewer.ok === true, {
      reviewer: REVIEWER,
      ...(reviewer.ok ? {} : { error: reviewer.error }),
      ...(policy.ok ? {} : { error: policy.error }),
    })
    await app.requestNode('task.add', {
      project,
      to: 'lead',
      body: `Run exactly this command in your shell, then reply with one line:\ncf task add --tier ${tiers[wanted[0]]} "Reply with exactly: ${marker}"`,
    })
    const reviewed = await until(
      async () =>
        (await board()).lanes
          .filter((l) => l.participant.agent !== null && !l.participant.roles.includes('reviewer'))
          .flatMap((l) => l.tasks)
          .find(
            (t) => t.number > before && t.kind === 'work' && ['review', 'done'].includes(t.state),
          ),
      300_000,
    )
    record('review-work-finished', Boolean(reviewed), {
      seconds: Math.round((Date.now() - started) / 1000),
      state: reviewed?.state,
      worker: reviewed?.assignee,
    })
    const review = reviewed
      ? await until(
          async () =>
            (await lane('bench-reviewer'))?.tasks.find((t) => t.reviewOf === reviewed.number),
          120_000,
        )
      : null
    record('review-created', Boolean(review), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(review
        ? { review: review.number }
        : { notes: (await inbox('lead')).filter((m) => m.kind === 'note').map((m) => m.body) }),
    })
    const verdict = review
      ? await until(async () => {
          const current = (await lane('bench-reviewer'))?.tasks.find(
            (t) => t.number === review.number,
          )
          return current?.state === 'done' ? current : null
        }, 300_000)
      : null
    record('review-verdict', Boolean(verdict?.verdict), {
      seconds: Math.round((Date.now() - started) / 1000),
      verdict: verdict?.verdict ?? null,
      ...(verdict ? {} : { reviewer: (await lane('bench-reviewer'))?.activity }),
    })
    // One delivery: the result reaches the lead with the verdict under it;
    // the reviewer's findings stay on the review task, for the board.
    const delivered = verdict
      ? await until(async () => {
          const result = (await inbox('lead')).find(
            (m) =>
              m.kind === 'result' && m.taskNumber === reviewed.number && m.state === 'delivered',
          )
          const findings = (
            await app.requestNode('task.get', { project, task: review.number })
          ).task?.messages.find((m) => m.kind === 'result')
          return result && findings ? { result, findings } : null
        }, 300_000)
      : null
    record('review-received-by-lead', Boolean(delivered), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(delivered ? { findings: delivered.findings.body.slice(0, 120) } : {}),
    })
  }

  // Restart: a new daemon and pane host over the same home, in the app's quit
  // order. The project must come back on its lead's own conversation.
  const leadFrame = app.openFrames.find((frame) => frame.id === `p${project}-lead`)
  app.killEditor()
  await until(() => app.uiExited(), 10_000, 100)
  await app.close({ preserveRoot: true })
  app = await startIntegration({ editor: EDITOR, fakeEnv: ENV, existingRoot: root })
  const back = await until(async () => {
    const projects = (await app.requestNode('projects.list', {})).projects
    return projects.find((s) => s.id === project)?.state === 'open'
  }, 120_000)
  const reopened = await until(
    () => app.openFrames.find((frame) => frame.id === `p${project}-lead`),
    60_000,
  )
  record('restart-restores-project', Boolean(back && reopened), {
    back: Boolean(back),
    before: leadFrame?.argv?.slice(-4),
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
