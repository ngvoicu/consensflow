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

/**
 * A window whose own question dialog is open waits for an answer there (the
 * chief's, from the human; a member's, from the board through its door):
 * nothing is pasted into it, and the board says it waits.
 */
export const dialogWaiting = (record) =>
  record?.asking === true ? { reason: 'its own question dialog is open' } : null

/**
 * A window that shows another conversation than its launch's (the human ran
 * /new, /clear or /resume in it): this reading is the old conversation's last
 * look, nothing in it settles, and the dispatcher follows the window to the
 * one it names. Until it has, a message waits rather than going in.
 */
export const switchedTo = (observed, nativeSession) => ({
  ...observed,
  settled: false,
  waiting: null,
  switched: { nativeSession },
})
export const SHOWS_ANOTHER = 'the window shows another conversation'

/** How the native channels answer a send, as an adapter delivery outcome. */
export function admission(sent, refusal, { queued = false } = {}) {
  if (sent?.ok === true) return queued ? { admitted: true, queued: true } : { admitted: true }
  if (sent?.admitted === null) return { admitted: null, reason: sent.cause ?? sent.error }
  return { admitted: false, reason: sent?.cause ?? sent?.error ?? refusal }
}
