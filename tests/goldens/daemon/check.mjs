/**
 * What every trace must be, whatever it recorded: the shape `FORMAT.md`
 * describes. The recorder checks each trace as it makes it, and the tests
 * check what is checked in, so a player can rely on these and not re-check
 * them: no step of a kind it does not know, no name of what varies where no
 * player puts it, no token used before it was issued, no exchange left waiting
 * for an end, a ledger that left a database ended by the `close` that compares
 * it, nothing that varies from run to run left in the text.
 */
import { validate } from './mask.mjs'

/**
 * Where each name of what varies stands in a trace, as the table of `FORMAT.md`
 * says: a player puts it in these fields and no others. A field is its keys
 * from the trace, an index as `#` and the name of a variable or of a file, in
 * an `env`, a `files` and a `wrote`, as `*`.
 */
const PLACES = {
  ledger: ['ledger.file'],
  root: ['steps.#.env.*', 'steps.#.response.body'],
  api: ['steps.#.env.*'],
  token: ['steps.#.env.*', 'steps.#.request.authorization', 'steps.#.request.target'],
  now: [
    'steps.#.files.*',
    'steps.#.files.*.text',
    'steps.#.wrote.*.before.text',
    'steps.#.wrote.*.after.text',
  ],
  frame: ['steps.#.files.*', 'steps.#.error.text'],
}
const MAPS = new Set(['env', 'files', 'wrote'])
const NAME = /«(ledger|root|api|token:T\d+|now|frame)»/g

/** The field `path` is, as `PLACES` writes one. */
const field = (path) =>
  path
    .map((key, at) => (typeof key === 'number' ? '#' : MAPS.has(path[at - 1]) ? '*' : key))
    .join('.')

/** Each name in `value` and the field it stands in; one in a key stands in no field. */
function* named(value, path = []) {
  if (typeof value === 'string') {
    for (const [, name] of value.matchAll(NAME)) yield [name.replace(/:T\d+$/, ''), field(path)]
  } else if (Array.isArray(value)) {
    for (const [at, item] of value.entries()) yield* named(item, [...path, at])
  } else if (value !== null && typeof value === 'object') {
    for (const [key, item] of Object.entries(value)) {
      for (const [, name] of key.matchAll(NAME))
        yield [name.replace(/:T\d+$/, ''), `${field(path)} (a key)`]
      yield* named(item, [...path, key])
    }
  }
}

const SURFACES = new Set(['api', 'cf', 'page', 'screens', 'trace', 'log'])
const KINDS = new Set([
  'ledger',
  'issue',
  'revoke',
  'exchange',
  'settle',
  'api.close',
  'run',
  'operation',
  'world',
  'kick',
  'trace.open',
  'trace.append',
  'trace.forget',
  'log.open',
  'log.write',
])

/** Throws, naming the trace, at the first thing wrong with it. */
export function check(name, trace) {
  const fail = (why) => {
    throw new Error(`${name}: ${why}`)
  }
  if (trace.format !== 1) fail(`format ${trace.format}, not 1`)
  if (!SURFACES.has(trace.surface)) fail(`surface ${trace.surface}`)
  if (!Array.isArray(trace.test?.path) || trace.test.path.length === 0) fail('no test path')
  if ('line' in trace.test) fail('a test line: it changes every trace after an edit to a suite')
  if (!Array.isArray(trace.steps) || trace.steps.length === 0) fail('no steps')
  try {
    validate(JSON.stringify(trace))
  } catch (cause) {
    fail(cause.message)
  }
  for (const [placeholder, where] of named(trace)) {
    if (!PLACES[placeholder].includes(where)) {
      fail(`«${placeholder}» is in ${where}, where no player puts it`)
    }
  }
  const issued = new Set()
  /** Intervals that others overlapped, until their `settle`: `exchange 3`, `run 1`, `close 1`. */
  const waiting = new Set()
  let worlds = 0
  for (const [at, step] of trace.steps.entries()) {
    const here = `step ${at} (${step.kind})`
    if (!KINDS.has(step.kind)) fail(`${here} is of a kind nobody knows`)
    for (const [, token] of JSON.stringify(step).matchAll(/«token:(T\d+)»/g)) {
      if (!issued.has(token)) fail(`${here} uses ${token}, which no step issued before it`)
    }
    if (step.kind === 'issue') {
      if (issued.has(step.token)) fail(`${here}: ${step.token} is issued twice`)
      issued.add(step.token)
    }
    if (step.kind === 'revoke' && !issued.has(step.token) && /^T\d+$/.test(step.token)) {
      fail(`${here} revokes ${step.token}, which was never issued`)
    }
    if (step.kind === 'exchange') {
      if (step.response === null && step.aborted !== true)
        fail(`${here} has no answer and is not marked aborted`)
      if (step.response?.page !== undefined && step.response.body !== undefined) {
        fail(`${here} holds a page twice: by name and by text`)
      }
      if (step.detached === true) waiting.add(`exchange ${step.id}`)
    }
    if (step.kind === 'operation') {
      // One queue of readings serves a player only if an operation's own and its stand-ins' do not mix.
      const through = step.seams.some((seam) =>
        seam.calls.some((call) => call.clock.length + call.names.length > 0),
      )
      if (through && step.clock.length + step.names.length > 0) {
        fail(
          `${here} reads the clock itself and through a stand-in: one queue could not tell them apart`,
        )
      }
    }
    if (step.kind === 'run' && step.detached === true) waiting.add(`run ${step.id}`)
    if (step.kind === 'api.close' && step.detached === true) waiting.add(`close ${step.id}`)
    if (step.kind === 'settle') {
      // An exchange of a run names the run too, as its owner; the exchange is what ends.
      const ended = [
        ['exchange', step.exchange],
        ['close', step.close],
        ['run', step.run],
      ].find(([, id]) => id !== undefined)
      const which = ended === undefined ? 'nothing' : `${ended[0]} ${ended[1]}`
      if (!waiting.delete(which)) fail(`${here} ends ${which}, which is not waiting`)
    }
    if (step.kind === 'world') {
      worlds += 1
      if (worlds === 1 && (step.env === undefined || step.files === undefined)) {
        fail(`${here}: the first world of a trace is whole`)
      }
    }
  }
  if (waiting.size > 0) fail(`${[...waiting].join(', ')} never settled`)
  if (trace.ledger !== null && trace.ledger?.unclosed !== true && trace.ledger?.final === null) {
    fail('its ledger has no database at the close')
  }
  if (trace.ledger && trace.ledger.final !== null) {
    // The database is compared where the trace closes the ledger: a player that has not
    // reached that step has compared none, so it is the last.
    const last = trace.steps.at(-1)
    if (last.kind !== 'ledger' || last.method !== 'close') {
      fail('its ledger left a database, and the trace does not end with the close that compares it')
    }
  }
}
