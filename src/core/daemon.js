import { randomBytes } from 'node:crypto'
import { mkdirSync } from 'node:fs'
import { delimiter, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createAdapters } from '../adapters/index.js'
import { Bridge } from '../bridge.js'
import { openLedger } from '../ledger/index.js'
import { agentRow, configRoot, listAgents, syncAgents } from '../roster.js'
import { agentsUi } from './agents-server.js'
import { Credentials, startApi } from './api.js'
import { Dispatcher } from './dispatcher.js'
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
  const ledger = openLedger(join(home, 'consensflow.db'), { trace })
  ledger.suspendForRestart()
  // What the app ships is what the roster and the teams have: a catalog
  // entry that moved reaches its saved agents, and every member's tier is
  // read again from its agent, at start and after any change to the agents.
  const followCatalog = () => {
    syncAgents(env)
    const agents = listAgents(env)
    return ledger.refreshMemberTiers(
      (name) => agents.find((agent) => agent.name === name)?.profile.workTier ?? null,
    )
  }
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
  onOut(JSON.stringify({ url: `${api.url}/`, token }))

  let stopping = null
  const stop = () => {
    stopping ??= (async () => {
      await loop?.stop()
      await api.close()
      ledger.close()
      exit(0)
    })()
    return stopping
  }
  const bridge = new Bridge({ input, output, onFatal: stop })
  const host = new PaneHost(bridge)
  const dispatcher = new Dispatcher({
    ledger,
    host,
    adapters: createAdapters(env, { peer }),
    credentials,
    roster: (agent) => agentRow(agent, env) ?? null,
    roles: (participant, project) => roleInstructions(participant.role, teamOf(project)),
    trace,
    paneEnv: (participant, project) => ({
      CONSENSFLOW_URL: api.url,
      CONSENSFLOW_PROJECT: String(project.id),
      CONSENSFLOW_PARTICIPANT: participant.handle,
      CONSENSFLOW_NODE: process.execPath,
      PATH: env.PATH ? `${BUNDLE_BIN}${delimiter}${env.PATH}` : BUNDLE_BIN,
    }),
  })
  loop = passLoop(() => dispatcher.pass())
  dispatcher.onChange(
    throttle(() => bridge.event('state.changed', { reason: 'core' }), STATE_EVENT_MS),
  )
  for (const [operation, handle] of Object.entries(
    pageOperations({ ledger, dispatcher, env, kick: () => loop.kick() }),
  )) {
    bridge.on(operation, async (body) => ({ ok: true, ...(await handle(body ?? {})) }))
  }
  bridge.on('ping', () => ({ ok: true }))

  input.on('end', stop)
  input.on('close', stop)
  input.resume()
  dispatcher
    .resumeAfterRestart()
    .then((outcomes) => {
      for (const outcome of outcomes) {
        if (!outcome.resumed) console.error(`consensflow resume ${outcome.project}:`, outcome.error)
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

/** Runs `work` on a timer and on demand, never two at once; a kick during a run runs it again after. */
function passLoop(work) {
  let running = null
  let again = false
  let stopped = false
  const run = () => {
    if (stopped) return running
    if (running !== null) {
      again = true
      return running
    }
    running = (async () => {
      try {
        await work()
      } catch (cause) {
        console.error('consensflow dispatcher:', cause)
      } finally {
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
  return {
    kick: () => setImmediate(run),
    stop: async () => {
      stopped = true
      clearInterval(timer)
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
