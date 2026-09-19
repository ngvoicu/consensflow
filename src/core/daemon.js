import { mkdirSync } from 'node:fs'
import { basename, delimiter, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createAdapters } from '../adapters/index.js'
import { Bridge } from '../bridge.js'
import { openLedger } from '../ledger/index.js'
import { agentRow, configRoot } from '../roster.js'
import { Credentials, startApi } from './api.js'
import { Dispatcher } from './dispatcher.js'
import { PaneHost } from './pane-host.js'

/**
 * The new core's daemon: the one process that owns ConsensFlow's state.
 *
 * It opens the ledger (whose lock refuses a second daemon on the same home),
 * marks the sessions that were open for a resume, starts the agents' API,
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
  const api = await startApi({ ledger, credentials, changed: () => loop?.kick() })
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
    paneEnv: (participant, session) => ({
      CONSENSFLOW_URL: api.url,
      CONSENSFLOW_SESSION: String(session.id),
      CONSENSFLOW_PARTICIPANT: participant.handle,
      CONSENSFLOW_NODE: process.execPath,
      PATH: env.PATH ? `${BUNDLE_BIN}${delimiter}${env.PATH}` : BUNDLE_BIN,
    }),
  })
  loop = passLoop(() => dispatcher.pass())
  dispatcher.onChange(
    throttle(() => bridge.event('state.changed', { reason: 'core' }), STATE_EVENT_MS),
  )
  pageOperations(bridge, { ledger, dispatcher, env, kick: () => loop.kick() })
  bridge.on('ping', () => ({ ok: true }))

  input.on('end', stop)
  input.on('close', stop)
  input.resume()
  dispatcher
    .resumeAfterRestart()
    .then((outcomes) => {
      for (const outcome of outcomes) {
        if (!outcome.resumed) console.error(`consensflow resume ${outcome.session}:`, outcome.error)
      }
    })
    .finally(() => loop.kick())
  return { stop }
}

/** What the page (and the tests standing in for it) may ask of the core. */
function pageOperations(bridge, { ledger, dispatcher, env, kick }) {
  const answer = (work) => async (body) => {
    const value = await work(body ?? {})
    kick()
    return { ok: true, ...value }
  }
  bridge.on(
    'session.open',
    answer(async ({ directory, name, harness }) => ({
      session: await dispatcher.openSession({
        directory,
        name: name ?? basename(directory),
        harness,
      }),
    })),
  )
  bridge.on(
    'session.resume',
    answer(async ({ session }) => ({ session: await dispatcher.resumeSession(session) })),
  )
  bridge.on(
    'sessions.list',
    answer(async () => ({ sessions: ledger.sessions() })),
  )
  bridge.on(
    'member.add',
    answer(async ({ session, agent, role = 'worker' }) => {
      const row = agentRow(agent, env)
      if (!row) throw new Error(`no agent named ${agent} in your agents`)
      return { member: ledger.addMember(session, { agent, harness: row.kind, role }) }
    }),
  )
  bridge.on(
    'task.add',
    answer(async ({ session, to, body, title }) =>
      ledger.createTask(session, { from: 'human', to, body, title }),
    ),
  )
  bridge.on(
    'board.get',
    answer(async ({ session }) => {
      const board = ledger.board(session)
      return {
        board: {
          ...board,
          lanes: board.lanes.map((lane) => ({
            ...lane,
            activity: dispatcher.activity(lane.participant.id),
            pane: dispatcher.pane(lane.participant.id),
          })),
        },
      }
    }),
  )
  bridge.on(
    'inbox.get',
    answer(async ({ session, participant = 'human' }) => {
      const owner = ledger.session(session)?.participants.find((p) => p.handle === participant)
      if (owner === undefined) throw new Error(`${participant} is not in session ${session}`)
      return { messages: ledger.inbox(owner.id) }
    }),
  )
  bridge.on(
    'message.read',
    answer(async ({ message }) => ({ message: ledger.markRead(message) })),
  )
  bridge.on(
    'message.answer',
    answer(async ({ question, body }) => ({
      message: ledger.answer(question, { from: 'human', body }),
    })),
  )
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
