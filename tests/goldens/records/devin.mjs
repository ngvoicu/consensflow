/**
 * Devin's scenarios: its store written a row at a time beside its wire log
 * in line pieces, as the chunked suite writes them, and read again when its
 * main chain moves, a wire log shrinks or goes, or a row goes; and the cases
 * of `tests/engine/completion.test.mjs`, each test's staging as data, then
 * its looks.
 */
import { both, fixtureJson, pieces } from './fixtures.mjs'

/**
 * Devin's store and one launch's wire log, empty. Its data folder is
 * `$ROOT/data/devin` on every platform: the XDG place, and `%APPDATA%`, which
 * Windows reads.
 */
const DEVIN_ENV = {
  HOME: '$ROOT',
  XDG_DATA_HOME: '$ROOT/data',
  APPDATA: '$ROOT/data',
  CONSENSFLOW_HOME: '$ROOT/home',
}
const DEVIN_STORE = '$ROOT/data/devin/cli/sessions.db'
const DEVIN_LAUNCHES = '$ROOT/home/integrations/devin'

function devinStore(db, sessionId) {
  return [
    { mkdir: '$ROOT/data/devin/cli' },
    { db, open: DEVIN_STORE },
    {
      db,
      exec: `CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id INTEGER);
    CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
      node_id INTEGER NOT NULL, parent_node_id INTEGER, chat_message TEXT NOT NULL,
      created_at INTEGER NOT NULL, UNIQUE(session_id, node_id))`,
    },
    { db, run: 'INSERT INTO sessions (id, main_chain_id) VALUES (?, NULL)', params: [sessionId] },
    { mkdir: `${DEVIN_LAUNCHES}/launch-1` },
    { write: `${DEVIN_LAUNCHES}/launch-1/wire.jsonl`, text: '' },
  ]
}

const devinRow = (db, sessionId, row) => ({
  db,
  run: 'INSERT INTO message_nodes (row_id, session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?, ?)',
  params: [
    row.row_id,
    sessionId,
    row.node_id,
    row.parent_node_id,
    row.chat_message,
    row.created_at,
  ],
})

const devinHead = (db, sessionId, node) => ({
  db,
  run: 'UPDATE sessions SET main_chain_id = ? WHERE id = ?',
  params: [node, sessionId],
})

/**
 * Devin's rows a row at a time (its main chain following each) beside its
 * wire log in line pieces, a look after each step, and last the fixture's
 * own main chain.
 */
function devinInPieces({ session, rows, wire: events }, label) {
  const at = both('devin', session.id)
  const wire = `${DEVIN_LAUNCHES}/launch-1/wire.jsonl`
  const steps = devinStore('writer', session.id)
  const wirePieces = pieces(events.map((event) => JSON.stringify(event)))
  const ordered = [...rows].sort((left, right) => left.row_id - right.row_id)
  const count = Math.max(ordered.length, wirePieces.length)
  let row = 0
  let piece = 0
  for (let step = 1; step <= count; step += 1) {
    for (; row < Math.ceil((step * ordered.length) / count); row += 1) {
      steps.push(
        devinRow('writer', session.id, ordered[row]),
        devinHead('writer', session.id, ordered[row].node_id),
      )
    }
    for (; piece < Math.ceil((step * wirePieces.length) / count); piece += 1)
      steps.push({ append: wire, text: wirePieces[piece] })
    steps.push(at)
  }
  steps.push(devinHead('writer', session.id, session.main_chain_id), at, {
    db: 'writer',
    close: true,
  })
  return { name: `devin: ${label} read a row and a wire piece at a time`, env: DEVIN_ENV, steps }
}

export function devinSequences() {
  const scenarios = ['native-tui', 'worker-tui'].map((name) =>
    devinInPieces(fixtureJson(`devin/${name}.json`), `${name}.json`),
  )
  const { streamed, stored } = fixtureJson('devin/file-link.json')
  const message = (fields) => JSON.stringify(fields)
  const chunk = (text) => ({
    sessionId: 'calm-river',
    turnClientMessageId: 'request-1',
    update: {
      sessionUpdate: 'agent_message_chunk',
      content: { type: 'text', text },
      _meta: { 'cognition.ai/streamingMessageId': 'stream-1' },
    },
  })
  const half = Math.floor(streamed.length / 2)
  scenarios.push(
    devinInPieces(
      {
        session: { id: 'calm-river', main_chain_id: 2 },
        rows: [
          {
            row_id: 1,
            node_id: 1,
            parent_node_id: null,
            created_at: 1789243781,
            chat_message: message({
              message_id: 'u-1',
              role: 'user',
              content: 'Review T-1',
              metadata: { extensions: { 'chisel/client-message-id': 'request-1' } },
            }),
          },
          {
            row_id: 2,
            node_id: 2,
            parent_node_id: 1,
            created_at: 1789243782,
            chat_message: message({ message_id: 'a-1', role: 'assistant', content: stored }),
          },
        ],
        wire: [
          chunk(streamed.slice(0, half)),
          chunk(streamed.slice(half)),
          { sessionId: 'calm-river', turnClientMessageId: 'request-1', cause: 'complete' },
        ],
      },
      'file-link.json',
    ),
    devinMovedAndCut(),
  )
  return scenarios
}

function devinMovedAndCut() {
  const fixture = fixtureJson('devin/worker-tui.json')
  const sessionId = fixture.session.id
  const at = both('devin', sessionId)
  const head = (node) => devinHead('writer', sessionId, node)
  const nodes = new Map(fixture.rows.map((row) => [row.node_id, row]))
  const lines = fixture.wire.map((event) => JSON.stringify(event))
  const last = nodes.get(fixture.session.main_chain_id)
  const unended = lines.filter((line) => JSON.parse(line).cause === undefined).slice(0, 2)
  const second = `${DEVIN_LAUNCHES}/launch-2/wire.jsonl`
  const chain = new Set()
  for (let node = fixture.session.main_chain_id; node !== null; ) {
    chain.add(node)
    node = nodes.get(node).parent_node_id
  }
  const offChain = fixture.rows.find((row) => !chain.has(row.node_id))
  return {
    name: 'devin: a main chain that moves, a wire log that shrinks or goes, and a store that loses a row are read again',
    env: DEVIN_ENV,
    steps: [
      ...devinStore('writer', sessionId),
      ...fixture.rows.map((row) => devinRow('writer', sessionId, row)),
      head(fixture.session.main_chain_id),
      { write: `${DEVIN_LAUNCHES}/launch-1/wire.jsonl`, text: `${lines.join('\n')}\n` },
      at,
      // The window went back a message, and forward again: only its main chain moved.
      head(last.parent_node_id),
      at,
      head(fixture.session.main_chain_id),
      at,
      // A message rewritten in place, as one still being written could be.
      {
        db: 'writer',
        run: 'UPDATE message_nodes SET chat_message = ? WHERE row_id = ?',
        params: [last.chat_message.replace(/"(text|content)":"/, '"$1":"Rewritten. '), last.row_id],
      },
      at,
      {
        db: 'writer',
        run: 'UPDATE message_nodes SET chat_message = ? WHERE row_id = ?',
        params: [last.chat_message, last.row_id],
      },
      at,
      // A second launch's log that says nothing of how a turn ended, then cut short.
      { mkdir: `${DEVIN_LAUNCHES}/launch-2` },
      { write: second, text: `${unended.join('\n')}\n` },
      at,
      { write: second, text: `${unended[0]}\n` },
      at,
      // The first launch's folder removed, as a launch's end removes it: what
      // its log said of how each turn ended goes with it.
      { remove: `${DEVIN_LAUNCHES}/launch-1` },
      at,
      // A row removed off the main chain.
      ...(offChain === undefined
        ? []
        : [
            {
              db: 'writer',
              run: 'DELETE FROM message_nodes WHERE row_id = ?',
              params: [offChain.row_id],
            },
            at,
          ]),
      {
        db: 'writer',
        run: 'DELETE FROM message_nodes WHERE node_id = ?',
        params: [fixture.session.main_chain_id],
      },
      at,
      { db: 'writer', close: true },
    ],
  }
}

// ------------------------------------------------- the completion suite's cases

const CALM = 'calm-river'

/**
 * `stageDevin`'s store, as steps: a user message and an assistant reply,
 * and a wire log of `events`. Its data folder is the same on every
 * platform: `$ROOT/data/devin`, or with `windows`, Devin's own Windows place,
 * `%APPDATA%\devin`, which this environment names.
 */
export function stagedDevin(stored, events, { windows = false, finish } = {}) {
  const env = {
    HOME: '$ROOT',
    XDG_DATA_HOME: '$ROOT/data',
    CONSENSFLOW_HOME: '$ROOT/home',
    ...(windows
      ? { OS: 'Windows_NT', APPDATA: '$ROOT/AppData/Roaming' }
      : { APPDATA: '$ROOT/data' }),
  }
  const folder = windows ? '$ROOT/AppData/Roaming/devin/cli' : '$ROOT/data/devin/cli'
  const node = (nodeId, parent, message, createdAt) => ({
    db: 'stage',
    run: 'INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?)',
    params: [CALM, nodeId, parent, JSON.stringify(message), createdAt],
  })
  const launch = '$ROOT/home/integrations/devin/launch-1'
  return {
    env,
    store: `${folder}/sessions.db`,
    steps: [
      { mkdir: folder },
      { db: 'stage', open: `${folder}/sessions.db` },
      {
        db: 'stage',
        exec: `CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
    CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, node_id TEXT,
      parent_node_id TEXT, chat_message TEXT, created_at TEXT)`,
      },
      node(
        'n-1',
        null,
        {
          message_id: 'u-1',
          role: 'user',
          content: 'Review T-1',
          metadata: { extensions: { 'chisel/client-message-id': 'request-1' } },
        },
        '2026-09-26T05:09:30Z',
      ),
      node(
        'n-2',
        'n-1',
        {
          message_id: 'a-1',
          role: 'assistant',
          content: stored,
          ...(finish === undefined ? {} : { metadata: { finish_reason: finish } }),
        },
        '2026-09-26T05:10:00Z',
      ),
      {
        db: 'stage',
        run: 'INSERT INTO sessions (id, main_chain_id) VALUES (?, ?)',
        params: [CALM, 'n-2'],
      },
      { db: 'stage', close: true },
      { mkdir: launch },
      {
        write: `${launch}/wire.jsonl`,
        text: `${events.map((event) => JSON.stringify(event)).join('\n')}\n`,
      },
    ],
  }
}

const devinChunk = (text, request = 'request-1', stream = 'stream-1') => ({
  sessionId: CALM,
  turnClientMessageId: request,
  update: {
    sessionUpdate: 'agent_message_chunk',
    content: { type: 'text', text },
    _meta: { 'cognition.ai/streamingMessageId': stream },
  },
})
const complete = { sessionId: CALM, turnClientMessageId: 'request-1', cause: 'complete' }
const freshLook = { look: 'fresh', kind: 'devin', session: CALM }

/** A test's one staged store and one fresh look at it. */
function devinOnce(name, stored, events, options) {
  const { steps, env } = stagedDevin(stored, events, options)
  return { name, env, steps: [...steps, freshLook] }
}

/** Steps that write to the staged store with a connection of their own, as the tests' edits do. */
const edit = (store, ...runs) => [
  { db: 'edit', open: store },
  ...runs.map((run) => ({ db: 'edit', ...run })),
  { db: 'edit', close: true },
]

export function devinCases() {
  const fileLink = fixtureJson('devin/file-link.json')
  const windowsLink = fixtureJson('devin/file-link-windows.json')
  const half = Math.floor(fileLink.streamed.length / 2)
  const replayedAt = { 'cognition.ai/timestamp': '2026-10-03T16:30:00Z' }
  const replay = (update) => ({ sessionId: CALM, update: { ...update, _meta: replayedAt } })
  const replayed = [
    replay({ sessionUpdate: 'user_message_chunk', content: { type: 'text', text: 'Review T-1' } }),
    replay({ sessionUpdate: 'tool_call', toolCallId: 't-1', title: 'Ran git' }),
    replay({ sessionUpdate: 'tool_call_update', toolCallId: 't-1', status: 'completed' }),
    replay({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Done.' } }),
    { sessionId: CALM, update: { sessionUpdate: 'session_info_update' } },
  ]
  const work = (sessionUpdate) => ({ sessionId: CALM, update: { sessionUpdate } })
  const ended = [devinChunk('Done.'), complete]
  return [
    devinOnce(
      'devin: on Windows its sessions are read from %APPDATA%\\devin, where Devin keeps them',
      'Done.',
      [devinChunk('Done.'), complete],
      { windows: true },
    ),
    questionOpen(),
    questionBelowHead(),
    devinOnce(
      'devin: a final message that links a file settles, though Devin stores the link as a tag',
      fileLink.stored,
      [
        devinChunk(fileLink.streamed.slice(0, half)),
        devinChunk(fileLink.streamed.slice(half)),
        complete,
      ],
    ),
    devinOnce(
      'devin: a reply naming files on Windows, linked and quoted, is the one Devin streamed',
      windowsLink.stored,
      [devinChunk(windowsLink.streamed), complete],
    ),
    devinOnce(
      "devin: a window reopened on its conversation reads a reply stored as its turn's end as finished",
      'Done.',
      replayed,
      { finish: 'stop' },
    ),
    devinOnce(
      'devin: a reply that ended in tool calls is no turn end, replayed or not',
      'Next I run the tests.',
      replayed,
      { finish: 'tool_calls' },
    ),
    devinOnce(
      'devin: a turn Devin ended for its quota is over, neither failed nor finished',
      'Done.',
      [
        { sessionId: CALM, update: { sessionUpdate: 'tool_call' } },
        devinChunk('Half'),
        {
          sessionId: CALM,
          turnClientMessageId: 'request-1',
          cause: 'quota_exhausted',
          errorMessage: 'Your daily usage quota has been exhausted.',
        },
      ],
    ),
    anotherWindowWroteLast(),
    devinOnce(
      'devin: a turn whose work shows on the wire after the last end is in flight',
      'Done.',
      [...ended, work('agent_thought_chunk'), work('tool_call'), work('tool_call_update')],
    ),
    devinOnce("devin: a tool's last word after its turn is not work", 'Done.', [
      ...ended,
      work('tool_call'),
      { sessionId: CALM, update: { sessionUpdate: 'tool_call_update', status: 'failed' } },
      { sessionId: CALM, turnClientMessageId: 'request-2', cause: 'cancelled' },
      { sessionId: CALM, update: { sessionUpdate: 'tool_call_update', status: 'completed' } },
    ]),
    devinOnce('devin: settings updates are not work', 'Done.', [
      { sessionId: CALM, update: { sessionUpdate: 'config_option_update' } },
      ...ended,
    ]),
  ]
}

function questionOpen() {
  const { steps, env, store } = stagedDevin('Voi întreba:', [])
  return {
    name: 'devin: a question dialog still open is asking, until its answer comes back',
    env,
    steps: [
      ...steps,
      freshLook,
      ...edit(store, {
        run: "UPDATE message_nodes SET chat_message = ? WHERE node_id = 'n-2'",
        params: [
          JSON.stringify({
            message_id: 'a-1',
            role: 'assistant',
            content: 'Voi întreba:',
            tool_calls: [{ id: 'call-1', name: 'ask_user_question', arguments: { questions: [] } }],
          }),
        ],
      }),
      freshLook,
      ...edit(
        store,
        {
          run: 'INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?)',
          params: [
            CALM,
            'n-3',
            'n-2',
            JSON.stringify({
              message_id: 't-1',
              role: 'tool',
              content: 'cariere',
              tool_call_id: 'call-1',
            }),
            '2026-09-26T05:11:00Z',
          ],
        },
        { run: "UPDATE sessions SET main_chain_id = 'n-3'" },
      ),
      freshLook,
    ],
  }
}

function questionBelowHead() {
  const { steps, env, store } = stagedDevin('Voi întreba:', [])
  const add = (node, parent, message) =>
    edit(store, {
      run: 'INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?)',
      params: [CALM, node, parent, JSON.stringify(message), '2026-09-26T05:11:00Z'],
    })
  return {
    name: "devin: a question its tool waits on below the chain's head is asking, until its answer comes",
    env,
    steps: [
      ...steps,
      ...add('n-3', 'n-2', {
        message_id: 'a-2',
        role: 'assistant',
        content: '',
        tool_calls: [{ id: 'call-1', name: 'ask_user_question', arguments: { questions: [] } }],
      }),
      freshLook,
      ...add('n-4', 'n-3', {
        message_id: 't-1',
        role: 'tool',
        content: 'cariere',
        tool_call_id: 'call-1',
      }),
      freshLook,
    ],
  }
}

function anotherWindowWroteLast() {
  const { steps, env } = stagedDevin('Done.', [
    devinChunk('Done.'),
    complete,
    { sessionId: CALM, update: { sessionUpdate: 'tool_call' } },
  ])
  const other = '$ROOT/home/integrations/devin/launch-2'
  return {
    name: "devin: a session at work reads as working, though another session's window wrote last",
    env,
    steps: [
      ...steps,
      { mkdir: other },
      {
        write: `${other}/wire.jsonl`,
        text: `${JSON.stringify({ sessionId: 'quiet-lake', turnClientMessageId: 'request-9', cause: 'complete' })}\n`,
      },
      { mtime: `${other}/wire.jsonl`, ago: -60_000 },
      freshLook,
    ],
  }
}
