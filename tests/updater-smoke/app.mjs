import { spawn } from 'node:child_process'
import { closeSync, constants, openSync, writeSync } from 'node:fs'
import { makeFifo } from './box.mjs'
import { alive, killTree, until } from './processes.mjs'

/**
 * The packaged app, started as the smoke starts it, and what it says: the
 * self-test's lines on its own stdout (`consensflow-selftest {…}`), which the
 * updater's page driver writes (app/ui/update-selftest.js).
 *
 * Node destroys a spawned child's managed stdin pipe when that PID exits, and
 * the updater restarts the app as another process. So the app's input is a
 * FIFO this side keeps open: the restarted app inherits its read end, and
 * closing the write end is the quit that both read as the end of input.
 */

/** What the page reports when something went wrong, which a case that wants one says so. */
const FAILURES = ['update-failure', 'page-error', 'page-rejection', 'failed', 'deadline']

/**
 * Starts the app at `binary` in `cwd` with `env`. `expected` names the failure
 * reports that the case is about: they are waited for like any report and fail
 * nothing else.
 */
export function launchApp(binary, env, cwd, { expected = [] } = {}) {
  const fifo = makeFifo(cwd)
  // Opened for reading and writing without blocking first, so that the open for
  // reading below has a writer to meet; then only the writer this side keeps.
  const hold = openSync(fifo, constants.O_RDWR | constants.O_NONBLOCK)
  const input = openSync(fifo, 'r')
  const control = openSync(fifo, 'w')
  closeSync(hold)
  let controlClosed = false
  const child = spawn(binary, [], {
    cwd,
    env,
    detached: true,
    stdio: [input, 'pipe', 'pipe'],
  })
  closeSync(input)
  const events = []
  const failures = []
  const stderr = []
  let buffer = ''
  const appPids = new Set([child.pid])
  child.stdout.setEncoding('utf8')
  child.stdout.on('data', (chunk) => {
    buffer += chunk
    let cut = buffer.indexOf('\n')
    while (cut !== -1) {
      const line = buffer.slice(0, cut)
      buffer = buffer.slice(cut + 1)
      if (line.startsWith('consensflow-selftest ')) {
        try {
          const event = JSON.parse(line.slice('consensflow-selftest '.length))
          events.push(event)
          process.stdout.write(`updater probe ${JSON.stringify(event)}\n`)
          if (Number.isInteger(event.pid) && event.pid > 0) appPids.add(event.pid)
          if (FAILURES.includes(event.event) && !expected.includes(event.event)) {
            failures.push(event)
          }
        } catch {
          failures.push({ event: 'malformed-report', data: { line } })
        }
      }
      cut = buffer.indexOf('\n')
    }
  })
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk) => stderr.push(chunk))
  const exited = new Promise((done) => {
    child.on('exit', (code, signal) => done({ code, signal }))
  })

  /**
   * The first report that `predicate` takes, or the failure the app reported
   * meanwhile. `unless` names a report that settles the wait the other way, and
   * says what it means: the case that waits for a refusal is not left to time
   * out when the app took the update.
   */
  async function waitFor(label, predicate, { unless = () => null } = {}) {
    return until(label, () => {
      const found = events.find(predicate)
      if (found !== undefined) return found
      for (const event of events) {
        const meaning = unless(event)
        if (meaning !== null) throw new Error(`${meaning}: ${JSON.stringify(event)}`)
      }
      if (failures.length > 0) {
        throw new Error(
          `packaged app reported failure: ${JSON.stringify(failures.at(-1))}\nstderr: ${stderr.join('').slice(-4000)}`,
        )
      }
      return null
    })
  }

  function closeControl() {
    if (controlClosed) return
    closeSync(control)
    controlClosed = true
  }

  return {
    child,
    events,
    stderr,
    appPids,
    exited,
    waitFor,
    /** Tells the page's blocked install to go on: the panes it was waiting for are closed. */
    continueUpdate() {
      writeSync(control, 'continue-updater\n')
    },
    /** The app's own quit: the end of its input, and its real exit. */
    closeInput: closeControl,
    /** Whether any process the app has been (the original and the restarted) still runs. */
    anyAlive: () => [...appPids].some(alive),
    /** Ends every process the app has been, its group and whatever is under it. */
    killRecorded() {
      closeControl()
      for (const pid of appPids) {
        try {
          process.kill(-pid, 'SIGKILL')
        } catch {
          // The process or its group already exited.
        }
        killTree(pid)
      }
    },
  }
}
