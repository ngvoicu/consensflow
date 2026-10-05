/**
 * What Codex's scenarios are made of (`codex.mjs`, `codex-roles.mjs`,
 * `codex-looks.mjs`, `codex-deliveries.mjs`): the launch they prepare, the
 * stand-in Codex that answers what a launch asks of it, the app-server and
 * the broker as the scenarios script them, and the rollout Codex keeps of a
 * thread.
 */
import { LAUNCH } from './claude.mjs'

export { LAUNCH }

/** A second launch's id. */
export const SECOND = '1b2c3d4e-5f60-4172-8b9c-0d1e2f3a4b5c'
/** The thread a resumed window opens on, and the one a /new or /resume leaves it on. */
export const THREAD = '0f8fad5b-d9cb-469f-a165-70867728950e'
export const OTHER = '7c9e6679-7425-40de-944b-e07fc1f90ae7'

export const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  PATH: '$ROOT/bin',
}

export const worker = {
  id: 3,
  projectId: 1,
  handle: 'diana',
  role: 'worker',
  agent: 'diana',
  harness: 'codex',
}
export const chief = { ...worker, handle: 'chief', role: 'chief', agent: null }
const TASK = '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser'
/** A text with what a window must not be given in it. */
export const MESSY = 'Hello\r\nworld\u001b[31m red\u001b[0m 50%\r60%\u0085'

/** A launch to prepare, `fields` over a worker's. */
export const launch = (fields = {}) => ({
  launchId: LAUNCH,
  participant: worker,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: '/work/app',
  resume: null,
  message: TASK,
  agent: { id: 'diana', kind: 'codex', model: 'gpt-5.6-luna', effort: 'low' },
  ...fields,
})
/** A launch of the chief: no model of its own, no first message. */
export const ofChief = (fields = {}) =>
  launch({ role: 'chief', participant: chief, agent: null, message: null, ...fields })

/** What the stand-in's `queue --help` says of a Codex that has the native queue. */
export const QUEUE_HELP = 'Usage: codex queue --thread <id> --message <text>\n'

/**
 * Codex installed: a stand-in that answers the three things a launch runs of
 * it (its native queue, its version, and the MCP servers a member's window
 * switches off: none), `says` over them. The app-server is the scenario's
 * (`appServer`): it is spawned, not run.
 */
export const codex = (says = {}) => ({
  executable: 'codex',
  says: {
    'queue --help': { stdout: QUEUE_HELP },
    '--version': { stdout: 'codex-cli 0.150.0\n' },
    'mcp list --json': { stdout: '[]\n' },
    ...says,
  },
})
/** The MCP servers the stand-in lists, as `codex mcp list --json` writes them. */
export const lists = (...servers) => ({
  'mcp list --json': { stdout: `${JSON.stringify(servers)}\n` },
})

/**
 * Codex's app-server as it answers the role's dialogue: initialized, and its
 * configuration read, holding `instructions` as the developer's. It goes on
 * until it is asked to end.
 */
export const appServer = (instructions = '') => ({
  lines: [
    '{"id":1,"result":{}}',
    JSON.stringify({ id: 2, result: { config: { developer_instructions: instructions } } }),
  ],
  ends: 'asked',
})
/** The step that prepares `fields`, with the app-servers it starts. */
export const prepares = (fields, ...servers) => ({
  prepare: launch(fields),
  children: { codex: servers },
})

/** What the broker answers to the question what the window shows. */
export const session = (thread, { available = true, launchId = LAUNCH } = {}) => ({
  status: 200,
  body: JSON.stringify({ launchId, sessionId: thread, available }),
})
/** The broker is asked once, and says the window shows `thread`. */
export const shows = (thread, options) => ({ 'GET /session': [session(thread, options)] })
/** The broker takes a message. */
export const takes = { 'POST /deliver': [{ status: 200, body: '{"ok":true,"admitted":true}' }] }
/** A broker's answer to a message, once. */
export const replies = (body, status = 200) => ({ 'POST /deliver': [{ status, body }] })

/** The pane host admits the send. */
export const claimed = { 'pane.claim': [{ ok: true }] }
/** A claim the host answers, once, with `answer`. */
export const claim = (answer) => ({ 'pane.claim': [answer] })

/** A scenario of a window resumed on a thread: Codex installed, prepared as the chief. */
export const opened = (name, steps, fields = {}) => ({
  name: `codex: ${name}`,
  harness: 'codex',
  env: ENV,
  steps: [codex(), prepares(ofChief({ resume: THREAD, ...fields }), appServer()), ...steps],
})
/** A scenario of a window whose thread its broker has not named yet. */
export const fresh = (name, steps, fields = {}) => ({
  name: `codex: ${name}`,
  harness: 'codex',
  env: ENV,
  steps: [codex(), prepares(ofChief(fields), appServer()), ...steps],
})

/** A line of Codex's rollout of a thread, as Codex writes it. */
const line = (fields) => `${JSON.stringify(fields)}\n`
const event = (payload) => line({ type: 'event_msg', payload })
export const turnStarted = (turn) => event({ type: 'task_started', turn_id: turn })
export const asked = (id, text) =>
  line({
    type: 'response_item',
    payload: { type: 'message', role: 'user', id, content: [{ type: 'input_text', text }] },
  })
export const answered = (id, text) =>
  event({
    type: 'item_completed',
    item: { type: 'AgentMessage', id, phase: 'final_answer', content: [{ text }] },
  })
export const turnCompleted = (turn, answer, fields = {}) =>
  event({ type: 'task_complete', turn_id: turn, last_agent_message: answer, ...fields })
export const tokens = (limits) => event({ type: 'token_count', rate_limits: limits })
/** The rollout of `thread`, in the folder of Codex's sessions. */
export const rollout = (thread, lines) => ({
  write: `$ROOT/home/.codex/sessions/rollout-2026-09-19T12-00-00-${thread}.jsonl`,
  text: lines.join(''),
})
/** Codex's record of a thread whose first turn settled. */
export const settledTurn = (thread) =>
  rollout(thread, [
    turnStarted('t1'),
    asked('u1', 'Write the parser'),
    answered('a1', 'Done.'),
    turnCompleted('t1', 'Done.'),
  ])
