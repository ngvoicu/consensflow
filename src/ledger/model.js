/**
 * The words the ledger's records hold: harnesses, roles, tiers, pools,
 * purposes and the task states it counts by, the limits on text, and the
 * checks that refuse anything else with a `LedgerError`, whose code is stable.
 */

export const HARNESSES = ['claude-code', 'codex', 'opencode', 'pi', 'devin', 'image']
/** Where a project's chief runs: a harness with a terminal the human works in. */
export const CHIEF_HARNESSES = ['claude-code', 'codex', 'opencode', 'pi', 'devin']
export const MEMBER_ROLES = ['worker', 'advisor', 'reviewer', 'designer']
/** Who hands out work and hears when the staff changes: the human and the chief. */
const COORDINATOR_HANDLES = ['human', 'chief']
export const COORDINATOR_ROLES = ['human', 'chief']
export const TIERS = ['critical', 'complex', 'standard', 'light']
/** Who takes a task on the board: a worker, an advisor (advice), a reviewer, or an image designer (no tier). */
export const POOLS = ['worker', 'advisor', 'reviewer', 'designer']
export const PURPOSES = ['critical-review', 'architecture', 'hard-problem', 'important-question']
export const ACTIVE_TASK_STATES = ['working', 'waiting']
/** A task on a member's hands: from assignment until its result. */
export const HELD_TASK_STATES = ['queued', 'working', 'waiting']
/** A task that is over, accepted or not: the board's last column, and what the human may delete. */
export const FINISHED_TASK_STATES = ['accepted', 'cancelled', 'failed']
export const MAX_BODY = 1_000_000
export const MAX_TITLE = 120
const AGENT_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/

export class LedgerError extends Error {
  constructor(code, message, status = 400) {
    super(message)
    this.name = 'LedgerError'
    this.code = code
    this.status = status
  }
}

export function requireText(value, field, max) {
  if (typeof value !== 'string' || value.trim().length === 0 || value.length > max) {
    throw new LedgerError(
      'invalid-text',
      `${field} must be text, not empty, at most ${max} characters`,
    )
  }
  return value
}

export function requireHarness(harness) {
  if (!HARNESSES.includes(harness)) {
    throw new LedgerError('invalid-harness', `unknown harness ${JSON.stringify(harness)}`)
  }
  return harness
}

/** A chief's harness: one of CHIEF_HARNESSES, when the project starts and at every Switch lead. */
export function requireChiefHarness(harness) {
  if (!CHIEF_HARNESSES.includes(harness)) {
    throw new LedgerError(
      'invalid-harness',
      `a chief runs on ${CHIEF_HARNESSES.join(', ')}, not ${JSON.stringify(harness)}`,
    )
  }
  return harness
}

/** A participant that is still in the project; a member who left is refused. */
export function requireActive(row) {
  if (row.left_at !== null) {
    throw new LedgerError('member-left', `@${row.handle} left the staff`, 409)
  }
  return row
}

/** Task numbers a task needs or comes before: a list of distinct positive integers. */
export const requireNumbers = (value, field) => {
  if (!Array.isArray(value) || value.some((number) => !Number.isInteger(number) || number <= 0)) {
    throw new LedgerError('invalid-needs', `${field} is a list of task numbers (T-3, T-4)`)
  }
  return [...new Set(value)]
}

export const requireGate = (gate) => {
  if (typeof gate !== 'boolean') {
    throw new LedgerError('invalid-gate', 'human approval is required (true) or not (false)')
  }
}

export function requireTier(tier) {
  if (!TIERS.includes(tier)) {
    throw new LedgerError(
      'invalid-tier',
      `a tier is ${TIERS.join(', ')}, not ${JSON.stringify(tier)}`,
    )
  }
  return tier
}

/** A member's roles, one or more of worker, advisor, reviewer and designer; the first one leads. */
export function requireRoles(roles) {
  if (
    !Array.isArray(roles) ||
    roles.length === 0 ||
    !roles.every((r) => MEMBER_ROLES.includes(r))
  ) {
    throw new LedgerError(
      'invalid-role',
      `a member is one or more of ${MEMBER_ROLES.join(', ')}, not ${JSON.stringify(roles)}`,
    )
  }
  return [...new Set(roles)]
}

/**
 * A saved agent's id, as the roster names it: what a lead runs on, and what
 * a member is. A member's id is its handle too, so it is never one of the
 * handles in `taken`.
 */
export function requireAgentId(agent, taken = []) {
  if (typeof agent !== 'string' || !AGENT_ID.test(agent) || taken.includes(agent)) {
    throw new LedgerError('invalid-agent', `not an agent id: ${JSON.stringify(agent)}`)
  }
  return agent
}

/** Whether a role fits an agent's harness: an image designer is an image agent, and an image agent is nothing else. */
export const fitsRole = (harness, role) => (role === 'designer') === (harness === 'image')

/** Refuses roles that do not fit `agent`, on `harness` (see `fitsRole`), saying which way. */
export function requireFittingRoles(agent, harness, roles) {
  if (roles.every((role) => fitsRole(harness, role))) return
  throw new LedgerError(
    'invalid-role',
    harness === 'image'
      ? `${agent} is an image agent, which can only be an image designer`
      : `only an image agent can be an image designer, and ${agent} is not one`,
  )
}

/** Validates a member and returns its roles, normalized. */
export function requireMember({ agent, harness, role, roles, tier }) {
  const set = requireRoles(roles ?? (role === undefined ? [] : [role]))
  requireAgentId(agent, COORDINATOR_HANDLES)
  requireHarness(harness)
  requireFittingRoles(agent, harness, set)
  requireTier(tier)
  return set
}

/** A card title: the first line that says something, shortened to fit. */
export function titleOf(body) {
  const line = body
    .split('\n')
    .map((text) => text.trim())
    .find(Boolean)
  return line.length <= MAX_TITLE ? line : `${line.slice(0, MAX_TITLE - 1)}…`
}

/** `text` cut at `max` characters, then a line saying how long it was (at 0, only that line); shorter, as it is. */
export const cut = (text, max) =>
  text.length <= max
    ? text
    : `${text.slice(0, max)}${max === 0 ? '' : '\n'}… (${text.length} characters; cut here)`
