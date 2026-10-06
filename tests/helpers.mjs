import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { roleConfiguration } from '../src/role-skills.js'
import { assertBuilt, choose, chooseHome, DEFAULT_DAEMON, NATIVE_CF } from './choice.mjs'

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
 * How a test starts a daemon, as `CONSENSFLOW_TEST_DAEMON` names it (the words
 * are tests/choice.mjs's): `node`, Node's, `node <nodeArgs>`; `native` or a JSON
 * array, a command and its arguments, the native one (`cf ui --json --no-open`
 * of the build under test); nothing, the tests' default, which is the native
 * one. The product chooses by the file in the home, not by the environment, so
 * the choice is made in the home the daemon is to run on: `home` is that
 * folder, which the Node daemon's gets the way back's file in and the native
 * one's has none (`chooseHome`), whatever else starts in it, and which
 * `assertStarted` holds to the choice. A start that names none leaves the home
 * to its caller. Either is told the runtime to name to the windows it opens
 * (`CONSENSFLOW_NODE`): the Node daemon names its own whatever this says, the
 * native one names what it is given. `env` is what to add to the environment
 * the test gives the daemon, and `kind` is the one chosen, `node` or `native`:
 * what `assertStarted` holds the daemon that starts to. A run labelled with
 * its leg (`CONSENSFLOW_TEST_LEG`) is refused a choice that is not its own.
 * The options are what a test sets to choose in its own words, not the
 * environment's: the selection, the leg, and the default.
 */
export function daemonCommand(
  nodeArgs,
  {
    named = process.env.CONSENSFLOW_TEST_DAEMON,
    leg = process.env.CONSENSFLOW_TEST_LEG,
    fallback = DEFAULT_DAEMON,
    home = undefined,
  } = {},
) {
  const chosen = choose('CONSENSFLOW_TEST_DAEMON', { named, leg, fallback })
  const env = { CONSENSFLOW_NODE: process.execPath }
  if (home !== undefined) chooseHome(chosen.kind, home)
  if (chosen.kind === 'node') {
    return {
      command: process.execPath,
      args: nodeArgs,
      env,
      native: false,
      kind: 'node',
    }
  }
  if (chosen.command === null) assertBuilt()
  const [command, ...args] = chosen.command ?? [NATIVE_CF, 'ui', '--json', '--no-open']
  return {
    command,
    args,
    env,
    native: true,
    kind: 'native',
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

/**
 * Where `actual` first differs from `expected`, a line of each around it: a
 * golden file is megabytes, often one long line per scenario, which an
 * assertion's own message cuts off before the difference.
 */
export function firstDifference(actual, expected) {
  const lines = actual.split('\n')
  const wanted = expected.split('\n')
  const line = lines.findIndex((text, at) => text !== wanted[at])
  const at = line === -1 ? lines.length : line
  const [got, want] = [lines[at] ?? '', wanted[at] ?? '']
  let column = 0
  while (column < got.length && got[column] === want[column]) column += 1
  const around = (text) => text.slice(Math.max(0, column - 300), column + 300)
  return `line ${at + 1}, column ${column + 1}:\n  actual   …${around(got)}…\n  expected …${around(want)}…`
}
