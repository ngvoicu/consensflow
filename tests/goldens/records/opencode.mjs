/**
 * OpenCode's scenarios: its store written an event at a time, as the
 * chunked suite writes it, and read again whole when it loses a row, is
 * replaced, or holds an event of a type not followed; and the cases of
 * `tests/engine/completion.test.mjs`, each test's staging as data, then
 * its looks.
 */
import { both, fixtureJson, upsert } from './fixtures.mjs'

/** OpenCode's store with a fixture's conversation, but none of its messages, parts or events yet. */
function opencodeStore(db, folder, fixture) {
  const session = fixture.session[0]
  return [
    { mkdir: folder },
    { db, open: `${folder}/opencode.db` },
    { db, exec: 'pragma journal_mode = WAL' },
    {
      db,
      exec: `create table session (${Object.keys(session)
        .map((column) => `"${column}" ${column === 'id' ? 'text primary key' : ''}`)
        .join(', ')});
    create table message (id text primary key, session_id text not null,
      time_created integer not null, time_updated integer not null, data text not null);
    create index message_session_idx on message (session_id, time_created, id);
    create table part (id text primary key, message_id text not null, session_id text not null,
      time_created integer not null, time_updated integer not null, data text not null);
    create index part_session_idx on part (session_id);
    create table event (id text primary key, aggregate_id text not null,
      seq integer not null, type text not null, data text not null);
    create unique index event_aggregate_seq_idx on event (aggregate_id, seq);`,
    },
    upsert(db, 'session', session),
  ]
}

/**
 * The conversation's events in order, each with the row it names as that
 * event leaves it (the fixture's own row at the row's last event).
 */
function opencodeGrowth(fixture, events) {
  const sessionId = fixture.session[0].id
  const rows = new Map([
    ...fixture.message.map((row) => [row.id, { table: 'message', row }]),
    ...fixture.part.map((row) => [row.id, { table: 'part', row }]),
  ])
  const ordered = [
    ...new Map(
      events.filter((row) => row.aggregate_id === sessionId).map((row) => [row.id, row]),
    ).values(),
  ].sort((left, right) => left.seq - right.seq)
  const lastEvent = new Map()
  const named = (event) => {
    const data = JSON.parse(event.data)
    return data.info?.id ?? data.part?.id
  }
  for (const event of ordered) lastEvent.set(named(event), event.id)
  return ordered.map((event) => {
    const found = rows.get(named(event))
    if (found === undefined) return { event, write: null }
    if (lastEvent.get(named(event)) === event.id) return { event, write: found }
    const data = JSON.parse(event.data)
    const { id, sessionID, messageID, ...rest } = data.info ?? data.part
    return {
      event,
      write: { table: found.table, row: { ...found.row, data: JSON.stringify(rest) } },
    }
  })
}

const nativeEvents = () => fixtureJson('opencode/native-events.json').event
const OPENCODE_ENV = { XDG_DATA_HOME: '$ROOT' }

/** One growth step's writes. */
const grow = (db, { event, write }) => [
  ...(write === null ? [] : [upsert(db, write.table, write.row)]),
  upsert(db, 'event', event),
]

export function opencodeSequences() {
  const scenarios = [
    'completion-window',
    'finish-length',
    'api-error',
    'tool-result',
    'v130-tool-loop',
  ].map((name) => {
    const fixture = fixtureJson(`opencode/${name}.json`)
    const sessionId = fixture.session[0].id
    const at = both('opencode', sessionId)
    const steps = opencodeStore('writer', '$ROOT/opencode', fixture)
    for (const step of opencodeGrowth(fixture, [...(fixture.event ?? []), ...nativeEvents()])) {
      steps.push(
        { db: 'writer', exec: 'begin immediate' },
        ...grow('writer', step),
        { db: 'writer', exec: 'commit' },
        at,
      )
    }
    steps.push(at, { db: 'writer', close: true })
    return {
      name: `opencode: ${name}.json read an event at a time`,
      env: OPENCODE_ENV,
      steps,
    }
  })
  return [...scenarios, opencodeLosesARow(), opencodeUnknownEvent()]
}

function opencodeLosesARow() {
  const fixture = fixtureJson('opencode/tool-result.json')
  const sessionId = fixture.session[0].id
  const growth = opencodeGrowth(fixture, nativeEvents())
  const at = both('opencode', sessionId)
  const removed = fixture.message.at(-1).id
  const part = fixture.part.find(
    (row) => row.message_id !== removed && JSON.parse(row.data).type === 'text',
  )
  const file = '$ROOT/opencode/opencode.db'
  return {
    name: 'opencode: a store that loses a row or is replaced is read again whole',
    env: OPENCODE_ENV,
    steps: [
      ...opencodeStore('writer', '$ROOT/opencode', fixture),
      ...growth.flatMap((step) => grow('writer', step)),
      at,
      // A message removed, with no event of its own: its parts go with it.
      { db: 'writer', run: 'delete from part where message_id = ?', params: [removed] },
      { db: 'writer', run: 'delete from message where id = ?', params: [removed] },
      at,
      { db: 'writer', close: true },
      // Another store in its place, as this one is but for one part's words:
      // as many rows and events, none of them new.
      { remove: file },
      { remove: `${file}-wal` },
      { remove: `${file}-shm` },
      ...opencodeStore('fresh', '$ROOT/fresh/opencode', fixture),
      ...growth.flatMap((step) => grow('fresh', step)),
      { db: 'fresh', run: 'delete from part where message_id = ?', params: [removed] },
      { db: 'fresh', run: 'delete from message where id = ?', params: [removed] },
      {
        db: 'fresh',
        run: 'update part set data = ? where id = ?',
        params: [JSON.stringify({ ...JSON.parse(part.data), text: 'in other words' }), part.id],
      },
      { db: 'fresh', close: true },
      { move: '$ROOT/fresh/opencode/opencode.db', to: file },
      at,
    ],
  }
}

function opencodeUnknownEvent() {
  const fixture = fixtureJson('opencode/tool-result.json')
  const sessionId = fixture.session[0].id
  const at = both('opencode', sessionId)
  const growth = opencodeGrowth(fixture, nativeEvents())
  // A later OpenCode that changes a part through an event this reader does not know.
  const part = fixture.part.find((row) => JSON.parse(row.data).type === 'text')
  const data = { ...JSON.parse(part.data), text: 'rewritten by an unknown event' }
  const seq = Math.max(...growth.map(({ event }) => event.seq)) + 1
  return {
    name: 'opencode: an event of a type not followed has the store read whole',
    env: OPENCODE_ENV,
    steps: [
      ...opencodeStore('writer', '$ROOT/opencode', fixture),
      ...growth.flatMap((step) => grow('writer', step)),
      at,
      {
        db: 'writer',
        run: 'update part set data = ? where id = ?',
        params: [JSON.stringify(data), part.id],
      },
      upsert('writer', 'event', {
        id: 'event-of-a-later-version',
        aggregate_id: sessionId,
        seq,
        type: 'message.part.revised.1',
        data: JSON.stringify({ sessionID: sessionId, partID: part.id }),
      }),
      at,
      { db: 'writer', close: true },
    ],
  }
}

// ------------------------------------------------- the completion suite's cases

const OPENCODE_SCHEMA = `
    create table session (
      id text primary key, project_id text not null, parent_id text, slug text not null,
      directory text not null, title text not null, version text not null, share_url text,
      summary_additions integer, summary_deletions integer, summary_files integer,
      summary_diffs text, revert text, permission text, time_created integer not null,
      time_updated integer not null, time_compacting integer, time_archived integer,
      workspace_id text, path text, agent text, model text, cost real not null default 0,
      tokens_input integer not null default 0, tokens_output integer not null default 0,
      tokens_reasoning integer not null default 0, tokens_cache_read integer not null default 0,
      tokens_cache_write integer not null default 0, metadata text
    );
    create table message (
      id text primary key, session_id text not null, time_created integer not null,
      time_updated integer not null, data text not null
    );
    create table part (
      id text primary key, message_id text not null, session_id text not null,
      time_created integer not null, time_updated integer not null, data text not null
    );
    create table event (
      id text primary key, aggregate_id text not null, seq integer not null,
      type text not null, data text not null
    );
  `

/** A row inserted into `table` with the writer's plain statement. */
const insert = (db, table, row) => {
  const columns = Object.keys(row)
  return {
    db,
    run: `insert into ${table} (${columns.join(', ')}) values (${columns.map(() => '?').join(', ')})`,
    params: columns.map((column) => row[column]),
  }
}

/**
 * `stageOpencode`'s store, as steps: the fixture's conversation with its
 * events, at `$ROOT/opencode/opencode.db`, cut at a `snapshot`, of another
 * `version`, `mutate`d, or `eventThrough` a seq, as the test asks.
 */
export function stagedOpencode(name, options = {}) {
  const fixture = fixtureJson(name)
  const nativeEvents = fixtureJson('opencode/native-events.json').event
  const sessions = structuredClone(fixture.session)
  const messages = structuredClone(fixture.message)
  const parts = structuredClone(fixture.part)
  const objectIds = new Set([...messages.map((row) => row.id), ...parts.map((row) => row.id)])
  let events = [...(fixture.event ?? []), ...nativeEvents]
    .filter((row) => row.aggregate_id === sessions[0].id)
    .filter((row) => {
      const data = JSON.parse(row.data)
      return row.type === 'session.updated.1' || objectIds.has(data.info?.id ?? data.part?.id)
    })
  events = [...new Map(events.map((row) => [row.id, structuredClone(row)])).values()]
  if (options.version) sessions[0].version = options.version
  if (options.snapshot) {
    const seq = options.snapshot === 'before' ? 13 : 15
    const event = fixture.event.find((candidate) => candidate.seq === seq)
    const { id, sessionID, ...data } = JSON.parse(event.data).info
    messages[0].data = JSON.stringify(data)
    messages[0].time_updated = data.time.completed ?? 1788707030574
    events = events.filter((row) => row.seq <= seq)
  }
  if (options.mutate) options.mutate({ sessions, messages, parts, events })
  if (options.eventThrough !== undefined) {
    events = events.filter((row) => row.seq <= options.eventThrough)
  }
  return {
    env: { XDG_DATA_HOME: '$ROOT' },
    events,
    steps: [
      { mkdir: '$ROOT/opencode' },
      { db: 'stage', open: '$ROOT/opencode/opencode.db' },
      { db: 'stage', exec: 'pragma journal_mode = WAL' },
      { db: 'stage', exec: OPENCODE_SCHEMA },
      ...sessions.map((row) => insert('stage', 'session', row)),
      ...messages.map((row) => insert('stage', 'message', row)),
      ...parts.map((row) => insert('stage', 'part', row)),
      ...events.map((row) => insert('stage', 'event', row)),
      { db: 'stage', close: true },
    ],
  }
}

/** A test's one staged store and one fresh look at it. */
function opencodeOnce(name, session, fixtureName, options = {}) {
  const { steps, env } = stagedOpencode(fixtureName, options)
  return { name, env, steps: [...steps, { look: 'fresh', kind: 'opencode', session }] }
}

const WINDOW = 'ses_f88c0c7cdffeANJRVLwiBceADi'
const TOOLS = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'

export function opencodeCases() {
  const errorAs = (error) => ({
    snapshot: 'after',
    mutate: ({ messages }) => {
      const data = JSON.parse(messages[0].data)
      data.error = error
      messages[0].data = JSON.stringify(data)
    },
  })
  return [
    ...['before', 'after'].map((snapshot) =>
      opencodeOnce(
        `opencode: step-finish is in-flight until native time.completed appears (${snapshot})`,
        WINDOW,
        'opencode/completion-window.json',
        { snapshot },
      ),
    ),
    opencodeOnce(
      'opencode: native length is settled incomplete',
      'ses_f886ed7dbffe1myK161SPisoR2',
      'opencode/finish-length.json',
    ),
    opencodeOnce(
      'opencode: native APIError is settled incomplete, a failure',
      'ses_f9905d94effe57fADEYMVwfKVF',
      'opencode/api-error.json',
    ),
    opencodeOnce(
      'opencode: MessageAbortedError is a failure, never cancellation without a native fixture',
      'ses_f9905d94effe57fADEYMVwfKVF',
      'opencode/api-error.json',
      {
        mutate({ messages }) {
          const data = JSON.parse(messages[0].data)
          data.error.name = 'MessageAbortedError'
          messages[0].data = JSON.stringify(data)
        },
      },
    ),
    storeFoundWhereKept(),
    questionAsking(),
    opencodeOnce(
      'opencode: tool output and long final text are emitted whole from one snapshot',
      TOOLS,
      'opencode/tool-result.json',
    ),
    competingWriter(),
    opencodeOnce(
      'opencode: a 429 on the message is exhaustion',
      WINDOW,
      'opencode/completion-window.json',
      errorAs({
        name: 'APIError',
        data: { message: 'Weekly usage limit reached. Resets in 1 day.', statusCode: 429 },
      }),
    ),
    opencodeOnce(
      "opencode: OpenRouter's spent credit is quota too",
      WINDOW,
      'opencode/completion-window.json',
      errorAs({
        name: 'APIError',
        data: { message: 'This request requires more credits.', statusCode: 402 },
      }),
    ),
  ]
}

function storeFoundWhereKept() {
  const { steps } = stagedOpencode('opencode/tool-result.json')
  const seen = (env, session = TOOLS) => ({ look: 'fresh', kind: 'opencode', session, env })
  const home = '$ROOT/home'
  const windows = { HOME: home, OS: 'Windows_NT', LOCALAPPDATA: '$ROOT' }
  return {
    name: 'opencode: its store is found where this OpenCode keeps it, the one holding the session first',
    env: windows,
    steps: [
      ...steps,
      { mkdir: home },
      // On Windows, under %LOCALAPPDATA%, where some versions keep it.
      seen(windows),
      // Wherever OPENCODE_DATA names.
      seen({ HOME: home, OPENCODE_DATA: '$ROOT/opencode' }),
      // A store at the first place that does not hold the session gives way to one that does.
      { mkdir: `${home}/.local/share/opencode` },
      { db: 'empty', open: `${home}/.local/share/opencode/opencode.db` },
      { db: 'empty', exec: 'create table session (id text primary key)' },
      { db: 'empty', close: true },
      seen(windows),
      // With none holding it, the first there is answers that the session is not in it.
      seen({ HOME: home, OS: 'Windows_NT' }, 'ses_unknown'),
    ],
  }
}

function questionAsking() {
  const { steps, env } = stagedOpencode('opencode/tool-result.json')
  const tool = fixtureJson('opencode/tool-result.json').part.find(
    (row) => JSON.parse(row.data).type === 'tool',
  )
  const part = JSON.parse(tool.data)
  const at = { look: 'fresh', kind: 'opencode', session: TOOLS }
  return {
    name: 'opencode: its question tool still running is asking; a finished one is not',
    env,
    steps: [
      ...steps,
      at,
      { db: 'edit', open: '$ROOT/opencode/opencode.db' },
      {
        db: 'edit',
        run: 'UPDATE part SET data = ? WHERE id = ?',
        params: [
          JSON.stringify({
            ...part,
            tool: 'question',
            state: { ...part.state, status: 'running' },
          }),
          tool.id,
        ],
      },
      { db: 'edit', close: true },
      at,
    ],
  }
}

function competingWriter() {
  const { steps, env, events } = stagedOpencode('opencode/tool-result.json')
  const fixture = fixtureJson('opencode/tool-result.json')
  const partId = 'prt_07834fd70001aAGLSN74ke5QX3'
  const row = fixture.part.find((candidate) => candidate.id === partId)
  const data = { ...JSON.parse(row.data), text: 'written between snapshot reads' }
  const seq = Math.max(...events.map((event) => event.seq)) + 1
  const eventData = JSON.stringify({
    sessionID: TOOLS,
    part: { id: partId, sessionID: TOOLS, messageID: row.message_id, ...data },
    time: 778,
  })
  return {
    name: 'opencode: one read transaction rejects a competing writer from its snapshot',
    env,
    steps: [
      ...steps,
      {
        look: 'fresh',
        kind: 'opencode',
        session: TOOLS,
        between: [
          { db: 'writer', open: '$ROOT/opencode/opencode.db' },
          { db: 'writer', exec: 'begin immediate' },
          {
            db: 'writer',
            run: 'update part set data = ?, time_updated = ? where id = ?',
            params: [JSON.stringify(data), 778, partId],
          },
          upsert('writer', 'event', {
            id: 'event-from-competing-writer',
            aggregate_id: TOOLS,
            seq,
            type: 'message.part.updated.1',
            data: eventData,
          }),
          { db: 'writer', exec: 'commit' },
          { db: 'writer', close: true },
        ],
      },
      { look: 'fresh', kind: 'opencode', session: TOOLS },
    ],
  }
}
