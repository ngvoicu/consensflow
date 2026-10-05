/**
 * The daemon's log, recorded: each line written, with the clock reading it
 * took and the error's text under it, and the files of the folder as they
 * stood after it. An error's stack is the machine's own, so the log is given a
 * stack of two frames named `«frame»` where the test's was, and a line the log
 * dated itself (the test gave no `now`) has the time named `«now»`.
 */
import { relative } from 'node:path'
import * as real from '../../../src/core/log.js'
import { added, folder, named } from './lines.mjs'
import * as session from './session.mjs'
import { treeOf } from './world.mjs'

export * from '../../../src/core/log.js'

const LIMIT = 5_000_000

/** What the log writes under a line for `error`: an `Error`'s stack, any other value as a string. */
function stackOf(error) {
  if (!(error instanceof Error)) return { given: error, text: String(error) }
  const stack = `${error.name}: ${error.message}\n    at «frame»\n    at «frame»`
  return {
    given: Object.assign(new Error(error.message), { name: error.name, stack }),
    text: stack,
  }
}

export function daemonLog(home, options = {}) {
  if (session.recording() === null) return real.daemonLog(home, options)
  const readings = []
  const clock =
    options.now === undefined
      ? undefined
      : () => {
          const at = options.now()
          readings.push(at.toISOString())
          return at
        }
  const log = real.daemonLog(home, { ...options, ...(clock === undefined ? {} : { now: clock }) })
  const id = session.next('trace')
  const root = treeOf(home)
  if (root !== null) session.addRoot(root)
  session.touch()
  session.push({
    kind: 'log.open',
    log: id,
    folder: root === null ? home : relative(root, home).split('\\').join('/') || '.',
    limit: options.limit ?? LIMIT,
    clock: options.now !== undefined,
  })
  const write = (level, message, error) => {
    const before = folder(home)
    const shown = error === undefined ? undefined : stackOf(error)
    log[level](...(shown === undefined ? [message] : [message, shown.given]))
    const now = folder(home)
    // A line the log dated with its own clock is a header, the time first; its stack lines are indented.
    if (clock === undefined) {
      for (const line of added(before, now)) {
        if (/^\S+ (info|warn|error) /.test(line)) session.dated().add(line)
      }
    }
    session.push({
      kind: 'log.write',
      log: id,
      level,
      message,
      ...(shown === undefined ? {} : { error: { text: shown.text } }),
      clock: readings.splice(0),
      files: named(now, session.dated()),
    })
  }
  return {
    file: log.file,
    info: (message) => write('info', message),
    warn: (message, error) => write('warn', message, error),
    error: (message, error) => write('error', message, error),
  }
}
