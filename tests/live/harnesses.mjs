import { accessSync, constants, existsSync, readFileSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, dirname, join, resolve, sep } from 'node:path'

/**
 * Where the harnesses' CLIs and stores are on this machine, and how a CLI is
 * run, for the evals and the live tools, which start the real harnesses
 * themselves (the daemon has its own, in Rust: crates/cf-process and
 * crates/cf-harness). Windows is the hard part: a CLI there is a `.cmd` or
 * `.exe`, npm's `.cmd` shim is a script Node refuses to spawn, and a window
 * can open on it only as the shim's own node and script.
 */

const HOMED = (parts) => (env) => join(home(env), ...parts)

/** The harnesses' CLIs, and where each installs itself beyond PATH. */
const HARNESSES = [
  { id: 'devin', command: 'devin', locations: [HOMED(['.local', 'bin'])] },
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
  { id: 'pi', command: 'pi', locations: [HOMED(['.pi', 'bin']), HOMED(['.local', 'bin'])] },
]

/**
 * Where npm puts the shims of a global install on Windows: %APPDATA%\npm. A
 * terminal reaches it through the shell's own setup (the PATH entry Node's
 * installer adds), which an app started from the Start menu never runs. It
 * comes from the APPDATA of the environment given, never the machine's own;
 * one that is missing or empty adds nothing, and so does any other system.
 */
const NPM_GLOBAL = (env) => (onWindows(env) && env.APPDATA ? join(env.APPDATA, 'npm') : null)

/**
 * Per-user bin directories any of them might land in: each a function of the
 * environment, naming a folder or, where the environment has none, null.
 * Deliberately all HOME-relative, but for npm's on Windows, which lies under
 * APPDATA: system-wide places are already on every login PATH.
 */
const COMMON = [
  HOMED(['.bun', 'bin']),
  HOMED(['.npm-global', 'bin']),
  HOMED(['.volta', 'bin']),
  NPM_GLOBAL,
]

function home(env) {
  // Windows sets USERPROFILE, not HOME; homedir() knows that, but an explicit
  // env (every test, and a login environment) may carry either.
  return env.HOME ?? env.USERPROFILE ?? homedir()
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
 * What an executable is called, per platform. On Windows a CLI on PATH is
 * `claude.cmd` or `claude.exe`, never the bare name: npm writes an
 * extensionless sh script beside each `.cmd` shim, which no Windows program
 * can start. There is no executable bit to test either, so PATHEXT decides, in
 * its order, among the kinds a window can start, and "the file is there" is
 * the whole check.
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
 * Where this command resolves on PATH, as an ABSOLUTE path, or null. A PATH
 * entry may be relative, and joining that with a command name yields a
 * relative candidate the pane host refuses outright.
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

/**
 * The absolute path to a harness's CLI on this machine, or null: on PATH,
 * else in the harness's own places, else in the common ones, in that order. A
 * pane is opened with an argv the pane host refuses unless argv[0] is
 * absolute, and the app's own PATH is not the login shell's, so a tool asks
 * for the path, not the name.
 */
export function harnessPath(id, env) {
  const harness = HARNESSES.find((candidate) => candidate.id === id)
  if (harness === undefined) return null
  const onPath = pathOnPath(harness.command, env)
  if (onPath !== null) return onPath
  for (const place of [...harness.locations, ...COMMON]) {
    const dir = place(env)
    if (dir === null) continue
    for (const name of candidateNames(harness.command, env)) {
      const candidate = resolve(dir, name)
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
 * `{ file, args, options }`. A `.cmd` or `.bat` on Windows (an npm-installed
 * CLI is one) is a script for cmd.exe, not a program, and Node refuses to
 * spawn it directly. An npm-style shim is read for what it runs, and that runs
 * directly: no cmd.exe, so an argument may hold anything, a newline included.
 * Any other `.cmd` goes through cmd.exe, with every argument quoted and
 * escaped the way cmd.exe reads its line, then read again by the script (the
 * shape npm itself uses through cross-spawn); cmd.exe ends an argument at a
 * newline, so that path cannot carry one. Anything else runs as it is.
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
