/**
 * The dispatcher, recording: everything the engine is given (the ledger, the
 * pane host, the adapters, credentials, the pane environment, the roster, the
 * roles, its trace, its log, the launch files) is handed to it wrapped, and
 * each call it makes of them is written down in the order it made it, with
 * what it was given and what it answered, a promise's once it settles. The
 * human's operations and the passes a test drives are written down too, where
 * they begin, so a trace reads in steps. The clock is not: when the engine
 * reads it is no behaviour, what it does with the time is.
 */
import { Dispatcher as Engine } from '../../../../src/core/dispatcher.js'
import { encode, failure, known } from './encode.mjs'
import { record } from './trace.mjs'

export * from '../../../../src/core/dispatcher.js'

/** The engine's operations a trace marks where they begin: the human's, a pass, an exit. */
const OPERATIONS = [
  'openProject',
  'resumeProject',
  'closeProject',
  'openWindow',
  'hideWindow',
  'backFromQuota',
  'reassignTask',
  'endSession',
  'deleteProject',
  'resumeAfterRestart',
  'removeMember',
  'pass',
  'paneExited',
  'switchChief',
]

/** A host's methods that only set it up, which no trace holds. */
const SETUP = new Set(['onExit'])

export class Dispatcher extends Engine {
  constructor(options) {
    super(seams(options))
    for (const name of OPERATIONS) {
      const operation = Engine.prototype[name]
      this[name] = (...args) => {
        record({ op: name, args: encode(args) })
        return operation.apply(this, args)
      }
    }
  }
}

/** The options the engine is made with, each seam in them recording. */
function seams(options) {
  const adapters = Object.fromEntries(
    Object.entries(options.adapters ?? {}).map(([harness, adapter]) => [
      harness,
      object(`adapter:${harness}`, adapter),
    ]),
  )
  return {
    ...options,
    ledger: object('ledger', options.ledger),
    host: object('host', options.host),
    adapters,
    credentials: object('credentials', options.credentials),
    log:
      options.log === null || options.log === undefined ? options.log : object('log', options.log),
    launchFiles: object('launchFiles', options.launchFiles ?? { forget: () => {} }),
    ...Object.fromEntries(
      ['paneEnv', 'roster', 'roles', 'trace']
        .filter((name) => options[name] !== undefined)
        .map((name) => [name, fn(name, options[name])]),
    ),
  }
}

/** A seam that is a function, each call of it written down, and of what it has besides (the trace's `forget`). */
function fn(seam, target) {
  const wrapped = called(seam, null, target)
  for (const [property, value] of Object.entries(target)) {
    wrapped[property] = typeof value === 'function' ? called(seam, property, value, target) : value
  }
  known.set(wrapped, `$${seam}`)
  known.set(target, `$${seam}`)
  return wrapped
}

/** `target` with each method call it takes written down as `seam`'s. */
function object(seam, target) {
  if (target === null || target === undefined) return target
  const wrapped = new Proxy(target, {
    get(of, property, receiver) {
      const value = Reflect.get(of, property, receiver)
      if (typeof value !== 'function' || typeof property !== 'string' || SETUP.has(property)) {
        return value
      }
      return called(seam, property, value, of)
    },
  })
  known.set(wrapped, `$${seam}`)
  known.set(target, `$${seam}`)
  return wrapped
}

/**
 * `fn`, each call of it written down as `seam`'s `method`, its answer once
 * there is one. A promise is watched from the side and handed on itself: a
 * promise of the recorder's own in its place would settle a turn later, and
 * the engine's work would interleave as it never does unrecorded.
 */
function called(seam, method, fn, self = undefined) {
  return (...args) => {
    const event = { seam, ...(method === null ? {} : { method }), args: encode(args) }
    record(event)
    let answer
    try {
      answer = fn.apply(self, args)
    } catch (cause) {
      event.threw = failure(cause)
      throw cause
    }
    if (answer instanceof Promise) {
      event.pending = true
      answer.then(
        (value) => {
          delete event.pending
          event.answer = encode(value)
        },
        (cause) => {
          delete event.pending
          event.threw = failure(cause)
        },
      )
      return answer
    }
    event.answer = encode(answer)
    return answer
  }
}
