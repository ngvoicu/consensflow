import { randomBytes } from 'node:crypto'
import { mkdirSync } from 'node:fs'
import { delimiter, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createAdapters } from '../adapters/index.js'
import { Bridge } from '../bridge.js'
import { openLedger } from '../ledger/index.js'
import { agentRow, configRoot, listAgents, normalizeRoster } from '../roster.js'
import { agentsUi } from './agents-server.js'
import { Credentials, startApi } from './api.js'
import { Dispatcher } from './dispatcher.js'
import { forgetLaunch, sweepLaunches } from './launch-files.js'
import { daemonLog } from './log.js'
import { pageOperations } from './page.js'
import { PaneHost } from './pane-host.js'
import { roleInstructions } from './roles.js'
import { eventTrace } from './trace.js'

/**
 * The new core's daemon: the one process that owns ConsensFlow's state.
 *
 * It opens the ledger (whose lock refuses a second daemon on the same home),
 * marks the projects that were open for a resume, starts the agents' API,
 * prints its handle line for the app, then speaks the bridge on stdin/stdout:
 * the pane host's requests and events come in, pane operations go out, and the
 * page's requests are answered here. The dispatcher runs once a second and
 * whenever something changes. Closing stdin ends it.
 */

const BUNDLE_BIN = fileURLToPath(new URL('../../bin', import.meta.url))
const PASS_MS = 1000
const STATE_EVENT_MS = 100
/** A pass this long is worth a line in the log. */
const SLOW_PASS_MS = 5_000
/** How often the daemon writes down that it is alive, and how big it is. */
const HEARTBEAT_MS = 10 * 60_000

export async function startCore(
  env,
  {
    input = process.stdin,
    output = process.stdout,
    onOut = (line) => output.write(`${line}\n`),
    peer,
    exit = (code) => process.exit(code),
  } = {},
) {
  const home = configRoot(env)
  mkdirSync(home, { recursive: true })
  // Every event and every change of a window goes to <home>/events.jsonl as
  // it happens, for whoever watches the daemon from outside.
  const trace = eventTrace(home)
  // What is worth knowing afterwards goes to <home>/daemon.log: the start,
  // the stop and why, a pass that failed or ran long, an error nobody caught.
  const log = daemonLog(home)
  log.info(`start pid ${process.pid} node ${process.version} home ${home}`)
  const uncaught = (kind) => (error) => {
    log.error(kind, error)
    trace({ kind: 'daemon.error', project: null, reason: `${kind}: ${error?.message ?? error}` })
  }
  process.on('uncaughtException', uncaught('uncaught exception'))
  process.on('unhandledRejection', uncaught('unhandled rejection'))
  process.on('exit', (code) => log.info(`exit ${code}`))
  // No window survives a restart: what every launch left in the home goes.
  const swept = sweepLaunches(home)
  if (swept > 0) log.info(`swept ${swept} launch folder${swept === 1 ? '' : 's'}`)
  const ledger = openLedger(join(home, 'consensflow.db'), { trace })
  ledger.suspendForRestart()
  // What the app ships is what the roster and the teams have: the roster is
  // the catalog plus the human's own agents, and every
  // member's tier is read again from its agent, at start and after any
  // change to the agents.
  const followCatalog = () => {
    const agents = listAgents(env)
    return ledger.refreshMemberTiers(
      (name) => agents.find((agent) => agent.name === name)?.profile.workTier ?? null,
    )
  }
  normalizeRoster(env)
  followCatalog()

  const credentials = new Credentials()
  let loop = null
  // The app's own token: it opens the human's agents screens and nothing else.
  const token = randomBytes(24).toString('hex')
  const api = await startApi({
    ledger,
    credentials,
    changed: () => loop?.kick(),
    roster: (agent) => agentRow(agent, env) ?? null,
    ui: agentsUi(env, {
      token,
      onRosterChange: () => {
        if (followCatalog().length > 0) bridge.event('state.changed', { reason: 'roster' })
        loop?.kick()
      },
    }),
  })
  let stopping = null
  const stop = (why = 'asked to stop') => {
    stopping ??= (async () => {
      const memory = process.memoryUsage()
      log.info(`stop: ${why}; rss ${Math.round(memory.rss / 1_048_576)} MB`)
      await loop?.stop()
      await api.close()
      ledger.close()
      exit(0)
    })()
    return stopping
  }
  // The handle line says the daemon is ready; whatever comes after it, a quit
  // included, finds its handler already in place.
  input.on('end', () => stop('stdin ended'))
  input.on('close', () => stop('stdin closed'))
  for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => stop(signal))
  onOut(JSON.stringify({ url: `${api.url}/`, token }))

  const bridge = new Bridge({
    input,
    output,
    onFatal: (cause) => {
      log.error('the bridge failed', cause)
      return stop('the bridge failed')
    },
  })
  const host = new PaneHost(bridge)
  const dispatcher = new Dispatcher({
    ledger,
    host,
    adapters: createAdapters(env, { peer }),
    credentials,
    roster: (agent) => agentRow(agent, env) ?? null,
    roles: (participant, project) => roleInstructions(participant.role, teamOf(project)),
    trace,
    launchFiles: { forget: (launch) => forgetLaunch(home, launch) },
    paneEnv: (participant, project) => ({
      CONSENSFLOW_URL: api.url,
      CONSENSFLOW_PROJECT: String(project.id),
      CONSENSFLOW_PARTICIPANT: participant.handle,
      CONSENSFLOW_NODE: process.execPath,
      PATH: env.PATH ? `${BUNDLE_BIN}${delimiter}${env.PATH}` : BUNDLE_BIN,
    }),
  })
  loop = passLoop(() => dispatcher.pass(), log)
  dispatcher.onChange(
    throttle(() => bridge.event('state.changed', { reason: 'core' }), STATE_EVENT_MS),
  )
  for (const [operation, handle] of Object.entries(
    pageOperations({ ledger, dispatcher, env, kick: () => loop.kick() }),
  )) {
    bridge.on(operation, async (body) => ({ ok: true, ...(await handle(body ?? {})) }))
  }
  bridge.on('ping', () => ({ ok: true }))

  input.resume()
  dispatcher
    .resumeAfterRestart()
    .then((outcomes) => {
      for (const outcome of outcomes) {
        if (!outcome.resumed) {
          console.error(`consensflow resume ${outcome.project}:`, outcome.error)
          log.error(`resume of project ${outcome.project} failed`, outcome.error)
        }
      }
    })
    .finally(() => loop.kick())
  return { stop }
}

/** The team as the lead's text lists it: each member's name, roles and tier. */
function teamOf(project) {
  return project.participants
    .filter((member) => member.agent !== null && member.memberId === null)
    .map((member) => ({ name: member.handle, roles: member.roles, workTier: member.tier }))
}

/**
 * Runs `work` on a timer and on demand, never two at once; a kick during a
 * run runs it again after. A pass that fails or runs long goes to the log,
 * and every ten minutes a line says the daemon is alive, how big it is and
 * how its passes have been.
 */
export function passLoop(work, log = null) {
  let running = null
  let again = false
  let stopped = false
  let passes = 0
  let slowest = 0
  const run = () => {
    if (stopped) return running
    if (running !== null) {
      again = true
      return running
    }
    running = (async () => {
      const started = Date.now()
      try {
        await work()
      } catch (cause) {
        console.error('consensflow dispatcher:', cause)
        log?.error('a pass failed', cause)
      } finally {
        const took = Date.now() - started
        passes += 1
        slowest = Math.max(slowest, took)
        if (took > SLOW_PASS_MS) log?.warn(`a pass took ${took} ms`)
        running = null
        if (again && !stopped) {
          again = false
          setImmediate(run)
        }
      }
    })()
    return running
  }
  const timer = setInterval(run, PASS_MS)
  const heartbeat = setInterval(() => {
    const memory = process.memoryUsage()
    log?.info(
      `alive: ${passes} passes, slowest ${slowest} ms, rss ${Math.round(memory.rss / 1_048_576)} MB, heap ${Math.round(memory.heapUsed / 1_048_576)} MB`,
    )
    passes = 0
    slowest = 0
  }, HEARTBEAT_MS)
  return {
    kick: () => setImmediate(run),
    stop: async () => {
      stopped = true
      clearInterval(timer)
      clearInterval(heartbeat)
      await running
    },
  }
}

function throttle(fn, ms) {
  let pending = false
  return () => {
    if (pending) return
    pending = true
    setTimeout(() => {
      pending = false
      fn()
    }, ms)
  }
}
