import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { assertBuilt, choose, NATIVE_CF } from './choice.mjs'

/**
 * Every test runs against a throwaway CONSENSFLOW_HOME and throwaway harness
 * homes. The env object is passed explicitly to every module call — modules
 * never read process.env on their own, which is what makes this guard airtight.
 */
export function tempEnv() {
  const root = mkdtempSync(join(tmpdir(), 'cfv3-'))
  const env = {
    HOME: join(root, 'home'),
    CONSENSFLOW_HOME: join(root, 'consensflow'),
    CLAUDE_CONFIG_DIR: join(root, 'home', '.claude'),
    CODEX_HOME: join(root, 'home', '.codex'),
    XDG_CONFIG_HOME: join(root, 'home', '.config'),
    PATH: join(root, 'bin'),
    CONSENSFLOW_BIN_DIR: join(root, 'consensflow', 'bin'),
  }
  return {
    root,
    env,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

/**
 * What Windows itself needs to start a process and to find a program: where its
 * system is, its command interpreter, and which extensions make a file a program.
 * Nothing elsewhere. `tempEnv` gives a test none of it, since a test that starts
 * only the process it names does without; one that starts what starts a `.cmd` or
 * a program by its name adds this to the environment it gives.
 */
export const windowsEnv = () =>
  process.platform === 'win32'
    ? {
        SystemRoot: process.env.SystemRoot,
        ComSpec: process.env.ComSpec,
        PATHEXT: process.env.PATHEXT,
      }
    : {}

/**
 * How a test starts the daemon: `cf ui --json --no-open` of the native `cf`
 * this checkout builds, or the command `CONSENSFLOW_TEST_DAEMON` names (a JSON
 * array, a command and its arguments; the words are tests/choice.mjs's). The
 * daemon that starts is held to the native one by the start line in its log
 * (`assertStarted`). `named` is what a test sets to choose in its own words, not
 * the environment's.
 */
export function daemonCommand({ named = process.env.CONSENSFLOW_TEST_DAEMON } = {}) {
  const chosen = choose('CONSENSFLOW_TEST_DAEMON', named)
  if (chosen === null) assertBuilt()
  const [command, ...args] = chosen ?? [NATIVE_CF, 'ui', '--json', '--no-open']
  return { command, args }
}

const WINDOWS = process.platform === 'win32'
/** The name a fake is found at: as given on POSIX, `.cmd` on Windows unless it already says so. */
function fakePath(file) {
  return WINDOWS && !/\.(cmd|bat)$/i.test(file) ? `${file}.cmd` : file
}

/**
 * A stand-in CLI for the tests, at `file`: a shell script on POSIX, a `.cmd`
 * on Windows, since Windows has no shebang. It prints `output` (or the file
 * `outputFile`), creates `touch` when asked, adds a line to `log` each time it
 * is run when asked (to count its runs), and exits with `exit`. Returns the
 * path the fake is found at, which is what `harnessPath` resolves.
 */
export function fakeExecutable(
  file,
  { output = '', outputFile = null, touch = null, log = null, exit = 0 } = {},
) {
  const path = fakePath(file)
  if (WINDOWS) {
    const lines = ['@echo off']
    if (log) lines.push(`echo run>> "${log}"`)
    for (const line of output.split('\n').filter(Boolean)) lines.push(`echo ${line}`)
    if (outputFile) lines.push(`type "${outputFile}"`)
    if (touch) lines.push(`type nul > "${touch}"`)
    lines.push(`exit /b ${exit}`)
    writeFileSync(path, `${lines.join('\r\n')}\r\n`)
    return path
  }
  const lines = ['#!/bin/sh']
  if (log) lines.push(`printf 'run\\n' >> '${log}'`)
  if (output) lines.push(`printf '%s\\n' '${output.replaceAll("'", "'\\''")}'`)
  if (outputFile) lines.push(`/bin/cat "${outputFile}"`)
  if (touch) lines.push(`printf called > '${touch}'`)
  lines.push(`exit ${exit}`)
  writeFileSync(path, `${lines.join('\n')}\n`, { mode: 0o755 })
  return path
}
