#!/usr/bin/env node
/**
 * The live bench: the production daemon, the real Rust pane host and the REAL
 * harness TUIs on cheap models, driven by code instead of a person.
 *
 * A lead (OpenCode on the free Muse Spark model by default; `--lead claude` for
 * a Claude Code lead on Sonnet) dispatches one task per worker harness, one at a
 * time. Each worker must answer in full-permission mode, its answer must be
 * recorded, the lead must receive it, and the worker's pane must read idle.
 * Then the app restarts, and the session must come back by itself.
 *
 * ConsensFlow's state lives in a throwaway home. The harnesses use the real
 * logins, so their own stores keep these sessions; the Claude and Pi ones are
 * removed afterwards. Opt-in, never part of `npm test`:
 *
 *   npm run bench:live [-- [--lead claude|opencode] opencode pi devin claude]
 *
 * Claude runs are opt-in: they spend the same weekly subscription limit the
 * human's own agents use.
 */
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { startIntegration } from '../integration/harness.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const EDITOR = join(HERE, 'live-editor.mjs')
const H = process.env.HOME
const WORKSPACE = join(H, '.consensflow-candidate', 'bench', 'workspace')

// One cheap model per harness (brain: operations/test-models.md). Kimi is
// paused; Codex waits for its quota.
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
const wanted = named.length ? named : ['opencode', 'pi', 'devin']
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
  CLAUDE_CONFIG_DIR: join(H, '.claude'),
  CODEX_HOME: join(H, '.codex'),
  XDG_CONFIG_HOME: join(H, '.config'),
  // A Claude lead runs on Sonnet; an OpenCode lead on the free Muse Spark.
  ANTHROPIC_MODEL: 'claude-sonnet-5',
  OPENCODE_CONFIG_CONTENT: JSON.stringify({ model: 'opencode/muse-spark-1.3-contributor-free' }),
}

const report = []
const record = (check, ok, detail = {}) => {
  const row = { check, ok, ...detail }
  report.push(row)
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
const readJson = (file) => {
  try {
    return JSON.parse(readFileSync(file, 'utf8'))
  } catch {
    return null
  }
}
/** Claude Code's own live status for a session, from its sessions/<pid>.json. */
function claudeStatus(sessionId) {
  const directory = join(H, '.claude', 'sessions')
  for (const name of existsSync(directory) ? readdirSync(directory) : []) {
    if (!/^\d+\.json$/.test(name)) continue
    const row = readJson(join(directory, name))
    if (row?.sessionId === sessionId) return row.status ?? null
  }
  return null
}
const writeRoster = (home) =>
  writeFileSync(
    join(home, 'agents.json'),
    `${JSON.stringify({ schemaVersion: 1, agents: wanted.map((name) => AGENTS[name]) }, null, 2)}\n`,
  )

mkdirSync(WORKSPACE, { recursive: true })
// The bench measures delivery, not judgment: a lead left to guess its job
// explores for minutes after every result, and each delivery waits for that.
const BRIEF =
  'This folder is an automated ConsensFlow bench. Do exactly what each message asks and ' +
  'nothing more. When a ConsensFlow delivery arrives, reply with one line that names it ' +
  'and run no tools.\n'
for (const name of ['AGENTS.md', 'CLAUDE.md']) writeFileSync(join(WORKSPACE, name), BRIEF)
process.stdout.write(
  `trust: ${execFileSync('python3', [join(HERE, 'trust-claude-folder.py'), WORKSPACE], { encoding: 'utf8' }).trim()}\n`,
)

let app = await startIntegration({ editor: EDITOR, fakeEnv: ENV })
const root = app.root
try {
  writeRoster(app.env.CONSENSFLOW_HOME)
  const tabs = () => readJson(join(app.env.CONSENSFLOW_HOME, 'app', 'tabs.json'))?.tabs ?? []

  const opened = await app.openTab({ dir: WORKSPACE, harness: LEAD_KIND })
  const tabId = opened.tab.id
  const leadToken = opened.leadEnv?.CONSENSFLOW_APP_TOKEN
  const pane = async (match) => {
    const state = await app.requestNode('state.list', {})
    return state?.tabs?.find((tab) => tab.id === tabId)?.panes?.find(match) ?? null
  }
  const leadPane = await until(async () => {
    const lead = await pane((candidate) => candidate.kind === 'lead')
    return lead?.alive !== false && !lead?.failure && !lead?.starting ? lead : null
  }, 120_000)
  const leadSession = tabs().find((tab) => tab.id === tabId)?.lead?.nativeSession
  record('lead-ready', Boolean(leadToken && leadPane), {
    tab: tabId,
    lead: LEAD,
    leadSession,
    ...(LEAD === 'claude' ? { claudeStatus: claudeStatus(leadSession) } : {}),
    pane: leadPane ? undefined : await pane((candidate) => candidate.kind === 'lead'),
  })

  for (const name of wanted) {
    const agent = AGENTS[name]
    const marker = `BENCH_OK_${name.toUpperCase()}`
    const started = Date.now()
    const consult = await app.http('/api/panes/consult', {
      method: 'POST',
      token: leadToken,
      body: {
        tab: tabId,
        agent: agent.id,
        task: `Reply with exactly: ${marker}`,
        fresh: true,
        opId: `bench-${name}-${started}`,
      },
    })
    const conversation = consult.body?.conversation
    record(`${name}-dispatched`, consult.status < 300 && Boolean(conversation), {
      status: consult.status,
      conversation,
    })
    if (!conversation) continue
    const answered = await until(
      () =>
        app
          .deliveries(WORKSPACE)
          .find((r) => r.conversation === conversation && r.answer?.includes(marker)),
      300_000,
    )
    const worker = await pane((candidate) => candidate.conversation === conversation)
    record(`${name}-answered`, Boolean(answered), {
      seconds: Math.round((Date.now() - started) / 1000),
      ...(answered
        ? {}
        : {
            pane: worker && {
              alive: worker.alive,
              starting: worker.starting,
              failure: worker.failure,
              activity: worker.activity,
            },
            thread: app.threads(WORKSPACE)[conversation],
            results: app
              .deliveries(WORKSPACE)
              .map((r) => ({ c: r.conversation, a: r.answer?.slice(0, 80) })),
          }),
    })
    if (!answered) continue
    const received = await until(
      () => app.deliveries(WORKSPACE).find((r) => r.id === answered.id && r.state === 'received'),
      300_000,
    )
    record(`${name}-received-by-lead`, Boolean(received), {
      seconds: Math.round((Date.now() - started) / 1000),
      state: app.deliveries(WORKSPACE).find((r) => r.id === answered.id)?.state,
    })
    const idle = await until(async () => {
      const worker = await pane((candidate) => candidate.conversation === conversation)
      return worker?.activity?.state === 'idle' ? worker : null
    }, 60_000)
    record(`${name}-marker-idle`, Boolean(idle), {
      activity: idle ? undefined : (await pane((c) => c.conversation === conversation))?.activity,
    })
  }

  // Restart: a new daemon and pane host over the same home. The session that
  // was open must come back on its lead's session without a manual Resume.
  const opens = app.openFrames.length
  // The app quits the same way: the daemon is killed first, then the pane host
  // reaps the panes, so the daemon never records the sessions as suspended.
  app.killEditor()
  await until(() => app.uiExited(), 10_000, 100)
  await app.close({ preserveRoot: true })
  app = await startIntegration({ editor: EDITOR, fakeEnv: ENV, existingRoot: root })
  writeRoster(app.env.CONSENSFLOW_HOME)
  const back = await until(() => tabs().find((tab) => tab.id === tabId)?.closed === false, 120_000)
  // Claude resumes with --resume <id>, OpenCode with --session <id>.
  const resumed = await until(
    () => app.openFrames.find((frame) => frame.argv?.includes(leadSession)),
    60_000,
  )
  record('restart-restores-session', Boolean(back && resumed), {
    back: Boolean(back),
    resumedWith: resumed?.argv?.slice(-4),
    leadSession,
    opensBefore: opens,
  })
} finally {
  await app.close()
  // The harnesses keep their own copies of these sessions; remove the ones
  // that are plain folders for this bench workspace.
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
