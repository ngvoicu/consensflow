import { randomBytes } from 'node:crypto'
import { delimiter, dirname, isAbsolute, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const DEFAULT_TICKET_MS = 60_000
const LEAD_OPS = [
  'consult',
  'say',
  'attach',
  'read',
  'results.list',
  'results.read',
  'seen',
  'notify.lead',
  'panes',
]
const CONTROLLER_OPS = ['session.bind', 'progress.set', 'sent.record']
const BUNDLE_BIN = join(dirname(fileURLToPath(import.meta.url)), '..', 'bin')

const tickets = new Map()
const credentials = new Map()
const capabilities = new Map()
/**
 * What each live launch is for, keyed by its launch id and known from the
 * moment the ticket is issued — before anyone redeems it. One identity
 * serves the store's reservation, Rust's `pane.open` dedupe, the answer the
 * lead is given and `endLaunch`, so nothing has to map between two.
 */
const owners = new Map()

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
 * Mint one short-lived launch ticket, and the launch identity it is for.
 *
 * The two are different things and both are returned: `ticket` is a
 * single-use bearer credential handed to the pane's process, and `launch`
 * is the launch's public identity — the id the store records on the
 * reservation, the id Rust deduplicates `pane.open` by, the id the lead is
 * told, and the id `endLaunch` takes. Only the ticket is a secret.
 *
 * `ticketMs` is injectable only so expiry can be proved without making a
 * test wait for the production minute.
 */
export function issueTicket(record = {}, { ticketMs = DEFAULT_TICKET_MS, launch: given } = {}) {
  const ownership = ownershipRecord(record)
  if (!Number.isFinite(ticketMs) || ticketMs <= 0) {
    throw new Error('ticketMs must be greater than zero')
  }
  sweepExpiredTickets()
  const ticket = opaqueToken()
  // A caller may bring the launch identity it has already committed to.
  // Admission records the launch on the reservation in one queued store
  // operation, and the ticket for that launch is issued afterwards — so
  // either the caller names the id or the two identities drift apart again.
  if (given !== undefined) {
    requireText(given, 'launch id')
    if (owners.has(given) || tickets.has(given)) throw new Error('that launch id is already live')
  }
  const launch = given ?? opaqueToken()
  tickets.set(ticket, { ownership, launch, expiresAt: Date.now() + ticketMs })
  owners.set(launch, ownership)
  return { ticket, launch }
}

/**
 * What this launch is for — the ownership its ticket was issued against.
 * A defensive copy, and `null` once the launch has ended, so a controller
 * op never takes the conversation it writes to from its own request body.
 */
export function ownerOf(launch) {
  if (typeof launch !== 'string' || launch.length === 0) return null
  const ownership = owners.get(launch)
  return ownership === undefined ? null : { ...ownership }
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

/**
 * Revoke everything that could still act for one launch: the capability a
 * controller holds, AND the ticket nobody redeemed. A pane that died before
 * its process ever redeemed leaves a ticket that would otherwise stay good
 * until it expired.
 */
export function endLaunch(launch) {
  const ownerRemoved = owners.delete(launch)
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
  if (capability === undefined) return ticketRemoved || ownerRemoved
  capabilities.delete(launch)
  credentials.delete(capability)
  return true
}

/**
 * The only ConsensFlow authority given to a lead process — and the one
 * thing it needs that is not authority: which runtime is ours.
 *
 * `PATH` starts with the bundle's `bin`, so the `cf` a lead types is this
 * version's shim even when a stale global one is installed. That shim runs
 * `cf.mjs` with `CONSENSFLOW_NODE`, never with a node it finds on PATH: a
 * shim that resolved its own runtime would defeat the shadowing it exists
 * to do. The path must be absolute for the same reason.
 */
export function leadEnv({ tab, pane, leadId, app, path, node } = {}) {
  requireText(tab, 'tab')
  requireText(pane, 'pane')
  requireText(leadId, 'lead id')
  requireText(node, 'runtime')
  if (!isAbsolute(node)) throw new Error(`the runtime must be an absolute path, not ${node}`)
  const { url } = requireApp(app)
  const inheritedPath = typeof path === 'string' && path.length > 0 ? path : ''
  return {
    CONSENSFLOW_APP: url,
    CONSENSFLOW_APP_TOKEN: scopedToken({ tab, ops: LEAD_OPS }),
    CONSENSFLOW_LEAD_ID: leadId,
    CONSENSFLOW_NODE: node,
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
    if (now < issued.expiresAt) continue
    tickets.delete(ticket)
    // A ticket nobody redeemed in time is a launch that never happened:
    // its ownership goes with it rather than outliving it in this map.
    owners.delete(issued.launch)
  }
}
