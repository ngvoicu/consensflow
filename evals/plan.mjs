import { statSync } from 'node:fs'
import { posix, win32 } from 'node:path'
import { questionSentences } from './measure.mjs'

/**
 * The pure part of an eval run: which agents make up the staff for a set of
 * harnesses, what environment gives a chief its model where the harness
 * takes one from the environment, and how the scripted human answers a
 * question. Everything here is tested without spending a token.
 */

/**
 * One cheap model per harness (brain: operations/test-models.md), for the
 * staff and, on OpenCode, for the chief. OpenCode's free Muse Spark died
 * mid-run twice out of two (a dead turn), so OpenCode runs a paid Go model.
 */
export const HARNESSES = {
  claude: { kind: 'claude-code', model: 'claude-haiku-4-5-20251001' },
  codex: { kind: 'codex', model: 'gpt-5.6-luna' },
  // OpenRouter only since 2026-09-28: OpenCode Go is out of ConsensFlow.
  pi: { kind: 'pi', model: 'openrouter/deepseek/deepseek-v4.1-flash' },
  opencode: { kind: 'opencode', model: 'openrouter/deepseek/deepseek-v4.1-flash' },
  devin: { kind: 'devin', model: 'swe-1-6-slow' },
}

/** The roles a staff harness fills; two workers, so parallel work has somewhere to run. */
const ROLES = [
  ['worker', 'worker'],
  ['worker-2', 'worker'],
  ['advisor', 'advisor'],
  ['reviewer', 'reviewer'],
]

/**
 * The roster rows and the project staff for these harnesses: for each, two
 * workers, an advisor and a reviewer on its cheap model (`models` overrides
 * a harness's model). Every member is standard tier, so the daemon picks
 * among them by its own rule and any harness may get any task. `effort` is
 * each member's reasoning level (Pi calls it thinking); Devin has none.
 */
export function staffFor(harnesses, models = {}, effort = undefined) {
  const agents = []
  const staff = []
  for (const name of harnesses) {
    const harness = HARNESSES[name]
    if (harness === undefined) throw new Error(`no such eval harness: ${name}`)
    for (const [suffix, role] of ROLES) {
      const id = `eval-${name}-${suffix}`
      agents.push({
        id,
        kind: harness.kind,
        model: models[name] ?? harness.model,
        workTier: 'standard',
        ...(effort === undefined || name === 'devin'
          ? {}
          : { [name === 'pi' ? 'thinking' : 'effort']: effort }),
      })
      staff.push({ agent: id, roles: [role] })
    }
  }
  return { agents, staff }
}

/**
 * A Pi or OpenCode chief's model: a step above the staff's DeepSeek Flash and
 * well below sol's price (2026-09-28, Gabriel's choice), the same for both so
 * the two harnesses compare as harnesses.
 */
export const CHIEF_MODELS = {
  pi: 'openrouter/deepseek/deepseek-v4-pro-0813',
  opencode: 'openrouter/deepseek/deepseek-v4-pro-0813',
}

/**
 * The chief has no model of its own in the roster: it runs its harness's
 * default. Claude Code and OpenCode take one from the environment (Opus, and
 * DeepSeek V4 Pro, unless `model` says otherwise), Codex and Pi from the
 * eval's wrappers (Codex's cheap model, DeepSeek V4 Pro); Devin runs the
 * model its own configuration names, so `model` is ignored there.
 */
export function chiefEnvironment(chief, model = undefined) {
  if (chief === 'claude') {
    const chosen = model ?? 'claude-opus-5'
    return { env: { ANTHROPIC_MODEL: chosen }, model: chosen }
  }
  if (chief === 'opencode') {
    const chosen = model ?? CHIEF_MODELS.opencode
    return { env: { OPENCODE_CONFIG_CONTENT: JSON.stringify({ model: chosen }) }, model: chosen }
  }
  // Pi takes it through the eval's Pi wrapper (`--model`, see run.mjs).
  if (chief === 'pi') return { env: {}, model: model ?? CHIEF_MODELS.pi }
  // Codex takes it through the eval's Codex wrapper (`-c model=…`, see run.mjs).
  if (chief === 'codex') return { env: {}, model: model ?? HARNESSES.codex.model }
  if (!(chief in HARNESSES)) throw new Error(`no such eval harness: ${chief}`)
  // Devin runs its staff's model, through the lead's agent (see run.mjs).
  return { env: {}, model: HARNESSES.devin.model }
}

/**
 * Where Claude Code keeps what it saves about a folder (`~/.claude/projects/
 * <key>`): the folder's path with every slash and dot made a dash. Its
 * `memory/` there is read by every later session in that folder, so a run's
 * chief would learn from the last run's; the runner clears it first.
 */
export function claudeProjectKey(directory) {
  return directory.replace(/[/.]/g, '-')
}

/**
 * The last `count` non-empty lines a window printed, control sequences already
 * stripped: near enough its screen, for a report to show why a chief said
 * nothing (a login page, a quota wall, a model that does not exist).
 */
export function lastLines(text, count = 40) {
  return text
    .split(/\r\n|\r|\n/)
    .map((line) => line.trimEnd())
    .filter((line) => line.trim() !== '')
    .slice(-count)
}

/**
 * The owner's reply to questions a chief left in its terminal: for each
 * question sentence, the scenario's answer on its subject or its fallback,
 * each answer once, in the order asked.
 */
export function terminalAnswer(scenario, text) {
  const answers = scenario.answers ?? []
  const replies = questionSentences(text).map(
    (question) => answers.find(({ match }) => match.test(question))?.text ?? scenario.fallback,
  )
  return [...new Set(replies)].join(' ')
}

/**
 * What a Windows program needs from its environment besides PATH: the
 * system's folders, the user's, and where Devin keeps its settings and
 * sessions (APPDATA).
 */
const WINDOWS_ENV = [
  'SystemRoot',
  'SystemDrive',
  'windir',
  'ComSpec',
  'PATHEXT',
  'USERPROFILE',
  'USERNAME',
  'APPDATA',
  'LOCALAPPDATA',
  'TEMP',
  'TMP',
  'ProgramData',
  'ProgramFiles',
  'ProgramFiles(x86)',
  'CommonProgramFiles',
  'HOMEDRIVE',
  'HOMEPATH',
  'NUMBER_OF_PROCESSORS',
  'PROCESSOR_ARCHITECTURE',
  'OS',
]

/**
 * The environment of a live run's windows, real harnesses on the user's own
 * home and logins: never this shell's session identity, and none of the test
 * pane host's sandbox (a null removes its default; with CLAUDE_CONFIG_DIR
 * set, Claude finds no completed onboarding and opens on its first-run
 * dialog). `bin`, when given, comes first on PATH: the eval's wrappers.
 */
export function liveEnvironment({
  home,
  bin = null,
  env = process.env,
  platform = process.platform,
}) {
  const windows = platform === 'win32'
  // The platform's own separator, whichever machine computes it.
  const { join } = windows ? win32 : posix
  const path = windows
    ? [bin, env.PATH ?? ''].filter(Boolean).join(';')
    : [
        bin,
        join(home, '.local', 'bin'),
        join(home, '.opencode', 'bin'),
        join(home, '.codex', 'bin'),
        join(home, '.pi', 'bin'),
        '/opt/homebrew/bin',
        '/usr/bin',
        '/bin',
        '/usr/sbin',
        '/sbin',
      ]
        .filter(Boolean)
        .join(':')
  return {
    HOME: home,
    USER: env.USER,
    LOGNAME: env.USER,
    LANG: 'en_US.UTF-8',
    TERM: 'xterm-256color',
    PATH: path,
    ...(windows
      ? Object.fromEntries(
          WINDOWS_ENV.filter((name) => env[name] !== undefined).map((name) => [name, env[name]]),
        )
      : {}),
    CLAUDE_CONFIG_DIR: null,
    CODEX_HOME: join(home, '.codex'),
    XDG_CONFIG_HOME: join(home, '.config'),
  }
}

/**
 * The real `name` (claude, codex) on this PATH, skipping the eval's own
 * wrapper directory, so a wrapper can exec it by absolute path.
 */
export function realOnPath(
  name,
  pathVariable,
  exists = defaultExists,
  platform = process.platform,
) {
  // Windows lists PATH with semicolons, and a command is a file with its kind's extension.
  const windows = platform === 'win32'
  const files = windows ? [`${name}.exe`, `${name}.cmd`] : [name]
  for (const dir of pathVariable.split(windows ? ';' : ':')) {
    const plain = dir.replace(/\\/g, '/').replace(/\/$/, '')
    if (plain === '' || plain.endsWith('/evals/bin')) continue
    // A terminal app's shims come first on PATH inside its panes (cmux has its
    // own claude) and start nothing outside it: an eval window on one printed
    // nothing for five minutes (2026-10-01).
    if (plain.includes('/cmux-cli-shims/') || plain.includes('/cmux.app/')) continue
    for (const file of files) {
      const candidate = windows ? `${dir.replace(/[\\/]$/, '')}\\${file}` : `${plain}/${file}`
      if (exists(candidate)) return candidate
    }
  }
  throw new Error(`${name} is not on PATH`)
}

/**
 * The `-c` overrides that switch off every MCP server Codex would start
 * (`codex mcp list --json`): each gets a harmless, disabled definition,
 * which also covers servers a plugin or the ChatGPT app adds outside
 * config.toml (a bare `enabled=false` is refused for those).
 */
export function codexIsolation(servers) {
  return servers.flatMap(({ name }) => {
    // Codex's -c takes the key's segments literally: a quoted name would
    // define a new server and leave the real one on. Refuse, never half-isolate.
    if (!/^[A-Za-z0-9_-]+$/.test(name)) {
      throw new Error(`cannot switch off the Codex MCP server ${JSON.stringify(name)}`)
    }
    return [
      '-c',
      `mcp_servers.${name}.command="/usr/bin/true"`,
      '-c',
      `mcp_servers.${name}.enabled=false`,
    ]
  })
}

function defaultExists(file) {
  try {
    return statSync(file).isFile()
  } catch {
    return false
  }
}
