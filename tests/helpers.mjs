import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { roleConfiguration } from '../src/role-skills.js'

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

/** Native config resolution is a subprocess boundary, covered in role-skills.test. */
export const testRoleConfiguration = (kind, options) =>
  roleConfiguration(kind, {
    ...options,
    readInstructions: async () => '',
  })

const WINDOWS = process.platform === 'win32'
/** The name a fake is found at: as given on POSIX, `.cmd` on Windows unless it already says so. */
function fakePath(file) {
  return WINDOWS && !/\.(cmd|bat)$/i.test(file) ? `${file}.cmd` : file
}

/**
 * A stand-in CLI for the tests, at `file`: a shell script on POSIX, a `.cmd`
 * on Windows, since Windows has no shebang. It prints `output` (or the file
 * `outputFile`), creates `touch` when asked, and exits with `exit`. Returns
 * the path the fake is found at, which is what `harnessPath` resolves.
 */
export function fakeExecutable(
  file,
  { output = '', outputFile = null, touch = null, exit = 0 } = {},
) {
  const path = fakePath(file)
  if (WINDOWS) {
    const lines = ['@echo off']
    for (const line of output.split('\n').filter(Boolean)) lines.push(`echo ${line}`)
    if (outputFile) lines.push(`type "${outputFile}"`)
    if (touch) lines.push(`type nul > "${touch}"`)
    lines.push(`exit /b ${exit}`)
    writeFileSync(path, `${lines.join('\r\n')}\r\n`)
    return path
  }
  const lines = ['#!/bin/sh']
  if (output) lines.push(`printf '%s\\n' '${output.replaceAll("'", "'\\''")}'`)
  if (outputFile) lines.push(`/bin/cat "${outputFile}"`)
  if (touch) lines.push(`printf called > '${touch}'`)
  lines.push(`exit ${exit}`)
  writeFileSync(path, `${lines.join('\n')}\n`, { mode: 0o755 })
  return path
}

/**
 * A stand-in CLI written in JavaScript, run by this very Node: on POSIX the
 * script itself with a shebang, on Windows the script beside a `.cmd` that
 * hands it the arguments. Returns the path the fake is found at.
 */
export function fakeNodeExecutable(file, source) {
  const body = source.replace(/^#!.*\n/, '')
  if (WINDOWS) {
    const script = `${file}.mjs`
    writeFileSync(script, body)
    const path = fakePath(file)
    writeFileSync(path, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`)
    return path
  }
  writeFileSync(file, `#!${process.execPath}\n${body}`, { mode: 0o755 })
  return file
}
