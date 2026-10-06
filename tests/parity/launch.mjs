/**
 * `npm run parity:launch`: how each harness's window is planned, by Node's
 * adapters and then by the Rust ones, with this machine's real CLIs, held to
 * the same plan. No window is opened and nothing is sent to a model.
 *
 * Node's half is here. It makes two roots of one shape, one for each
 * implementation, under a name that holds a space, `%` and `#`, seeded the
 * same: everything an adapter or a harness reads or writes is under its root
 * (HOME, CONSENSFLOW_HOME, XDG_*, APPDATA, LOCALAPPDATA, CODEX_HOME,
 * CLAUDE_CONFIG_DIR, the temporary folders), and its PATH holds the folders the
 * CLIs are in, and what they need to run. While Node plans, this process's own
 * environment is replaced by the root's, so nothing falls back to the real one
 * (`os.homedir()`, `process.env`): nothing under the human's home or
 * `~/.consensflow` is read or written.
 *
 * The CLIs run only as a plan runs them: their version and help questions,
 * Codex's app-server, OpenCode's throwaway `serve`. One at a time, case after
 * case, never two at once: they hang when run together here. A harness whose
 * CLI is not installed is left out and said so.
 *
 * Each harness is planned for the launches a daemon makes: a fresh member
 * window with a first message, a resumed one, the chief's, a member on a
 * catalog agent with a model and an effort, and its own: Codex's own config
 * with instructions of the owner's, and an image agent; OpenCode's fresh
 * conversation, made on its throwaway server; Pi's private bundle, and one
 * that differs from this build; Devin's owner config absent, present and
 * unreadable; and a chief with no role text, which each refuses after writing
 * what it had.
 * Claude's resumed window is planned with its transcript there and without.
 *
 * Node writes what it found, one case a line, raw: its root, its values, no
 * `$ROOT` and no names for what was drawn. Then it runs the Rust half
 * (`crates/cf-harness/tests/parity_launch/`, told the file in
 * CF_PARITY_LAUNCH), which plans the same cases in the other root, writes
 * both sides' findings the same way, with one normalizer, and holds them
 * equal: each plan's argv, environment (in its order), variables dropped and
 * conversation; a refusal's sentence; and everything the plan changed in the
 * tree (paths, modes, the text of each file) but for the folders a CLI keeps
 * its own state in (`OWNED`), which are counted. It times each side's plans
 * and prints the table. The roots hold what the CLIs wrote: they are removed,
 * unless the halves differ, when they are kept for the difference to be
 * looked at again.
 *
 *   npm run parity:launch
 */
import { spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { performance } from 'node:perf_hooks'
import { AGENT_PRESETS } from '../../hosts/lib/presets.js'
import { createAdapters } from '../../src/adapters/index.js'
import { BUNDLE_BIN, BUNDLE_CF, PANE_CF } from '../../src/core/pane-cf.js'
import { harnessPath } from '../../src/harnesses.js'
import { preparePrivateIntegration } from '../../src/private-integration.js'
import { harnessForKind } from '../../src/roster.js'
import { changes, gained, rootForms, snapshot } from './tree.mjs'

const REPO = path.join(import.meta.dirname, '..', '..')
const WINDOWS = process.platform === 'win32'
/** The kinds of window, in the order they are planned: one harness at a time. */
const KINDS = ['claude-code', 'codex', 'pi', 'opencode', 'devin']

/**
 * The folders a CLI keeps its own state in as it answers a plan's questions:
 * Codex's app-server fills its home (databases, the system skills it unpacks,
 * names it draws), OpenCode's server its XDG places, and the temporary folder
 * is the CLIs'. No adapter writes there: they are counted, never compared.
 */
const OWNED = [
  'codex',
  'codex-owner',
  'xdg/cache/opencode',
  'xdg/config/opencode',
  'xdg/data/opencode',
  'xdg/state/opencode',
  'appdata/local/opencode',
  'appdata/roaming/opencode',
  'tmp',
]

/** What a program needs of its environment on Windows, to start and to ask the system anything, taken from this one. */
const WINDOWS_VARIABLES = [
  'SystemRoot',
  'SystemDrive',
  'windir',
  'ComSpec',
  'PATHEXT',
  'OS',
  'USERNAME',
  'USERDOMAIN',
  'LOGONSERVER',
  'COMPUTERNAME',
  'PROCESSOR_ARCHITECTURE',
  'NUMBER_OF_PROCESSORS',
  'ProgramData',
  'ProgramFiles',
  'ProgramFiles(x86)',
  'CommonProgramFiles',
]

/** The conversations the resumed windows open again, fixed: a plan is given them, never draws them. */
const RESUME = {
  recorded: '0199a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b',
  lost: '0199a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2c',
  codex: '0199a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2d',
  pi: 'cf-7-rhea-0a1b2c3d',
  opencode: 'ses_0199a1b2c3d4eRESUME0001',
  devin: '0199a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2e',
}

/** A role's text with what a plan has to carry: quotes, backslashes, braces, a tab and text outside ASCII. */
const INSTRUCTIONS = [
  '# ConsensFlow worker',
  '',
  'You are @rhea on the project "Parity". Keep answers short, and name paths as C:\\Users\\rhea\\work or /home/rhea/work.',
  'A tab:\there, braces {like: "this"}, an accent (café), an arrow (→) and an emoji (🙂).',
  '',
].join('\n')

/** A first message with what a window takes only changed: a tab, an escape, a lone return, a C1 control. */
const MESSAGE = [
  '[ConsensFlow m-7 · T-3 · task from @chief]',
  'Write the parser for "config.toml" and reply with --done.',
  'Mind the tab:\tthe café (e + \u0301 is é), 🙂, an escape \u001b[0m, a lone return \r and a C1 control \u0085.',
].join('\n')

/** Claude's own record of a conversation it kept, so that a window on it resumes. */
const TRANSCRIPT = '{"type":"user","message":{"role":"user","content":"hello"}}\n'

/** An owner's Devin config, with what its reader strips (comments, trailing commas) and what it must not (`//` in a string). */
const OWNER_CONFIG = `{
  // the owner's own settings
  "theme": "dark", /* inline */
  "homepage": "https://example.com/not-a-comment",
  "hooks": {
    "SessionStart": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "echo owner", "timeout": 3 },], },
    ],
    "PreToolUse": [],
  },
}
`

/** Codex's own config with instructions of the owner's, which a window's role text follows. */
const OWNER_CODEX_CONFIG = `developer_instructions = "The owner's own note."
`

/** The participant every launch is for. */
const PROJECT = 7
const HANDLE = 'rhea'

/** The launch a case is given unless it says otherwise: a member's, on a fresh conversation. */
const MEMBER = {
  role: 'worker',
  resume: null,
  message: MESSAGE,
  agent: null,
  instructions: INSTRUCTIONS,
}
const NO_ROLE_TEXT = {
  role: 'chief',
  message: null,
  instructions: '',
  refuses: 'the chief window needs its role text',
}

/**
 * Each harness's cases: a name, what differs from a member's fresh launch,
 * and the environment it runs in where it differs (`env`, from a root's own
 * places). `refuses` is what the case's refusal begins with: both
 * implementations must refuse it, and in the same words.
 */
const CASES = {
  'claude-code': [
    ['member-fresh', {}],
    ['member-resumed', { resume: RESUME.recorded }],
    ['member-resumed-lost', { resume: RESUME.lost }],
    ['chief', { role: 'chief', message: null }],
    ['member-agent', { role: 'reviewer', agent: 'calliope' }],
    ['chief-without-role-text', NO_ROLE_TEXT],
  ],
  codex: [
    ['member-fresh', {}],
    ['member-resumed', { resume: RESUME.codex }],
    ['chief', { role: 'chief', message: null }],
    ['member-agent', { role: 'advisor', agent: 'hemera' }],
    ['member-designer', { role: 'designer', agent: 'pygmalion' }],
    ['member-owner-config', { env: (at) => ({ CODEX_HOME: at('codex-owner') }) }],
    ['chief-without-role-text', NO_ROLE_TEXT],
  ],
  pi: [
    ['member-fresh', {}],
    ['member-resumed', { resume: RESUME.pi }],
    ['chief', { role: 'chief', message: null }],
    ['member-agent', { role: 'reviewer', agent: 'leto' }],
    [
      'bundle-differs',
      {
        env: (at) => ({ CONSENSFLOW_HOME: at('divergent') }),
        refuses:
          "ConsensFlow's Pi extension could not be installed: Private pi integration differs from this build; existing files were preserved",
      },
    ],
    ['chief-without-role-text', NO_ROLE_TEXT],
  ],
  opencode: [
    ['member-fresh', {}],
    ['member-resumed', { resume: RESUME.opencode }],
    ['chief', { role: 'chief', message: null }],
    ['member-agent', { role: 'advisor', agent: 'arvakr' }],
    [
      'tui-config-taken',
      {
        env: (at) => ({ OPENCODE_TUI_CONFIG: at('tui.json') }),
        refuses: 'OpenCode has a custom OPENCODE_TUI_CONFIG',
      },
    ],
    ['chief-without-role-text', NO_ROLE_TEXT],
  ],
  devin: [
    ['member-fresh', {}],
    ['member-resumed', { resume: RESUME.devin }],
    ['chief', { role: 'chief', message: null }],
    ['member-agent', { role: 'worker', agent: 'thoth' }],
    ['owner-config', { env: (at) => ownerPlaces(at, 'owner') }],
    [
      'owner-config-unreadable',
      {
        env: (at) => ownerPlaces(at, 'broken'),
        refuses: 'Cannot read native Devin configuration; the original was preserved',
      },
    ],
    ['chief-without-role-text', NO_ROLE_TEXT],
  ],
}

/** Where Devin reads its owner's config from, on each platform, in the folder `name` of a root. */
const ownerPlaces = (at, name) => ({
  XDG_CONFIG_HOME: at(name, 'xdg'),
  APPDATA: at(name, 'appdata'),
})

/** A catalog agent as a launch is given it. */
function agentOf(preset) {
  if (preset === null) return null
  const agent = AGENT_PRESETS.find((candidate) => candidate.preset === preset)
  if (agent === undefined) throw new Error(`the catalog has no agent named ${preset}`)
  const { model, effort, thinking, designer } = agent
  return { model, effort, thinking, designer }
}

const real = { ...process.env }
// Found as the daemon finds them, through process.env itself: on Windows it
// reads a name in any case (its Path is PATH), and a copy of it does not.
const found = Object.fromEntries(
  KINDS.map((kind) => [kind, harnessPath(harnessForKind(kind), process.env)]),
)

/**
 * The folders the CLIs are in, and what they need to run: the Node that
 * their shims start, and the system's own programs.
 */
const PATH = [
  ...new Set([
    ...Object.values(found)
      .filter((file) => file !== null)
      .map((file) => path.dirname(file)),
    path.dirname(process.execPath),
    ...(WINDOWS
      ? [`${real.SystemRoot}\\System32`, real.SystemRoot, `${real.SystemRoot}\\System32\\Wbem`]
      : ['/usr/bin', '/bin']),
  ]),
].join(path.delimiter)

/** The environment a root's windows and CLIs run with: all of it its own places, but PATH, and `overrides` of it. */
function environment(root, overrides = () => ({})) {
  const at = (...parts) => path.join(root, ...parts)
  const home = at('home')
  const drive = path.parse(home).root.slice(0, 2)
  return {
    HOME: home,
    USERPROFILE: home,
    ...(WINDOWS ? { HOMEDRIVE: drive, HOMEPATH: home.slice(drive.length) } : {}),
    CONSENSFLOW_HOME: at('consensflow'),
    CLAUDE_CONFIG_DIR: at('claude'),
    CODEX_HOME: at('codex'),
    XDG_CONFIG_HOME: at('xdg', 'config'),
    XDG_DATA_HOME: at('xdg', 'data'),
    XDG_CACHE_HOME: at('xdg', 'cache'),
    XDG_STATE_HOME: at('xdg', 'state'),
    APPDATA: at('appdata', 'roaming'),
    LOCALAPPDATA: at('appdata', 'local'),
    TMPDIR: at('tmp'),
    TMP: at('tmp'),
    TEMP: at('tmp'),
    ...(real.LANG === undefined ? {} : { LANG: real.LANG }),
    ...(WINDOWS
      ? Object.fromEntries(
          WINDOWS_VARIABLES.filter((name) => real[name] !== undefined).map((name) => [
            name,
            real[name],
          ]),
        )
      : {}),
    PATH,
    ...overrides(at),
  }
}

/**
 * Makes `root` as every root is made: its folders, and what the cases find
 * there. ConsensFlow's folder is left to the adapters, which make it.
 */
function seed(root) {
  const at = (...parts) => path.join(root, ...parts)
  const write = (file, text) => {
    fs.mkdirSync(path.dirname(file), { recursive: true })
    fs.writeFileSync(file, text)
  }
  for (const folder of [
    'home',
    'work',
    'tmp',
    'codex',
    'codex-owner',
    'xdg/config',
    'xdg/data',
    'xdg/cache',
    'xdg/state',
    'appdata/roaming',
    'appdata/local',
  ]) {
    fs.mkdirSync(at(...folder.split('/')), { recursive: true })
  }
  write(at('claude', 'projects', '-work', `${RESUME.recorded}.jsonl`), TRANSCRIPT)
  write(at('codex-owner', 'config.toml'), OWNER_CODEX_CONFIG)
  for (const [name, text] of [
    ['owner', OWNER_CONFIG],
    ['broken', '{ "hooks": '],
  ]) {
    write(at(name, 'xdg', 'devin', 'config.json'), text)
    write(at(name, 'appdata', 'devin', 'config.json'), text)
  }
  // A bundle of Pi's extension published by another build: the same name, other bytes.
  const file = 'hosts/pi-extension/consensflow-delivery.mjs'
  const bundle = preparePrivateIntegration({ CONSENSFLOW_HOME: at('divergent') }, 'pi', [file])
  fs.writeFileSync(path.join(bundle, file), '// another build\n')
}

/** Runs `work` with this process's environment replaced by `env`, and put back after. */
async function within(env, work) {
  const saved = { ...process.env }
  const replace = (variables) => {
    for (const name of Object.keys(process.env)) delete process.env[name]
    Object.assign(process.env, variables)
  }
  replace(env)
  try {
    return await work()
  } finally {
    replace(saved)
  }
}

/** The plan Node's adapter makes of a launch, or its refusal, and how long it took. */
async function planned(adapter, launch) {
  const started = performance.now()
  let outcome
  try {
    const prepared = await adapter.prepare(launch)
    outcome = {
      plan: {
        argv: prepared.argv,
        env: Object.entries(prepared.env),
        dropEnv: prepared.dropEnv,
        nativeSession: prepared.nativeSession,
      },
    }
  } catch (error) {
    outcome = { refused: String(error?.message ?? error) }
  }
  return { outcome, ms: performance.now() - started }
}

const base = fs.mkdtempSync(
  path.join(fs.realpathSync.native(os.tmpdir()), 'consensflow launch %#-'),
)
const ROOTS = { node: path.join(base, 'node'), rust: path.join(base, 'rust') }
const FILE = path.join(base, 'plans.jsonl')

// The roots first, alike; then every plan, each harness after the last.
let kept = true
try {
  for (const root of Object.values(ROOTS)) seed(root)
  const lines = [
    {
      type: 'header',
      zone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      rustRoot: ROOTS.rust,
      forms: { node: rootForms(ROOTS.node), rust: rootForms(ROOTS.rust) },
      bundle: { bin: BUNDLE_BIN, cf: BUNDLE_CF, paneCf: PANE_CF },
      owned: OWNED,
    },
  ]
  for (const kind of KINDS) {
    if (found[kind] === null) {
      const reason = `${harnessForKind(kind)} is not installed on this machine`
      console.log(`${kind}: left out: ${reason}`)
      lines.push({ type: 'skipped', kind, reason })
      continue
    }
    let spent = 0
    for (const [name, spec] of CASES[kind]) {
      const { env, refuses = null, agent = null, ...fields } = { ...MEMBER, ...spec }
      // One launch id for both sides: it is given to a plan, as it is a daemon's, never drawn by one.
      const launch = { launchId: randomUUID(), ...fields, agent: agentOf(agent) }
      const nodeEnv = environment(ROOTS.node, env)
      // As it is given: a plan hands this object on to its children, and Node may add to it.
      const nodeSetting = Object.entries(nodeEnv)
      const directory = path.join(ROOTS.node, 'work')
      const asked = { ...launch, participant: { projectId: PROJECT, handle: HANDLE }, directory }
      const before = snapshot(ROOTS.node, OWNED)
      const { outcome, ms } = await within(nodeEnv, () =>
        planned(createAdapters(nodeEnv)[kind], asked),
      )
      const after = snapshot(ROOTS.node, OWNED)
      spent += ms
      lines.push({
        type: 'case',
        kind,
        name,
        refuses,
        launch: { ...launch, project: PROJECT, handle: HANDLE },
        node: {
          env: nodeSetting,
          directory,
          outcome,
          changes: changes(before.entries, after.entries),
          owned: gained(before.counts, after.counts),
          ms,
        },
        rust: {
          env: Object.entries(environment(ROOTS.rust, env)),
          directory: path.join(ROOTS.rust, 'work'),
        },
      })
    }
    console.log(`${kind}: ${CASES[kind].length} cases planned by Node in ${spent.toFixed(0)} ms`)
  }
  fs.writeFileSync(FILE, `${lines.map((line) => JSON.stringify(line)).join('\n')}\n`)

  // Optimised, as the daemon is built: the Rust half times its plans.
  const rust = spawnSync(
    'cargo',
    [
      'test',
      '--release',
      '-p',
      'cf-harness',
      '--test',
      'parity_launch',
      '--',
      '--ignored',
      '--nocapture',
    ],
    { cwd: REPO, stdio: 'inherit', env: { ...process.env, CF_PARITY_LAUNCH: FILE } },
  )
  process.exitCode = rust.status ?? 1
  kept = process.exitCode !== 0
} finally {
  if (kept) console.log(`the roots are kept, to look at again: ${base}`)
  else fs.rmSync(base, { recursive: true, force: true })
}
