import { mkdirSync } from 'node:fs'
import { delimiter, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createAdapters } from '../adapters/index.js'
import { Bridge } from '../bridge.js'
import { openLedger } from '../ledger/index.js'
import { agentRow, configRoot, listAgents } from '../roster.js'
import { Credentials, startApi } from './api.js'
import { Dispatcher } from './dispatcher.js'
import { pageOperations } from './page.js'
import { PaneHost } from './pane-host.js'
import { roleInstructions } from './roles.js'

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
  const ledger = openLedger(join(home, 'consensflow.db'))
  ledger.suspendForRestart()

  const credentials = new Credentials()
  let loop = null
  const api = await startApi({
    ledger,
    credentials,
    changed: () => loop?.kick(),
    roster: (agent) => agentRow(agent, env) ?? null,
  })
  onOut(JSON.stringify({ url: `${api.url}/`, token: null }))

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
    roles: (participant, project) =>
      roleInstructions(participant.role, teamOf(project, participant, env)),
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

/** The agents a coordinator chooses from: the lead's workers and reviewers, the PM's advisors. */
function teamOf(project, participant, env) {
  const roles = { lead: ['worker', 'reviewer'], pm: ['advisor'] }[participant.role] ?? []
  const members = new Set(
    project.participants
      .filter((member) => roles.includes(member.role))
      .map((member) => member.agent),
  )
  return listAgents(env).filter((agent) => members.has(agent.name))
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
