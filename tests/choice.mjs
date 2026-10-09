import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Which daemon and which cf a test runs, and the proof of which daemon did. The
 * native `cf` this checkout builds into bin/ (`npm run build:cf`) is the only
 * implementation there is: `cf ui --json --no-open` is the daemon, and the
 * product chooses nothing (the flip release sent a home to Node by a `use-node`
 * file, and the app that ships no Node reads none). `CONSENSFLOW_TEST_DAEMON`
 * can name another build of it, in these words (the suites of the CLI, which
 * `CONSENSFLOW_TEST_CLI` named another build for, are Rust's now: crates/cf-e2e
 * builds the `cf` it runs, and chooses nothing):
 *
 *   native    the native `cf` this checkout builds: what nothing at all selects too
 *   [...]     a JSON array of strings, a command and its arguments
 *
 * `CONSENSFLOW_DAEMON`, which chose before the flip, is read by nothing.
 */

const REPO = fileURLToPath(new URL('..', import.meta.url))

/** The native `cf` of this checkout, where `npm run build:cf` puts it. */
export const NATIVE_CF = join(REPO, 'bin', process.platform === 'win32' ? 'cf.exe' : 'cf')

/**
 * Says the native `cf` of this checkout is built, which every suite that runs
 * it (and every one that runs nothing else) needs: `npm test` runs the native
 * one by default, so it needs `npm run build:cf` first, as CI's prepare-sidecar
 * step has it. A missing one is this sentence and not an ENOENT from wherever
 * it was first copied or started.
 */
export function assertBuilt(file = NATIVE_CF) {
  assert.ok(existsSync(file), `missing built cf: ${file}; build it with npm run build:cf`)
}

/**
 * Builds the native `cf` of this checkout and puts it in bin/ (`cargo xtask
 * build-cf`, which `npm run build:cf` is), for the drivers that run a suite
 * against it. `offline` is handed to cargo as it is. The path it is at, said to
 * be there.
 */
export function buildNativeCf({ offline = false } = {}) {
  execFileSync('cargo', ['xtask', 'build-cf', ...(offline ? ['--offline'] : [])], {
    cwd: REPO,
    stdio: 'inherit',
  })
  assertBuilt()
  return NATIVE_CF
}

/**
 * The command `variable` names in `named` (the variable's value): the array a
 * JSON selector gave, or null for the native `cf` of this checkout, which
 * `native` and nothing at all name.
 */
export function choose(variable, named) {
  if (named === undefined || named === '' || named === 'native') return null
  let parsed
  try {
    parsed = JSON.parse(named)
  } catch {}
  if (
    !Array.isArray(parsed) ||
    parsed.length === 0 ||
    parsed.some((part) => typeof part !== 'string')
  ) {
    throw new Error(
      `${variable} is native, or a JSON array of strings, a command and its arguments: ${named}`,
    )
  }
  return parsed
}

/**
 * How a daemon's log starts its first line, `start pid <pid> <runtime> <version>
 * home <home>` (crates/cf-daemon/src/start.rs; Node's daemon, which the apps of
 * the releases before the deletion ran, wrote `node v…`), by kind.
 */
export const START_WORDS = { node: 'node v', native: 'rust ' }

/**
 * The start line a daemon's log holds for `pid` (the last one, the log being
 * written on to by each start; any pid when none is given): which daemon
 * started, by the line it wrote first of all, or null when there is none.
 */
export function startLine(log, pid = null) {
  const found = [...log.matchAll(/^\S+ info start pid (\d+) ((node v|rust )\S+) home .*$/gm)]
    .filter((hit) => pid === null || Number(hit[1]) === pid)
    .at(-1)
  if (found === undefined) return null
  return {
    pid: Number(found[1]),
    runtime: found[2],
    kind: found[3] === START_WORDS.native ? 'native' : 'node',
    line: found[0],
  }
}

/**
 * The lines of a daemon log that one process wrote, from its start line to the
 * start of the next: the log is written on to by every start on the home.
 */
export function linesOf(log, pid) {
  const lines = log.split('\n')
  const started = (line) => /^\S+ info start pid (\d+) /.exec(line)?.[1]
  const from = lines.findIndex((line) => started(line) === String(pid))
  if (from === -1) return []
  const to = lines.findIndex((line, at) => at > from && started(line) !== undefined)
  return lines.slice(from, to === -1 ? undefined : to).filter(Boolean)
}

/** The lines among them that say an error: its level is the line's second word. */
export const errorLines = (lines) => lines.filter((line) => /^\S+ error /.test(line))

/**
 * Each error among them with what the log says under it (the error's own first
 * line, indented): an error is logged as what was being done, and the cause
 * follows.
 */
export const errorsWithCause = (lines) =>
  lines.flatMap((line, at) =>
    /^\S+ error /.test(line)
      ? [lines[at + 1]?.startsWith('    ') ? `${line} ${lines[at + 1].trim()}` : line]
      : [],
  )

/**
 * Says the daemon that started is the native one: `log` is the daemon's log and
 * `pid` its process. Returns its start line. A daemon that is not, whatever a
 * selector named, is refused, which fails the suite by itself; and a stand-in
 * that a test starts to see it refused is tests/integration/liar-daemon.mjs.
 */
export function assertStarted(log, pid) {
  const start = startLine(log, pid)
  assert.notEqual(
    start,
    null,
    `the native daemon was asked for, and its log holds no start line of pid ${pid}`,
  )
  assert.equal(
    start.kind,
    'native',
    `the native daemon was asked for, but the start line in its log says ${start.runtime}: ${start.line}`,
  )
  return start
}
