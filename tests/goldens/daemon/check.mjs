/**
 * What every trace must be, whatever it recorded: the shape `FORMAT.md`
 * describes. The recorder checks each trace as it makes it, and the tests
 * check what is checked in, so a player can rely on these and not re-check
 * them: no step of a kind it does not know, no token used before it was
 * issued, no exchange left waiting for an end, nothing that varies from run
 * to run left in the text.
 */
import { validate } from './mask.mjs'

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
}
