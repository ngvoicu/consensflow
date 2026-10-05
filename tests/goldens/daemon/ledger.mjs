/**
 * The ledger, as the daemon recorder hands it to a test: the recording ledger
 * of step 3.1 (`../ledger/recording.mjs`), watched. Each call made on it is
 * the test's own, to be replayed, or the work of a surface answering (an
 * exchange, an operation): that one is folded into what it drew and logged,
 * since the Rust surface makes its own calls and needs only the readings and
 * names to answer with. A call a stand-in of the test makes (a dispatcher's)
 * is kept with the stand-in's.
 */
import { sessionName } from '../../../src/ledger/names.js'
import * as recording from '../ledger/recording.mjs'
import * as session from './session.mjs'

export * from '../ledger/recording.mjs'

/** Every ledger a run opens starts at the same instant and reads a second later each time. */
const START = Date.UTC(2026, 0, 1)
const steadyClock = () => {
  let readings = 0
  return () => new Date(START + 1000 * readings++)
}

/** The names the ledger draws for sessions, from one fixed sequence per ledger. */
function steadyNames() {
  let state = 0x9e3779b9
  const random = () => {
    state = (state + 0x6d2b79f5) | 0
    let mixed = Math.imul(state ^ (state >>> 15), 1 | state)
    mixed = (mixed + Math.imul(mixed ^ (mixed >>> 7), 61 | mixed)) ^ mixed
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296
  }
  return () => sessionName(random)
}

/**
 * The ledger's methods that only read. A test's call to one that drew no
 * reading and logged nothing changes nothing a player must repeat, and a test
 * that waits for a window's question (polling `inbox` as it comes) makes as
 * many as the machine is slow: those are left out. A method is listed here by
 * hand, never by what one call did: `deleteProject` logs nothing either.
 */
const READS = new Set([
  'activeTask',
  'answerTo',
  'board',
  'candidates',
  'chiefHistory',
  'currentConversation',
  'holdsWork',
  'inbox',
  'inFlight',
  'lastStaff',
  'lastTask',
  'members',
  'message',
  'pending',
  'pausedTask',
  'project',
  'projects',
  'task',
  'transcript',
])

/** A ledger opened by a test; one with no clock or names of its own is given steady ones. */
export function openLedger(file, options = {}) {
  const given = {
    now: options.now !== undefined,
    names: options.names !== undefined,
    trace: options.trace !== undefined,
  }
  const ledger = recording.openLedger(file, {
    ...options,
    now: options.now ?? steadyClock(),
    names: options.names ?? steadyNames(),
  })
  const record = recording.recordings.get(ledger)
  session.ledgerOpened(record, given)
  const watched = new Proxy(ledger, {
    get(target, property, receiver) {
      const method = Reflect.get(target, property, receiver)
      if (typeof method !== 'function' || typeof property !== 'string') return method
      return (...args) => {
        const before = record.calls.length
        try {
          return method.apply(target, args)
        } finally {
          const call = record.calls[before]
          if (call !== undefined && record.calls.length > before) saw(call)
        }
      }
    },
  })
  session.recorded(watched)
  return watched
}

const idle = (call) => call.clock.length + call.names.length + call.events.length === 0

/** A call just made on the ledger, folded into whoever made it. */
function saw(call) {
  if (session.recording() === null) return
  const frame = session.frames.getStore()
  if (frame === undefined) {
    if (READS.has(call.method) && idle(call)) return
    session.push({ kind: 'ledger', ...call })
  } else if (frame.kind === 'seam') {
    frame.target.calls.push({ kind: 'ledger', ...call })
  } else {
    frame.target.clock.push(...call.clock)
    frame.target.names.push(...call.names)
    frame.target.events.push(...call.events)
  }
}
