import {
  accessSync,
  chmodSync,
  constants,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * The terminal command, installed by the app.
 *
 * The app already carries a working runtime and a working copy of the CLI,
 * so putting `consensflow` on PATH is a launcher pointing at those — not a
 * second installation to keep in sync, and nothing to do with npm. This is
 * the same move VS Code makes with its `code` command.
 */

const NAMES = ['consensflow', 'cf']

/**
 * Where a user-installed command can go, best first. `/usr/local/bin` is on
 * everyone's PATH but often needs privileges; `~/.local/bin` always belongs
 * to the user. `CONSENSFLOW_BIN_DIR` overrides both — which is also what
 * keeps tests off the real machine.
 */
function defaultCandidates(env) {
  const explicit = env.CONSENSFLOW_BIN_DIR
  if (typeof explicit === 'string' && explicit.length > 0) return [explicit]
  return ['/usr/local/bin', join(env.HOME ?? homedir(), '.local', 'bin')]
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

function launcher() {
  const { runtime, cli } = selfPaths()
  return `#!/bin/sh
# Installed by ConsensFlow. Runs the app's own runtime and its own copy of
# the CLI, so the terminal and the window never drift apart.
exec "${runtime}" "${cli}" "$@"
`
}

export function terminalCommandStatus(env, options = {}) {
  const candidates = options.candidates ?? defaultCandidates(env)
  for (const dir of candidates) {
    const path = join(dir, NAMES[0])
    if (!existsSync(path)) continue
    // Only ours: a `consensflow` someone else put there is not ours to report
    // as installed, and certainly not ours to remove.
    if (!readFileSync(path, 'utf8').includes('Installed by ConsensFlow')) continue
    const onPath = (env.PATH ?? '').split(delimiter).includes(dir)
    return { installed: true, path, dir, onPath }
  }
  return { installed: false, onPath: false }
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

  const script = launcher()
  for (const name of NAMES) {
    const path = join(dir, name)
    // Someone else's `cf` is left alone; ours is replaced.
    if (existsSync(path) && !readFileSync(path, 'utf8').includes('Installed by ConsensFlow')) {
      continue
    }
    writeFileSync(path, script)
    chmodSync(path, 0o755)
  }

  return terminalCommandStatus(env, options)
}

export function removeTerminalCommand(env, options = {}) {
  const candidates = options.candidates ?? defaultCandidates(env)
  const removed = []
  for (const dir of candidates) {
    for (const name of NAMES) {
      const path = join(dir, name)
      if (!existsSync(path)) continue
      if (!readFileSync(path, 'utf8').includes('Installed by ConsensFlow')) continue
      rmSync(path, { force: true })
      removed.push(path)
    }
  }
  return { removed }
}
