import { execFile, spawnSync } from 'node:child_process'
import { accessSync, constants, existsSync, readFileSync, realpathSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, dirname, join, resolve, sep } from 'node:path'
import { promisify } from 'node:util'

/** Discover CLI executables on PATH and native user install paths.
 * Finder-launched apps may lack the interactive shell's tool directories.
 */
const HOMED = (parts) => (env) => join(home(env), ...parts)

const HARNESSES = [
  {
    id: 'devin',
    command: 'devin',
    locations: [HOMED(['.local', 'bin'])],
  },
  {
    id: 'claude',
    command: 'claude',
    locations: [HOMED(['.local', 'bin']), HOMED(['.claude', 'local'])],
  },
  {
    id: 'codex',
    command: 'codex',
    locations: [HOMED(['.codex', 'bin']), HOMED(['.local', 'bin'])],
  },
  {
    id: 'opencode',
    command: 'opencode',
    locations: [HOMED(['.opencode', 'bin']), HOMED(['.local', 'bin'])],
  },
  {
    id: 'pi',
    command: 'pi',
    locations: [HOMED(['.pi', 'bin']), HOMED(['.local', 'bin'])],
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

function piAgentDir(env) {
  return piPath(env.PI_CODING_AGENT_DIR || join(home(env), '.pi', 'agent'), env)
}

export function piSessionDir(env) {
  return piPath(env.PI_CODING_AGENT_SESSION_DIR || join(piAgentDir(env), 'sessions'), env)
}

/** Windows, by the environment's own word or the platform this runs on. */
export function onWindows(env) {
  return (env.OS ?? '').toLowerCase().includes('windows') || process.platform === 'win32'
}

/**
 * Where OpenCode's store, opencode.db, may be, likeliest first: OPENCODE_DB,
 * or OPENCODE_DATA, when set (OpenCode honours both everywhere); else its XDG
 * data place, which it uses on every platform; on Windows, %LOCALAPPDATA%
 * and %APPDATA% too, where some of its versions keep it.
 */
export function opencodeStores(env) {
  if (env.OPENCODE_DB) return [env.OPENCODE_DB]
  if (env.OPENCODE_DATA) return [join(env.OPENCODE_DATA, 'opencode.db')]
  const xdg = join(
    env.XDG_DATA_HOME ?? join(home(env), '.local', 'share'),
    'opencode',
    'opencode.db',
  )
  if (!onWindows(env)) return [xdg]
  return [
    xdg,
    ...[env.LOCALAPPDATA, env.APPDATA]
      .filter(Boolean)
      .map((folder) => join(folder, 'opencode', 'opencode.db')),
  ]
}

/**
 * Devin's own folders: `config` holds its config.json, `data` its cli/
 * sessions.db. On Windows both are %APPDATA%\devin (Devin's docs); elsewhere
 * the XDG places, ~/.config/devin and ~/.local/share/devin unless set.
 */
export function devinFolders(env) {
  if (onWindows(env)) {
    const roaming = join(env.APPDATA ?? join(home(env), 'AppData', 'Roaming'), 'devin')
    return { config: roaming, data: roaming }
  }
  return {
    config: join(env.XDG_CONFIG_HOME ?? join(home(env), '.config'), 'devin'),
    data: join(env.XDG_DATA_HOME ?? join(home(env), '.local', 'share'), 'devin'),
  }
}

/** The kinds of file a window can start on Windows: a program, or a script cmd.exe runs. */
const STARTABLE = new Set(['.com', '.exe', '.bat', '.cmd'])

/**
 * What an executable is called, per platform.
 *
 * On Windows a CLI on PATH is `claude.cmd` or `claude.exe`, never the bare
 * name: npm writes an extensionless sh script beside each `.cmd` shim, which
 * no Windows program can start. There is no executable bit to test either, so
 * PATHEXT decides, in its order, among the kinds a window can start, and "the
 * file is there" is the whole check.
 */
function candidateNames(command, env) {
  if (onWindows(env)) {
    return (env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD')
      .split(';')
      .map((ext) => ext.toLowerCase())
      .filter((ext) => STARTABLE.has(ext))
      .map((ext) => `${command}${ext}`)
  }
  return [command]
}

/**
 * Where this command resolves on PATH, as an ABSOLUTE path, or null.
 *
 * A PATH entry may be relative — `PATH=.:…`, or a `bin` some launcher
 * exported from wherever it happened to be — and joining that with a
 * command name yields a relative candidate the pane host refuses outright
 * (`validate_open_request` in `app/src-tauri/src/pane_handlers.rs`). Resolving here means every caller
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

/** cmd.exe's own special characters; each is escaped with a caret. */
const CMD_META = /([()\][%!^"`<>&|;, *?])/g

/**
 * What an npm-style `.cmd` shim runs, read from its last line: `"<program>"
 * "<script>" %*`, where the program is `%_prog%` or `%NODE_EXE%` (the node
 * beside the shim, else the node on PATH, else this one) and the script may
 * begin with `%dp0%` or `%~dp0`, the shim's own directory. Null when the file
 * is not of that shape, or names something that is not there.
 */
function shimTarget(shim, env) {
  let text
  try {
    text = readFileSync(shim, 'utf8')
  } catch {
    return null
  }
  const line = text
    .split(/\r?\n/)
    .reverse()
    .find((candidate) => candidate.includes('%*'))
  if (line === undefined) return null
  const quoted = [...line.matchAll(/"([^"]+)"/g)].map((match) => match[1])
  if (quoted.length < 2) return null
  const dir = dirname(shim)
  // The shim spells its paths with backslashes; the platform's separator here.
  const expand = (value) =>
    value.replace(/^%(?:~dp0|dp0%)\\?/i, `${dir}${sep}`).replaceAll('\\', sep)
  const [program, script] = quoted.slice(-2)
  const scriptPath = expand(script)
  if (scriptPath.includes('%') || !existsSync(scriptPath)) return null
  let programPath = expand(program)
  if (/^%(_prog|NODE_EXE)%$/i.test(program)) {
    const beside = join(dir, 'node.exe')
    programPath = existsSync(beside) ? beside : (pathOnPath('node', env) ?? process.execPath)
  }
  if (programPath.includes('%')) return null
  return { program: programPath, script: scriptPath }
}

/**
 * How to run `executable` with `args` on this machine, for spawn or execFile:
 * `{ file, args, options }`.
 *
 * A `.cmd` or `.bat` on Windows (an npm-installed CLI is one) is a script for
 * cmd.exe, not a program, and Node refuses to spawn it directly. An npm-style
 * shim is read for what it runs, and that runs directly: no cmd.exe, so an
 * argument may hold anything, a newline included. Any other `.cmd` goes
 * through cmd.exe, with every argument quoted and escaped the way cmd.exe
 * reads its line, then read again by the script (the shape npm itself uses
 * through cross-spawn); cmd.exe ends an argument at a newline, so that path
 * cannot carry one. Anything else runs as it is.
 */
export function runnable(executable, args = [], env = process.env) {
  if (!/\.(cmd|bat)$/i.test(executable)) return { file: executable, args, options: {} }
  const target = shimTarget(executable, env)
  if (target !== null) return { file: target.program, args: [target.script, ...args], options: {} }
  const quote = (arg) =>
    `"${String(arg)
      .replace(/(\\*)"/g, '$1$1\\"')
      .replace(/(\\+)$/, '$1$1')}"`
      .replace(CMD_META, '^$1')
      .replace(CMD_META, '^$1')
  const line = [executable.replace(CMD_META, '^$1'), ...args.map(quote)].join(' ')
  // Named absolutely: a launch environment may carry a PATH of its own that
  // has no System32 on it, and cmd.exe must still be found.
  return {
    file: env.ComSpec ?? process.env.ComSpec ?? 'C:\\Windows\\System32\\cmd.exe',
    args: ['/d', '/s', '/c', `"${line}"`],
    options: { windowsVerbatimArguments: true },
  }
}

/**
 * A window's program as the pane host can start it. The host starts a file
 * with each argument quoted the way programs read them, which cmd.exe does
 * not, so an npm-installed harness on Windows (a `.cmd` shim) opens as the
 * shim's own node and script; a `.cmd` of any other shape cannot open a window.
 */
export function paneArgv(argv, env = process.env) {
  const [executable, ...args] = argv
  if (!/\.(cmd|bat)$/i.test(executable)) return argv
  const target = shimTarget(executable, env)
  if (target === null) {
    throw new Error(`${executable} is not an npm shim, and only cmd.exe could run it in a window`)
  }
  return [target.program, target.script, ...args]
}

const execute = promisify(execFile)
const probes = new Map()

/**
 * What a CLI answers to `args` (`--version`, `queue --help`): its output and
 * exit code. It is asked once per executable as it is on disk: an update
 * gives the file another identity, so an updated CLI is asked again and an
 * unchanged one never twice. One that does not answer in time (a busy
 * machine took seconds to answer `--help`) or cannot start is not remembered.
 */
export function probeExecutable(executable, args, env, { timeoutMs = 30_000 } = {}) {
  const file = realpathSync(executable)
  const { dev, ino, size, mtimeMs, ctimeMs } = statSync(file)
  const key = JSON.stringify([file, dev, ino, size, mtimeMs, ctimeMs, args])
  let probe = probes.get(key)
  if (probe === undefined) {
    const run = runnable(executable, args, env)
    probe = execute(run.file, run.args, {
      ...run.options,
      env,
      timeout: timeoutMs,
      maxBuffer: 128 * 1024,
      encoding: 'utf8',
      windowsHide: true,
    }).then(
      ({ stdout }) => ({ stdout, code: 0 }),
      (error) => {
        if (typeof error.code !== 'number') throw error
        return { stdout: error.stdout ?? '', code: error.code }
      },
    )
    probes.set(key, probe)
    probe.catch(() => probes.delete(key))
  }
  return probe
}

/**
 * Ends a child started through `runnable`, and everything it started. On
 * Windows the child may be the cmd.exe wrapper of a `.cmd`, and killing it
 * alone leaves the program it started running, so the whole tree goes, at
 * once (Windows has no gentle signal a process can act on). Elsewhere the
 * signal goes to the child as asked.
 */
export function terminate(child, signal = 'SIGTERM') {
  if (process.platform !== 'win32') {
    child.kill(signal)
    return
  }
  if (child.pid === undefined || child.exitCode !== null || child.signalCode !== null) return
  spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
    stdio: 'ignore',
    windowsHide: true,
  })
}

/**
 * The absolute path to a harness's CLI on this machine, or null.
 *
 * A pane is opened with an argv the pane host refuses unless argv[0] is
 * absolute (`validate_open_request` in `app/src-tauri/src/pane_handlers.rs`),
 * and the app's own PATH is not the login shell's — the same reason
 * detection looks past PATH at all. So the launcher asks for the path, not
 * the name.
 */
export function harnessPath(id, env) {
  const harness = HARNESSES.find((candidate) => candidate.id === id)
  return harness === undefined ? null : locate(harness, env)
}

/** The supported harnesses whose CLI is not installed here, by id. */
export function missingHarnesses(env) {
  return HARNESSES.filter((harness) => !isInstalled(harness, env)).map((harness) => harness.id)
}

/**
 * Agents as the pickers offer them: one whose harness is not installed here is
 * hidden, so only the Harnesses page shows that harness, where it is installed.
 * One saved for a harness this build does not run (Kimi, dropped) is hidden
 * too: no window could open for it.
 */
export function offerable(agents, missing) {
  return agents.map((agent) =>
    agent.unsupported || missing.includes(agent.harness)
      ? { ...agent, hidden: true, notInstalled: true }
      : agent,
  )
}

/** All supported harness identities, whether installed or not. */
export function knownHarnesses() {
  return HARNESSES.map(({ id }) => ({ id }))
}

export function detectHarnesses(env) {
  return HARNESSES.filter((harness) => isInstalled(harness, env)).map((harness) => ({
    id: harness.id,
    command: harness.command,
  }))
}
