import { randomBytes } from 'node:crypto'
import { delimiter, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const DEFAULT_TICKET_MS = 60_000
const LEAD_OPS = ['consult', 'say', 'attach', 'read', 'seen', 'notify.lead', 'panes']
const CONTROLLER_OPS = ['session.bind', 'progress.set', 'sent.record']
const BUNDLE_BIN = join(dirname(fileURLToPath(import.meta.url)), '..', 'bin')

const tickets = new Map()
const credentials = new Map()
const capabilities = new Map()

export class LaunchTicketError extends Error {
  constructor(message) {
    super(message)
    this.name = 'LaunchTicketError'
  }
}

/** An opaque bearer token whose authority is held only in this process. */
export function scopedToken({ tab, ops } = {}) {
  requireText(tab, 'tab')
  const allowed = requireOps(ops)
  const token = opaqueToken()
  credentials.set(token, { tab, ops: allowed })
  return token
}

/** True only when every supplied dimension belongs to this bearer token. */
export function checkScope(token, { tab, launch, generation, op } = {}) {
  if (typeof token !== 'string' || token.length === 0) return false
  const scope = credentials.get(token)
  if (scope === undefined) return false
  if (tab !== undefined && scope.tab !== tab) return false
  if (launch !== undefined && scope.launch !== launch) return false
  if (generation !== undefined && scope.generation !== generation) return false
  if (op !== undefined && !scope.ops.has(op)) return false
  return true
}

/** A defensive copy of one credential's authority, or null when unknown. */
export function scopeOf(token) {
  if (typeof token !== 'string' || token.length === 0) return null
  const scope = credentials.get(token)
  if (scope === undefined) return null
  return {
    ...(scope.tab === undefined ? {} : { tab: scope.tab }),
    ...(scope.launch === undefined ? {} : { launch: scope.launch }),
    ...(scope.generation === undefined ? {} : { generation: scope.generation }),
    ops: [...scope.ops],
  }
}

/**
 * Mint one short-lived launch ticket. `ticketMs` is injectable only so expiry
 * can be proved without making a test wait for the production minute.
 */
export function issueTicket(record = {}, { ticketMs = DEFAULT_TICKET_MS } = {}) {
  const ownership = ownershipRecord(record)
  if (!Number.isFinite(ticketMs) || ticketMs <= 0) {
    throw new Error('ticketMs must be greater than zero')
  }
  sweepExpiredTickets()
  const ticket = opaqueToken()
  tickets.set(ticket, { ownership, launch: opaqueToken(), expiresAt: Date.now() + ticketMs })
  return ticket
}

/** Consume a ticket once and return its ownership with a controller bearer. */
export function redeem(ticket) {
  const issued = typeof ticket === 'string' ? tickets.get(ticket) : undefined
  const now = Date.now()
  sweepExpiredTickets(now)
  if (issued === undefined) throw new LaunchTicketError('launch ticket is invalid or already used')
  if (now >= issued.expiresAt) throw new LaunchTicketError('launch ticket expired')
  tickets.delete(ticket)

  const capability = opaqueToken()
  credentials.set(capability, {
    launch: issued.launch,
    generation: issued.ownership.generation,
    ops: new Set(CONTROLLER_OPS),
  })
  capabilities.set(issued.launch, capability)
  return { ...issued.ownership, launch: issued.launch, capability }
}

/** Revoke everything that could still act for one launch. */
export function endLaunch(launch) {
  let ticketRemoved = tickets.delete(launch)
  if (!ticketRemoved) {
    for (const [ticket, issued] of tickets) {
      if (issued.launch !== launch) continue
      tickets.delete(ticket)
      ticketRemoved = true
      break
    }
  }
  const capability = capabilities.get(launch)
  if (capability === undefined) return ticketRemoved
  capabilities.delete(launch)
  credentials.delete(capability)
  return true
}

/** The only ConsensFlow authority given to a lead process. */
export function leadEnv({ tab, pane, leadId, app, path } = {}) {
  requireText(tab, 'tab')
  requireText(pane, 'pane')
  requireText(leadId, 'lead id')
  const { url } = requireApp(app)
  const inheritedPath = typeof path === 'string' && path.length > 0 ? path : ''
  return {
    CONSENSFLOW_APP: url,
    CONSENSFLOW_APP_TOKEN: scopedToken({ tab, ops: LEAD_OPS }),
    CONSENSFLOW_LEAD_ID: leadId,
    CONSENSFLOW_TAB: tab,
    CONSENSFLOW_PANE_ID: pane,
    PATH: inheritedPath.length > 0 ? `${BUNDLE_BIN}${delimiter}${inheritedPath}` : BUNDLE_BIN,
  }
}

/** The bootstrap authority given to `cf run --in-pane`, and nothing else. */
export function controllerEnv({ pane, app, ticket } = {}) {
  requireText(pane, 'pane')
  requireText(ticket, 'launch ticket')
  const { url } = requireApp(app)
  return {
    CONSENSFLOW_APP: url,
    CONSENSFLOW_LAUNCH: ticket,
    CONSENSFLOW_PANE_ID: pane,
  }
}

/** A shell is intentionally outside every ConsensFlow HTTP authority. */
export function shellEnv() {
  return {}
}

function ownershipRecord({ tab, pane, lead, requester, conversation, generation }) {
  for (const [label, value] of [
    ['tab', tab],
    ['pane', pane],
    ['lead', lead],
    ['requester', requester],
    ['conversation', conversation],
  ]) {
    requireText(value, label)
  }
  if (!Number.isInteger(generation) || generation < 1) {
    throw new Error('generation must be a positive integer')
  }
  return { tab, pane, lead, requester, conversation, generation }
}

function requireApp(app) {
  if (app === null || typeof app !== 'object' || Array.isArray(app)) {
    throw new Error('app must carry its url')
  }
  requireText(app.url, 'app url')
  return app
}

function requireOps(ops) {
  if (!Array.isArray(ops) || ops.length === 0) throw new Error('ops must be a non-empty array')
  const allowed = new Set()
  for (const op of ops) {
    requireText(op, 'operation')
    allowed.add(op)
  }
  return allowed
}

function requireText(value, label) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new Error(`${label} is required`)
  }
}

function opaqueToken() {
  return randomBytes(32).toString('base64url')
}

function sweepExpiredTickets(now = Date.now()) {
  for (const [ticket, issued] of tickets) {
    if (now >= issued.expiresAt) tickets.delete(ticket)
  }
}
