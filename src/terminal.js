import {
  accessSync,
  chmodSync,
  constants,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs'
import { delimiter, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { configRoot } from './roster.js'

/**
 * The terminal command, installed by the app.
 *
 * The app already carries a working runtime and a working copy of the CLI,
 * so putting `consensflow` on PATH is a launcher pointing at those — not a
 * second installation to keep in sync, and nothing to do with npm. This is
 * the same move VS Code makes with its `code` command.
 */

const NAMES = ['consensflow', 'cf']

/** The marker that says a launcher is ours to replace or remove. */
const MARKER = 'Installed by ConsensFlow'

function isWindows(env) {
  return process.platform === 'win32' || (env.OS ?? '').toLowerCase().includes('windows')
}

/** Runtime launchers belong to ConsensFlow home, never a project or global bin. */
function defaultCandidates(env) {
  return [join(configRoot(env), 'bin')]
}

function writable(dir) {
  try {
    accessSync(dir, constants.W_OK)
    return true
  } catch {
    return false
  }
}

/** The runtime and entry point of the copy running right now. */
function selfPaths() {
  const cli = join(dirname(dirname(fileURLToPath(import.meta.url))), 'bin', 'cf.mjs')
  return { runtime: process.execPath, cli }
}

function launcher(env) {
  const { runtime, cli } = selfPaths()
  if (isWindows(env)) {
    // A .cmd shim, because Windows has no shebang: the same idea, spelled the
    // way cmd.exe understands, and `%*` forwards the arguments.
    return `@echo off\r\nREM ${MARKER}. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n"${runtime}" "${cli}" %*\r\n`
  }
  return `#!/bin/sh
# ${MARKER}. Runs the app's own runtime and its own copy of
# the CLI, so the terminal and the window never drift apart.
exec "${runtime}" "${cli}" "$@"
`
}

/** What the launcher is called here: `cf` on POSIX, `cf.cmd` on Windows. */
function launcherNames(env) {
  return isWindows(env) ? NAMES.map((name) => `${name}.cmd`) : NAMES
}

export function terminalCommandStatus(env, options = {}) {
  const candidates = options.candidates ?? defaultCandidates(env)
  for (const dir of candidates) {
    const path = join(dir, launcherNames(env)[0])
    if (!existsSync(path)) continue
    // Only ours: a `consensflow` someone else put there is not ours to report
    // as installed, and certainly not ours to remove.
    if (!readFileSync(path, 'utf8').includes(MARKER)) continue
    const onPath = (env.PATH ?? '').split(delimiter).includes(dir)
    return { installed: true, path, dir, onPath }
  }
  return { installed: false, onPath: false }
}

/**
 * The runtime the launcher names, whether it is still there, and whether it is
 * the copy asking.
 *
 * Nothing ConsensFlow installs assumes Node on PATH, and after the host
 * payloads went the launcher is the only installed file that names a runtime
 * at all — absolutely, so a machine without Node still works. The cost is that
 * moving or deleting whatever provided it (from the app, its own bundled Node)
 * breaks every `cf` the skill teaches, so `cf doctor` says so rather than
 * letting it fail one command at a time.
 *
 * `mine` is the other half of that, and the one nothing reported: a launcher
 * naming a runtime that still EXISTS looks healthy from every angle while
 * running an older ConsensFlow. It happens whenever a machine holds two — an
 * app in /Applications and a build in a repo — and the newer one is not the
 * one on PATH. Only a copy that is not the launcher's can notice, which is
 * why the app's own page is where this shows: `cf` itself is, by definition,
 * whatever the launcher started.
 */
export function terminalRuntime(env, options = {}) {
  const status = terminalCommandStatus(env, options)
  if (!status.installed) return null
  // Both halves of the line, because both answer a question. The runtime is
  // what `cf doctor` reports; `entry` is the cf.mjs the command actually runs,
  // which is how a developer finds which COPY of ConsensFlow is on PATH —
  // `app/scripts/sync-cli.mjs` mirrored only the repo's own bundle until
  // 2026-09-02, and reported five untouched skills as success. Reported as
  // written, not interpreted: what a path means is the caller's business.
  const line = readFileSync(status.path, 'utf8').match(/"([^"]+)"\s+"([^"]+cf\.mjs)"/)
  if (line === null) return null
  const [, runtime, entry] = line
  return { runtime, entry, exists: existsSync(runtime), mine: runtime === selfPaths().runtime }
}

export function installTerminalCommand(env, options = {}) {
  const candidates = options.candidates ?? defaultCandidates(env)
  // A user-owned directory is created rather than reported missing; a
  // system one is never created.
  for (const candidate of candidates) {
    if (existsSync(candidate)) continue
    if (candidate.startsWith('/usr') || candidate.startsWith('/opt')) continue
    try {
      mkdirSync(candidate, { recursive: true })
    } catch {
      // Cannot make it; the writability check below will say so.
    }
  }

  const dir = candidates.find((candidate) => writable(candidate))
  if (dir === undefined) {
    throw new Error(
      `no writable directory for the command (tried ${candidates.join(', ')}) — create one, or add it yourself`,
    )
  }

  const script = launcher(env)
  for (const name of launcherNames(env)) {
    const path = join(dir, name)
    // Someone else's `cf` is left alone; ours is replaced.
    if (existsSync(path) && !readFileSync(path, 'utf8').includes(MARKER)) {
      continue
    }
    writeFileSync(path, script)
    if (!isWindows(env)) chmodSync(path, 0o755)
  }

  return terminalCommandStatus(env, options)
}
