import { harnessPath } from '../harnesses.js'
import { harnessForKind } from '../roster.js'

/** The absolute path of a harness's CLI here, or a refusal that names it. */
export function executableFor(kind, env) {
  const harness = harnessForKind(kind) ?? kind
  const path = harnessPath(harness, env)
  if (path === null) throw new Error(`${harness} is not installed on this machine`)
  return path
}

/**
 * What a harness record says, in the dispatcher's terms. A conversation with
 * no messages and nothing in flight is idle: a window opened without a task
 * has nothing to finish, and must still be able to receive.
 */
export function recordState(record) {
  const items = Array.isArray(record?.items) ? record.items : []
  const state = record?.settlement?.state
  const empty = items.length === 0 && record?.inFlight !== true
  return {
    items,
    settled: state === 'settled' || (state !== 'in-flight' && empty),
    failed: record?.failed === true,
    quota: record?.quota ?? null,
  }
}

/** How the native channels answer a send, as an adapter delivery outcome. */
export function admission(sent, refusal) {
  if (sent?.ok === true) return { admitted: true }
  if (sent?.admitted === null) return { admitted: null, reason: sent.cause ?? sent.error }
  return { admitted: false, reason: sent?.cause ?? sent?.error ?? refusal }
}
