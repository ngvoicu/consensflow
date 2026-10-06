/**
 * Plays a scenario against the legacy CLI, `node bin/cf.mjs`, and writes down
 * what it did: the oracle `crates/cf` is held to, case by case, by the Rust
 * player (`crates/cf/tests/cli_goldens/`).
 *
 * A scenario is data: `{ name, args, env, stdin, files, pipe, kept }`.
 * - `args`: the words after `cf`.
 * - `env`: the environment the CLI is given, over `ENV`, the one every
 *   scenario starts from (`tempEnv`'s, tests/helpers.mjs, and a PATH that is the
 *   scenario's own folder): a value of `null` takes the variable away. Nothing
 *   else is given, apart from what Node cannot start without (`SystemRoot` on
 *   Windows), which the player gives alike. `$ROOT` in any value is the
 *   scenario's own folder, made afresh, as the system names it.
 * - `stdin`: what the CLI is given to read, or none (it reads nothing).
 * - `files`: what the folder holds before: `{ path, text, executable }`, or
 *   `{ dir }`, the paths relative to it, written with `/`.
 * - `pipe`: where the CLI's output goes: `closed`, a pipe nobody reads, closed
 *   before the CLI starts to write (`cf … | false`), or `first-line`, one
 *   closed once its first line was read (`cf … | head -1`). Both say only that
 *   line, the error output and the exit code.
 * - `kept`: a difference Rust keeps from Node on purpose: `{ why, rust }`,
 *   `rust` the `{ stdout, stderr, code }` it gives where Node's are recorded,
 *   or a function of `recorded(name)`, the record of an earlier scenario, that
 *   makes them.
 *
 * What is recorded is the scenario's argument words, its environment, its
 * input, the files of the folder before and after (a folder with nothing in it
 * is `{ dir }`; the files of an extension, whose bytes are the repository's
 * own, are `$PAYLOAD`), and the output, the error output and the exit code.
 * The CLI's clock reads one instant (`clock.mjs`). What is of this machine is
 * not in what is recorded: the folder is `$ROOT`, the runtime that runs the
 * CLI `$NODE`, the repository `$REPO`, the version `$VERSION`, and the hash
 * that names the bundle of an extension `$HASH`. A text with any other of the
 * machine's places in it is refused, as the recorder meets it.
 */
import { spawn, spawnSync } from 'node:child_process'
import {
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = fileURLToPath(new URL('../../..', import.meta.url)).replace(/[\\/]$/, '')
const CF = join(REPO, 'bin', 'cf.mjs')
// A URL, not a path: `--import` reads `D:\…` on Windows as a URL with the scheme `d:`.
const CLOCK = new URL('./clock.mjs', import.meta.url).href
const VERSION = JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8')).version
const WINDOWS = process.platform === 'win32'

/** The environment every scenario starts from: `tempEnv`'s (tests/helpers.mjs). */
export const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  CLAUDE_CONFIG_DIR: '$ROOT/home/.claude',
  CODEX_HOME: '$ROOT/home/.codex',
  XDG_CONFIG_HOME: '$ROOT/home/.config',
  PATH: '$ROOT/bin',
  CONSENSFLOW_BIN_DIR: '$ROOT/consensflow/bin',
}

/** What a run may hold of this machine and nothing else of it, found by what it is. */
const BUNDLE = /([\\/]extensions[\\/](?:pi|opencode)[\\/])[0-9a-f]{64}/g
const PAYLOAD = /(^|\/)extensions\/(pi|opencode)\//

/** A text as it is recorded: what is of this machine, written as its name. */
function normalize(text, root) {
  const places = [
    [root, '$ROOT'],
    [process.execPath, '$NODE'],
    [REPO, '$REPO'],
  ].sort((a, b) => b[0].length - a[0].length)
  let said = text
  for (const [place, name] of places) said = said.replaceAll(place, name)
  return said.replaceAll(VERSION, '$VERSION').replace(BUNDLE, '$1$$HASH')
}

/**
 * The places of this machine a recorded text must not hold, whether the text is
 * the one recorded or its JSON, which writes a backslash of a Windows path twice.
 */
function leaks(text) {
  const places = [tmpdir(), realpathSync(tmpdir()), homedir(), REPO]
  return places.filter(
    (place) =>
      place.length > 3 &&
      (text.includes(place) || text.includes(JSON.stringify(place).slice(1, -1))),
  )
}

/** The folder's entries: each file with its text, each folder with nothing in it, by path. */
function listing(root) {
  const entries = []
  const named = (path) => relative(root, path).split(sep).join('/')
  const walk = (dir) => {
    const items = readdirSync(dir, { withFileTypes: true })
    if (items.length === 0 && dir !== root) entries.push({ dir: normalize(named(dir), root) })
    for (const item of items) {
      const path = join(dir, item.name)
      const name = named(path)
      if (item.isDirectory()) walk(path)
      else if (item.isFile()) {
        const text = PAYLOAD.test(name) ? '$PAYLOAD' : normalize(readFileSync(path, 'utf8'), root)
        const executable = !WINDOWS && (statSync(path).mode & 0o111) !== 0
        entries.push({
          path: normalize(name, root),
          text,
          ...(executable ? { executable } : {}),
        })
      } else throw new Error(`${path} is neither a file nor a folder`)
    }
  }
  walk(root)
  return entries.sort((a, b) => ((a.path ?? a.dir) < (b.path ?? b.dir) ? -1 : 1))
}

/** The folder, made as the scenario says. */
function make(root, files) {
  for (const entry of files) {
    if (entry.dir !== undefined) {
      mkdirSync(join(root, ...entry.dir.split('/')), { recursive: true })
      continue
    }
    const path = join(root, ...entry.path.split('/'))
    mkdirSync(dirname(path), { recursive: true })
    const text = entry.text
      .replaceAll('$ROOT', root)
      .replaceAll('$NODE', process.execPath)
      .replaceAll('$REPO', REPO)
    writeFileSync(path, text, entry.executable ? { mode: 0o755 } : {})
  }
}

/** What the scenario gives the CLI as its environment, `$ROOT` and all. */
function environment(scenario) {
  const env = { ...ENV, ...scenario.env }
  for (const [name, value] of Object.entries(env)) if (value === null) delete env[name]
  return env
}

/**
 * The run of a CLI whose output is read only so far: its line, its error
 * output, its exit code. On POSIX the CLI is the first of a shell's pipeline, as
 * a person runs it, and so writes to a pipe: what Node's own `spawn` gives a
 * child is a socket, which a closed end answers with ENOTCONN on macOS where a
 * pipe answers EPIPE. Windows gives a pipe either way.
 */
function piped(command, args, env, mode) {
  if (!WINDOWS) {
    const reader = mode === 'closed' ? 'false' : '/usr/bin/head -1'
    const ran = spawnSync(
      '/bin/bash',
      ['-c', `"$1" "\${@:2}" | ${reader}; exit \${PIPESTATUS[0]}`, 'cf', command, ...args],
      { env, encoding: 'utf8', timeout: 60_000 },
    )
    if (ran.error) throw ran.error
    return Promise.resolve({
      stdout: mode === 'closed' ? null : ran.stdout,
      stderr: ran.stderr,
      code: ran.status,
    })
  }
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    })
    let stderr = ''
    let first = null
    child.stderr.on('data', (chunk) => {
      stderr += chunk
    })
    child.on('error', reject)
    child.on('close', (code, signal) => {
      if (code === null) reject(new Error(`ended by ${signal}`))
      else resolve({ stdout: first, stderr, code })
    })
    if (mode === 'closed') {
      child.stdout.destroy()
      return
    }
    let seen = ''
    child.stdout.on('data', (chunk) => {
      if (first !== null) return
      seen += chunk
      const end = seen.indexOf('\n')
      if (end === -1) return
      first = seen.slice(0, end + 1)
      child.stdout.destroy()
    })
  })
}

/**
 * Plays `scenario` and records it. `recorded(name)` is the record of an
 * earlier scenario, which a kept difference may be made of.
 */
export async function play(scenario, recorded = () => undefined) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'cfcli-')))
  try {
    make(root, scenario.files ?? [])
    const given = environment(scenario)
    const env = Object.fromEntries(
      Object.entries(given).map(([name, value]) => [name, value.replaceAll('$ROOT', root)]),
    )
    if (WINDOWS) env.SystemRoot = process.env.SystemRoot
    const before = listing(root)
    const args = ['--import', CLOCK, CF, ...scenario.args]
    let ran
    if (scenario.pipe === undefined) {
      const done = spawnSync(process.execPath, args, {
        env,
        input: scenario.stdin ?? '',
        encoding: 'utf8',
        timeout: 60_000,
        maxBuffer: 256 * 1024 * 1024,
        windowsHide: true,
      })
      if (done.error) throw done.error
      if (done.status === null) throw new Error(`${scenario.name}: ended by ${done.signal}`)
      ran = { stdout: done.stdout, stderr: done.stderr, code: done.status }
    } else {
      ran = await piped(process.execPath, args, env, scenario.pipe)
    }
    const record = {
      name: scenario.name,
      args: scenario.args,
      env: given,
      stdin: scenario.stdin ?? null,
      before,
      stdout: ran.stdout === null ? null : normalize(ran.stdout, root),
      stderr: normalize(ran.stderr, root),
      code: ran.code,
      after: listing(root),
      ...(scenario.pipe === undefined ? {} : { pipe: scenario.pipe }),
    }
    const found = leaks(JSON.stringify(record))
    if (found.length > 0) {
      throw new Error(`${scenario.name}: what is recorded names ${found.join(', ')}`)
    }
    if (scenario.kept !== undefined) {
      const { why, rust } = scenario.kept
      record.kept = { why, rust: typeof rust === 'function' ? rust(recorded) : rust }
    }
    return record
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}
