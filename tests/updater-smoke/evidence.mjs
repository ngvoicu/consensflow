import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { errorLines, linesOf, startLine } from '../choice.mjs'
import { terminalEnv } from './box.mjs'
import { alive, processTable, until } from './processes.mjs'

/**
 * What the smoke reads to know that an app is up and what is under it: the
 * app's own process, its daemon's process and readiness, and the ledger's one
 * holder. Each is read from what the machine shows (the process table, the
 * logs the app and its daemon write, the ledger's own refusal), not from a file
 * nothing writes, and each reader is a function of what it is given, so that
 * tests/updater-smoke-evidence.test.mjs can hold every one to its words.
 */

/** A file of the app's or the daemon's in the home, or nothing where it is not there yet. */
function homeFile(box, ...parts) {
  const file = join(box.state, ...parts)
  return existsSync(file) ? readFileSync(file, 'utf8') : ''
}

export const daemonLog = (box) => homeFile(box, 'daemon.log')
export const appLog = (box) => homeFile(box, 'app', 'app.log')

/** The word the daemon's command line says it is: Node's runs `cf.mjs`, the native one is `cf`. */
export function kindOfCommand(command) {
  return /\/cf\.mjs ui --json --no-open$/.test(command) ? 'node' : 'native'
}

/** The processes under `bundle` (the root of one app) that are a daemon: `ui --json --no-open`. */
export function daemonRows(table, bundle) {
  return table.filter(
    (row) =>
      row.command.includes(`${bundle}/Contents/`) && /\bui --json --no-open$/.test(row.command),
  )
}

/** The app is running: the process of `pid` is this bundle's executable. */
export function assertApp(table, pid, binary) {
  const row = table.find((each) => each.pid === pid)
  assert.ok(row !== undefined && !row.state.startsWith('Z'), `the app (pid ${pid}) is not running`)
  assert.equal(row.command, binary, `pid ${pid} is not the app: ${row.command}`)
  return row
}

/** The pid of every start the daemon's log holds, in order. */
const startsOf = (log) =>
  [...log.matchAll(/^\S+ info start pid (\d+) /gm)].map((hit) => Number(hit[1]))

/**
 * The daemon of the home, as the machine shows it: the one process under the
 * app's own that runs this bundle's `cf ui --json --no-open`, and the only
 * daemon of the bundle that runs. Its log holds the start line it wrote, which
 * says which daemon it is (Node's or the native one), as its command line does.
 * And no daemon the app has started on this home was refused or failed: none of
 * their starts, a replaced app's included, is followed by an error or by `exit 1`.
 * What a probe started (`probes`, the pids of the second daemons the smoke
 * tried) says of its refusal is another's. `log` is daemon.log, `app` the app's
 * pid, `bundle` the app's root.
 */
export function daemonEvidence({ log, table, app, bundle, probes = [] }) {
  const rows = daemonRows(table, bundle)
  const mine = rows.filter((row) => row.ppid === app)
  assert.equal(
    mine.length,
    1,
    `the app (pid ${app}) has ${mine.length} daemons, and these run: ${rows.map((row) => `${row.pid} (parent ${row.ppid}) ${row.command}`).join('; ') || 'none'}`,
  )
  assert.equal(
    rows.length,
    1,
    `one daemon serves the home, and these run: ${rows.map((row) => row.command).join('; ')}`,
  )
  const [row] = mine
  const start = startLine(log, row.pid)
  assert.notEqual(start, null, `the daemon (pid ${row.pid}) logged no start line:\n${log}`)
  assert.equal(
    kindOfCommand(row.command),
    start.kind,
    `the log says ${start.runtime} and the process runs ${row.command}`,
  )
  for (const pid of startsOf(log).filter((each) => !probes.includes(each))) {
    const written = linesOf(log, pid)
    assert.deepEqual(errorLines(written), [], `a daemon logged errors:\n${written.join('\n')}`)
    assert.ok(
      !written.some((line) => / info exit 1$/.test(line)),
      `a daemon failed to start:\n${written.join('\n')}`,
    )
  }
  return { pid: row.pid, kind: start.kind, runtime: start.runtime, command: row.command }
}

/**
 * Once every app is gone: the ledger refused no daemon but the probes the smoke
 * tried. A daemon refused logs `exit 1` as its last line, so the lines of it in
 * the log are the refusals, and each probe is one: an app's daemon that was
 * refused (the old one not let go when the new one started, or two at once) is
 * one more than the probes.
 */
export function assertOnlyProbesRefused(log, probes) {
  const refused = log.split('\n').filter((line) => / info exit 1$/.test(line)).length
  assert.equal(
    refused,
    probes.size,
    `${refused} daemons were refused the ledger, and ${probes.size} were probes the smoke tried:\n${log}`,
  )
}

/** Waits for the daemon of `app` to be the one `daemonEvidence` takes, and returns it. */
export async function daemonOf(box, { app, bundle, probes }) {
  let last
  return until(`the daemon of pid ${app} is running and logged its start`, () => {
    try {
      return daemonEvidence({
        log: daemonLog(box),
        table: processTable(),
        app,
        bundle,
        probes: [...(probes ?? [])],
      })
    } catch (cause) {
      last = cause
      return null
    }
  }).catch((cause) => {
    throw new Error(`${cause.message}: ${last?.message ?? 'nothing was seen'}`)
  })
}

/**
 * What the app's error log says of the daemon it starts: the bundle's `cf`
 * (daemon_command.rs), by its path. Only the app that ships no Node says it; the
 * log is written on by every app of the home, so a line of an earlier one's
 * (the flip's, which said which daemon it chose and why) is not this one's.
 */
export function assertStartedDaemon(appLogText, cf) {
  const said = `starting the daemon: ${cf} ui --json --no-open`
  assert.ok(
    appLogText.split('\n').some((line) => line.endsWith(said)),
    `app.log does not say the app started the bundle's cf (${said}):\n${appLogText.slice(-2000)}`,
  )
}

/**
 * A second ConsensFlow on the home, started the way the daemon is (the bundle's
 * `cf ui --json --no-open`, its input closed) while the app's daemon runs: the
 * ledger refuses it, in its own words, with no handle line out. Exit 1 alone
 * could be a program the bundle lacks, so the words are the proof.
 */
export function assertLedgerHeld(attempt, db) {
  assert.equal(attempt.signal, null, `the second cf ui never ended: ${attempt.out}${attempt.err}`)
  assert.equal(
    attempt.code,
    1,
    `the second cf ui ended ${attempt.code}: ${attempt.out}${attempt.err}`,
  )
  const words = new RegExp(
    `another ConsensFlow has ${db.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} open`,
  )
  assert.ok(
    words.test(attempt.err),
    `the second cf ui was refused, but not for the ledger's lock: ${attempt.err}`,
  )
  assert.equal(attempt.out, '', `the second cf ui printed a handle line: ${attempt.out}`)
}

/** Runs the bundle's `cf ui --json --no-open` on the box's home, as a second ConsensFlow, and says how it ended. */
export function secondDaemon(bundle, box) {
  const env = { ...terminalEnv(box) }
  // The way back runs on the bundle's Node, which the bundle's cf finds beside itself or is told.
  if (bundle.node) env.CONSENSFLOW_NODE = join(bundle.app, 'Contents', 'MacOS', 'node')
  return new Promise((done) => {
    const child = spawn(bundle.cf, ['ui', '--json', '--no-open'], {
      cwd: box.probe,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let out = ''
    let err = ''
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    child.stdout.on('data', (chunk) => {
      out += chunk
    })
    child.stderr.on('data', (chunk) => {
      err += chunk
    })
    const timer = setTimeout(() => child.kill('SIGKILL'), 30_000)
    child.on('exit', (code, signal) => {
      clearTimeout(timer)
      done({ pid: child.pid, code, signal, out, err })
    })
  })
}

/**
 * The ledger is held by the app's daemon: a second one is refused. Said once the
 * daemon has answered the page, which is after it has the ledger: a probe
 * before that would be the one to take it. The probe's pid goes in `probes`.
 */
export async function ledgerHeld(bundle, box, probes) {
  const attempt = await secondDaemon(bundle, box)
  probes.add(attempt.pid)
  assertLedgerHeld(attempt, join(box.state, 'consensflow.db'))
}

/** Says a process is gone, which is what the ledger's lock needs of the daemon that held it. */
export const gone = (pid) => !alive(pid)
