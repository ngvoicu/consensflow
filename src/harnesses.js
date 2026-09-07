import { accessSync, constants, existsSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, join, resolve } from 'node:path'

/**
 * Where each coding harness keeps its skills, and whether it is installed here.
 * Detection is "the CLI resolves on PATH" — the same test the generated
 * commands live or die by. Directories honour each harness's own override
 * variable, which is also what keeps tests off the real machine.
 */

/**
 * A host that ships its own ConsensFlow already has a richer path than the
 * generated skill — consensflow-cc packets the live Claude Code conversation,
 * consensflow-pi does the same inside pi. Installing our skill there too
 * would put two entries with the same name and the same trigger in front of
 * one harness, competing for its skills budget. So we detect them and stand
 * aside (`--all` overrides).
 */
const NATIVE = {
  claude: (env) => join(home(env), '.claude', 'plugins', 'cache', 'consensflow-cc'),
  pi: (env) => join(piAgentDir(env), 'git', 'github.com', 'ngvoicu', 'consensflow-pi'),
}

/**
 * Where these CLIs install themselves, beyond whatever PATH we were handed.
 *
 * PATH alone is not a reliable answer to "is this harness on the machine". A
 * .app from Finder inherits almost none, and the login shell we ask instead is
 * NON-interactive — `zsh -lc` reads .zshenv/.zprofile/.zlogin but never
 * .zshrc, which is where per-tool bin directories usually get added. So the
 * app saw claude, codex and pi but not opencode, decided opencode was out of
 * scope, and took its skill back on every mode apply — while a terminal put it
 * straight back. Detection has to be the same answer wherever it runs.
 */
const HOMED = (parts) => (env) => join(home(env), ...parts)

const HARNESSES = [
  {
    id: 'claude',
    command: 'claude',
    locations: [HOMED(['.local', 'bin']), HOMED(['.claude', 'local'])],
    skillsDir: (env) => join(env.CLAUDE_CONFIG_DIR ?? join(home(env), '.claude'), 'skills'),
  },
  {
    id: 'codex',
    command: 'codex',
    locations: [HOMED(['.codex', 'bin']), HOMED(['.local', 'bin'])],
    skillsDir: (env) => join(env.CODEX_HOME ?? join(home(env), '.codex'), 'skills'),
  },
  {
    id: 'opencode',
    command: 'opencode',
    locations: [HOMED(['.opencode', 'bin']), HOMED(['.local', 'bin'])],
    skillsDir: (env) =>
      join(env.XDG_CONFIG_HOME ?? join(home(env), '.config'), 'opencode', 'skills'),
  },
  {
    id: 'pi',
    command: 'pi',
    locations: [HOMED(['.pi', 'bin']), HOMED(['.local', 'bin'])],
    skillsDir: (env) => join(piAgentDir(env), 'skills'),
  },
  {
    // The CLI is `kimi`; the product is Kimi Code, which is why its home is
    // `.kimi-code`. `KIMI_CODE_HOME` moves the whole thing, its own importer
    // skill says so, and skills sit at User scope directly under it.
    id: 'kimi',
    command: 'kimi',
    locations: [HOMED(['.kimi-code', 'bin']), HOMED(['.local', 'bin'])],
    skillsDir: (env) => join(env.KIMI_CODE_HOME ?? join(home(env), '.kimi-code'), 'skills'),
  },
]

/**
 * Per-user bin directories any of them might land in.
 *
 * Deliberately all HOME-relative. System-wide places like /opt/homebrew/bin
 * and /usr/local/bin are already on every login PATH, so adding them here
 * would buy nothing — and would break the rule that a test with a throwaway
 * HOME sees only the harnesses it stubbed, by finding the real machine's.
 */
const COMMON = [HOMED(['.bun', 'bin']), HOMED(['.npm-global', 'bin']), HOMED(['.volta', 'bin'])]

function home(env) {
  // Windows sets USERPROFILE, not HOME; homedir() knows that, but an explicit
  // env (every test, and the app passing a login environment) may carry either.
  return env.HOME ?? env.USERPROFILE ?? homedir()
}

function piPath(configured, env) {
  if (configured === '~') return home(env)
  if (configured.startsWith('~/')) return join(home(env), configured.slice(2))
  if (process.platform === 'win32' && configured.startsWith('~\\')) {
    return join(home(env), configured.slice(2))
  }
  return configured
}

export function piAgentDir(env) {
  return piPath(env.PI_CODING_AGENT_DIR || join(home(env), '.pi', 'agent'), env)
}

export function piSessionDir(env) {
  return piPath(env.PI_CODING_AGENT_SESSION_DIR || join(piAgentDir(env), 'sessions'), env)
}

/**
 * What an executable is called, per platform.
 *
 * On Windows a CLI on PATH is `claude.cmd` or `claude.exe` — never the bare
 * name — and there is no executable bit to test, so PATHEXT decides and
 * "the file is there" is the whole check.
 */
function candidateNames(command, env) {
  if ((env.OS ?? '').toLowerCase().includes('windows') || process.platform === 'win32') {
    const exts = (env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD').split(';').filter(Boolean)
    return [command, ...exts.map((ext) => `${command}${ext.toLowerCase()}`)]
  }
  return [command]
}

/**
 * Where this command resolves on PATH, as an ABSOLUTE path, or null.
 *
 * A PATH entry may be relative — `PATH=.:…`, or a `bin` some launcher
 * exported from wherever it happened to be — and joining that with a
 * command name yields a relative candidate the pane host refuses outright
 * (`app/src-tauri/src/commands.rs:1121`). Resolving here means every caller
 * gets a path it can spawn, not one that happened to work from this
 * process's current directory.
 */
function pathOnPath(command, env) {
  const executable = process.platform !== 'win32'
  for (const dir of (env.PATH ?? '').split(delimiter)) {
    if (dir.length === 0) continue
    for (const name of candidateNames(command, env)) {
      const candidate = resolve(dir, name)
      try {
        if (!statSync(candidate).isFile()) continue
        if (executable) accessSync(candidate, constants.X_OK)
        return candidate
      } catch {
        // Not here; keep looking.
      }
    }
  }
  return null
}

/** PATH first, then the places these CLIs actually install themselves. */
function isInstalled(harness, env) {
  return locate(harness, env) !== null
}

/** The absolute path this harness's CLI resolves to here, or null. */
function locate(harness, env) {
  const onPath = pathOnPath(harness.command, env)
  if (onPath !== null) return onPath
  for (const dir of [...(harness.locations ?? []), ...COMMON]) {
    for (const name of candidateNames(harness.command, env)) {
      const candidate = resolve(dir(env), name)
      try {
        if (!statSync(candidate).isFile()) continue
        if (process.platform !== 'win32') accessSync(candidate, constants.X_OK)
        return candidate
      } catch {
        // Not here either; keep looking.
      }
    }
  }
  return null
}

/**
 * The absolute path to a harness's CLI on this machine, or null.
 *
 * A pane is opened with an argv the pane host refuses unless argv[0] is
 * absolute (`app/src-tauri/src/commands.rs:1121`), and the app's own PATH is
 * not the login shell's — the same reason detection looks past PATH at all.
 * So the launcher asks for the path, not the name.
 */
export function harnessPath(id, env) {
  const harness = HARNESSES.find((candidate) => candidate.id === id)
  return harness === undefined ? null : locate(harness, env)
}

/**
 * Every harness we know of and where its skills live — installed or not.
 *
 * Scope decisions may narrow to what is detected, but REMOVAL must not: a
 * harness that simply did not resolve on this run has not stopped existing,
 * and taking its skill away on that basis is how the same skill flapped in
 * and out on every mode apply.
 */
export function knownHarnesses(env) {
  return HARNESSES.map((harness) => ({ id: harness.id, skillsDir: harness.skillsDir(env) }))
}

export function detectHarnesses(env) {
  return HARNESSES.filter((harness) => isInstalled(harness, env)).map((harness) => ({
    id: harness.id,
    command: harness.command,
    skillsDir: harness.skillsDir(env),
    native: NATIVE[harness.id] !== undefined && existsSync(NATIVE[harness.id](env)),
  }))
}
