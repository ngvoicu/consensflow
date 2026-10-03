/**
 * A record read on, a piece at a time, says what a reading of it whole says.
 * Every fixture is written the way its harness writes it, piece by piece (a
 * JSONL line in two halves and then its newline; OpenCode's store an event
 * at a time, with the row the event names; Devin's store a row at a time
 * beside its wire log in line pieces), and after each piece one reader that
 * reads on is compared with a fresh reading of the whole record. A record
 * that shrinks or is replaced is read again from its start.
 */
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import test, { after } from 'node:test'
import { fileURLToPath } from 'node:url'
import { answers, cachedAnswers } from '../../hosts/lib/completion.js'
import { devinFolders } from '../../src/harnesses.js'

const FIX = fileURLToPath(new URL('./fixtures/completion/', import.meta.url))
const PI_QUIET_MS = 120_000

/** Every temporary root a test here makes, removed when the file's tests end. */
const roots = []
after(() => Promise.all(roots.map((root) => fs.rm(root, { recursive: true, force: true }))))

async function tempRoot() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-chunked-'))
  roots.push(root)
  return root
}

/** A look by `read` that reads on, checked against a reading of the whole record. */
async function agrees(read, kind, session, env, options, label) {
  const whole = await answers(kind, session, env, options)
  assert.deepEqual(await read(kind, session, env, options), whole, label)
  return whole
}

/** Each line in two halves and then its newline, so a look finds every kind of last line. */
function pieces(lines) {
  return lines
    .flatMap((line) => {
      const half = Math.floor(line.length / 2)
      return [line.slice(0, half), line.slice(half), '\n']
    })
    .filter((piece) => piece.length > 0)
}

const fixtureLines = async (fixture) =>
  (await fs.readFile(path.join(FIX, fixture), 'utf8')).trimEnd().split('\n')

/** Where each JSONL harness keeps a session's transcript, under `root`. */
async function transcript(kind, session, root) {
  if (kind === 'codex') {
    const dir = path.join(root, 'sessions', '2026', '09', '06')
    await fs.mkdir(dir, { recursive: true })
    return {
      file: path.join(dir, `rollout-2026-09-06T00-00-00-${session}.jsonl`),
      env: { CODEX_HOME: root },
    }
  }
  if (kind === 'claude-code') {
    const dir = path.join(root, 'projects', '-work-app')
    await fs.mkdir(dir, { recursive: true })
    return { file: path.join(dir, `${session}.jsonl`), env: { CLAUDE_CONFIG_DIR: root } }
  }
  const dir = path.join(root, '.pi', 'agent', 'sessions', '--work-app--')
  await fs.mkdir(dir, { recursive: true })
  return { file: path.join(dir, `2026-09-06T00-00-00-000Z_${session}.jsonl`), env: { HOME: root } }
}

// ------------------------------------------------------------------ JSONL

const transcripts = [
  ['codex', 'codex/completed.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['codex', 'codex/errored-task-complete.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['codex', 'codex/interrupted.jsonl', '01a077f6-6663-7bc2-81cd-e287ccaabdbd'],
  ['codex', 'codex/forked.jsonl', '01a077fa-5968-7b62-8fdd-043410a3d4b9'],
  ['codex', 'codex/big-answer.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['claude-code', 'claude-code/fragments.jsonl', '15fba934-d727-4777-8791-123675a63649'],
  ['claude-code', 'claude-code/frontier-history.jsonl', '15fba934-d727-4777-8791-123675a63649'],
  ['claude-code', 'claude-code/queued-turn.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/queue-pop-all.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/interrupted.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/provider-429.jsonl', '33383216-87a0-4e6d-a273-07c4b229cdb1'],
  ['claude-code', 'claude-code/compaction.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/v263-tool-loop.jsonl', '5cbf8973-f472-448a-8763-59fb4268a9d7'],
  ['claude-code', 'claude-code/v265-tool-loop.jsonl', '17499106-8778-48e1-a306-87bd186c9f7e'],
  ['claude-code', 'claude-code/v266-tool-loop.jsonl', '47b1090f-b1c7-4d19-95b9-24c09a7f164a'],
  ['claude-code', 'claude-code/v268-clear.jsonl', 'fb561379-bcab-4045-92d2-d460bb19ed36'],
  ['claude-code', 'claude-code/v268-late-ancestors.jsonl', '4e761651-511b-4065-8a65-6ff21582faad'],
  ['pi', 'pi/between-tool-steps.jsonl', 'hazy-ridge'],
  ['pi', 'pi/tool-loop.jsonl', 'hazy-ridge'],
  ['pi', 'pi/provider-429.jsonl', 'triton-jade-fern'],
]

/**
 * Writes `lines` a piece at a time, a look after each. Pi is read three
 * ways: just written, quiet past its window, and with its extension's
 * evidence naming the last record.
 */
async function readInPieces(kind, session, lines, label) {
  const ways = kind === 'pi' ? ['fresh', 'quiet', 'evidence'] : ['fresh']
  for (const way of ways) {
    const root = await tempRoot()
    const { file, env } = await transcript(kind, session, root)
    let options = {}
    if (way === 'evidence') {
      const directory = path.join(root, 'settled')
      await fs.mkdir(directory)
      const frontier = { id: JSON.parse(lines.at(-1)).id }
      await fs.writeFile(
        path.join(directory, 'launch-1.json'),
        JSON.stringify({ launchId: 'launch-1', sessionId: session, frontier }),
      )
      options = { piSettlement: { directory, launchId: 'launch-1' } }
    }
    const read = cachedAnswers()
    let written = 0
    for (const piece of pieces(lines)) {
      await fs.appendFile(file, piece)
      written += piece.length
      if (way === 'quiet') {
        const old = new Date(Date.now() - PI_QUIET_MS - 1_000)
        await fs.utimes(file, old, old)
      }
      await agrees(read, kind, session, env, options, `${label} ${way} at ${written}`)
    }
  }
}

for (const [kind, fixture, session] of transcripts) {
  test(`completion/${kind}: ${fixture} read in pieces is read whole`, async () => {
    await readInPieces(kind, session, await fixtureLines(fixture), fixture)
  })
}

test('completion/claude-code: a record read later that an earlier decision looked up has the transcript read again', async () => {
  // Late ancestors decide whether a user record opened a turn; a record that
  // arrives later under a uuid that decision looked up can decide it otherwise.
  const session = '4e761651-511b-4065-8a65-6ff21582faad'
  const records = (await fixtureLines('claude-code/v268-late-ancestors.jsonl')).map(JSON.parse)
  for (const [name, later] of [
    ['a duplicate user', records[6]],
    ['a duplicate attachment', records[7]],
    ['a next user', { ...records[6], uuid: 'next-user', parentUuid: records[3].uuid }],
  ]) {
    await readInPieces(
      'claude-code',
      session,
      [...records, later].map((record) => JSON.stringify(record)),
      name,
    )
  }
})

test('completion/codex: an answer after an errored turn, read in pieces, is read whole', async () => {
  const lines = [
    ...(await fixtureLines('codex/errored-task-complete.jsonl')),
    ...(await fixtureLines('codex/completed.jsonl')),
  ]
  await readInPieces(
    'codex',
    '01a074ec-7aff-74b0-8cf6-aa00d8e451cb',
    lines,
    'errored, then completed',
  )
})

// ------------------------------------------------- shrunk, replaced, moved

test('completion: a transcript that shrinks, is rewritten, is replaced or moves is read again from its start', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const completed = await fixtureLines('codex/completed.jsonl')
  const errored = await fixtureLines('codex/errored-task-complete.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('codex', session, root)
  const read = cachedAnswers()
  const look = (label) => agrees(read, 'codex', session, env, {}, label)

  await fs.writeFile(file, `${[...errored, ...completed].join('\n')}\n`)
  assert.equal((await look('whole')).settlement.state, 'settled')
  // Shorter: a rewrite that shrank it.
  await fs.writeFile(file, `${errored.slice(0, 11).join('\n')}\n`)
  assert.equal((await look('shrunk')).inFlight, true)
  // As long and longer, but other bytes where the last look stopped.
  await fs.writeFile(file, `${[...completed, ...errored].join('\n')}\n`)
  await look('rewritten in place')
  // Another file in its place.
  const other = `${file}.new`
  await fs.writeFile(other, `${[...errored, ...completed, ...completed].join('\n')}\n`)
  await fs.rename(other, file)
  await look('replaced')
  // Another one again, the same but for a word early on: its end is the same bytes.
  const earlier = [...errored, ...completed, ...completed]
    .join('\n')
    .replaceAll('deferred', 'DEFERRED')
  await fs.writeFile(other, `${earlier}\n`)
  await fs.rename(other, file)
  const sameEnd = await look('replaced, the same at its end')
  assert.ok(sameEnd.items.some((item) => item.text.includes('DEFERRED')))
  // Gone from where it was, and found where it is now.
  const moved = path.join(root, 'sessions', '2026', '09', '07')
  await fs.mkdir(moved, { recursive: true })
  await fs.rename(file, path.join(moved, path.basename(file)))
  await look('moved')
  await fs.rm(path.join(moved, path.basename(file)))
  assert.equal((await look('gone')).unknown, true)
})

test('completion: a whole unterminated last record that then grows into something else is read again', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const lines = await fixtureLines('codex/completed.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('codex', session, root)
  const read = cachedAnswers()
  await fs.writeFile(file, lines.join('\n'))
  const unterminated = await agrees(read, 'codex', session, env, {}, 'whole but unterminated')
  assert.equal(unterminated.settlement.state, 'settled', 'its task_complete is read')
  await fs.appendFile(file, '   ')
  await agrees(read, 'codex', session, env, {}, 'whitespace after it')
  await fs.appendFile(file, '{"type":"event_msg"}\n')
  const malformed = await agrees(read, 'codex', session, env, {}, 'more JSON on its line')
  assert.equal(malformed.unknown, true)
})

test('completion: a whole unterminated last record written over before its newline is read again', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const lines = await fixtureLines('codex/completed.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('codex', session, root)
  const read = cachedAnswers()
  await fs.writeFile(file, lines.join('\n'))
  await agrees(read, 'codex', session, env, {}, 'whole but unterminated')
  // The same bytes before it; in its place, another record just as long.
  const other = lines.at(-1).replace('the merge.', 'the merge!')
  await fs.writeFile(file, `${[...lines.slice(0, -1), other].join('\n')}\n`)
  const after = await agrees(read, 'codex', session, env, {}, 'written over')
  assert.notEqual(after.settlement.state, 'settled', 'its final answer no longer matches')
})

test('completion/pi: evidence that arrives while the transcript does not change settles the next look', async () => {
  const lines = await fixtureLines('pi/tool-loop.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('pi', 'hazy-ridge', root)
  const directory = path.join(root, 'settled')
  await fs.mkdir(directory)
  const options = { piSettlement: { directory, launchId: 'launch-1' } }
  const read = cachedAnswers()
  await fs.writeFile(file, `${lines.join('\n')}\n`)
  const before = await agrees(read, 'pi', 'hazy-ridge', env, options, 'no evidence yet')
  assert.equal(before.settlement.state, 'in-flight')
  await fs.writeFile(
    path.join(directory, 'launch-1.json'),
    JSON.stringify({ launchId: 'launch-1', sessionId: 'hazy-ridge', frontier: { id: '3f9b029e' } }),
  )
  const settled = await agrees(read, 'pi', 'hazy-ridge', env, options, 'its evidence written')
  assert.equal(settled.settlement.state, 'settled')
})

test('completion: looks at one conversation at the same time take turns', async () => {
  const session = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
  const lines = await fixtureLines('claude-code/queue-pop-all.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('claude-code', session, root)
  const read = cachedAnswers()
  for (const line of lines) {
    await fs.appendFile(file, `${line}\n`)
    const looks = await Promise.all([1, 2, 3].map(() => read('claude-code', session, env)))
    const whole = await answers('claude-code', session, env)
    for (const look of looks) assert.deepEqual(look, whole)
  }
})

// --------------------------------------------------------------- OpenCode

function upsert(db, table, row) {
  const columns = Object.keys(row)
  db.prepare(
    `insert or replace into ${table} (${columns.map((column) => `"${column}"`).join(', ')}) values (${columns.map(() => '?').join(', ')})`,
  ).run(...columns.map((column) => row[column]))
}

/** OpenCode's store with a fixture's conversation, but none of its messages, parts or events yet. */
async function opencodeStore(fixture) {
  const root = await tempRoot()
  await fs.mkdir(path.join(root, 'opencode'))
  const db = new DatabaseSync(path.join(root, 'opencode', 'opencode.db'))
  db.exec('pragma journal_mode = WAL')
  const session = fixture.session[0]
  db.exec(`create table session (${Object.keys(session)
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
    create unique index event_aggregate_seq_idx on event (aggregate_id, seq);`)
  upsert(db, 'session', session)
  return { db, env: { XDG_DATA_HOME: root } }
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
      write: {
        table: found.table,
        row: { ...found.row, data: JSON.stringify(rest) },
      },
    }
  })
}

const nativeEvents = async () =>
  JSON.parse(await fs.readFile(path.join(FIX, 'opencode/native-events.json'), 'utf8')).event

for (const name of [
  'completion-window',
  'finish-length',
  'api-error',
  'tool-result',
  'v130-tool-loop',
]) {
  test(`completion/opencode: ${name}.json read an event at a time is read whole`, async () => {
    const fixture = JSON.parse(await fs.readFile(path.join(FIX, `opencode/${name}.json`), 'utf8'))
    const { db, env } = await opencodeStore(fixture)
    const sessionId = fixture.session[0].id
    const read = cachedAnswers()
    try {
      const steps = opencodeGrowth(fixture, [...(fixture.event ?? []), ...(await nativeEvents())])
      assert.ok(steps.length > 0)
      for (const [index, { event, write }] of steps.entries()) {
        db.exec('begin immediate')
        if (write !== null) upsert(db, write.table, write.row)
        upsert(db, 'event', event)
        db.exec('commit')
        await agrees(read, 'opencode', sessionId, env, {}, `${name} after event ${index}`)
      }
      const whole = await agrees(read, 'opencode', sessionId, env, {}, `${name} unchanged`)
      assert.equal(whole.unknown, undefined, whole.reason)
    } finally {
      db.close()
    }
  })
}

test('completion/opencode: a store that loses a row or is replaced is read again whole', async () => {
  const fixture = JSON.parse(await fs.readFile(path.join(FIX, 'opencode/tool-result.json'), 'utf8'))
  const growth = opencodeGrowth(fixture, await nativeEvents())
  const grow = (db, steps) => {
    for (const { event, write } of steps) {
      if (write !== null) upsert(db, write.table, write.row)
      upsert(db, 'event', event)
    }
  }
  const { db, env } = await opencodeStore(fixture)
  const sessionId = fixture.session[0].id
  const read = cachedAnswers()
  grow(db, growth)
  const before = await agrees(read, 'opencode', sessionId, env, {}, 'whole')
  // A message removed, with no event of its own: its parts go with it.
  const removed = fixture.message.at(-1).id
  db.prepare('delete from part where message_id = ?').run(removed)
  db.prepare('delete from message where id = ?').run(removed)
  const after = await agrees(read, 'opencode', sessionId, env, {}, 'a message removed')
  assert.notDeepEqual(after, before)
  db.close()
  // Another store in its place, as this one is but for one part's words: as
  // many rows and events, none of them new.
  const file = path.join(env.XDG_DATA_HOME, 'opencode', 'opencode.db')
  for (const suffix of ['', '-wal', '-shm']) await fs.rm(`${file}${suffix}`, { force: true })
  const fresh = await opencodeStore(fixture)
  grow(fresh.db, growth)
  fresh.db.prepare('delete from part where message_id = ?').run(removed)
  fresh.db.prepare('delete from message where id = ?').run(removed)
  const part = fixture.part.find(
    (row) => row.message_id !== removed && JSON.parse(row.data).type === 'text',
  )
  fresh.db
    .prepare('update part set data = ? where id = ?')
    .run(JSON.stringify({ ...JSON.parse(part.data), text: 'in other words' }), part.id)
  fresh.db.close()
  await fs.rename(path.join(fresh.env.XDG_DATA_HOME, 'opencode', 'opencode.db'), file)
  const replaced = await agrees(read, 'opencode', sessionId, env, {}, 'replaced')
  assert.ok(replaced.items.some((item) => item.text === 'in other words'))
})

test('completion/opencode: an event of a type not followed has the store read whole', async () => {
  const fixture = JSON.parse(await fs.readFile(path.join(FIX, 'opencode/tool-result.json'), 'utf8'))
  const { db, env } = await opencodeStore(fixture)
  const sessionId = fixture.session[0].id
  const read = cachedAnswers()
  try {
    for (const { event, write } of opencodeGrowth(fixture, await nativeEvents())) {
      if (write !== null) upsert(db, write.table, write.row)
      upsert(db, 'event', event)
    }
    await agrees(read, 'opencode', sessionId, env, {}, 'whole')
    // A later OpenCode that changes a part through an event this reader does not know.
    const part = fixture.part.find((row) => JSON.parse(row.data).type === 'text')
    const data = { ...JSON.parse(part.data), text: 'rewritten by an unknown event' }
    db.prepare('update part set data = ? where id = ?').run(JSON.stringify(data), part.id)
    const seq = db.prepare('select max(seq) as seq from event').get().seq + 1
    upsert(db, 'event', {
      id: 'event-of-a-later-version',
      aggregate_id: sessionId,
      seq,
      type: 'message.part.revised.1',
      data: JSON.stringify({ sessionID: sessionId, partID: part.id }),
    })
    const after = await agrees(read, 'opencode', sessionId, env, {}, 'an unknown event')
    assert.ok(after.items.some((item) => item.text.includes('rewritten by an unknown event')))
  } finally {
    db.close()
  }
})

// ------------------------------------------------------------------ Devin

/** Devin's store and one launch's wire log, empty, in a temporary home. */
async function devinStore(sessionId) {
  const root = await tempRoot()
  const env = {
    HOME: root,
    XDG_DATA_HOME: path.join(root, 'data'),
    CONSENSFLOW_HOME: path.join(root, 'home'),
  }
  const store = path.join(devinFolders(env).data, 'cli')
  await fs.mkdir(store, { recursive: true })
  const db = new DatabaseSync(path.join(store, 'sessions.db'))
  db.exec(`CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id INTEGER);
    CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
      node_id INTEGER NOT NULL, parent_node_id INTEGER, chat_message TEXT NOT NULL,
      created_at INTEGER NOT NULL, UNIQUE(session_id, node_id))`)
  db.prepare('INSERT INTO sessions (id, main_chain_id) VALUES (?, NULL)').run(sessionId)
  const launch = path.join(env.CONSENSFLOW_HOME, 'integrations', 'devin', 'launch-1')
  await fs.mkdir(launch, { recursive: true })
  const wire = path.join(launch, 'wire.jsonl')
  await fs.writeFile(wire, '')
  return { env, db, wire, launch }
}

/**
 * Writes Devin's rows a row at a time (its main chain following each) beside
 * its wire log in line pieces, a look after each step, and last the fixture's
 * own main chain.
 */
async function devinInPieces({ session, rows, wire: events }, label) {
  const { env, db, wire } = await devinStore(session.id)
  const read = cachedAnswers()
  try {
    const wirePieces = pieces(events.map((event) => JSON.stringify(event)))
    const ordered = [...rows].sort((left, right) => left.row_id - right.row_id)
    const steps = Math.max(ordered.length, wirePieces.length)
    let row = 0
    let piece = 0
    for (let step = 1; step <= steps; step += 1) {
      for (; row < Math.ceil((step * ordered.length) / steps); row += 1) {
        const { row_id, node_id, parent_node_id, chat_message, created_at } = ordered[row]
        db.prepare(
          'INSERT INTO message_nodes (row_id, session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?, ?)',
        ).run(row_id, session.id, node_id, parent_node_id, chat_message, created_at)
        db.prepare('UPDATE sessions SET main_chain_id = ? WHERE id = ?').run(node_id, session.id)
      }
      for (; piece < Math.ceil((step * wirePieces.length) / steps); piece += 1)
        await fs.appendFile(wire, wirePieces[piece])
      await agrees(read, 'devin', session.id, env, {}, `${label} at step ${step}`)
    }
    db.prepare('UPDATE sessions SET main_chain_id = ? WHERE id = ?').run(
      session.main_chain_id,
      session.id,
    )
    return await agrees(read, 'devin', session.id, env, {}, `${label} on its main chain`)
  } finally {
    db.close()
  }
}

for (const name of ['native-tui', 'worker-tui']) {
  test(`completion/devin: ${name}.json read a row and a wire piece at a time is read whole`, async () => {
    const fixture = JSON.parse(await fs.readFile(path.join(FIX, `devin/${name}.json`), 'utf8'))
    const whole = await devinInPieces(fixture, name)
    assert.equal(whole.unknown, undefined, whole.reason)
  })
}

test('completion/devin: file-link.json read in pieces is read whole', async () => {
  const { streamed, stored } = JSON.parse(
    await fs.readFile(path.join(FIX, 'devin/file-link.json'), 'utf8'),
  )
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
  const whole = await devinInPieces(
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
    'file-link',
  )
  assert.equal(whole.settlement.state, 'settled')
})

test('completion/devin: a main chain that moves, a wire log that shrinks or goes, and a store that loses a row are read again', async () => {
  const fixture = JSON.parse(await fs.readFile(path.join(FIX, 'devin/worker-tui.json'), 'utf8'))
  const { env, db, wire, launch } = await devinStore(fixture.session.id)
  const sessionId = fixture.session.id
  const head = (node) =>
    db.prepare('UPDATE sessions SET main_chain_id = ? WHERE id = ?').run(node, sessionId)
  const read = cachedAnswers()
  try {
    for (const row of fixture.rows) {
      const { row_id, node_id, parent_node_id, chat_message, created_at } = row
      db.prepare(
        'INSERT INTO message_nodes (row_id, session_id, node_id, parent_node_id, chat_message, created_at) VALUES (?, ?, ?, ?, ?, ?)',
      ).run(row_id, sessionId, node_id, parent_node_id, chat_message, created_at)
    }
    head(fixture.session.main_chain_id)
    const lines = fixture.wire.map((event) => JSON.stringify(event))
    await fs.writeFile(wire, `${lines.join('\n')}\n`)
    const whole = await agrees(read, 'devin', sessionId, env, {}, 'whole')
    assert.equal(whole.unknown, undefined, whole.reason)
    // The window went back a message, and forward again: only its main chain moved.
    const nodes = new Map(fixture.rows.map((row) => [row.node_id, row]))
    head(nodes.get(fixture.session.main_chain_id).parent_node_id)
    const back = await agrees(read, 'devin', sessionId, env, {}, 'its main chain moved back')
    assert.notDeepEqual(back.items, whole.items)
    head(fixture.session.main_chain_id)
    await agrees(read, 'devin', sessionId, env, {}, 'and forward again')
    // A message rewritten in place, as one still being written could be.
    const last = nodes.get(fixture.session.main_chain_id)
    db.prepare('UPDATE message_nodes SET chat_message = ? WHERE row_id = ?').run(
      last.chat_message.replace(/"(text|content)":"/, '"$1":"Rewritten. '),
      last.row_id,
    )
    const rewritten = await agrees(read, 'devin', sessionId, env, {}, 'a row rewritten in place')
    assert.notDeepEqual(rewritten.items, whole.items, 'the rewrite shows')
    db.prepare('UPDATE message_nodes SET chat_message = ? WHERE row_id = ?').run(
      last.chat_message,
      last.row_id,
    )
    await agrees(read, 'devin', sessionId, env, {}, 'and back')
    // A second launch's log that says nothing of how a turn ended, then cut short.
    const second = path.join(path.dirname(launch), 'launch-2', 'wire.jsonl')
    const unended = lines.filter((line) => JSON.parse(line).cause === undefined).slice(0, 2)
    await fs.mkdir(path.dirname(second))
    await fs.writeFile(second, `${unended.join('\n')}\n`)
    await agrees(read, 'devin', sessionId, env, {}, 'a second launch')
    await fs.writeFile(second, `${unended[0]}\n`)
    await agrees(read, 'devin', sessionId, env, {}, 'its log cut short')
    // The first launch's folder removed, as a launch's end removes it: what
    // its log said of how each turn ended goes with it.
    await fs.rm(launch, { recursive: true })
    const gone = await agrees(read, 'devin', sessionId, env, {}, 'the first launch gone')
    assert.notDeepEqual(gone.items, whole.items)
    // A row removed off the main chain.
    const chain = new Set()
    for (let node = fixture.session.main_chain_id; node !== null; ) {
      chain.add(node)
      node = nodes.get(node).parent_node_id
    }
    const offChain = fixture.rows.find((row) => !chain.has(row.node_id))
    if (offChain !== undefined) {
      db.prepare('DELETE FROM message_nodes WHERE row_id = ?').run(offChain.row_id)
      await agrees(read, 'devin', sessionId, env, {}, 'a row removed')
    }
    db.prepare('DELETE FROM message_nodes WHERE node_id = ?').run(fixture.session.main_chain_id)
    assert.equal(
      (await agrees(read, 'devin', sessionId, env, {}, 'its head removed')).unknown,
      true,
    )
  } finally {
    db.close()
  }
})
