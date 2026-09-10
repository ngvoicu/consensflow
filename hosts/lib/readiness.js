/**
 * Lead readiness — pure, no I/O, no env, no clock.
 *
 * A delivery is written into the lead's pane only when the lead is provably
 * ready. "Provably" is the whole point: the first version of this module read
 * the last transcript item's `settled` boolean, and hyperion disproved that
 * derivation — a queued user item or a completed tool result satisfies the
 * same boolean as a finished turn. A bare boolean authorises nothing here.
 *
 * `completion.answers()` supplies the aggregate proof instead, beside
 * `items` / `inFlight` / `cancelled` / `replaced` / `version`:
 *
 *   settlement = {
 *     state: 'settled' | 'in-flight' | 'unknown',
 *     provenance: 'native' | 'derived',
 *     cursor: <native total-order position of the boundary>,
 *     boundary: <the native record that proves it: task_complete,
 *                turn.ended, the post-turn hook>,
 *     evidence: { complete, openTools, queuedTurns, hooksInFlight },
 *   }
 *
 * `ready` needs ALL of:
 *
 *   - `settlement.state === 'settled'`, naming a real post-turn boundary at a
 *     usable native cursor;
 *   - provenance `native`, or `derived` with `complete` true — completion may
 *     only derive under that condition, so a derived proof without it is a
 *     malformed proof, not a slow turn;
 *   - for Pi AUTOMATIC delivery, provenance must be `native`: its quiet-window
 *     result is a derived fallback and is not sufficient for automatic work;
 *     MANUAL delivery may proceed on that derived proof;
 *   - the boundary on the CURRENT frontier: no open tool, no queued turn, no
 *     hook in flight. This binds a native marker too — `task_complete`
 *     followed by a queued turn is an earlier turn's marker, not this one's;
 *   - the pane's draft latch reported and EXPLICITLY clear, or admission
 *     delegated to the native Pi editor (checked again at send), or a proven
 *     native queue that never writes into the terminal composer;
 *   - a valid decision epoch, so Rust can refuse the write if human bytes
 *     arrived since;
 *   - a non-empty transcript — empty history is no proof of anything;
 *   - when the caller names `sinceCursor` (the settlement cursor at the last
 *     automated submission to this pane), a settlement strictly past it.
 *     Re-reading the same snapshot is not freshness.
 *
 * Two failure directions, kept apart because the page treats them
 * differently: **demonstrated ongoing work is `busy`** — wait, it resolves;
 * **missing, legacy or malformed proof is `unknown`** — automatic delivery
 * suspends and the page says so. Anything unrecognised falls to `unknown`,
 * never to `ready`.
 *
 * The decision returns `{state, epoch, cursor, provenance, reason}`. It
 * carries the native cursor and the provenance on EVERY state so the watcher
 * (TEST-PANE-31) can require a newer qualified boundary after each automated
 * submission — the input epoch guards against human typing and cannot do
 * that job.
 *
 * PTY silence is a polling hint, never a condition (Decision Log,
 * 2026-09-06). `paneIdleMs` and `humanDraftMs` are accepted and deliberately
 * never read: callers already hold them, and a signature that takes them
 * makes "silence is not a condition" and "a latch is never cleared by time"
 * things the tests can prove rather than things a future caller must
 * remember.
 *
 * A latched draft outranks a busy lead. Both block, but `busy` clears itself
 * when the turn settles and the draft is still there afterwards, so reporting
 * `lead busy` would show the page a reason that goes stale while the block
 * persists. `draft open` is the one only the human can clear, and the one
 * **Deliver now** may never bypass.
 */

import { settledAfter } from './completion.js'

const NATIVE = 'native'
const DERIVED = 'derived'

/**
 * What the page shows while a delivery waits (TEST-PANE-39). Every `busy`
 * reason begins with `lead busy` and every unreadable one says why, so the
 * page can match the prefix and still show the detail.
 */
const BUSY_REASON = 'lead busy'
const DRAFT_REASON = 'draft open'

/** The three evidence lists that must all be empty for a boundary to be the frontier. */
const WORK_LISTS = [
  ['openTools', 'lead busy: a tool is still open'],
  ['queuedTurns', 'lead busy: a turn is queued behind the boundary'],
  ['hooksInFlight', 'lead busy: a hook is still in flight'],
]

/**
 * Is the lead provably ready to receive a delivery right now?
 *
 * @param {object} input
 * @param {object} input.answers — a `completion.answers()` result.
 * @param {string} [input.kind] — the harness adapter that minted both
 *   cursors; required only when `sinceCursor` is supplied.
 * @param {'automatic'|'manual'} [input.purpose='automatic'] — why this
 *   caller is asking to deliver. Only an explicit `manual` purpose bypasses
 *   Pi's derived-settlement gate.
 * @param {boolean} input.draftLatched — the pane's draft latch, from Rust.
 *   Must be reported: a missing latch is missing evidence, not a clear one.
 * @param {'terminal'|'pi-native-editor'|'native-queue'} [input.composerAuthority='terminal']
 *   Native delivery preserves Rust's latch and cannot authorize a PTY paste.
 * @param {number} input.epoch — the pane's input epoch, a non-negative
 *   integer; echoed back so a stale write can be refused.
 * @param {number} [input.sinceCursor] — the settlement cursor at the last
 *   automated submission to this pane; omit for the first delivery.
 * @param {number} [input.paneIdleMs] — accepted, never read.
 * @param {number} [input.humanDraftMs] — accepted, never read.
 * @returns {{state: 'ready'|'busy'|'draft'|'unknown', epoch: number|null,
 *   cursor: number|null, provenance: 'native'|'derived'|null, reason: string}}
 */
export function leadReady({
  answers,
  kind,
  purpose = 'automatic',
  draftLatched,
  composerAuthority = 'terminal',
  epoch,
  sinceCursor,
} = {}) {
  const carry = {
    epoch: isPosition(epoch) ? epoch : null,
    ...carriedProof(answers),
  }
  const unknown = (reason) => ({ state: 'unknown', ...carry, reason })
  const busy = (reason) => ({ state: 'busy', ...carry, reason })

  const unreadable = unreadableReason(answers)
  if (unreadable !== null) return unknown(unreadable)

  const nativeEditor = kind === 'pi' && composerAuthority === 'pi-native-editor'
  const nativeQueue =
    ['claude-code', 'codex', 'opencode'].includes(kind) && composerAuthority === 'native-queue'
  if (composerAuthority !== 'terminal' && !nativeEditor && !nativeQueue) {
    return unknown('unknown: no native composer authority for this harness')
  }
  if (!nativeEditor && !nativeQueue && draftLatched === true)
    return { state: 'draft', ...carry, reason: DRAFT_REASON }
  if (typeof draftLatched !== 'boolean') {
    return unknown(
      "unknown: the pane's draft latch was not reported — a clear latch must be explicit",
    )
  }

  if (carry.epoch === null) {
    return unknown('unknown: no valid input epoch was supplied for this decision')
  }

  // Demonstrated work first: it is the one thing that resolves on its own.
  const working = ongoingWork(answers)
  if (working !== null) return busy(working)

  const settlement = answers.settlement
  const unqualified = disqualifyingReason(settlement, kind, purpose)
  if (unqualified !== null) return unknown(unqualified)

  if (sinceCursor !== undefined && sinceCursor !== null) {
    const freshness = settledAfter(kind, settlement, sinceCursor)
    if (freshness === null) {
      return unknown('unknown: the settlement cursors are not recognised by this adapter')
    }
    if (!freshness) {
      return busy('lead busy: no settlement newer than the last delivery')
    }
  }

  return {
    state: 'ready',
    ...carry,
    reason: `ready: the turn settled at ${settlement.boundary} (${settlement.provenance}, cursor ${settlement.cursor}) with no work in flight and ${nativeEditor ? 'editor admission checked by native Pi' : nativeQueue ? 'native delivery preserves the composer' : 'no draft latched'}`,
  }
}

/**
 * The cursor and provenance to report, whatever the decision turns out to be
 * — `null` where the settlement does not name a usable one. The watcher needs
 * these on a refusal as much as on a `ready`.
 */
function carriedProof(answers) {
  const settlement = answers?.settlement
  const provenance = settlement?.provenance
  return {
    cursor: isPosition(settlement?.cursor) ? settlement.cursor : null,
    provenance: provenance === NATIVE || provenance === DERIVED ? provenance : null,
  }
}

/**
 * Why this completion result cannot be judged at all — or `null` when it can.
 *
 * A replaced session is unknown, never busy and certainly never ready:
 * `/new`, `/resume` or a fork means the transcript in front of us is not the
 * session we bound, so nothing in it authorises a write.
 */
function unreadableReason(answers) {
  if (answers === null || typeof answers !== 'object') {
    return 'unknown: no completion result for this session'
  }
  if (answers.unknown === true) {
    return textOr(answers.reason, 'unknown: the completion model could not read this session')
  }
  if (answers.replaced === true) {
    return textOr(
      answers.reason,
      "replaced: the pane's native session was replaced — the binding and every decision for it are void",
    )
  }
  if (!Array.isArray(answers.items)) {
    return 'unknown: the completion result carries no items'
  }
  if (answers.items.length === 0) {
    return 'unknown: an empty transcript carries no settlement proof'
  }
  return null
}

/**
 * Work the harness is demonstrably still doing — the reason to wait, or
 * `null`. A non-empty evidence list means the boundary is behind the
 * frontier whatever its provenance; a list that is not a list is not work,
 * it is a malformed proof, and `disqualifyingReason` refuses it.
 */
function ongoingWork(answers) {
  const evidence = answers.settlement?.evidence
  for (const [field, reason] of WORK_LISTS) {
    if (Array.isArray(evidence?.[field]) && evidence[field].length > 0) return reason
  }
  if (answers.settlement?.state === 'in-flight') return BUSY_REASON
  if (answers.inFlight === true) return BUSY_REASON
  return null
}

/** Why this settlement is not a proof we may act on — or `null` when it is. */
function disqualifyingReason(settlement, kind, purpose) {
  if (settlement === null || typeof settlement !== 'object') {
    return 'unknown: no settlement proof for the current turn'
  }
  if (settlement.state !== 'settled') {
    return 'unknown: the completion model did not settle the current turn'
  }
  const provenance = settlement.provenance
  if (provenance !== NATIVE && provenance !== DERIVED) {
    return 'unknown: the settlement names no provenance this module accepts'
  }
  if (kind === 'pi' && provenance === DERIVED && purpose !== 'manual') {
    return 'unknown: Pi derived settlement requires native settlement evidence'
  }
  if (!isPosition(settlement.cursor)) {
    return 'unknown: the settlement carries no native cursor'
  }
  if (typeof settlement.boundary !== 'string' || settlement.boundary.length === 0) {
    return 'unknown: the settlement names no observed post-turn boundary'
  }
  const evidence = settlement.evidence
  if (evidence === null || typeof evidence !== 'object') {
    return 'unknown: the settlement carries no evidence'
  }
  for (const [field] of WORK_LISTS) {
    if (!Array.isArray(evidence[field])) {
      return `unknown: the settlement's ${field} is not a list`
    }
  }
  // Completion may derive only from a complete reply; without that flag the
  // derivation is the weak one, not a slow turn.
  if (provenance === DERIVED && evidence.complete !== true) {
    return 'unknown: a derived settlement needs a complete reply'
  }
  return null
}

/** A native total-order position, or an input epoch: a non-negative integer. */
function isPosition(value) {
  return Number.isInteger(value) && value >= 0
}

function textOr(value, fallback) {
  return typeof value === 'string' && value.trim().length > 0 ? value : fallback
}
