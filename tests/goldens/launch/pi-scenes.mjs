/**
 * What Pi's scenarios are made of (`pi.mjs`, `pi-looks.mjs`,
 * `pi-deliveries.mjs`): the launch they prepare, where its files are, and
 * the steps that play the extension's part and Pi's own files.
 */
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { LAUNCH } from './claude.mjs'

export { LAUNCH }

/** A second launch's id. */
export const SECOND = '1b2c3d4e-5f60-4172-8b9c-0d1e2f3a4b5c'
export const SESSION = 'cf-1-zeus-0000abcd'
export const OTHER = '0199a6f0-4cc1-7d3e-9f7a-3c5b2e1d0a98'

/**
 * The `count` bytes of the scripted stream (`runner.mjs`) from byte `from`, as
 * hex: a fresh window's session takes the first 4 (its name ends in them),
 * and each message after it the next 16.
 */
const drawnHex = (from, count) =>
  Buffer.from(Array.from({ length: count }, (_, at) => ((from + at) * 7 + 3) % 256)).toString('hex')
export const DRAWN = `cf-1-zeus-${drawnHex(0, 4)}`
/** The id of the message whose 16 bytes begin at byte `from` of the stream. */
export const messageId = (from) => `m-${drawnHex(from, 16)}`
/** What a fresh window sends first, second and third. */
export const FIRST = messageId(4)
export const SECOND_MESSAGE = messageId(20)
export const THIRD = messageId(36)

export const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  PATH: '$ROOT/bin',
}

/** Where the launch's own files are. */
export const FOLDER = `$ROOT/consensflow/integrations/pi/${LAUNCH}`
export const SETTLED = `${FOLDER}/settled`
export const SHOWN = `${SETTLED}/${LAUNCH}.shown.json`
export const inboxFile = (id) => `${FOLDER}/inbox/${id}.json`
export const ackFile = (id) => `${FOLDER}/ack/${id}.json`

/** The extension's file in the folder its bundle is published in, which the hash of the file names. */
const EXTENSION = 'hosts/pi-extension/consensflow-delivery.mjs'
const BUNDLE = createHash('sha256')
  .update(EXTENSION)
  .update('\0')
  .update(readFileSync(new URL(`../../../${EXTENSION}`, import.meta.url)))
  .update('\0')
  .digest('hex')
export const PUBLISHED = `$ROOT/consensflow/extensions/pi/${BUNDLE}/${EXTENSION}`

export const worker = {
  id: 3,
  projectId: 1,
  handle: 'zeus',
  role: 'worker',
  agent: 'zeus',
  harness: 'pi',
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
  agent: { id: 'zeus', kind: 'pi', model: 'openrouter/meta/muse-spark-1.3', thinking: 'high' },
  ...fields,
})

/** A line of Pi's session file, as Pi writes it. */
const line = (fields) => `${JSON.stringify(fields)}\n`
export const header = (session) =>
  line({ type: 'session', version: 3, id: session, timestamp: '2026-09-19T12:00:00.000Z' })
export const user = (id, text) =>
  line({
    type: 'message',
    id,
    timestamp: '2026-09-19T12:00:01.000Z',
    message: { role: 'user', content: [{ type: 'text', text }] },
  })
export const assistant = (id, stopReason, text, message = {}) =>
  line({
    type: 'message',
    id,
    timestamp: '2026-09-19T12:00:02.000Z',
    message: { role: 'assistant', stopReason, content: [{ type: 'text', text }], ...message },
  })
/** The session file of `session`, in Pi's folder for the sessions of /work/app. */
export const transcript = (session, lines) => ({
  write: `$ROOT/home/.pi/agent/sessions/--work-app--/2026-09-19T12-00-00-000Z_${session}.jsonl`,
  text: lines.join(''),
})
/** Pi's record of the first conversation a fresh window shows. */
export const record = transcript(DRAWN, [
  header(DRAWN),
  user('u1', 'Write the parser'),
  assistant('a1', 'stop', 'Done.'),
])
/** What the extension writes when Pi settles on the leaf entry `frontier`, and when a turn starts. */
export const settled = (session, frontier) => ({
  write: `${SETTLED}/${LAUNCH}.json`,
  text: `${JSON.stringify({ launchId: LAUNCH, sessionId: session, frontier: { id: frontier }, settledAt: 1 })}\n`,
})
export const working = (session) => ({
  write: `${SETTLED}/${LAUNCH}.working.json`,
  text: `${JSON.stringify({ launchId: LAUNCH, sessionId: session, startedAt: 1 })}\n`,
})
/** What the extension writes when its window starts on a conversation. */
export const shows = (session, launchId = LAUNCH) => ({
  write: SHOWN,
  text: `${JSON.stringify({ launchId, sessionId: session })}\n`,
})
/** The extension's verdict on a message. */
export const acknowledges = (id, fields) => ({
  write: ackFile(id),
  text: `${JSON.stringify({ id, ...fields })}\n`,
})

/** The pane host admits the send. */
export const claimed = { 'pane.claim': [{ ok: true }] }
/** A claim the host answers, once, with `answer`. */
export const claim = (answer) => ({ 'pane.claim': [answer] })

/** Pi is installed. */
export const installed = { executable: 'pi' }

/** A scenario of a fresh worker window: Pi installed, prepared. */
export const opened = (name, steps, fields = {}, { pendingAtEnd } = {}) => ({
  name: `pi: ${name}`,
  harness: 'pi',
  env: ENV,
  steps: [installed, { prepare: launch(fields) }, ...steps],
  ...(pendingAtEnd ? { pendingAtEnd } : {}),
})
