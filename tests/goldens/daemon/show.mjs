/**
 * A trace, a line a step, for whoever reads one to write a player: the kind of
 * each step and what it is about, the exchanges with their answers.
 *
 *   node tests/goldens/daemon/show.mjs core-api-001
 *
 * takes a name under crates/cf-daemon/tests/goldens/ or the path of a trace.
 */
import { existsSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { gunzipSync } from 'node:zlib'

const GOLDENS = fileURLToPath(new URL('../../../crates/cf-daemon/tests/goldens/', import.meta.url))

/** What one step is about, in a line. */
function line(step) {
  switch (step.kind) {
    case 'ledger':
      return `ledger ${step.method}${step.result?.$error ? ` (refused: ${step.result.$error.message})` : ''}`
    case 'issue':
      return `issue ${step.token} for @${step.participant.handle}`
    case 'revoke':
      return `revoke ${step.token}`
    case 'exchange': {
      const by = step.client === 'cf' ? ` by run ${step.run ?? '?'}` : ''
      const answer = step.response === null ? 'no answer' : step.response.status
      const marks = [
        step.detached && 'detached',
        step.screens && 'screens',
        step.wrote && `wrote ${Object.keys(step.wrote).join(', ')}`,
        step.kicks > 0 && `${step.kicks} kick${step.kicks === 1 ? '' : 's'}`,
        step.seams.length > 0 &&
          `${step.seams.length} stand-in call${step.seams.length === 1 ? '' : 's'}`,
      ].filter(Boolean)
      return `exchange ${step.id}${by}: ${step.request.method} ${step.request.target} as ${step.request.authorization ?? 'nobody'} → ${answer}${marks.length > 0 ? ` (${marks.join(', ')})` : ''}`
    }
    case 'settle':
      return `settle ${Object.entries(step)
        .filter(([key]) => key !== 'kind')
        .map(([key, id]) => `${key} ${id}`)
        .join(' of ')}`
    case 'api.close':
      return `api.close ${step.id}${step.detached ? ' (detached)' : ''}`
    case 'run':
      return `run ${step.id}: cf ${step.argv.join(' ').slice(0, 60)} → ${step.code ?? step.signal}${step.detached ? ' (detached)' : ''}`
    case 'operation':
      return `operation ${step.id}: ${step.name}${step.refusal ? ` refused: ${step.refusal.message}` : ''}${step.kicks > 0 ? ` (${step.kicks} kick)` : ''}${step.seams.length > 0 ? ` (${step.seams.map((seam) => `${seam.seam}.${seam.method}`).join(', ')})` : ''}`
    case 'world':
      return `world: ${[...Object.keys(step.env ?? {}), ...Object.keys(step.files ?? {})].join(', ')}`
    default:
      return `${step.kind}${step.entry ? ` ${JSON.stringify(step.entry).slice(0, 60)}` : ''}${step.message ? ` ${step.level}: ${step.message}` : ''}`
  }
}

/** A trace as lines: its test, then each step with its number. */
export function show(trace) {
  return [
    `${trace.surface}: ${trace.test.path.join(' › ')} (${trace.test.file})`,
    ...trace.steps.map((step, at) => `${String(at).padStart(3)}  ${line(step)}`),
  ]
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const given = process.argv[2]
  if (given === undefined) throw new Error('which trace? a name such as core-api-001, or a path')
  const named = join(GOLDENS, `${given}.json.gz`)
  const path = existsSync(named) ? named : resolve(given)
  const bytes = readFileSync(path)
  const trace = JSON.parse(path.endsWith('.gz') ? gunzipSync(bytes) : bytes)
  process.stdout.write(`${show(trace).join('\n')}\n`)
}
