import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * Every test of the page (`app/tests`) runs against a throwaway CONSENSFLOW_HOME
 * and throwaway harness homes. The env object is passed explicitly to every
 * program the test starts, which never reads the machine's own environment: that
 * is what makes this guard airtight.
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
 * path the fake is found at, which is what the daemon's lookup of a harness's
 * CLI on PATH resolves.
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
