/**
 * What Devin's scenarios are made of (`devin.mjs`): the launch, the stand-in
 * `devin`, one line of its wire log, its store of sessions, and the scenario
 * of a window prepared and then looked at.
 */

/** The launch's id: a uuid, as the engine mints one. */
export const LAUNCH = '2c3d4e5f-6071-4283-9cad-1e2f3a4b5c6d'
/** A second launch's id, and a third's. */
export const SECOND = '3d4e5f60-7182-4394-8dbe-2f3a4b5c6d7e'
export const THIRD = '4e5f6071-8293-45a4-9ecf-3a4b5c6d7e8f'
/** The conversation a resumed window is on, and one the human may switch it to. */
export const SESSION = 'mild-coin'
export const OTHER = 'fresh-leaf'

/** Devin's config and data are one folder here, on every platform. */
export const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  XDG_CONFIG_HOME: '$ROOT/xdg',
  XDG_DATA_HOME: '$ROOT/xdg',
  APPDATA: '$ROOT/xdg',
  PATH: '$ROOT/bin',
}
/** A Windows machine's, by its own word. */
export const WINDOWS = { ...ENV, OS: 'Windows_NT' }

export const CONFIG = '$ROOT/xdg/devin/config.json'
export const FOLDER = `$ROOT/consensflow/integrations/devin/${LAUNCH}`
export const WIRE = `${FOLDER}/wire.jsonl`
const STORE = '$ROOT/xdg/devin/cli/sessions.db'

export const worker = {
  id: 3,
  projectId: 1,
  handle: 'zeus',
  role: 'worker',
  agent: 'zeus',
  harness: 'devin',
}
export const chief = { ...worker, handle: 'chief', role: 'chief', agent: null }
export const TASK = '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser'

/** A launch to prepare, `fields` over a worker's. */
export const launch = (fields = {}) => ({
  launchId: LAUNCH,
  participant: worker,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: '/work/app',
  resume: null,
  message: TASK,
  agent: { id: 'zeus', kind: 'devin', model: 'swe-1-6-slow' },
  ...fields,
})

/** The stand-in `devin`, answering its version. */
export const devin = (version = '3000.11.3', name = 'devin') => ({
  executable: name,
  output: `devin ${version}`,
})
/**
 * The stand-in of a machine whose environment says Windows: a program with an
 * extension Windows starts, which a POSIX machine finds in its place. (A
 * Windows machine makes its own stand-in a `.cmd`.)
 */
export const windowsDevin = () =>
  devin('3000.11.3', process.platform === 'win32' ? 'devin' : 'devin.exe')

/** One line of Devin's wire log. */
export const line = (event) => `${JSON.stringify(event)}\n`
/** The line the log gains when its window configures a conversation it opens. */
export const shows = (sessionId) =>
  line({
    sessionId,
    update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] },
  })
/** The line a prompt makes. */
export const prompted = (id = 2, sessionId = SESSION) =>
  line({ jsonrpc: '2.0', id, method: 'session/prompt', params: { sessionId } })
/** A piece of the agent's message. */
export const said = (text, sessionId = SESSION) =>
  line({
    sessionId,
    update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } },
  })

/** A piece of the reply Devin streams to the request the human's message `request` made. */
export const streamed = (text, request = 'request-1', sessionId = SESSION) =>
  line({
    sessionId,
    turnClientMessageId: request,
    update: {
      sessionUpdate: 'agent_message_chunk',
      content: { type: 'text', text },
      _meta: { 'cognition.ai/streamingMessageId': 'stream-1' },
    },
  })
/** The end of a turn, by its cause, and what Devin says of it. */
export const ended = (cause, fields = {}, request = 'request-1', sessionId = SESSION) =>
  line({ sessionId, turnClientMessageId: request, cause, ...fields })

/** What Devin's window has: its wire log, written whole, or added to. */
export const wire = (...lines) => ({ write: WIRE, text: lines.join('') })
export const appended = (...lines) => ({ append: WIRE, text: lines.join('') })

/** The answer of a pane host to the snapshot a window is asked for. */
export const snapshot = (fields) => ({
  'pane.snapshot': [{ ok: true, pasteInFlight: false, unsent: false, ...fields }],
})

/** What Rust says where Node's V8 threw its own error, and why. */
export const refusedInWords = (why, throws) => ({ why, answer: { throws } })

/**
 * What a window that is not ready yet is: Node says false, which the contract
 * calls `Held::Unsaid` and the player writes as the sentence the engine gives it.
 */
export const NOT_YET = {
  why: "Node says false for a window not ready yet, the contract's Held::Unsaid, which the player writes as the sentence the engine gives it",
  answer: { answer: 'a paste is on its way' },
}

/** A scenario of a window prepared on `fields`, `before` the prepare and `after` it. */
export function prepared(name, fields, before = [], { env = ENV, after = [], kept } = {}) {
  return {
    name: `devin: ${name}`,
    harness: 'devin',
    env,
    steps: [...before, { prepare: launch(fields), ...(kept ? { kept } : {}) }, ...after],
  }
}

/** A scenario of a window on a conversation it knows: resumed, with no first message. */
export const resumed = (name, steps, { env = ENV, fields = {}, standIn = devin() } = {}) =>
  prepared(name, { resume: SESSION, message: null, ...fields }, [standIn], { env, after: steps })

/** A scenario of a fresh window, which has not said yet which conversation it opened. */
export const fresh = (name, steps, { env = ENV, fields = {}, standIn = devin() } = {}) =>
  prepared(name, fields, [standIn], { env, after: steps })

/**
 * Devin's store of its sessions with the conversation `session` in it, as steps:
 * its rows, then the head of its main chain.
 */
export function conversation(session, rows, head = rows.at(-1)?.node ?? null) {
  return [
    { mkdir: '$ROOT/xdg/devin/cli' },
    { db: 'store', open: STORE },
    {
      db: 'store',
      exec: `CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id INTEGER);
        CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
          node_id INTEGER NOT NULL, parent_node_id INTEGER, chat_message TEXT NOT NULL,
          created_at INTEGER NOT NULL, UNIQUE(session_id, node_id))`,
    },
    {
      db: 'store',
      run: 'INSERT INTO sessions (id, main_chain_id) VALUES (?, ?)',
      params: [session, head],
    },
    ...rows.map((row) => ({
      db: 'store',
      run: 'INSERT INTO message_nodes (row_id, session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?, ?)',
      params: [
        row.node,
        session,
        row.node,
        row.parent ?? null,
        JSON.stringify(row.message),
        1789819200 + row.node,
      ],
    })),
    { db: 'store', close: true },
  ]
}

/** The rows of a conversation: what the human asked, what the agent answered, its question tool. */
export const asked = (node, text, parent) => ({
  node,
  parent,
  message: {
    message_id: `user-${node}`,
    role: 'user',
    content: text,
    metadata: { extensions: { 'chisel/client-message-id': `request-${node}` } },
  },
})
export const replied = (node, text, parent, fields = {}) => ({
  node,
  parent,
  message: {
    message_id: `assistant-${node}`,
    role: 'assistant',
    content: text,
    metadata: { finish_reason: 'stop' },
    ...fields,
  },
})
/** An assistant's call of its question tool, and the tool's own answer to it. */
export const questioned = (node, parent) =>
  replied(node, 'Which parser?', parent, {
    metadata: {},
    tool_calls: [{ id: 'call-1', name: 'ask_user_question' }],
  })
export const answered = (node, parent) => ({
  node,
  parent,
  message: {
    message_id: `tool-${node}`,
    role: 'tool',
    content: 'The fast one',
    tool_call_id: 'call-1',
  },
})
