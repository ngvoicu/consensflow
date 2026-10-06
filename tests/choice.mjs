import assert from 'node:assert/strict'
import { appendFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Which implementation a test runs, Node's or the native one, and the proof of
 * which one did. `CONSENSFLOW_TEST_DAEMON` and `CONSENSFLOW_TEST_CLI` choose,
 * each in the same words:
 *
 *   node      Node's daemon (`src/core`), Node's cf (`bin/cf.mjs`)
 *   native    the native `cf` this checkout builds into bin/ (`npm run build:cf`):
 *             `cf ui --json --no-open` as the daemon
 *   [...]     a JSON array of strings, a command and its arguments, as the native one
 *   (none)    what the tests run when told nothing: DEFAULT_DAEMON, DEFAULT_CLI
 *
 * A runner that runs a suite once per implementation labels each run with
 * `CONSENSFLOW_TEST_LEG` (`node` or `native`), apart from what it selects:
 * what an empty selection comes to is the default's, and when the default is
 * the native one, a leg that only selected would run the native one under
 * Node's name. A selection that is not the leg's own is refused. The runner
 * reads, once the leg is over, what the run said it ran (`noteRan`, into the
 * file `CONSENSFLOW_TEST_RAN`), and fails a leg that ran what is not its own,
 * or did not say.
 *
 * The product chooses by the home, not by the environment: a `use-node` file in
 * ConsensFlow's home sends everything that writes that home to Node
 * (`src/use-node.js`, `crates/cf-base/src/way_back.rs`), and without it
 * everything is native. So a test's choice is made in its own home
 * (`chooseHome`): the Node leg's home has the file, the native leg's has not,
 * whatever starts in it. `CONSENSFLOW_DAEMON`, which chose before the flip, is
 * read by nothing.
 */

/** The native `cf` of this checkout, where `npm run build:cf` puts it. */
export const NATIVE_CF = join(
  fileURLToPath(new URL('..', import.meta.url)),
  'bin',
  process.platform === 'win32' ? 'cf.exe' : 'cf',
)

/**
 * Says the native `cf` of this checkout is built, which every suite that runs
 * it (and, since the flip, every one that runs nothing else) needs: `npm test`
 * runs the native one by default, so it needs `npm run build:cf` first, as CI's
 * prepare-sidecar step has it. A missing one is this sentence and not an ENOENT
 * from wherever it was first copied or started.
 */
export function assertBuilt(file = NATIVE_CF) {
  assert.ok(existsSync(file), `missing built cf: ${file}; build it with npm run build:cf`)
}

/**
 * What a test runs when it is told nothing: the native one, as the product
 * does since the flip. Nothing that names its leg moves with these two words.
 */
export const DEFAULT_DAEMON = 'native'
export const DEFAULT_CLI = 'native'

/** The two implementations, by the word that selects each. */
export const KINDS = ['node', 'native']

/** The file whose presence in a home is the way back to Node (`src/use-node.js`). */
export const WAY_BACK = 'use-node'

/**
 * Makes `home` the home of the implementation `kind` names: `node`'s has the
 * way back's file in it, `native`'s has not. Taken away as well as made, since
 * a home that went back to Node stays there until the file goes, and a restart
 * on the other implementation is a restart of the same home.
 */
export function chooseHome(kind, home) {
  assert.ok(KINDS.includes(kind), `no implementation is called ${kind}`)
  assert.ok(typeof home === 'string' && home !== '', `a home to choose in: ${home}`)
  const file = join(home, WAY_BACK)
  if (kind === 'node') {
    mkdirSync(home, { recursive: true })
    writeFileSync(file, '')
  } else {
    rmSync(file, { force: true })
  }
}

/** How each is said in a sentence. */
const NAMES = { node: "Node's", native: 'the native' }

/**
 * How a daemon's log starts its first line, `start pid <pid> <runtime> <version>
 * home <home>` (src/core/daemon.js, crates/cf-daemon/src/start.rs), by kind.
 */
export const START_WORDS = { node: 'node v', native: 'rust ' }

/**
 * The implementation `variable` names, in `named` (the variable's value) when it
 * says anything, else `fallback`: `{ kind, command, how }` where `command` is the
 * array a JSON selector gave, and `how` is `named` or `default`. `leg` is what
 * the run says it is, which the choice must be.
 */
export function choose(variable, { named, leg, fallback }) {
  const how = named === undefined || named === '' ? 'default' : 'named'
  const word = how === 'default' ? fallback : named
  let kind = word
  let command = null
  if (!KINDS.includes(word)) {
    let parsed
    try {
      parsed = JSON.parse(word)
    } catch {}
    if (
      !Array.isArray(parsed) ||
      parsed.length === 0 ||
      parsed.some((part) => typeof part !== 'string')
    ) {
      throw new Error(
        `${variable} is node, native, or a JSON array of strings, a command and its arguments: ${word}`,
      )
    }
    kind = 'native'
    command = parsed
  }
  if (leg !== undefined && leg !== '') {
    if (!KINDS.includes(leg)) throw new Error(`CONSENSFLOW_TEST_LEG is node or native: ${leg}`)
    if (leg !== kind) {
      throw new Error(
        `CONSENSFLOW_TEST_LEG says this is the ${leg} leg, but ${variable} ${
          how === 'default' ? `is not set, and the default is ${fallback}` : `names ${kind}`
        }`,
      )
    }
  }
  return { kind, command, how }
}

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
 * Says which implementation ran, to the runner that labelled the run: a line in
 * the file `CONSENSFLOW_TEST_RAN` names, which the runner reads once the leg is
 * over and holds the leg to (tests/legs.mjs), whatever the suites chose and
 * said. Nothing when no runner asked.
 */
export function noteRan(kind) {
  const file = process.env.CONSENSFLOW_TEST_RAN
  if (file) appendFileSync(file, `${kind}\n`)
}

/**
 * Says the daemon that started is the one that was asked for: `asked` is what
 * `daemonCommand` chose, `log` the daemon's log and `pid` its process. Returns
 * its start line. The daemon that started is noted for the runner (`noteRan`)
 * once it is the one asked for: one that is not is refused, which fails the
 * suite by itself, and a stand-in that a test starts to see it refused
 * (tests/integration/liar-daemon.mjs) is not what the leg ran.
 *
 * With the `home` it ran on, the home is held to the choice as well: the
 * product's `cf` verbs choose by the file in it, so a home that says another
 * implementation than the daemon would have the daemon and the verbs differ for
 * one home.
 */
export function assertStarted(asked, log, pid, home = undefined) {
  const start = startLine(log, pid)
  assert.notEqual(
    start,
    null,
    `${NAMES[asked.kind]} daemon was asked for, and its log holds no start line of pid ${pid}`,
  )
  assert.equal(
    start.kind,
    asked.kind,
    `${NAMES[asked.kind]} daemon was asked for, but the start line in its log says ${start.runtime}: ${start.line}`,
  )
  if (home !== undefined) {
    assert.equal(
      existsSync(join(home, WAY_BACK)),
      asked.kind === 'node',
      `${NAMES[asked.kind]} daemon was asked for, but ${home} ${
        asked.kind === 'node' ? 'has no' : 'has a'
      } ${WAY_BACK} file: the cf verbs of that home would be ${
        asked.kind === 'node' ? "the native cf's" : "Node's"
      } (chooseHome)`,
    )
  }
  noteRan(start.kind)
  return start
}
