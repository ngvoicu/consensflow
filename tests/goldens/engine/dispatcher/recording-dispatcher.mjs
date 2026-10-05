/**
 * The dispatcher, recording: everything the engine is given (the ledger, the
 * pane host, the adapters, credentials, the pane environment, the roster, the
 * roles, its trace, its log, the launch files) is handed to it wrapped, and
 * each call it makes of them is written down in the order it made it, with
 * what it was given and what it answered, a promise's once it settles. The
 * human's operations and the passes a test drives are written down too, where
 * they begin and with what they answered or failed with, so a trace reads in
 * steps and holds what the engine told its callers. The clock is not: when
 * the engine reads it is no behaviour, what it does with the time is.
 */
import { Dispatcher as Engine } from '../../../../src/core/dispatcher.js'
import { encode, failure, known } from './encode.mjs'
import { record } from './trace.mjs'

export * from '../../../../src/core/dispatcher.js'

/**
 * The engine's operations a trace marks where the test calls them: the
 * human's, a pass. What the engine calls of its own (an exit it settles in
 * place, a resume inside the restart, every exit the host sends) is its
 * insides, which its effects say, and is not marked. Each is written down
 * with what it answered or threw, once it has.
 */
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

/** The engine made with every seam it is given recording. */
class Recording extends Engine {
  constructor(options) {
    super(seams(options))
  }
}

/**
 * The engine as the test makes it: the test holds a proxy that marks its
 * calls, and the engine holds itself, so its calls of its own operations go
 * unmarked. What the test sets on the proxy (a wrapper of an operation) is
 * the test's, kept apart where the engine never reads it; a method is bound
 * to the engine, whose private fields a proxy would not reach.
 */
export const Dispatcher = new Proxy(Recording, {
  construct(Engine, args) {
    const engine = new Engine(...args)
    const set = new Map()
    return new Proxy(engine, {
      get(target, property) {
        if (set.has(property)) return set.get(property)
        const value = Reflect.get(target, property, target)
        if (OPERATIONS.includes(property)) {
          return (...given) =>
            watched({ op: property, args: encode(given) }, () => value.apply(target, given))
        }
        return typeof value === 'function' ? value.bind(target) : value
      },
      set(_target, property, value) {
        set.set(property, value)
        return true
      },
    })
  },
})

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
    // What the engine is not given it makes do without, as `dispatcher.js`
    // does: each is written down all the same, so every trace holds them.
    log: object('log', options.log ?? { error: () => {} }),
    launchFiles: object('launchFiles', options.launchFiles ?? { forget: () => {} }),
    ...Object.fromEntries(
      Object.entries(DEFAULTS).map(([name, made]) => [name, fn(name, options[name] ?? made)]),
    ),
  }
}

/** The seams that are functions, and what the engine does without each (`dispatcher.js`). */
const DEFAULTS = {
  paneEnv: () => ({}),
  roster: () => null,
  roles: () => undefined,
  trace: () => {},
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

/** `fn`, each call of it written down as `seam`'s `method`, its answer once there is one (`watched`). */
function called(seam, method, fn, self = undefined) {
  return (...args) =>
    watched({ seam, ...(method === null ? {} : { method }), args: encode(args) }, () =>
      fn.apply(self, args),
    )
}

/**
 * `event` written down where `call` is made, and its answer, or what it
 * threw, once there is one. A promise is watched from the side and handed on
 * itself: a promise of the recorder's own in its place would settle a turn
 * later, and the engine's work would interleave as it never does unrecorded.
 */
function watched(event, call) {
  record(event)
  let answer
  try {
    answer = call()
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
