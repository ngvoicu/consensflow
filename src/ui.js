import { spawn } from 'node:child_process'
import { createHash, randomBytes, randomUUID, timingSafeEqual } from 'node:crypto'
import { fstatSync, readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { ArtificialAnalysis, METRICS, withBenchmarks } from '../hosts/lib/benchmarks.js'
import { Bridge } from './bridge.js'
import { agentProfile, CATALOG, EFFORTS } from './catalog.js'
import { Watcher } from './delivery-watch.js'
import { HarnessAdmin } from './harness-admin.js'
import { harnessPage } from './harness-page.js'
import { harnessPath } from './harnesses.js'
import { Inbox } from './inbox.js'
import { uninstallSkills } from './install.js'
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
  refreshAgentProfiles,
  removeAgent,
  syncAgents,
} from './roster.js'
import { agentCommand } from './skill.js'
import { Store, StoreRefusal } from './store.js'
import { healOnOpen, refreshInstalledSkill } from './sync.js'
import { leadIdentity, Tabs } from './tabs.js'

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

/** The line this agent becomes in the skill — shown verbatim in the UI. */
function withCommand(agent, benchmarks) {
  const command = agentCommand(agent)
  return {
    ...agent,
    profile: withBenchmarks(agent, agentProfile(agent), benchmarks),
    ...(command === undefined ? {} : { command }),
  }
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
    'result.body': (body) => page.resultBody(body),
    'result.collect': (body) => page.collectResult(body),
    'result.cancel': (body) => page.cancelResult(body),
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
  const harnessAdmin = new HarnessAdmin(env, { latest: harnessLatest })
  const artificialAnalysis = new ArtificialAnalysis(configRoot(env))
  // The app is an entry point too: a machine from before the merge should not
  // have to run the CLI once to be tidied up.
  migrateStateRoot(env)
  refreshAgentProfiles(env)

  const store = new Store(configRoot(env))
  const tabs = new Tabs(store)
  const inbox = new Inbox({ store, tabs, env })
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
  // One scanner indexes complete answers and reconciles native receipts. Receivers collect them.
  const watcher = new Watcher({ store, tabs, env })
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
      if (request.method === 'POST' && url.pathname.startsWith('/api/receiver/')) {
        if (!scoped?.ops.includes('receiver')) return send(403, { error: 'forbidden' })
        await storeReady()
        try {
          return send(
            200,
            await inbox.receive(url.pathname.slice('/api/receiver/'.length), scoped, body),
          )
        } catch (cause) {
          return send(cause.status ?? 500, { error: cause.message })
        }
      }
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
      if (request.method === 'GET' && url.pathname === '/library') {
        return send(200, PAGE(token, true), 'text/html; charset=utf-8')
      }
      if (request.method === 'GET' && url.pathname === '/harnesses') {
        return send(200, harnessPage(token), 'text/html; charset=utf-8')
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
        const benchmarks = await artificialAnalysis.refresh()
        refreshAgentProfiles(env, benchmarks)
        return send(200, {
          agents: listAgents(env).map((agent) => withCommand(agent, benchmarks)),
          drift: agentDrift(env),
          harnesss: HARNESSES,
          catalog: Object.fromEntries(
            Object.entries(CATALOG).map(([harness, entries]) => [
              harness,
              entries.map((entry) => withCommand({ ...entry, harness }, benchmarks)),
            ]),
          ),
          efforts: EFFORTS,
          benchmarks: {
            status: benchmarks.status,
            tier: benchmarks.tier,
            fetchedAt: benchmarks.fetchedAt,
            indexVersion: benchmarks.indexVersion,
            metrics: METRICS,
          },
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
     * Drain the scanner and admitted store mutations before closing the server.
     * Native receivers own insertion; shutdown never submits or retries a result.
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
  // Opening the app prepares its launcher and private role context.
  healOnOpen(env)
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

const BROWSING_CONTROLS = `
  <div class="filters">
    <label class="filter-search">Search agents<input type="search" placeholder="Name, model, harness or task…" autocomplete="off"></label>
    <label>Category<select name="category" aria-label="Category"><option value="all">All categories</option><option value="coding">Coding</option><option value="lead">Recommended lead</option><option value="pm">Recommended PM</option><option value="reviewer">Reviewer / second opinion</option><option value="images">Images</option></select></label>
    <label>Group by<select name="group" aria-label="Group by"><option value="none">None</option><option value="harness">Harness</option><option value="model-reasoning" selected>Model and reasoning</option></select></label>
    <label>Sort by<select name="sort" aria-label="Sort by"><option value="default">Model and reasoning</option></select></label>
    <button type="button">Clear filters</button>
  </div>
  <p class="benchmark-source"></p>
  <details class="benchmark-guide"><summary>About benchmark scores</summary><div></div></details>`

const PAGE = (token, library = false) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ConsensFlow — ${library ? 'Agent library' : 'Your agents'}</title>
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
    --pill-coding: #63C7B2; --pill-lead: #ABC9F1; --pill-pm: #DDC6EF; --pill-images: #EAC58B;
    --ui: Archivo, "Helvetica Neue", system-ui, sans-serif;
    --mono: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --ink: #E9F1EF; --panel: #FFFFFF; --line: #C9DAD8; --foam: #0C1E23;
      --muted: #52717A; --accent-text: #16766A; --buoy: #C2402F;
      --pill-coding: #176B5F; --pill-lead: #285A9C; --pill-pm: #734A91; --pill-images: #835D15;
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
  .filters input[type=search] {
    width: 100%; box-sizing: border-box; padding: 8px 10px;
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
    color: var(--foam); font: inherit; font-size: 13px;
  }
  .filters input[type=search]:focus-visible { outline: 2px solid var(--seafoam); outline-offset: 1px; }

  form { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 14px; }
  input, select {
    font: inherit; font-size: 13px; padding: 7px 10px; color: var(--foam);
    background: var(--panel); border: 1px solid var(--line); border-radius: 4px;
  }
  input::placeholder { color: var(--muted); }
  .full { grid-column: 1 / -1; }
  .alert { color: var(--buoy); font-size: 13px; margin: 0; }
  .note { font-size: 13px; color: var(--muted); min-height: 20px; margin: 10px 0 0; }
  .empty { color: var(--muted); font-size: 13.5px; border: 1px dashed var(--line); border-radius: 4px; padding: 18px; }
  @media (max-width: 620px) { form { grid-template-columns: 1fr; } .offer { flex-wrap: wrap; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }

  h1 { display: flex; flex-wrap: wrap; align-items: baseline; gap: 12px; }
  h3 { font-weight: 500; }
  .section-count { font: 12px var(--mono); color: var(--muted); margin-left: auto; }
  .filters { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 24px 0 32px; }
  .filters label { display: grid; gap: 5px; color: var(--muted); font-size: 12px; }
  .filters .filter-search { flex: 1 1 100%; }
  .filters input, .filters select { width: 100%; min-width: 0; }
  .filters button { padding: 7px 10px; }
  .offer { align-items: start; }
  .offer__what { min-width: 0; overflow-wrap: anywhere; }
  .offer__what p { margin: 4px 0 0; }
  .category-pills { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 6px 0; }
  .category-pill { border: 1px solid currentColor; border-radius: 999px; padding: 2px 8px; font-size: 11px; line-height: 1.4; white-space: nowrap; color: var(--pill-coding); background: var(--panel); }
  .category-pill[data-category=lead] { color: var(--pill-lead); }
  .category-pill[data-category=pm] { color: var(--pill-pm); }
  .category-pill[data-category=reviewer] { color: var(--foam); }
  .category-pill[data-category=images] { color: var(--pill-images); }
  .benchmark-source, .benchmark-guide, .benchmark-details, .benchmark-missing, .benchmark-context { font-size: 12px; color: var(--muted); }
  .benchmark-source { margin: -16px 0 4px; }
  .benchmark-guide { margin: 0 0 22px; }
  .benchmark-pills { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 8px 0; }
  .benchmark-pill { color: var(--foam); background: var(--panel); border: 1px solid var(--line); border-radius: 999px; padding: 3px 8px; font: 11px/1.4 var(--mono); max-width: 100%; }
  .benchmark-pill[data-selected=true] { border-color: var(--accent-text); }
  .benchmark-details summary, .benchmark-guide summary { cursor: pointer; width: fit-content; color: var(--accent-text); }
  .benchmark-details p, .benchmark-guide p { margin: 8px 0; overflow-wrap: anywhere; }
  .benchmark-details a, .benchmark-source a { color: var(--accent-text); }
  .benchmark-guide dt, .benchmark-details dt { margin-top: 8px; color: var(--foam); font-weight: 600; }
  .benchmark-guide dd, .benchmark-details dd { margin: 2px 0 10px; }
  .model-group { border: 1px solid var(--line); border-radius: 8px; padding: 16px; margin: 16px 0; }
  .model-summary { padding-bottom: 14px; overflow-wrap: anywhere; }
  .model-summary h3 { color: var(--foam); font-size: 18px; font-weight: 600; margin: 0 0 10px; }
  .model-summary .agent-focus { margin: 8px 0; }
  .model-group .offer, .model-group .member { padding: 12px 0; border-top: 1px solid var(--line); }
  .model-group .offer:last-child, .model-group .member:last-child { padding-bottom: 0; border-bottom: none; }
  .agent-focus { color: var(--foam); font-size: 13px; }
  .agent-route, .agent-route-note { color: var(--muted); font-size: 11px; }
  .member .agent-focus, .member .agent-route, .member .agent-route-note { margin: 0; }
  .offer__model { color: var(--foam); opacity: 1; display: block; overflow-wrap: anywhere; }
  .offer__actions { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; max-width: 220px; }
  .offer__actions button { overflow-wrap: anywhere; max-width: 100%; }
  .member__desc, .tag { overflow-wrap: anywhere; }
  button:disabled { opacity: .6; cursor: default; }
  @media (max-width: 620px) {
    .filters label { flex: 1 1 45%; min-width: 0; }
    .offer__name { min-width: 80px; }
    .offer__what { flex: 1 1 180px; }
    .offer__actions { margin-left: auto; max-width: 100%; }
  }
</style>
</head>
<body>
<main>
  <p class="mark"><span>consensflow</span> <span>v${VERSION}</span></p>
  ${
    library
      ? `
  <section id="catalog-section" aria-label="Agent library">
  <h1>Agent library <span id="catalog-count" class="section-count"></span></h1>
  <p class="lede lede--tight">Add an agent with its model and reasoning effort already configured.</p>
  <p id="roster-note" class="note" role="status"></p>
  ${BROWSING_CONTROLS}
  <div id="catalog"></div>
  </section>

`
      : `
  <section id="roster-section" aria-label="Your agents">
  <h1>Your agents <span id="roster-count" class="section-count"></span></h1>
  <p class="lede" id="lede">Configure the workers your lead can consult by name.</p>
  <p id="roster-note" class="note" role="status"></p>
  ${BROWSING_CONTROLS}
  <div id="roster"></div>
  </section>

  <p class="eyebrow eyebrow--section">Define your own</p>
  <form id="add">
    <input name="name" placeholder="callsign, lowercase" required>
    <select name="harness"></select>
    <input class="full" name="model" placeholder="model — anything this harness accepts" required>
    <input name="effort" list="effort-options" placeholder="effort (optional)">
    <datalist id="effort-options"></datalist>
    <button class="primary">Add agent</button>
    <p id="error" class="alert full"></p>
  </form>`
  }

</main>
<script>
const TOKEN = ${JSON.stringify(token)};
const LIBRARY = ${library};
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

const HARNESS_LABELS = { claude: 'Claude Code', codex: 'Codex', opencode: 'OpenCode', pi: 'Pi', kimi: 'Kimi', image: 'Images' };
const CATEGORY_LABELS = { coding: 'Coding', lead: 'Recommended lead', pm: 'Recommended PM', reviewer: 'Reviewer / second opinion', images: 'Images' };
const EFFORT_ORDER = ['ultra', 'max', 'xhigh', 'high', 'medium', 'low', 'minimal', 'off', 'default', 'kimi-setting', 'not-applicable'];
const effortValue = p => p.harness === 'image' ? 'not-applicable' : (p.effort || (p.harness === 'kimi' ? 'kimi-setting' : 'default'));
const effortLabel = value => value === 'not-applicable' ? 'Not applicable' : value === 'kimi-setting' ? 'Kimi setting' : EFFORT_ORDER.includes(value) ? value.charAt(0).toUpperCase() + value.slice(1) : value;
const rank = (values, value) => values.includes(value) ? values.indexOf(value) : values.length;
const compareText = (a, b) => String(a).localeCompare(String(b));
// Curated family/tier order. Provider paths affect routing and identity, not rank.
// Versions and aliases stay separate; their numeric label is only a tie-breaker.
const MODEL_ORDER = [
  /^claude-fable(?:-|$)/, /^claude-opus(?:-|$)/, /^claude-sonnet(?:-|$)/, /^claude-haiku(?:-|$)/, /^claude-/,
  /^gpt-[0-9.]+-astra(?:-|$)/, /^gpt-[0-9.]+-sol(?:-|$)/, /^gpt-[0-9.]+-terra(?:-|$)/, /^gpt-[0-9.]+-luna(?:-|$)/, /^gpt-/,
  /^gemini-[0-9.]+-pro(?:-|$)/, /^gemini-[0-9.]+-flash(?:-|$)/, /^gemini-/,
  /^deepseek-v4-pro(?:-|$)/, /^deepseek-v4-flash(?:-|$)/, /^deepseek-/,
  /^glm-[0-9.]+$/, /^glm-[0-9.]+-flash(?:-|$)/, /^glm-/,
  /^grok-/, /^kimi-/, /^laguna-/, /^minimax-/, /^muse-/, /^nemotron-/,
  /^qwen[0-9.]+-max(?:-|$)/, /^qwen[0-9.]+-27b(?:-|$)/, /^qwen[0-9]/,
  /^codex-image$/,
];
function modelRank(key) {
  const index = MODEL_ORDER.findIndex(pattern => pattern.test(key.split('/').at(-1)));
  return index < 0 ? MODEL_ORDER.length : index;
}
const compareModels = (a, b) => modelRank(a.modelKey) - modelRank(b.modelKey) ||
  b.modelLabel.localeCompare(a.modelLabel, undefined, { numeric: true }) || compareText(a.modelKey, b.modelKey);
const compareEffort = (a, b) => rank(EFFORT_ORDER, a) - rank(EFFORT_ORDER, b) || compareText(a, b);
const compareAgents = (a, b) => compareModels(a.profile, b.profile) ||
  compareEffort(effortValue(a), effortValue(b)) || compareText(a.name, b.name);
function compareScores(a, b, metric) {
  if (!metric) return 0;
  const av = a.profile.benchmarks?.scores[metric.id], bv = b.profile.benchmarks?.scores[metric.id];
  const ah = Number.isFinite(av), bh = Number.isFinite(bv);
  if (ah !== bh) return ah ? -1 : 1;
  return ah ? (metric.direction === 'asc' ? av - bv : bv - av) : 0;
}
const selectedMetric = () => LAST?.benchmarks?.metrics.find(m => m.id === document.querySelector('[name=sort]').value);

function browsingGroups(entries, sectionId) {
  const section = document.querySelector(sectionId);
  const needle = section.querySelector('input[type=search]').value.trim().toLowerCase();
  const category = section.querySelector('[name=category]').value;
  const by = section.querySelector('[name=group]').value;
  const metric = selectedMetric();
  const filtered = entries.filter(p => (category === 'all' || p.profile.categories.includes(category)) &&
    [p.name, p.model, p.description, p.detail, p.harness, HARNESS_LABELS[p.harness], effortLabel(effortValue(p)),
      p.profile.modelLabel, p.profile.routeLabel, p.profile.routeNote, p.profile.goodFor, ...p.profile.categories.map(c => CATEGORY_LABELS[c])]
      .filter(Boolean).join(' ').toLowerCase().includes(needle));
  section.querySelector('.section-count').textContent = filtered.length + ' of ' + entries.length + ' shown';
  const groups = new Map();
  for (const p of filtered) {
    const effort = effortValue(p);
    const key = by === 'model-reasoning' ? JSON.stringify([p.profile.modelKey, effort]) : by === 'harness' ? p.harness : '';
    const title = by === 'model-reasoning' ? p.profile.modelLabel + ' · ' + effortLabel(effort) : by === 'harness' ? (HARNESS_LABELS[key] || key) : '';
    if (!groups.has(key)) groups.set(key, { key, title, modelGroup: by === 'model-reasoning', modelKey: p.profile.modelKey, modelLabel: p.profile.modelLabel, effort, rows: [] });
    groups.get(key).rows.push(p);
  }
  for (const group of groups.values()) {
    group.rows.sort((a, b) => compareScores(a, b, metric) || compareAgents(a, b));
    group.shared = group.modelGroup ? ['categories', 'goodFor', 'benchmarks'].filter(field =>
      group.rows.every(p => JSON.stringify(p.profile[field]) === JSON.stringify(group.rows[0].profile[field]))) : [];
  }
  return [...groups.values()].sort((a, b) =>
    (by === 'harness' ? rank(Object.keys(HARNESS_LABELS), a.key) - rank(Object.keys(HARNESS_LABELS), b.key) :
      by === 'model-reasoning' ? compareScores(a.rows[0], b.rows[0], metric) || compareModels(a, b) || compareEffort(a.effort, b.effort) : 0) || compareText(a.title, b.title) || compareText(a.key, b.key));
}
function groupSection(group) {
  const section = el('section', group.modelGroup ? 'agent-group model-group' : 'agent-group');
  if (group.modelGroup) {
    const summary = el('header', 'model-summary');
    summary.append(el('h3', null, group.title + ' · ' + group.rows.length));
    appendProfile(summary, group.rows[0], group.shared);
    section.append(summary);
  } else if (group.title) section.append(el('h3', 'eyebrow eyebrow--tool', group.title + ' · ' + group.rows.length));
  return section;
}
function appendProfile(host, p, fields = ['categories', 'goodFor', 'routeLabel', 'benchmarks']) {
  if (fields.includes('categories') && p.profile.categories.length) {
    const categories = el('ul', 'category-pills');
    categories.setAttribute('aria-label', 'Categories');
    categories.setAttribute('role', 'list');
    for (const category of p.profile.categories) {
      const pill = el('li', 'category-pill', CATEGORY_LABELS[category]);
      pill.dataset.category = category;
      categories.append(pill);
    }
    host.append(categories);
  }
  if (fields.includes('goodFor')) host.append(el('p', 'agent-focus', 'Good for: ' + p.profile.goodFor));
  if (fields.includes('routeLabel')) {
    host.append(el('p', 'agent-route', p.profile.routeLabel));
    if (p.profile.routeNote) host.append(el('p', 'agent-route-note', p.profile.routeNote));
  }
  if (fields.includes('benchmarks')) appendBenchmarks(host, p);
}

const metricValue = (metric, value) => value.toFixed(1) + (metric.unit === '%' ? '%' : metric.unit === 'Elo' ? ' Elo' : '');
function benchmarkPills(metrics, scores) {
  const list = el('ul', 'benchmark-pills');
  list.setAttribute('aria-label', 'Artificial Analysis scores');
  for (const metric of metrics) {
    const pill = el('li', 'benchmark-pill', metric.label + ' ' + metricValue(metric, scores[metric.id]));
    pill.dataset.metric = metric.id;
    pill.dataset.selected = String(selectedMetric()?.id === metric.id);
    list.append(pill);
  }
  return list;
}
function appendBenchmarks(host, p) {
  const snapshot = p.profile.benchmarks;
  if (!snapshot) {
    if (LAST?.benchmarks?.fetchedAt) host.append(el('p', 'benchmark-missing', 'No AA score for this model and reasoning setting.'));
    return;
  }
  const metrics = LAST.benchmarks.metrics.filter(m => Number.isFinite(snapshot.scores[m.id]));
  const primary = m => ['intelligence', 'coding', 'agentic', selectedMetric()?.id].includes(m.id);
  if (snapshot.reasoningMatch === 'unspecified') host.append(el('p', 'benchmark-context', 'AA reasoning level not specified'));
  host.append(benchmarkPills(metrics.filter(primary), snapshot.scores));
  const details = el('details', 'benchmark-details');
  details.append(el('summary', null, 'Benchmark details · ' + metrics.length + ' scores'));
  details.append(el('p', null, 'Tested: ' + snapshot.testedModel));
  details.append(el('p', null, 'AA index v' + snapshot.indexVersion + ' · Retrieved ' + new Date(snapshot.fetchedAt).toLocaleDateString()));
  details.append(el('p', null, 'AA tests its own configuration. Results can differ with this agent’s harness and provider.'));
  const link = el('a', null, 'AA model result');
  link.href = snapshot.url; link.target = '_blank'; link.rel = 'noopener noreferrer';
  details.append(link);
  details.append(benchmarkPills(metrics.filter(m => !primary(m)), snapshot.scores));
  const definitions = el('dl');
  for (const metric of metrics) {
    definitions.append(el('dt', null, metric.label + ': ' + metricValue(metric, snapshot.scores[metric.id])));
    definitions.append(el('dd', null, metric.description));
  }
  details.append(definitions);
  host.append(details);
}
function renderBenchmarkControls(data) {
  const info = data.benchmarks;
  const rows = [...data.agents, ...Object.values(data.catalog).flat()];
  const select = document.querySelector('[name=sort]');
  const selected = select.value;
  select.replaceChildren(new Option('Model and reasoning', 'default'));
  for (const metric of info.metrics) {
    if (rows.some(row => Number.isFinite(row.profile.benchmarks?.scores[metric.id])))
      select.add(new Option(metric.label + ' · ' + (metric.direction === 'asc' ? 'lowest first' : 'highest first'), metric.id));
  }
  if ([...select.options].some(option => option.value === selected)) select.value = selected;
  const source = document.querySelector('.benchmark-source');
  source.replaceChildren();
  const link = el('a', null, 'Artificial Analysis');
  link.href = 'https://artificialanalysis.ai/'; link.target = '_blank'; link.rel = 'noopener noreferrer';
  source.append(link);
  source.append(document.createTextNode(info.fetchedAt ? ' · Index v' + info.indexVersion + ' · Updated ' + new Date(info.fetchedAt).toLocaleDateString() : ' · Scores unavailable'));
  if (info.status === 'stale') source.append(document.createTextNode(' · Refresh unavailable; showing saved scores'));
  const guide = document.querySelector('.benchmark-guide > div');
  guide.replaceChildren();
  guide.append(el('p', null, info.tier === 'free' ? 'Free access includes Intelligence, Coding and Agentic indexes. Individual benchmark scores, including hallucinations, require higher AA access.' : info.status === 'unconfigured' ? 'AA scores are not configured on this installation.' : info.status === 'unavailable' ? 'Could not retrieve AA scores. Agent browsing remains available; scores will retry later.' : 'Available scores are shown for each tested model and reasoning setting. Missing scores are not zero.'));
  guide.append(el('p', null, 'Sort by orders scored entries first, then entries without a score. Group by stays independent. Default ordering keeps the model families and descending reasoning effort. Scores refresh daily.'));
  const definitions = el('dl');
  for (const metric of info.metrics) {
    definitions.append(el('dt', null, metric.label + ' · ' + metric.unit + ' · ' + (metric.direction === 'asc' ? 'lower is better' : 'higher is better')));
    definitions.append(el('dd', null, metric.description));
  }
  guide.append(definitions);
}

function renderRoster(data) {
  const host = document.querySelector('#roster');
  const editors = new Map([...host.querySelectorAll('.member[data-agent-name]')]
    .map(card => [card.dataset.agentName, card.querySelector('form')]).filter(([, form]) => form));
  host.innerHTML = '';
  const groups = browsingGroups(data.agents, '#roster-section');
  document.querySelector('#lede').textContent = data.agents.length === 0
    ? 'Add the first worker your lead can consult by name.'
    : 'Configure the workers your lead can consult by name.';

  if (data.agents.length === 0) {
    host.appendChild(el('p', 'empty', 'No agents yet. Add one from Agent library, or define your own below.'));
    return;
  }
  if (groups.length === 0) { host.append(el('p', 'empty', 'No agents match these filters.')); return; }
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
  for (const group of groups) {
    const section = groupSection(group);
    host.append(section);
    for (const p of group.rows) {
    const card = el('div', 'member');
    card.dataset.agentName = p.name;
    const head = el('div', 'member__head');
    head.append(el('span', 'callsign', p.name));
    head.append(el('span', 'tag', group.modelGroup ? (HARNESS_LABELS[p.harness] || p.harness) : p.profile.modelLabel + ' · ' + (HARNESS_LABELS[p.harness] || p.harness) + ' · ' + effortLabel(effortValue(p))));
    head.append(el('span', 'spacer'));
    const edit = el('button', null, 'Edit');
    edit.onclick = () => openEditor(card, p);
    head.append(edit);
    head.append(removeButton(p, 'Remove'));
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
    appendProfile(card, p, ['categories', 'goodFor', 'routeLabel', 'benchmarks'].filter(field => !group.shared.includes(field)));
    if (p.description) card.append(el('p', 'member__desc', p.description));
    if (p.command) card.append(renderCommand(p));
    else card.append(el('p', 'member__desc', p.harness + ' agents are not run by this tool — it leaves them alone.'));
    if (editors.has(p.name)) card.append(editors.get(p.name));
    section.append(card);
    }
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
  for (const [name, value, placeholder] of fields.filter(([name]) => agent.harness !== 'image' || name === 'description')) {
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
    if (res.ok) { form.remove(); load(); }
    else document.querySelector('#roster-note').textContent = (await res.json()).error;
  };
  card.append(form);
}

const pendingRemovals = new Set();
function removeButton(agent, label) {
  const button = el('button', 'danger', pendingRemovals.has(agent.name) ? 'Removing…' : label);
  button.dataset.removeAgent = agent.name;
  button.disabled = pendingRemovals.has(agent.name);
  button.onclick = async () => {
    if (pendingRemovals.has(agent.name)) return;
    pendingRemovals.add(agent.name);
    for (const action of document.querySelectorAll('[data-remove-agent]')) {
      if (action.dataset.removeAgent !== agent.name) continue;
      action.disabled = true;
      action.textContent = 'Removing…';
    }
    const status = document.querySelector('#roster-note');
    status.textContent = '';
    try {
      const response = await fetch('/api/agents/' + encodeURIComponent(agent.name), { method: 'DELETE', headers });
      if (!response.ok) throw new Error((await response.json()).error || 'Could not remove agent');
    } catch (error) {
      status.textContent = error.message || 'Could not remove agent';
    } finally {
      pendingRemovals.delete(agent.name);
      try { await load(); } catch {
        renderLists();
        if (!status.textContent) status.textContent = 'Could not refresh agents. Reopen this screen to try again.';
      }
    }
  };
  return button;
}

const pendingAdds = new Set();
function catalogMatches(entry, agents) {
  return agents.filter(p => p.preset === entry.preset || (!p.preset && p.name === entry.name &&
    p.harness === entry.harness && p.model === entry.model && (p.effort || '') === (entry.effort || '')));
}
function catalogState(entry, agents) {
  if (pendingAdds.has(entry.preset)) return 'Adding…';
  if (catalogMatches(entry, agents).length > 0) return 'Already added';
  return agents.some(p => p.name === entry.name) ? 'Name in use' : 'Add';
}
function renderCatalog(data) {
  const host = document.querySelector('#catalog');
  host.innerHTML = '';
  const entries = Object.entries(data.catalog).flatMap(([harness, entries]) => entries.map(p => ({ ...p, harness })));
  const groups = browsingGroups(entries, '#catalog-section');
  for (const group of groups) {
    const section = groupSection(group);
    for (const entry of group.rows) {
      const row = el('div', 'offer');
      row.append(el('span', 'offer__name', entry.name));
      const what = el('div', 'offer__what');
      what.append(el('span', 'offer__model', group.modelGroup ? (HARNESS_LABELS[entry.harness] || entry.harness) : entry.profile.modelLabel + ' · ' + (HARNESS_LABELS[entry.harness] || entry.harness) + ' · ' + effortLabel(effortValue(entry))));
      appendProfile(what, entry, ['categories', 'goodFor', 'routeLabel', 'benchmarks'].filter(field => !group.shared.includes(field)));
      row.append(what);
      const state = catalogState(entry, data.agents);
      const add = el('button', null, state);
      add.disabled = state !== 'Add';
      add.onclick = async () => {
        if (pendingAdds.has(entry.preset)) return;
        pendingAdds.add(entry.preset);
        add.disabled = true;
        add.textContent = 'Adding…';
        const status = document.querySelector('#roster-note');
        status.textContent = '';
        try {
          const response = await fetch('/api/agents', {
            method: 'POST', headers,
            body: JSON.stringify({ name: entry.name, harness: entry.harness, model: entry.model,
              ...(entry.effort ? { effort: entry.effort } : {}), description: entry.description, preset: entry.preset }),
          });
          const result = await response.json();
          if (!response.ok) throw new Error(result.error || 'Could not add agent');
        } catch (error) {
          status.textContent = error.message || 'Could not add agent';
        } finally {
          pendingAdds.delete(entry.preset);
          try { await load(); } catch {
            renderLists();
            if (!status.textContent) status.textContent = 'Could not refresh agents. Reopen Agents to try again.';
          }
        }
      };
      const actions = el('div', 'offer__actions');
      actions.append(add);
      const matches = catalogMatches(entry, data.agents);
      for (const agent of matches) {
        actions.append(removeButton(agent, matches.length === 1 && agent.name === entry.name ? 'Remove' : 'Remove ' + agent.name));
      }
      row.append(actions);
      section.append(row);
    }
    host.append(section);
  }
  if (groups.length === 0) host.append(el('p', 'empty', 'No ready-made agents match these filters.'));
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
  const model = document.querySelector('#add [name=model]');
  const effort = document.querySelector('#add [name=effort]');
  if (harness === 'image') {
    if (!model.hidden) model.dataset.previous = model.value;
    model.value = 'codex-image';
  } else if (model.hidden) {
    model.value = model.dataset.previous || '';
    delete model.dataset.previous;
  }
  model.hidden = harness === 'image';
  effort.hidden = harness === 'image';
  effort.disabled = harness === 'image';
  const list = document.querySelector('#effort-options');
  list.innerHTML = '';
  for (const e of efforts[harness] ?? []) list.appendChild(new Option(e, e));
}

async function post(path, body, note) {
  const status = document.querySelector('#roster-note');
  status.textContent = note;
  const res = await fetch(path, { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  if (!res.ok) { status.textContent = data.error; return; }
  status.textContent = data.applied.length === 0
    ? 'already up to date'
    : data.applied.map((a) => a.name + ': ' + a.changes
        .map((c) => c.field + ' → ' + (c.to ?? '-')).join(', ')).join(' · ');
  load();
}

function renderLists() { if (LAST !== null) (LIBRARY ? renderCatalog : renderRoster)(LAST); }
for (const [id, render] of [['#roster-section', renderRoster], ['#catalog-section', renderCatalog]]) {
  const filters = document.querySelector(id + ' .filters');
  if (!filters) continue;
  const refresh = () => { if (LAST !== null) render(LAST); };
  filters.querySelector('input').addEventListener('input', refresh);
  for (const select of filters.querySelectorAll('select')) select.addEventListener('change', refresh);
  filters.querySelector('button').onclick = () => {
    filters.querySelector('input').value = '';
    filters.querySelector('[name=category]').value = 'all';
    filters.querySelector('[name=group]').value = 'model-reasoning';
    filters.querySelector('[name=sort]').value = 'default';
    refresh();
  };
}

// The last roster payload, so filtering the catalog re-renders without a fetch.
let LAST = null;

async function load() {
  const response = await fetch('/api/agents', { headers });
  if (!response.ok) throw new Error('Could not refresh agents. Reopen this screen to try again.');
  LAST = await response.json();
  renderBenchmarkControls(LAST);
  renderLists();
  if (!LIBRARY) renderForm(LAST);
}

if (!LIBRARY) document.querySelector('#add').onsubmit = async (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = Object.fromEntries([...form.entries()].filter(([, v]) => v !== ''));
  const res = await fetch('/api/agents', { method: 'POST', headers, body: JSON.stringify(body) });
  const data = await res.json();
  document.querySelector('#error').textContent = res.ok ? '' : data.error;
  if (res.ok) { event.target.reset(); load(); }
};
function refreshAgents() {
  load().catch(error => { document.querySelector('#roster-note').textContent = error.message; });
}
window.addEventListener('message', event => {
  if (event.source === window.parent && event.data === 'consensflow:refresh-agents') refreshAgents();
});
refreshAgents();
</script>
</body>
</html>
`
