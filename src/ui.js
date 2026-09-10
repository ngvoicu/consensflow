import { spawn } from 'node:child_process'
import { createHash, randomBytes, randomUUID, timingSafeEqual } from 'node:crypto'
import { fstatSync, readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Bridge } from './bridge.js'
import { CATALOG, EFFORTS } from './catalog.js'
import { Watcher } from './delivery-watch.js'
import { HarnessAdmin, integrationEvidence } from './harness-admin.js'
import { detectHarnesses, harnessPath } from './harnesses.js'
import {
  resetEverything,
  resetPreview,
  skillsStatus,
  skillsSummary,
  turnOff,
  uninstallSkills,
} from './install.js'
import {
  controllerEnv as buildControllerEnv,
  leadEnv as buildLeadEnv,
  checkScope,
  issueTicket,
  LaunchTicketError,
  redeem,
  scopeOf,
} from './launch.js'
import { Page } from './page.js'
import { PaneError, Panes, paneErrorBody } from './panes.js'
import {
  addAgent,
  agentDrift,
  agentRow,
  configRoot,
  editAgent,
  HARNESSES,
  harnessForKind,
  listAgents,
  migrateStateRoot,
  removeAgent,
  syncAgents,
} from './roster.js'
import { agentCommand } from './skill.js'
import { Store, StoreRefusal } from './store.js'
import { healOnOpen, refreshInstalledSkill, skillGaps, staleSkills } from './sync.js'
import { leadIdentity, Tabs } from './tabs.js'
import { terminalCommandStatus, terminalRuntime } from './terminal.js'

/**
 * The minimal roster editor: one ephemeral loopback HTTP server, one inline
 * page, a random bearer token. No daemon, no lock file — Ctrl-C ends it.
 * Every mutation persists to the roster and regenerates the installed skill,
 * exactly as the CLI verbs do.
 */

function tokenMatches(presented, token) {
  return timingSafeEqual(
    createHash('sha256').update(presented).digest(),
    createHash('sha256').update(token).digest(),
  )
}

const VERSION = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'package.json'), 'utf8'),
).version

/** Everything `cf doctor` and `cf skills status` would tell you, as data. */
function systemState(env) {
  const files = skillsStatus(env)
  return {
    version: VERSION,
    terminal: terminalCommandStatus(env),
    harnesses: detectHarnesses(env).map((harness) => ({
      id: harness.id,
      native: harness.native === true,
      skillsDir: harness.skillsDir,
    })),
    agents: listAgents(env).length,
    // The two facts `cf doctor` prints that the panel used to omit. The runtime
    // is the load-bearing one: move whatever provided it and the wiring stops.
    home: configRoot(env),
    runtime: terminalRuntime(env),
    // What a reset would destroy, so the page's dialog and `cf reset`'s
    // refusal quote the same two numbers.
    reset: resetPreview(env),
    // In scope but carrying no skill — a silent refusal looks exactly like health.
    gaps: skillGaps(env),
    // Files, not skills: a skill is a directory. Report both, and whose —
    // plus how many carry an older ConsensFlow's text, which an app upgrade
    // creates and nothing else reports: the files are ours and unedited, so
    // every other count calls them healthy.
    skills: { owned: files.length, ...skillsSummary(env), stale: staleSkills(env).length },
    // What opening the app put right before this page was served. A write
    // nobody reported is the quiet this whole feature exists to end.
    opened,
  }
}

/**
 * Set once, by the editor the app opens — never by `startUiServer` on its own,
 * which is what tests and other callers use.
 */
let opened = null

/** The line this agent becomes in the skill — shown verbatim in the UI. */
function withCommand(agent) {
  const command = agentCommand(agent)
  return command === undefined ? agent : { ...agent, command }
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let body = ''
    request.on('data', (chunk) => {
      body += chunk
      if (body.length > 64 * 1024) reject(new Error('body too large'))
    })
    request.on('end', () => resolve(body))
    request.on('error', reject)
  })
}

const FORBIDDEN_IDENTITY_FIELDS = ['by', 'lead', 'owner']

/**
 * The runtime this app is running on, which is the runtime everything it
 * launches must run on: inside the bundle it IS the bundled node, and
 * resolving one from PATH instead is how a stale install gets a say.
 */
const RUNTIME = process.execPath

class InternalInvariantError extends Error {
  constructor(message) {
    super(message)
    this.name = 'InternalInvariantError'
  }
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function paneOperation(method, pathname) {
  if (method === 'GET' && pathname === '/api/panes') return 'panes'
  if (method !== 'POST' || !pathname.startsWith('/api/panes/')) return null
  const operation = pathname.slice('/api/panes/'.length)
  return operation.length > 0 && !operation.includes('/') ? operation : null
}

/**
 * One pane operation, dispatched after the credential has already decided
 * it may run. `dimensions` is what the token proved — the tab for a lead,
 * the launch for a controller — so no operation takes its subject from the
 * body: a lead's tab and a controller's conversation are the credential's,
 * never the caller's.
 *
 * Returns the `send` arguments so the route stays one line.
 */
async function runPaneOperation(panes, op, dimensions, body) {
  switch (op) {
    case 'panes':
      return [200, await panes.list(dimensions.tab)]
    case 'consult':
      return [200, await panes.consult(dimensions.tab, body)]
    case 'say':
      return [200, await panes.say(dimensions.tab, body)]
    case 'attach':
      return [200, await panes.attach(dimensions.tab, body)]
    case 'session.bind':
      return [200, await panes.sessionBind(dimensions.launch, body)]
    case 'progress.set':
      return [200, await panes.progressSet(dimensions.launch, body)]
    case 'sent.record':
      return [200, await panes.sentRecord(dimensions.launch, body)]
    case 'read':
      return [200, await panes.read(dimensions.tab, body)]
    case 'results.list':
      return [200, await panes.results(dimensions.tab)]
    case 'lead.send':
      return [200, await panes.pmSend(dimensions.tab, body)]
    case 'lead.read':
      return [200, await panes.pmRead(dimensions.tab, body)]
    case 'results.read':
      return [200, await panes.readResult(dimensions.tab, body)]
    case 'seen':
      return [200, await panes.seen(dimensions.tab, body)]
    default:
      return [404, { error: 'not found' }]
  }
}

/**
 * Every request Rust sends over the bridge, answered.
 *
 * `app/src-tauri/src/commands.rs` sends twelve (`grep request_node`); an op
 * with no handler answers `unknown-op`, which Rust turns into
 * `not-available-yet` and the page renders as "not available yet". So this
 * table is the page's whole vocabulary, and a name missing from it is a
 * dead button.
 *
 * The page has no credential and needs none: its authority IS Rust's
 * presence on the pipe, which only the app it is embedded in has. That is
 * why these take their tab from the body where the HTTP routes take it from
 * a lead token. It also means they are not idempotent by an `opId` the way
 * the controller's writes are — a page whose click was lost clicks again,
 * with a person watching, and one minted here would only make a retry
 * silently do nothing.
 */
function attachPage(bridge, { panes, page, store, tabs }) {
  const ops = {
    'tab.open': (body) => panes.tabOpen(body),
    'pm.open': (body) => panes.pmOpen(body),
    'tab.resume': (body) => panes.tabResume(body),
    'tab.delete': (body) => panes.tabDelete(body),
    'tab.rename': async (body) => {
      const tab = await tabs.rename(body?.tab, body?.name)
      return { outcome: 'renamed', tab: tab.id, name: tab.name }
    },
    'shell.open': (body) => panes.shellOpen(body),
    consult: (body) => panes.consult(body?.tab, { ...body, opId: randomUUID() }),
    attach: (body) => panes.attach(body?.tab, { ...body, opId: randomUUID() }),
    'pane.close': (body) => panes.paneClose(body),
    'pane.delete': (body) => panes.paneDelete(body),
    'notify.set': (body) => panes.notifySet(body),
    'state.list': async () => {
      const state = await page.state()
      const stored = await tabs.list()
      const names = new Map(stored.map((tab) => [tab.id, tab.name]))
      return {
        ...state,
        tabs: state.tabs.map((tab) =>
          typeof names.get(tab.id) === 'string' ? { ...tab, name: names.get(tab.id) } : tab,
        ),
      }
    },
    'answers.list': (body) => page.answersList(body),
    'deliver.now': (body) => page.deliverNow(body),
    'deliver.cancel': (body) => page.deliverCancel(body),
    'held.send': (body) => page.heldSend(body),
  }
  for (const [op, run] of Object.entries(ops)) {
    bridge.on(op, async (body) => {
      try {
        const answer = await run(isObject(body) ? body : {})
        return isObject(answer) ? { ok: true, ...answer } : { ok: true, answer }
      } catch (cause) {
        // The page shows the person a message, so a refusal says what it
        // was; a fault on this side says only that it was ours.
        if (cause instanceof PaneError) return { ok: false, ...paneErrorBody(cause) }
        if (cause instanceof StoreRefusal) {
          return { ok: false, error: cause.code, reason: cause.message }
        }
        return {
          ok: false,
          error: 'internal_error',
          ...(cause instanceof Error && cause.message.length > 0 ? { reason: cause.message } : {}),
        }
      }
    })
  }
  // One notification per mutation, straight off the store's queue. The page
  // re-reads `state.list` when it arrives, so this carries no state of its
  // own — a frame that raced the write it describes would be worse than no
  // frame at all, and this one cannot: the queue announces after the write.
  return store.onMutation(({ op }) => {
    bridge.event('state.changed', { op })
  })
}

const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value)

/**
 * True when this descriptor is the parent's end of a pipe. Node's `'pipe'`
 * stdio is a socketpair on macOS and a FIFO elsewhere, so both count; a
 * terminal or /dev/null is neither.
 */
function isPipe(fd) {
  try {
    const stats = fstatSync(fd)
    return stats.isFIFO() || stats.isSocket()
  } catch {
    return false
  }
}

/**
 * True when this stream is the parent's end of a pipe. A stream carrying a
 * real fd is checked the way `isPipe` always did; a stream without one (a
 * test double) counts as a pipe unless it says it is a TTY. No stream at all
 * falls back to the historical check on fd 0. Exported for the bridge
 * activation tests; `serveUi` is the real caller.
 */
export function stdinIsPipe(stream) {
  if (stream !== null && stream !== undefined) {
    if (typeof stream.fd === 'number') return isPipe(stream.fd)
    if (stream.isTTY === true) return false
    return true
  }
  return isPipe(0)
}

export async function startUiServer(
  env,
  { paneOpenDeadlineMs, prepareRole, prepareChannel, harnessLatest } = {},
) {
  const harnessAdmin = new HarnessAdmin(env, {
    latest: harnessLatest,
    integration: async (id) => {
      await storeReady()
      const current = await tabs.list()
      const deliveries = []
      for (const directory of new Set(current.map((tab) => tab.directory))) {
        deliveries.push(...Object.values(await store.readDeliveries(directory)))
      }
      return integrationEvidence(id, current, deliveries)
    },
  })
  // The app is an entry point too: a machine from before the merge should not
  // have to run the CLI once to be tidied up.
  migrateStateRoot(env)

  const store = new Store(configRoot(env))
  const tabs = new Tabs(store)
  let storePromise
  const storeReady = () => {
    storePromise ??= store.open()
    return storePromise
  }
  const token = randomBytes(24).toString('hex')
  let app = null
  const panes = new Panes({
    store,
    tabs,
    agents: {
      row: (name) => agentRow(name, env),
      names: () => listAgents(env).map((agent) => agent.name),
    },
    app: () => app,
    // The Node deciding is the Node that will run the pane: inside the
    // bundle this IS the bundled runtime, and `pane.open` refuses a
    // relative argv[0].
    node: RUNTIME,
    harnessPath: (kind) => harnessPath(harnessForKind(kind) ?? kind, env),
    shell: typeof env.SHELL === 'string' && env.SHELL.length > 0 ? env.SHELL : '/bin/sh',
    env,
    ...(paneOpenDeadlineMs === undefined ? {} : { paneOpenDeadlineMs }),
    ...(prepareRole === undefined ? {} : { prepareRole }),
    ...(prepareChannel === undefined ? {} : { prepareChannel }),
  })
  const page = new Page({
    store,
    tabs,
    agents: {
      row: (name) => agentRow(name, env),
      names: () => listAgents(env).map((agent) => agent.name),
    },
    env,
  })
  // One watcher for the app, started here and nowhere else: it is the only
  // thing that submits a delivery, and two of them would submit each twice.
  const watcher = new Watcher({ store, tabs, env, bindLead: (tab) => panes.bindLead(tab) })
  page.attachWatcher(watcher)
  panes.attachWatcher(watcher)
  let stopAnnouncing = null

  const server = createServer(async (request, reply) => {
    const url = new URL(request.url, 'http://127.0.0.1')
    const send = (status, body, type = 'application/json') => {
      reply.writeHead(status, { 'content-type': type })
      reply.end(typeof body === 'string' ? body : JSON.stringify(body))
    }
    let bodyPromise
    const jsonBody = () => {
      bodyPromise ??= readBody(request).then((body) => JSON.parse(body || '{}'))
      return bodyPromise
    }

    try {
      let body
      if (!['GET', 'HEAD'].includes(request.method)) {
        body = await jsonBody()
        if (!isRecord(body)) return send(400, { error: 'request body must be an object' })
        if (FORBIDDEN_IDENTITY_FIELDS.some((field) => Object.hasOwn(body, field))) {
          return send(400, { error: 'request bodies cannot supply identity' })
        }
      }

      if (request.method === 'POST' && url.pathname === '/api/launch/redeem') {
        return send(200, redeem(body.ticket))
      }

      const bearer = (request.headers.authorization ?? '').replace(/^Bearer /, '')
      const pageToken = bearer || (url.searchParams.get('token') ?? '')
      const uiAuthorized = pageToken.length > 0 && tokenMatches(pageToken, token)
      const scoped = scopeOf(bearer)
      if (!uiAuthorized && !scoped) return send(401, { error: 'unauthorized' })

      if (scoped && body !== undefined) {
        if (Object.hasOwn(body, 'tab') && scoped.tab !== body.tab) {
          return send(400, { error: 'request tab does not match the token' })
        }
        for (const field of ['launch', 'generation']) {
          if (Object.hasOwn(body, field) && scoped[field] !== body[field]) {
            return send(403, { error: 'forbidden' })
          }
        }
      }

      const panesPath = url.pathname === '/api/panes' || url.pathname.startsWith('/api/panes/')
      if (panesPath) {
        if (uiAuthorized) return send(403, { error: 'forbidden' })
        const op = paneOperation(request.method, url.pathname)
        const dimensions = {}
        if (scoped.tab !== undefined) {
          if (request.method !== 'GET' && !Object.hasOwn(body, 'tab')) {
            return send(403, { error: 'forbidden' })
          }
          dimensions.tab = body === undefined ? scoped.tab : body.tab
        }
        if (scoped.launch !== undefined || scoped.generation !== undefined) {
          if (
            body === undefined ||
            !Object.hasOwn(body, 'launch') ||
            !Object.hasOwn(body, 'generation')
          ) {
            return send(403, { error: 'forbidden' })
          }
          dimensions.launch = body.launch
          dimensions.generation = body.generation
        }
        if (op === null || !checkScope(bearer, { ...dimensions, op })) {
          return send(403, { error: 'forbidden' })
        }
        await storeReady()
        try {
          return send(...(await runPaneOperation(panes, op, dimensions, body)))
        } catch (cause) {
          // A pane operation validates what it was given and refuses it in
          // its own words, with a code. Anything else that escapes is a
          // fault on THIS side — a bug, a broken transport, a store that
          // could not write — and answering 400 would tell the caller it
          // sent something wrong and invite it to change the request.
          if (cause instanceof LaunchTicketError) return send(401, { error: 'unauthorized' })
          if (cause instanceof PaneError) return send(cause.status, paneErrorBody(cause))
          // `internal_error` is the code, and it is the whole answer to
          // "whose fault": ours. The words still come along, because the
          // one reading this is the person who ran the app on their own
          // machine, and "cannot read deliveries.json: not JSON" is the
          // difference between fixing it and filing a bug.
          return send(500, {
            error: 'internal_error',
            ...(cause instanceof Error && cause.message.length > 0
              ? { reason: cause.message }
              : {}),
          })
        }
      }

      if (!uiAuthorized) return send(403, { error: 'forbidden' })

      if (request.method === 'GET' && url.pathname === '/') {
        return send(200, PAGE(token), 'text/html; charset=utf-8')
      }
      if (request.method === 'POST' && url.pathname === '/api/tabs') {
        await storeReady()
        // ONE way to open a tab. This route used to write the records and
        // stop, so a tab created here had a lead pane in the store and no
        // process behind it, and nothing else ever launched one. It is the
        // same operation `tab.open` is; only the response shape is this
        // route's own, and callers depend on it.
        try {
          const launched = await panes.tabOpen({ dir: body?.dir, harness: body?.harness })
          const tab = await tabs.get(launched.tab)
          // The response shape is unchanged on purpose: callers depend on
          // exactly `{tab, leadEnv}`, and the launch is now implied by the
          // tab existing at all.
          const pane = tab?.panes?.find((candidate) => candidate.kind === 'lead')
          if (tab === null || pane === undefined) {
            throw new InternalInvariantError('the new tab has no lead pane')
          }
          return send(201, {
            tab,
            leadEnv: buildLeadEnv({
              tab: tab.id,
              pane: pane.id,
              leadId: leadIdentity(tab),
              app,
              path: env?.PATH,
              node: RUNTIME,
            }),
          })
        } catch (cause) {
          if (cause instanceof PaneError) return send(cause.status, paneErrorBody(cause))
          throw cause
        }
      }
      if (request.method === 'POST' && url.pathname === '/api/launch') {
        await storeReady()
        const tab = await tabs.get(body.tab)
        if (tab === null) throw new Error(`no tab ${body.tab}`)
        if (tab.closed === true) throw new Error(`the tab ${tab.id} is closed`)
        const pane = tab.panes.find((candidate) => candidate.id === body.pane)
        if (pane === undefined) throw new Error(`no pane ${body.pane} in tab ${tab.id}`)
        const requester = tab.panes.find((candidate) => candidate.kind === 'lead')
        if (requester === undefined) {
          throw new InternalInvariantError(`the tab ${tab.id} has no lead pane`)
        }
        const { ticket, launch } = issueTicket({
          tab: tab.id,
          pane: pane.id,
          lead: leadIdentity(tab),
          requester: requester.id,
          conversation: body.conversation,
          generation: pane.generation,
        })
        return send(201, {
          ticket,
          launch,
          controllerEnv: buildControllerEnv({ pane: pane.id, app, ticket }),
        })
      }
      if (request.method === 'GET' && url.pathname === '/api/agents') {
        return send(200, {
          agents: listAgents(env).map(withCommand),
          drift: agentDrift(env),
          harnesss: HARNESSES,
          catalog: Object.fromEntries(
            Object.entries(CATALOG).map(([harness, entries]) => [
              harness,
              entries.map((entry) => withCommand({ ...entry, harness })),
            ]),
          ),
          efforts: EFFORTS,
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/agents/sync') {
        // A named operation, like every other one here: it re-resolves
        // catalog-backed agents and nothing else.
        const applied = syncAgents(env, {
          ...(typeof body.name === 'string' ? { name: body.name } : {}),
        })
        if (applied.length > 0) refreshInstalledSkill(env)
        return send(200, { applied, agents: listAgents(env).map(withCommand) })
      }
      if (request.method === 'POST' && url.pathname === '/api/agents') {
        const added = addAgent(body, env)
        refreshInstalledSkill(env)
        return send(201, { agent: added })
      }
      if (request.method === 'GET' && url.pathname === '/api/system') {
        return send(200, systemState(env))
      }
      if (request.method === 'POST' && url.pathname === '/api/harnesses/check') {
        if (
          body.id !== undefined &&
          !['claude', 'codex', 'opencode', 'pi', 'kimi'].includes(body.id)
        ) {
          return send(400, { error: 'Unknown harness' })
        }
        return send(200, {
          harnesses: await harnessAdmin.check(body.id ?? null, { refresh: body.refresh === true }),
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/skills/install') {
        return send(410, {
          error: 'Role skills are included with ConsensFlow. Update the application instead.',
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/reset') {
        if (body.confirm !== true) {
          return send(400, { error: 'confirm before resetting ConsensFlow' })
        }
        const outcome = resetEverything(env)
        const { agents, runs } = outcome.removed
        return send(200, {
          ...outcome,
          report: [
            `Reset — ${agents} agent${agents === 1 ? '' : 's'}, ${runs} run${runs === 1 ? '' : 's'} and every installed file are gone`,
          ],
          system: systemState(env),
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/off') {
        if (body.confirm !== true) {
          return send(400, { error: 'confirm before turning ConsensFlow off' })
        }
        const outcome = turnOff(env, { force: body.force === true })
        return send(200, {
          ...outcome,
          // Off is not a one-way door, and the way back is a section further
          // up this page: say so here, where the reader is looking.
          report: [
            'ConsensFlow is off — your agents are kept. Update skills or reopen the app to reinstall.',
          ],
          system: systemState(env),
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/skills/uninstall') {
        // A click that removes 300 files says so first; the flag is the say-so.
        if (body.confirm !== true) {
          return send(400, { error: 'confirm the removal before it runs' })
        }
        return send(200, { report: uninstallSkills(env, { force: body.force === true }) })
      }

      const named = /^\/api\/agents\/([a-z][a-z0-9-]*)$/.exec(url.pathname)
      if (named !== null && request.method === 'PATCH') {
        const edited = editAgent(named[1], body, env)
        refreshInstalledSkill(env)
        return send(200, { agent: edited })
      }
      if (named !== null && request.method === 'DELETE') {
        removeAgent(named[1], env)
        refreshInstalledSkill(env)
        reply.writeHead(204)
        return reply.end()
      }
      return send(404, { error: 'not found' })
    } catch (cause) {
      if (cause instanceof LaunchTicketError) return send(401, { error: 'unauthorized' })
      if (cause instanceof PaneError) return send(cause.status, paneErrorBody(cause))
      if (cause instanceof InternalInvariantError) return send(500, { error: 'internal_error' })
      return send(400, { error: cause instanceof Error ? cause.message : String(cause) })
    }
  })

  try {
    await storeReady()
    await watcher.start()
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  } catch (cause) {
    await watcher.close()
    await store.close()
    throw cause
  }
  const { port } = server.address()
  app = { url: `http://127.0.0.1:${port}/`, token }

  return {
    url: `http://127.0.0.1:${port}`,
    token,
    // The pane host arrives after the handle line: `serveUi` builds the
    // bridge on the stdio it was handed and gives it to the server here.
    attachBridge: (bridge) => {
      // `null` is how the app says the pane host is gone. Everything that
      // hangs off a bridge comes off with it, the store listener included:
      // a frame written to a dead bridge is a frame nobody reads.
      stopAnnouncing?.()
      stopAnnouncing = null
      panes.attachBridge(bridge)
      if (bridge !== null && bridge !== undefined) {
        watcher.attachBridge(bridge)
        stopAnnouncing = attachPage(bridge, { panes, page, store, tabs, env })
      }
      return panes
    },
    /**
     * Everything this editor started, finished and on disk.
     *
     * Deliberately independent of the HTTP server: a keep-alive connection can
     * hold `server.close()` open indefinitely, and what must not wait behind it
     * is the delivery whose write is settling. `watcher.close()` awaits the
     * watcher's own queue — an in-flight submission handles its rejection
     * inside that queue — and `store.close()` then drains every mutation it
     * admitted. After this, `submitting` has become whatever it really is.
     */
    drain: async () => {
      stopAnnouncing?.()
      await watcher.close()
      await store.close()
    },
    close: async () => {
      try {
        await new Promise((resolve, reject) => {
          server.close((error) => (error === undefined ? resolve() : reject(error)))
        })
      } finally {
        stopAnnouncing?.()
        await watcher.close()
        await store.close()
      }
    },
  }
}

/**
 * How long the editor may spend finishing its writes when its parent goes.
 *
 * Long enough for a settling delivery and a store queue, short enough that a
 * wedged one still ends the process — the parent is already gone, so nothing
 * is served by waiting longer.
 */
const DRAIN_MS = 5_000

/**
 * `cf ui`: start, say where it is, run until Ctrl-C.
 *
 * `json` prints one machine-readable handle line first, so a host program
 * (the desktop app) can start the editor and point a window at it instead of
 * scraping prose. `open: false` leaves the browser alone for the same reason.
 */
export async function serveUi(
  env,
  {
    onOut,
    json = false,
    open = true,
    stdin = null,
    stdout = null,
    registerDrain = null,
    serverOptions = undefined,
  },
) {
  // Opening the app IS the act: it does what its own buttons do, before the
  // page is served, so the first render already tells the truth.
  opened = healOnOpen(env)
  const server = await startUiServer(env, serverOptions)
  const url = `${server.url}/?token=${server.token}`

  if (json) {
    onOut(JSON.stringify({ url: `${server.url}/`, token: server.token }))
  } else {
    onOut(`roster editor: ${url}`)
    onOut('Ctrl-C to stop — nothing keeps running after it.')
  }
  if (open) spawn('open', [url], { stdio: 'ignore', detached: true }).unref()

  // A parent that holds a pipe to our stdin is telling us it wants to own
  // this editor's lifetime: when that pipe closes the parent is gone, and an
  // editor nobody can see must not keep serving. A stdin that is a terminal
  // or /dev/null says nothing of the sort, so it is left alone. A piped
  // stdin additionally speaks the JSON-lines bridge (Phase 1), but only in
  // --json mode: prose and frames must never share one stdout. The streams
  // arrive the way `env` does, from the entry point, so this module owns no
  // stdio of its own.
  const input = stdin ?? process.stdin
  const output = stdout ?? process.stdout

  /**
   * The parent is gone, so this editor stops — but not in the same tick.
   *
   * `process.exit(0)` here used to end the process synchronously, and a
   * delivery whose paste the pane accepted and whose Return never landed was
   * left `submitting` on disk for ever. It reads as "still going", which is
   * the one thing it is not. The bridge rejects every outstanding request
   * before this runs, so the write is already settling inside the watcher's
   * queue; draining lets it reach `uncertain` — never a replay, and never a
   * pretence that it failed cleanly.
   *
   * Bounded, because the parent is not coming back: a drain that will not
   * finish must still let the process go, and the exit stays 0 either way —
   * a parent closing its pipe is the ordinary end of a session, not a fault.
   */
  let stopping = null
  const stop = () => {
    stopping ??= (async () => {
      const expired = new Promise((resolve) => {
        const timer = setTimeout(resolve, DRAIN_MS)
        timer.unref?.()
      })
      try {
        await Promise.race([server.drain(), expired])
      } catch {
        // A drain that throws has still had its chance; the parent is gone
        // and holding the process open over it helps nobody.
      }
      process.exit(0)
    })()
    return stopping
  }
  registerDrain?.(stop)

  if (stdinIsPipe(input)) {
    if (json) {
      // A fatal bridge failure ends the session the way stdin EOF does: the
      // parent is gone or the pipe is broken, and an editor nobody can read
      // must not keep serving.
      const bridge = new Bridge({ input, output, onFatal: stop })
      bridge.on('ping', () => ({ ok: true }))
      server.attachBridge(bridge)
    }
    input.on('end', stop)
    input.on('close', stop)
    input.resume()
  }
  await new Promise(() => {})
}

const PAGE = (token) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ConsensFlow roster</title>
<style>
  /* Dark-first: this window lives beside a terminal. Brand marine palette;
     Archivo and IBM Plex Mono when installed locally, never fetched — a local
     tool must not wait on a font CDN. */
  :root {
    --ink: #0C1E23;
    --panel: #12262C;
    --line: #1E3A42;
    --foam: #E9F1EF;
    --muted: #8FA9AF;
    --seafoam: #63C7B2;
    /* Seafoam on foam is unreadable; light mode gets a deeper teal for text
       while keeping seafoam for fills and borders. */
    --accent-text: #63C7B2;
    --buoy: #FF6B5A;
    --ui: Archivo, "Helvetica Neue", system-ui, sans-serif;
    --mono: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --ink: #E9F1EF; --panel: #FFFFFF; --line: #C9DAD8; --foam: #0C1E23;
      --muted: #52717A; --accent-text: #16766A; --buoy: #C2402F;
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; padding: 40px 24px 64px; background: var(--ink); color: var(--foam);
    font-family: var(--ui); font-size: 15px; line-height: 1.5;
    -webkit-font-smoothing: antialiased;
  }
  main { max-width: 760px; margin: 0 auto; }

  .mark { font-family: var(--mono); font-size: 12px; letter-spacing: .14em; text-transform: uppercase; color: var(--accent-text); }
  h1 { font-size: 26px; font-weight: 600; letter-spacing: -.015em; margin: 6px 0 4px; }
  .lede { color: var(--muted); font-size: 13.5px; margin: 0 0 32px; max-width: 52ch; }

  .eyebrow {
    font-family: var(--mono); font-size: 11px; letter-spacing: .16em; text-transform: uppercase;
    color: var(--muted); display: flex; align-items: center; gap: 12px; margin: 34px 0 12px;
  }
  .eyebrow::after { content: ""; flex: 1; height: 1px; background: var(--line); }
  /* The section head announces; the tool heads inside it only sort. */
  .eyebrow--section { color: var(--foam); font-size: 12px; margin-top: 46px; }
  .cmds { margin: 14px 0 0; }
  .cmds dt { margin-top: 14px; }
  .cmds dt code { color: var(--seafoam); font-size: 13px; }
  .cmds dd { margin: 4px 0 0; color: var(--muted); font-size: 13px; line-height: 1.5; }
  .cmds dd code { color: var(--seafoam); }
  .cmds dd strong { color: var(--foam); }
  .eyebrow--tool { margin: 22px 0 4px; }
  .eyebrow--tool::after { display: none; }

  /* An agent IS a command: the callsign names it, the line below is
     exactly what lands in the skill and exactly what an harness will run. */
  .member { border-top: 1px solid var(--line); padding: 14px 0; display: grid; gap: 8px; }
  /* Grid children default to min-width:auto, so a long command line would
     stretch the row and push the controls off the page instead of scrolling. */
  .member > * { min-width: 0; }
  .member:last-of-type { border-bottom: 1px solid var(--line); }
  .member__head { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .callsign { font-size: 17px; font-weight: 600; color: var(--accent-text); letter-spacing: -.01em; }
  .tag { font-family: var(--mono); font-size: 11px; color: var(--muted); }
  .member__head .spacer { flex: 1; }
  .member__desc { color: var(--muted); font-size: 13px; margin: 0; }
  .cmd-wrap { position: relative; }
  /* A long command scrolls rather than wrapping (it stays one readable line);
     the fade is the only hint that there is more to the right. */
  .cmd-wrap::after {
    content: ""; position: absolute; inset: 1px 1px 1px auto; width: 44px; border-radius: 0 4px 4px 0;
    background: linear-gradient(90deg, transparent, var(--panel)); pointer-events: none;
  }
  .cmd {
    font-family: var(--mono); font-size: 11.5px; line-height: 1.6; color: var(--muted);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    padding: 9px 11px; margin: 0; overflow-x: auto; white-space: pre; scrollbar-width: thin;
  }
  .cmd b { color: var(--foam); font-weight: 500; }

  button {
    font: inherit; font-size: 13px; color: var(--foam); background: transparent;
    border: 1px solid var(--line); border-radius: 4px; padding: 4px 12px; cursor: pointer;
    transition: border-color .12s ease, color .12s ease, background .12s ease;
  }
  button:hover { border-color: var(--seafoam); color: var(--accent-text); }
  button.danger:hover { border-color: var(--buoy); color: var(--buoy); }
  button[data-armed="true"] { border-color: var(--buoy); color: var(--buoy); font-weight: 600; }
  button.primary { background: var(--seafoam); border-color: var(--seafoam); color: #06171C; font-weight: 600; }
  button.primary:hover { filter: brightness(1.08); color: #06171C; }
  :focus-visible { outline: 2px solid var(--seafoam); outline-offset: 2px; }

  .offer { display: flex; align-items: baseline; gap: 12px; padding: 8px 0; border-top: 1px solid var(--line); }
  .offer:first-of-type { border-top: none; }
  .offer__name { font-family: var(--mono); font-size: 13px; color: var(--foam); min-width: 96px; }
  .offer__what { color: var(--muted); font-size: 13px; flex: 1; }
  .offer__model { font-family: var(--mono); font-size: 11px; color: var(--muted); opacity: .8; }
  .member__drift { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin: 6px 0 0; font-size: 13px; color: var(--muted); }
  .tag--moved { background: var(--buoy); color: var(--ink); }
  .lede--tight { margin: 0 0 8px; }
  #catalog-filter {
    width: 100%; box-sizing: border-box; margin-bottom: 10px; padding: 8px 10px;
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    color: var(--ink); font: inherit; font-size: 13px;
  }
  #catalog-filter:focus-visible { outline: 2px solid var(--seafoam); outline-offset: 1px; }

  form { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 14px; }
  input, select {
    font: inherit; font-size: 13px; padding: 7px 10px; color: var(--foam);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
  }
  input::placeholder { color: var(--muted); }
  .full { grid-column: 1 / -1; }
  .alert { color: var(--buoy); font-size: 13px; margin: 0; }
  .facts { display: grid; gap: 4px; margin: 0 0 14px; }
  .fact { display: flex; gap: 12px; font-size: 13px; }
  .fact dt { color: var(--muted); min-width: 104px; font-family: var(--mono); font-size: 11.5px; letter-spacing: .04em; text-transform: uppercase; padding-top: 2px; }
  .fact dd { margin: 0; }
  .host { font-family: var(--mono); font-size: 12.5px; }
  .host + .host { margin-top: 2px; }
  .host span { color: var(--muted); }
  .actions { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
  .check { font-size: 13px; color: var(--muted); display: flex; align-items: center; gap: 6px; }
  .note { font-size: 13px; color: var(--muted); min-height: 20px; margin: 10px 0 0; }
  .empty { color: var(--muted); font-size: 13.5px; border: 1px dashed var(--line); border-radius: 4px; padding: 18px; }
  @media (max-width: 620px) { form { grid-template-columns: 1fr; } .offer { flex-wrap: wrap; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }
</style>
</head>
<body>
<main>
  <p class="mark"><span>consensflow</span> <span id="version"></span></p>
  <h1>Your agents</h1>
  <p class="lede" id="lede">Each one is a real coding-harness CLI. Every change here rewrites the skill installed in your harnesses, so they can consult it by name.</p>

  <section id="roster"></section>

  <p class="eyebrow eyebrow--section">Ready-made</p>
  <p class="lede lede--tight">Pick a name and it is configured for you. <span id="catalog-count"></span></p>
  <input id="catalog-filter" type="search" placeholder="Filter by name, model or engine…" autocomplete="off">
  <div id="catalog"></div>

  <p class="eyebrow eyebrow--section">Installed</p>
  <div id="system"></div>
  <div class="actions">
    <button id="off" class="danger">Turn off</button>
    <button id="reset" class="danger" title="Removes your agents, every run artifact (packets, transcripts, generated images), and every file ConsensFlow installed — including skill files you edited. The ConsensFlow.app bundle stays. This cannot be undone.">Reset everything</button>
  </div>
  <p id="skills-note" class="note"></p>

  <p class="eyebrow eyebrow--section">Talking to an agent</p>
  <p class="lede">Your coding agent runs these for you — you just say "ask hyperion…".
  They are here so you can drive it yourself when you want to.</p>
  <dl class="cmds">
    <dt><code>cf run @name "&lt;task&gt;" --new</code></dt>
    <dd>Open a fresh conversation in an app pane and print its name. Completed replies arrive automatically when the lead can receive them.</dd>
    <dt><code>cf run @name "&lt;task&gt;"</code></dt>
    <dd>Continue the current conversation with that agent.</dd>
    <dt><code>cf say &lt;conversation&gt; "&lt;task&gt;"</code></dt>
    <dd>Send a follow-up to a named conversation.</dd>
    <dt><code>cf sessions</code></dt>
    <dd>List the conversations recorded in this folder.</dd>
    <dt><code>cf results [&lt;conversation&gt;]</code></dt>
    <dd>List completed worker results and which ones have been read.</dd>
    <dt><code>cf read &lt;conversation&gt;</code></dt>
    <dd>Read a complete result. For a long answer, follow the returned delivery ID and read every numbered part.</dd>
    <dt><code>cf attach &lt;conversation&gt;</code></dt>
    <dd>Focus or reopen its app pane with the existing history.</dd>
  </dl>

  <p class="eyebrow eyebrow--section">Define your own</p>
  <form id="add">
    <input name="name" placeholder="callsign, lowercase" required>
    <select name="harness"></select>
    <input class="full" name="model" placeholder="model — anything this harness accepts" required>
    <input name="effort" list="effort-options" placeholder="effort (optional)">
    <datalist id="effort-options"></datalist>
    <button class="primary">Add agent</button>
    <p id="error" class="alert full"></p>
  </form>
</main>
<script>
const TOKEN = ${JSON.stringify(token)};
const headers = { authorization: 'Bearer ' + TOKEN, 'content-type': 'application/json' };
const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

/** The model and effort are what the reader is scanning for: mark them. */
function renderCommand(entry) {
  const pre = el('pre', 'cmd');
  let rest = entry.command;
  for (const value of [entry.model, entry.effort].filter(Boolean)) {
    const at = rest.indexOf(value);
    if (at === -1) continue;
    pre.append(rest.slice(0, at));
    pre.append(el('b', null, value));
    rest = rest.slice(at + value.length);
  }
  pre.append(rest);
  const wrap = el('div', 'cmd-wrap');
  wrap.append(pre);
  return wrap;
}

function renderRoster(data) {
  const host = document.querySelector('#roster');
  host.innerHTML = '';
  document.querySelector('#lede').textContent = data.agents.length === 0
    ? 'Each one is a real coding-harness CLI. Add the first and the skill installs itself into every harness you have.'
    : 'Each one is a real coding-harness CLI. Every change here rewrites the skill installed in your harnesses, so they can consult it by name.';

  if (data.agents.length === 0) {
    host.appendChild(el('p', 'empty', 'No agents yet. Take a ready-made one below, or define your own.'));
    return;
  }
  if ((data.drift ?? []).length > 1) {
    const all = el('div', 'member');
    const line = el('p', 'member__drift');
    line.append(el('span', 'tag tag--moved', data.drift.length + ' agents moved'));
    line.append(el('span', null, ' the catalog has newer models for them '));
    const update = el('button', null, 'Update all');
    update.onclick = () => post('/api/agents/sync', {}, 'Updating…');
    line.append(update);
    all.append(line);
    host.append(all);
  }
  for (const p of data.agents) {
    const card = el('div', 'member');
    const head = el('div', 'member__head');
    head.append(el('span', 'callsign', p.name));
    head.append(el('span', 'tag', p.harness + (p.effort ? ' · ' + p.effort : '')));
    head.append(el('span', 'spacer'));
    const edit = el('button', null, 'Edit');
    edit.onclick = () => openEditor(card, p);
    head.append(edit);
    const remove = el('button', 'danger', 'Remove');
    remove.onclick = async () => {
      await fetch('/api/agents/' + p.name, { method: 'DELETE', headers });
      load();
    };
    head.append(remove);
    card.append(head);
    const moved = (data.drift ?? []).find((d) => d.name === p.name);
    if (moved) {
      const note = el('p', 'member__drift');
      note.append(el('span', 'tag tag--moved', 'catalog moved'));
      note.append(el('span', null, ' ' + moved.changes
        .map((c) => c.field + ': ' + (c.from ?? '-') + ' → ' + (c.to ?? '-')).join(', ') + ' '));
      const update = el('button', null, 'Update');
      update.onclick = () => post('/api/agents/sync', { name: p.name }, 'Updating ' + p.name + '…');
      note.append(update);
      card.append(note);
    }
    if (p.description) card.append(el('p', 'member__desc', p.description));
    if (p.command) card.append(renderCommand(p));
    else card.append(el('p', 'member__desc', p.harness + ' agents are not run by this tool — it leaves them alone.'));
    host.append(card);
  }
}

/** Editing an agent is changing its model, effort or description. */
function openEditor(card, agent) {
  if (card.querySelector('form')) return;
  const form = el('form', 'form');
  const fields = [
    ['model', agent.model, 'model'],
    ['effort', agent.effort ?? '', 'effort (blank for none)'],
    ['description', agent.description ?? '', 'description'],
  ];
  for (const [name, value, placeholder] of fields) {
    const input = document.createElement('input');
    input.name = name;
    input.value = value;
    input.placeholder = placeholder;
    input.className = 'full';
    form.append(input);
  }
  const save = el('button', 'primary', 'Save');
  save.type = 'submit';
  const cancel = el('button', null, 'Cancel');
  cancel.type = 'button';
  cancel.onclick = () => form.remove();
  form.append(save, cancel);
  form.onsubmit = async (event) => {
    event.preventDefault();
    const entries = Object.fromEntries(new FormData(form).entries());
    const res = await fetch('/api/agents/' + agent.name, {
      method: 'PATCH',
      headers,
      body: JSON.stringify(entries),
    });
    if (res.ok) load();
    else document.querySelector('#skills-note').textContent = (await res.json()).error;
  };
  card.append(form);
}

function renderCatalog(data) {
  const taken = new Set(data.agents.map((p) => p.name));
  const host = document.querySelector('#catalog');
  const needle = (document.querySelector('#catalog-filter').value || '').trim().toLowerCase();
  const matches = (entry, harness) => needle.length === 0 ||
    [entry.name, entry.model, entry.description, entry.detail, harness]
      .filter(Boolean).join(' ').toLowerCase().includes(needle);
  host.innerHTML = '';
  let shown = 0, free = 0;
  for (const [harness, entries] of Object.entries(data.catalog)) {
    free += entries.filter((e) => !taken.has(e.name)).length;
    const available = entries.filter((e) => !taken.has(e.name) && matches(e, harness));
    if (available.length === 0) continue;
    shown += available.length;
    const group = el('section');
    group.append(el('p', 'eyebrow eyebrow--tool', harness + ' · ' + available.length));
    for (const entry of available) {
      const row = el('div', 'offer');
      row.append(el('span', 'offer__name', entry.name));
      const what = el('span', 'offer__what', entry.description);
      if (entry.detail) what.title = entry.detail;
      row.append(what);
      row.append(el('span', 'offer__model', entry.model + (entry.effort ? ' · ' + entry.effort : '')));
      const add = el('button', null, 'Add');
      add.onclick = async () => {
        await fetch('/api/agents', {
          method: 'POST', headers,
          body: JSON.stringify({
            name: entry.name, harness, model: entry.model,
            ...(entry.effort ? { effort: entry.effort } : {}),
            description: entry.description,
            preset: entry.preset,
          }),
        });
        load();
      };
      row.append(add);
      group.append(row);
    }
    host.append(group);
  }
  const count = document.querySelector('#catalog-count');
  count.textContent = needle.length > 0
    ? shown + ' of ' + free + ' shown'
    : free + ' available across every engine';
  if (shown === 0) host.append(el('p', 'note', needle.length > 0
    ? 'No ready-made agent matches that.'
    : 'Every ready-made agent is already on your roster.'));
}

function renderForm(data) {
  const harnessSelect = document.querySelector('select[name=harness]');
  if (harnessSelect.options.length === 0) {
    for (const r of data.harnesss) harnessSelect.add(new Option(r, r));
    harnessSelect.onchange = () => showEfforts(data.efforts, harnessSelect.value);
  }
  showEfforts(data.efforts, harnessSelect.value);
}

function showEfforts(efforts, harness) {
  const list = document.querySelector('#effort-options');
  list.innerHTML = '';
  for (const e of efforts[harness] ?? []) list.appendChild(new Option(e, e));
}

let HARNESS_ROWS = [];
async function checkHarnesses(id = null, button = null) {
  if (button) { button.disabled = true; button.textContent = 'Checking…'; }
  try {
    const response = await fetch('/api/harnesses/check', { method: 'POST', headers, body: JSON.stringify({ ...(id ? { id } : {}), refresh: button !== null }) });
    if (!response.ok) throw new Error('Harness check failed');
    const { harnesses } = await response.json();
    HARNESS_ROWS = id ? HARNESS_ROWS.map(row => row.id === id ? harnesses[0] : row) : harnesses;
    if (LAST_SYSTEM) renderSystem(LAST_SYSTEM);
  } catch (error) {
    if (button) button.textContent = error.message + ' — retry';
  } finally { if (button) button.disabled = false; }
}

function renderHarness(row) {
  const line = el('div', 'host');
  line.append(el('strong', null, row.id + (row.lead ? '' : ' (worker only)')));
  line.append(el('div', null, row.installed ? 'Installed: ' + row.path : 'Not installed'));
  if (row.installed) {
    line.append(el('div', null, 'Version: ' + (row.version.value || row.version.reason || row.version.state)));
    const update = row.update;
    const text = update.state === 'available' ? 'New release: ' + update.value : update.state === 'current' ? 'Up to date' : update.reason || 'Update availability not verified';
    line.append(el('div', null, text));
    if (update.note) line.append(el('small', null, update.note));
    line.append(el('div', null, 'Integration: ' + row.integration.state + ' — ' + row.integration.reason));
    if (row.extension) {
      const failed = row.extension.state === 'error' || row.extension.state === 'not-installed';
      const status = el('div', null, failed ? 'Pi extension missing / installation failed: ' + (row.extension.reason || '') : row.integration.state === 'ok' ? 'Pi extension installed; complete result delivery verified' : 'Pi extension installed; live connection not verified');
      if (failed) status.style.color = '#f47769';
      line.append(status);
      if (failed) {
        const retry = el('button', null, 'Retry extension installation');
        retry.onclick = () => checkHarnesses(row.id, retry); line.append(retry);
      }
    }
  }
  const link = el('a', null, row.installed ? 'Update instructions' : 'Installation instructions');
  link.href = row.instructions; link.target = '_blank'; link.rel = 'noopener noreferrer'; line.append(link);
  const check = el('button', null, 'Check again');
  check.onclick = () => checkHarnesses(row.id, check); line.append(check);
  line.append(el('small', null, 'Checked: ' + new Date(row.checkedAt).toLocaleString()));
  return line;
}

function renderSystem(system) {
  document.querySelector('#version').textContent = 'v' + system.version;
  const host = document.querySelector('#system');
  host.innerHTML = '';
  const list = el('dl', 'facts');

  const hosts = el('div');
  const checkAll = el('button', null, 'Check all harnesses');
  checkAll.onclick = () => checkHarnesses(null, checkAll); hosts.append(checkAll);
  if (HARNESS_ROWS.length) for (const row of HARNESS_ROWS) hosts.append(renderHarness(row));
  else hosts.append(el('div', null, 'Harnesses have not been checked yet'));
  const skills = 'Role skills included in ConsensFlow ' + system.version + ' — lead and PM only';

  const runtime = system.runtime === null
    ? null
    : !system.runtime.exists
      ? system.runtime.runtime + ' — MISSING, reinstall from the app'
      : system.runtime.mine === false
        ? system.runtime.runtime + ' — another ConsensFlow: the cf command runs that one, not this app'
        : system.terminal.installed && !system.terminal.onPath
          ? system.runtime.runtime + ' — but ' + system.terminal.path + ' is not on your PATH, so cf resolves to nothing'
          : system.runtime.runtime;

  const rows = [['Harnesses', hosts], ['Installed', skills]];
  if (runtime) rows.push(['Runtime', runtime]);
  rows.push(['Home', system.home]);

  for (const [label, value] of rows) {
    const dt = el('dt', null, label);
    const dd = el('dd');
    if (typeof value === 'string') dd.textContent = value;
    else dd.append(value);
    const row = el('div', 'fact');
    row.append(dt, dd);
    list.append(row);
  }
  host.append(list);
}

async function post(path, body, note) {
  const el2 = document.querySelector('#skills-note');
  el2.textContent = note;
  const res = await fetch(path, { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  if (!res.ok) { el2.textContent = data.error; return; }
  const counts = {};
  for (const row of data.changes ?? data.report ?? []) {
    if (row && row.action) counts[row.action] = (counts[row.action] ?? 0) + 1;
  }
  const summary = Object.entries(counts).map(([action, n]) => n + ' ' + action).join(' · ');
  // The report is a list of sentences from some endpoints and a list of change
  // ROWS from others. Joining rows printed [object Object] on every skills
  // update; count those instead, and answer whatever shape did come back.
  const sentences = Array.isArray(data.report) && data.report.every((r) => typeof r === 'string')
    ? data.report
    : null;
  if (Array.isArray(data.applied)) {
    el2.textContent = data.applied.length === 0
      ? 'already up to date'
      : data.applied.map((a) => a.name + ': ' + a.changes
          .map((c) => c.field + ' → ' + (c.to ?? '-')).join(', ')).join(' · ');
  } else if (sentences !== null && sentences.length > 0) {
    el2.textContent = sentences.join(' · ');
  } else if (summary.length > 0) {
    // All it did was confirm every file is already right — say that, do not
    // make the reader parse a count to discover nothing happened.
    const only = Object.keys(counts);
    el2.textContent = only.length === 1 && only[0] === 'unchanged'
      ? 'already up to date — ' + counts.unchanged + ' files checked'
      : summary;
  } else if (Array.isArray(data.removed)) {
    const n = data.removed.length;
    el2.textContent = n === 0 ? 'nothing to remove' : 'removed ' + n + ' file' + (n === 1 ? '' : 's');
  } else {
    el2.textContent = 'nothing to do';
  }
  load();
}

// Keep installed skills current with the roster.
document.querySelector('#catalog-filter').addEventListener('input', () => {
  if (LAST !== null) renderCatalog(LAST);
});

/**
 * A confirmation the host cannot swallow.
 *
 * window.confirm is a no-op in the app: it is a WKWebView and nothing on the
 * Rust side implements the JS dialog panels, so the call returns false without
 * showing anything and the button silently does nothing. Two deliberate clicks
 * work in every webview and in a plain browser — the first arms the button and
 * says what is about to happen, the second does it, and walking away disarms.
 */
function arming(button, resting, armedLabel, run) {
  let timer = null;
  const disarm = () => {
    clearTimeout(timer);
    timer = null;
    button.textContent = resting;
    button.dataset.armed = 'false';
  };
  disarm();
  button.onclick = () => {
    if (timer !== null) { disarm(); run(); return; }
    button.textContent = armedLabel();
    button.dataset.armed = 'true';
    timer = setTimeout(disarm, 6000);
  };
}

arming(
  document.querySelector('#off'),
  'Turn off',
  () => 'Click again to turn off — agents are kept',
  () => post('/api/off', { confirm: true }, 'Turning off…'),
);

arming(
  document.querySelector('#reset'),
  'Reset everything',
  () => {
    const c = LAST_SYSTEM === null ? { agents: 0, runs: 0 } : LAST_SYSTEM.reset;
    const say = (n, noun) => n + ' ' + noun + (n === 1 ? '' : 's');
    return 'Click again to destroy ' + say(c.agents, 'agent') + ' and ' + say(c.runs, 'run');
  },
  () => post('/api/reset', { confirm: true }, 'Resetting…'),
);

// The last roster payload, so filtering the catalog re-renders without a fetch.
let LAST = null;
// The last system payload, for the counts the reset dialog has to name.
let LAST_SYSTEM = null;

async function load() {
  const [data, system] = await Promise.all([
    (await fetch('/api/agents', { headers })).json(),
    (await fetch('/api/system', { headers })).json(),
  ]);
  LAST = data;
  LAST_SYSTEM = system;
  renderRoster(data);
  renderCatalog(data);
  renderForm(data);
  renderSystem(system);
  if (!HARNESS_ROWS.length) checkHarnesses();
}

document.querySelector('#add').onsubmit = async (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = Object.fromEntries([...form.entries()].filter(([, v]) => v !== ''));
  const res = await fetch('/api/agents', { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  document.querySelector('#error').textContent = res.ok ? '' : data.error;
  if (res.ok) { event.target.reset(); load(); }
};
load();
</script>
</body>
</html>
`
