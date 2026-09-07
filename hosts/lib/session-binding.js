import { parseLaunchNonce, withoutInjectedBlocks } from './packets.js'

/**
 * Native-session binding by launch evidence — pure, no I/O, no env.
 *
 * A native session (the harness's own) binds to a lead or a worker only with
 * launch-unique evidence: an id we preallocated (`claude --session-id`, pi's
 * name), an id the harness reported on our stream, or the non-secret launch
 * nonce as the first non-empty line (after injected blocks) of one of the
 * session's first five user turns. Task text alone never binds, and neither
 * does a marker quoted thirty lines down or inside a fenced block — the
 * marker sits verbatim in a file every agent in that directory can read.
 *
 * `src/store.js` `session.bind` MUST call `bindEvidence(kind, candidate,
 * launch)` against the launch record it already holds — never trust a bare
 * `evidence` string, which anyone can send. `acceptsBinding` below is only
 * the vocabulary check for that string, not proof. Every decision carries
 * the `generation` the launch record names (null when it names none), so
 * Phase 3 can invalidate a binding by generation.
 *
 * A `reportedId` is trusted only when it comes from a structured harness
 * line on our own stream (`exec --json` thread id, `session.resume_hint`,
 * …) — never free PTY or model text, because the model can print any
 * session id it has read.
 */

const PREALLOCATED_KINDS = new Set(['claude-code', 'pi'])

/**
 * User turns examined for the nonce, earliest first — the seed opens.
 * Exported so discovery (which does the scanning) and this module (which
 * checks the one turn discovery returns) share a single cap.
 */
export const TURNS_EXAMINED = 5

/**
 * Decide whether a candidate native session is this launch's.
 *
 * `candidate` is what discovery observed: `{ sessionId, turn?,
 * currentSessionId? }` — where `turn` is the matching turn discovery
 * returns, the one whose opening line carries the nonce. `launch` is what we
 * started: `{ nonce?, preallocatedId?, reportedId?, generation? }`.
 *
 * Returns `{ bound: true, evidence, generation }`, `{ bound: false, reason,
 * generation }`, or `{ bound: false, replaced: true, reason, generation }`
 * when the live pane shows a different native session than the one bound
 * (`/new`, `/resume`, a fork) — the binding and every dependent decision die
 * with it, whatever else the candidate carries.
 */
export function bindEvidence(kind, candidate = {}, launch = {}) {
  const generation = launch?.generation ?? null
  const sessionId = candidate?.sessionId ?? null
  const current = candidate?.currentSessionId ?? null
  if (
    sessionId !== null &&
    sessionId !== undefined &&
    String(sessionId).length > 0 &&
    current !== null &&
    current !== undefined &&
    String(current).length > 0 &&
    current !== sessionId
  ) {
    return {
      bound: false,
      replaced: true,
      reason: 'replaced: the live pane now shows a different native session',
      generation,
    }
  }

  const nonce = launch?.nonce ?? null
  const preallocatedId = launch?.preallocatedId ?? null
  const reportedId = launch?.reportedId ?? null

  if (sessionId === null || sessionId === undefined || String(sessionId).length === 0) {
    return { bound: false, reason: 'unbound: no candidate session', generation }
  }
  if (PREALLOCATED_KINDS.has(kind) && preallocatedId !== null && sessionId === preallocatedId) {
    // A reported id equal to the minted one is the same launch seen twice:
    // accepted as reported (the file check lives in discovery, which tries
    // the preallocated file first). A differing reported id never reaches
    // here — the mismatch below refuses it.
    if (reportedId === preallocatedId) {
      return { bound: true, evidence: 'reported', generation }
    }
    return { bound: true, evidence: 'preallocated', generation }
  }
  if (PREALLOCATED_KINDS.has(kind) && preallocatedId !== null) {
    return {
      bound: false,
      reason: 'unbound: the candidate is not the preallocated session',
      generation,
    }
  }
  if (
    reportedId !== null &&
    reportedId !== undefined &&
    String(reportedId).length > 0 &&
    sessionId === reportedId
  ) {
    return { bound: true, evidence: 'reported', generation }
  }
  if (nonce !== null && nonce !== undefined && String(nonce).trim().length > 0) {
    if (openingLineCarriesNonce(candidate?.turn, nonce)) {
      return { bound: true, evidence: 'nonce', generation }
    }
    return {
      bound: false,
      reason: 'unbound: the matching turn carries no launch marker for this launch',
      generation,
    }
  }
  return {
    bound: false,
    reason: 'unbound: no launch evidence (preallocated id, reported id, or nonce)',
    generation,
  }
}

/**
 * Vocabulary check for an `evidence` string — NOT proof. The store binds on
 * `bindEvidence`, never on this.
 */
export function acceptsBinding(evidence) {
  return evidence === 'preallocated' || evidence === 'reported' || evidence === 'nonce'
}

/**
 * Does this user turn open with our launch nonce? The marker rides on the
 * seed's first line, so after the harness's injected blocks are stripped the
 * turn's first non-empty line must BE the marker — a whole line, so task
 * text that merely mentions a nonce can never bind.
 */
export function openingLineCarriesNonce(text, nonce) {
  const wanted = String(nonce ?? '').trim()
  if (wanted.length === 0 || text === null || text === undefined) return false
  const first =
    withoutInjectedBlocks(String(text))
      .split('\n')
      .map((line) => line.trim())
      .find((line) => line.length > 0) ?? ''
  return first.length > 0 && parseLaunchNonce(first) === wanted
}
