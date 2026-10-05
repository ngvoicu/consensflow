/**
 * What OpenCode's scenarios are made of (`opencode.mjs`, `opencode-creates.mjs`,
 * `opencode-seeds.mjs`, `opencode-looks.mjs`, `opencode-deliveries.mjs`): the
 * launch they prepare, the folder it works in, and the answers of the servers
 * a window talks to. Each prepare of a fresh window draws two ports, the
 * plugin's first, then the window's, and two tokens: 24 bytes of the scripted
 * stream each, the plugin's first (`runner.mjs`).
 */
import { LAUNCH } from './claude.mjs'

export { LAUNCH }

/** A second launch's id. */
export const SECOND = '1b2c3d4e-5f60-4172-8b9c-0d1e2f3a4b5c'

/** The conversation a fresh window opens on, which the throwaway server answers with. */
export const CREATED = 'ses_abc123'
/** A conversation OpenCode kept, and another the human opens in the window. */
export const KEPT = 'ses_kept789'
export const OTHER = 'ses_other456'

export const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  PATH: '$ROOT/bin',
}

/** The folder a window works in: a folder that is there, which OpenCode is asked for the real name of. */
export const WORK = '$ROOT/work'
export const workspace = { write: `${WORK}/.keep`, text: '' }

/**
 * OpenCode is installed: a stand-in that a window and a throwaway server
 * start, and that is run for nothing (`standIn`, `runner.mjs`).
 */
export const installed = { standIn: { name: 'opencode', answers: {} } }

export const worker = {
  id: 3,
  projectId: 1,
  handle: 'zeus',
  role: 'worker',
  agent: 'zeus',
  harness: 'opencode',
}
export const chief = { ...worker, handle: 'chief', role: 'chief', agent: null }
const TASK = '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser'
/** A text with what a window must not be given in it. */
export const MESSY = 'Hello\r\nworld\u001b[31m red\u001b[0m 50%\r60%\u0085'

/** The model OpenCode's agent runs, and the effort it is given. */
export const MODEL = 'opencode/muse-spark-1.3-contributor-free'

/** When a scenario's clock starts (`runner.mjs`). */
export const NOW = Date.parse('2026-09-19T12:00:00Z')

/** A launch to prepare, `fields` over a worker's. */
export const launch = (fields = {}) => ({
  launchId: LAUNCH,
  participant: worker,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: WORK,
  resume: null,
  message: TASK,
  agent: { id: 'zeus', kind: 'opencode', model: MODEL, effort: 'high' },
  ...fields,
})

/** The window's server (the first fresh launch's), and the plugin's. */
export const SERVER = 'http://127.0.0.1:41001'
export const PLUGIN = 'http://127.0.0.1:41000'

/** The pane host admits a send. */
export const claimed = { 'pane.claim': [{ ok: true }] }
/** A claim the host answers, once, with `answer`. */
export const claim = (answer) => ({ 'pane.claim': [answer] })

/** A server that is up, as OpenCode answers its health check. */
export const HEALTHY = { status: 200, body: '{"healthy":true}' }

/**
 * The throwaway server a fresh window's conversation is made on, `times`
 * windows' worth: it is a child that ends when asked, answers its health
 * check at once, and makes `session` in the folder the window works in.
 */
export function created({ times = 1, session = CREATED, directory = WORK } = {}) {
  const made = { status: 200, body: { json: { id: session, directory } } }
  return {
    served: {
      'GET /global/health': Array.from({ length: times }, () => HEALTHY),
      'POST /session': Array.from({ length: times }, () => made),
    },
    children: { opencode: Array.from({ length: times }, () => ({ ends: 'asked' })) },
  }
}

/** The conversation's own record, as OpenCode keeps a conversation that ran on `variant`. */
export const settings = (session, fields = {}) => ({
  id: session,
  agent: 'review',
  model: { id: 'native-model', providerID: 'openrouter', variant: 'medium' },
  ...fields,
})

/** What the plugin says it shows: `session` (null on the home screen) and what OpenCode is doing. */
export const shows = (session, status = null, launchId = LAUNCH) => ({
  status: 200,
  body: JSON.stringify({ launchId, sessionId: session, status }),
})

/** The steps that set the scene and are not recorded (`runner.mjs`). */
const SCENE = [
  'executable',
  'write',
  'remove',
  'status',
  'statusText',
  'opened',
  'follow',
  'holdLooks',
]

/**
 * Answers scripted for a step that are asked over the steps that follow it
 * (a poll every tenth of a second, a request held until it is released) are
 * optional until the last step, whose answers must all have been asked.
 */
export function patient(steps) {
  const recorded = (step) => !SCENE.some((key) => step[key] !== undefined)
  const last = steps.findLastIndex(recorded)
  const scripted = new Set()
  return steps.map((step, at) => {
    for (const route of [...Object.keys(step.served ?? {}), ...Object.keys(step.answers ?? {})]) {
      scripted.add(route)
    }
    return at < last && recorded(step) && scripted.size > 0
      ? { ...step, optional: [...scripted] }
      : step
  })
}

/** A scenario of a window resumed on `KEPT`, which draws no conversation of its own. */
export const resumed = (name, steps, fields = {}, { pendingAtEnd, env = ENV } = {}) => ({
  name: `opencode: ${name}`,
  harness: 'opencode',
  env,
  steps: patient([
    installed,
    workspace,
    { prepare: launch({ resume: KEPT, message: null, ...fields }) },
    ...steps,
  ]),
  ...(pendingAtEnd ? { pendingAtEnd } : {}),
})

/** A scenario of a fresh window, its conversation made first. */
export const opened = (name, steps, fields = {}, { pendingAtEnd, env = ENV } = {}) => ({
  name: `opencode: ${name}`,
  harness: 'opencode',
  env,
  steps: patient([installed, workspace, { prepare: launch(fields), ...created() }, ...steps]),
  ...(pendingAtEnd ? { pendingAtEnd } : {}),
})
