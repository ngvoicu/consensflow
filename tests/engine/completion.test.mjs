/**
 * Phase 3, TEST-PANE-23: lossless completion extraction and settlement.
 *
 * Fixtures are selected complete native records from the stores named in
 * fixtures/completion/README.md. Only leaf values are redacted. The tests
 * stage copies; production stores are opened read-only.
 */
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import * as completion from '../../hosts/lib/completion.js'
import { leadReady } from '../../hosts/lib/readiness.js'

const FIX = fileURLToPath(new URL('./fixtures/completion/', import.meta.url))
const { answers } = completion
const PI_QUIET_MS = 120_000

async function stageJsonl(kind, sessionId, fixture, options = {}) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-completion-'))
  const source = await fs.readFile(path.join(FIX, fixture), 'utf8')
  let records = source.trimEnd().split('\n')
  if (options.take !== undefined) records = records.slice(0, options.take)
  if (options.mutate)
    records = options.mutate(records.map((line) => JSON.parse(line))).map(JSON.stringify)
  if (options.prepend) records = [...options.prepend.map(JSON.stringify), ...records]

  let file
  let env
  if (kind === 'codex') {
    const dir = path.join(root, 'sessions')
    await fs.mkdir(dir, { recursive: true })
    file = path.join(dir, `rollout-2026-09-06T00-00-00-${sessionId}.jsonl`)
    env = { CODEX_HOME: root }
  } else if (kind === 'claude-code') {
    const dir = path.join(root, 'projects')
    await fs.mkdir(dir, { recursive: true })
    file = path.join(dir, `${sessionId}.jsonl`)
    env = { CLAUDE_CONFIG_DIR: root }
  } else if (kind === 'pi') {
    const dir = path.join(root, '.pi', 'agent', 'sessions', 'project')
    await fs.mkdir(dir, { recursive: true })
    file = path.join(dir, `2026-09-06T00-00-00-000Z_${sessionId}.jsonl`)
    env = { HOME: root }
  } else if (kind === 'kimi') {
    const dir = path.join(root, 'sessions', 'wd_fixture', sessionId, 'agents', 'main')
    await fs.mkdir(dir, { recursive: true })
    file = path.join(dir, 'wire.jsonl')
    env = { KIMI_CODE_HOME: root }
  } else {
    throw new Error(`no JSONL staging for ${kind}`)
  }

  if (kind === 'pi' && options.settlement !== undefined) {
    const settled = path.join(root, 'settled')
    await fs.mkdir(settled, { recursive: true })
    const settlement = options.settlement
    await fs.writeFile(
      path.join(settled, `${settlement.launchId}.json`),
      `${JSON.stringify({
        launchId: settlement.launchId,
        sessionId: settlement.sessionId ?? sessionId,
        frontier: { id: settlement.frontierId },
        settledAt: Date.now(),
      })}\n`,
    )
    env = {
      ...env,
      CF_DELIVERY_SETTLED: settled,
      CF_DELIVERY_LAUNCH_ID: settlement.expectedLaunchId ?? settlement.launchId,
    }
  }

  const ending = options.finalAppend ?? '\n'
  await fs.writeFile(file, `${records.join('\n')}${ending}`)
  if (options.ageMs !== undefined) {
    const old = new Date(Date.now() - options.ageMs)
    await fs.utimes(file, old, old)
  }
  return { env, file, root }
}

async function stageOpencode(fixtureName, options = {}) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-completion-opencode-'))
  const dir = path.join(root, 'opencode')
  await fs.mkdir(dir, { recursive: true })
  const fixture = JSON.parse(await fs.readFile(path.join(FIX, fixtureName), 'utf8'))
  const nativeEvents = JSON.parse(
    await fs.readFile(path.join(FIX, 'opencode/native-events.json'), 'utf8'),
  ).event
  const db = new DatabaseSync(path.join(dir, 'opencode.db'))
  db.exec('pragma journal_mode = WAL')
  db.exec(`
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
  `)

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
    assert.equal(id, messages[0].id)
    assert.equal(sessionID, messages[0].session_id)
    events = events.filter((row) => row.seq <= seq)
  }
  if (options.mutate) {
    options.mutate({ sessions, messages, parts, events })
  }
  if (options.eventThrough !== undefined) {
    events = events.filter((row) => row.seq <= options.eventThrough)
  }

  insertRows(db, 'session', sessions)
  insertRows(db, 'message', messages)
  insertRows(db, 'part', parts)
  insertRows(db, 'event', events)
  db.close()
  return { XDG_DATA_HOME: root }
}

function insertRows(db, table, rows) {
  for (const row of rows) {
    const columns = Object.keys(row)
    const statement = db.prepare(
      `insert into ${table} (${columns.join(', ')}) values (${columns.map(() => '?').join(', ')})`,
    )
    statement.run(...columns.map((column) => row[column]))
  }
}

function shape(result) {
  assert.equal(result.unknown, undefined)
  assert.ok(Array.isArray(result.items))
  assert.equal(new Set(result.items.map((item) => item.id)).size, result.items.length, 'unique ids')
  for (const item of result.items) {
    assert.equal(typeof item.id, 'string')
    assert.ok(item.id.length > 0)
    assert.ok(['user', 'assistant', 'tool'].includes(item.role))
    assert.equal(typeof item.text, 'string')
    assert.equal(typeof item.complete, 'boolean')
    assert.equal(typeof item.settled, 'boolean')
    assert.ok(item.at !== undefined && item.at !== null, 'native timestamp present')
    assert.ok(item.seq !== undefined && item.seq !== null, 'native total-order position present')
  }
  assert.equal(typeof result.inFlight, 'boolean')
  assert.equal(typeof result.cancelled, 'boolean')
  assert.equal(typeof result.replaced, 'boolean')
  assert.equal(typeof result.failed, 'boolean')
  assert.ok(result.failure === null || typeof result.failure === 'string')
  assert.ok(result.cursor !== undefined && result.cursor !== null)
  assert.equal(typeof result.version, 'string')
  assert.ok(['settled', 'in-flight', 'unknown'].includes(result.settlement.state))
  assert.ok(['native', 'derived', 'unknown'].includes(result.settlement.provenance))
  assert.equal(typeof result.settlement.evidence.complete, 'boolean')
  for (const key of ['openTools', 'queuedTurns', 'hooksInFlight']) {
    assert.ok(Array.isArray(result.settlement.evidence[key]))
  }
  if (result.settlement.state === 'settled') {
    assert.ok(result.settlement.cursor !== null)
    assert.equal(typeof result.settlement.boundary, 'string')
  }
  assert.ok(['ready', 'busy', 'draft', 'unknown'].includes(readiness(result).state))
}

function readiness(result, sinceCursor, kind) {
  return leadReady({
    answers: result,
    ...(kind === undefined ? {} : { kind }),
    draftLatched: false,
    epoch: 17,
    ...(sinceCursor === undefined ? {} : { sinceCursor }),
  })
}

function assertNotReady(result) {
  assert.notEqual(readiness(result).state, 'ready')
}

// ---------------------------------------------------------------- codex

test('completion/codex: native ids survive duplicate text and task_complete settles an exact final', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const { env } = await stageJsonl('codex', session, 'codex/completed.jsonl')
  const result = await answers('codex', session, env)
  shape(result)

  const users = result.items.filter((item) => item.role === 'user')
  assert.equal(users.length, 2, 'two distinct native ids are not text-deduplicated')
  assert.equal(users[0].text, users[1].text)
  assert.notEqual(users[0].id, users[1].id)
  const final = result.items.find((item) => item.role === 'assistant')
  assert.equal(final.id, 'msg_04d9db96cf5fb8de016a9da268ba0c87d296ccd58a43338425')
  assert.equal(final.complete, true)
  assert.equal(final.settled, true)
  assert.equal(result.version, '0.153.4')
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'native')
  assert.equal(result.settlement.boundary, 'task_complete')
  assert.equal(result.settlement.cursor, result.cursor)
  assert.deepEqual(result.settlement.evidence.openTools, [])
  assert.equal(readiness(result).state, 'ready')
})

test('completion/codex: native AgentMessage remains canonical when its response mirror has injected text', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const { env } = await stageJsonl('codex', session, 'codex/completed.jsonl', {
    mutate(records) {
      const mirror = records.find(
        (record) =>
          record.type === 'response_item' &&
          record.payload?.role === 'assistant' &&
          record.payload?.id === 'msg_04d9db96cf5fb8de016a9da268ba0c87d296ccd58a43338425',
      )
      mirror.payload.content[0].text +=
        '\n<oai-mem-citation>injected mirror text</oai-mem-citation>'
      return records
    },
  })
  const result = await answers('codex', session, env)
  shape(result)

  const final = result.items.find(
    (item) => item.id === 'msg_04d9db96cf5fb8de016a9da268ba0c87d296ccd58a43338425',
  )
  assert.equal(final.text, 'Understood. I’ll leave the tree untouched until you report the merge.')
  assert.equal(final.complete, true)
  assert.equal(final.settled, true)
  assert.equal(result.settlement.state, 'settled')
})

test('completion/codex: errored task_complete never promotes commentary and waits for sub-agent activity', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const partialStage = await stageJsonl('codex', session, 'codex/errored-task-complete.jsonl', {
    take: 11,
  })
  const partial = await answers('codex', session, partialStage.env)
  shape(partial)
  assert.equal(partial.inFlight, true)
  assert.equal(partial.settlement.state, 'in-flight')
  assert.deepEqual(partial.settlement.evidence.openTools, [
    'subagent:01a07741-0722-7131-82df-b7996049a6d5',
  ])

  const { env } = await stageJsonl('codex', session, 'codex/errored-task-complete.jsonl')
  const result = await answers('codex', session, env)
  shape(result)
  const commentary = result.items.find((item) => item.id.startsWith('msg_04d9'))
  assert.equal(commentary.complete, false)
  assert.equal(commentary.settled, false)
  assert.equal(result.items.filter((item) => item.role === 'user').length, 2)
  assert.ok(result.items.some((item) => item.id === 'exec-cda76eea-949c-497b-8122-95c610f87930'))
  assert.ok(result.items.some((item) => item.id === 'fco_01a07741-07e8-7cf3-8588-4c96e88b4323'))
  assert.equal(result.failed, true)
  assert.match(result.failure, /capacity/i)
  assert.equal(result.cancelled, false)
  assert.equal(result.inFlight, false)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'native')
  assert.equal(result.settlement.evidence.complete, false)
  assert.equal(result.settlement.boundary, 'task_complete')
  assert.ok(
    result.settlement.cursor < result.cursor,
    'later sub-agent activity follows the boundary',
  )
})

test('completion/codex: an earlier provider failure does not label a later successful turn failed', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const stage = await stageJsonl('codex', session, 'codex/errored-task-complete.jsonl')
  const completed = await fs.readFile(path.join(FIX, 'codex/completed.jsonl'), 'utf8')
  await fs.appendFile(stage.file, completed)

  const result = await answers('codex', session, stage.env)
  shape(result)
  assert.equal(result.failed, false)
  assert.equal(result.failure, null)
  assert.equal(result.cancelled, false)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.evidence.complete, true)
})

test('completion/codex: verified turn_aborted is cancellation; a native fork is replacement', async () => {
  const cancelledSession = '01a077f6-6663-7bc2-81cd-e287ccaabdbd'
  const cancelledStage = await stageJsonl('codex', cancelledSession, 'codex/interrupted.jsonl')
  const cancelled = await answers('codex', cancelledSession, cancelledStage.env)
  shape(cancelled)
  assert.equal(cancelled.cancelled, true)
  assert.equal(cancelled.failed, false)
  assert.equal(cancelled.inFlight, false)
  assert.equal(cancelled.settlement.state, 'settled')
  assert.equal(cancelled.settlement.boundary, 'turn_aborted')

  const forkSession = '01a077fa-5968-7b62-8fdd-043410a3d4b9'
  const forkStage = await stageJsonl('codex', forkSession, 'codex/forked.jsonl')
  const fork = await answers('codex', forkSession, forkStage.env)
  shape(fork)
  assert.equal(fork.replaced, true)
  assert.deepEqual(
    fork.items.map((item) => item.id),
    [
      'msg_01a077f8-bf56-7ae2-ae08-6700ad4bb8f1',
      'msg_015cc9d48025f610016a9db15cf11887d293d9a0435bc9a8b8',
    ],
  )
  assert.equal(fork.settlement.state, 'settled')
  assert.equal(readiness(fork).state, 'unknown', 'replacement voids a populated native proof')
})

test('completion/codex: a 60,000-character answer is never display-normalised', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const { env } = await stageJsonl('codex', session, 'codex/big-answer.jsonl')
  const result = await answers('codex', session, env)
  shape(result)
  assert.equal(result.items.find((item) => item.role === 'assistant').text.length, 60000)
})

// ------------------------------------------------------------- claude-code

test('completion/claude-code: fragments share message.id, server tool result is lossless, hook settles', async () => {
  const session = '15fba934-d727-4777-8791-123675a63649'
  const beforeStage = await stageJsonl('claude-code', session, 'claude-code/fragments.jsonl', {
    take: 5,
  })
  const before = await answers('claude-code', session, beforeStage.env)
  shape(before)
  assert.equal(before.items.filter((item) => item.role === 'assistant').length, 1)
  assert.equal(before.items.find((item) => item.role === 'assistant').complete, false)
  assert.equal(before.settlement.state, 'in-flight')
  assert.deepEqual(before.settlement.evidence.openTools, [])
  assert.equal(before.settlement.evidence.hooksInFlight.length, 1)

  const { env } = await stageJsonl('claude-code', session, 'claude-code/fragments.jsonl')
  const result = await answers('claude-code', session, env)
  shape(result)
  const assistant = result.items.find((item) => item.role === 'assistant')
  assert.equal(assistant.id, 'msg_011CeTZ4moLoyUafGxCpFzhW')
  assert.ok(assistant.text.endsWith('X'.repeat(7302)), 'the real long final fragment is whole')
  assert.equal(assistant.complete, true)
  assert.equal(assistant.settled, true)
  const tool = result.items.find((item) => item.role === 'tool')
  assert.equal(tool.id, 'srvtoolu_01EDse4eJ8ri24eacy6VeNmi')
  assert.match(tool.text, /advisor_redacted_result/)
  assert.match(tool.text, /redacted 4852 chars/)
  assert.equal(result.version, '2.1.247')
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'derived')
  assert.equal(result.settlement.boundary, 'system.stop_hook_summary')
  assert.deepEqual(result.settlement.evidence, {
    complete: true,
    openTools: [],
    queuedTurns: [],
    hooksInFlight: [],
  })
  assert.equal(readiness(result).state, 'ready')
})

test('completion/claude-code: fragment identity supports growth and repeated equal text', async () => {
  const session = '15fba934-d727-4777-8791-123675a63649'
  const { env } = await stageJsonl('claude-code', session, 'claude-code/fragments.jsonl', {
    mutate(records) {
      const first = records[0]
      const last = records[4]
      first.message.content[0].text = 'A'
      const grown = structuredClone(first)
      grown.message.content[0].text = 'AB'
      last.message.content[0].text = 'AB'
      records.splice(1, 0, grown)
      return records
    },
  })
  const result = await answers('claude-code', session, env)
  shape(result)
  assert.equal(result.items.find((item) => item.role === 'assistant').text, 'AB\nAB')
})

test('completion/claude-code: a real supported mid-session CLI upgrade reports the latest version', async () => {
  const session = '15fba934-d727-4777-8791-123675a63649'
  const { env } = await stageJsonl('claude-code', session, 'claude-code/fragments.jsonl', {
    mutate(records) {
      records.at(-1).version = '2.1.250'
      return records
    },
  })
  const result = await answers('claude-code', session, env)
  shape(result)
  assert.equal(result.version, '2.1.250')
  assert.equal(result.settlement.state, 'settled')
})

test('completion/claude-code: historical interrupt, advisor, and removed queue do not poison the current frontier', async () => {
  const session = '15fba934-d727-4777-8791-123675a63649'
  const history = (await fs.readFile(path.join(FIX, 'claude-code/frontier-history.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
  const { env } = await stageJsonl('claude-code', session, 'claude-code/fragments.jsonl', {
    mutate(records) {
      records.unshift(...history.slice(0, 3))
      records.splice(-1, 0, ...history.slice(3))
      return records
    },
  })
  const result = await answers('claude-code', session, env)
  shape(result)

  assert.equal(result.cancelled, false)
  assert.equal(result.inFlight, false)
  assert.equal(result.settlement.state, 'settled')
  assert.deepEqual(result.settlement.evidence.openTools, [])
  assert.deepEqual(result.settlement.evidence.queuedTurns, [])
})

test('completion/claude-code: queued work prevents a settled window through dequeue and hooks', async () => {
  const session = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
  const queuedStage = await stageJsonl('claude-code', session, 'claude-code/queued-turn.jsonl', {
    take: 4,
  })
  const queued = await answers('claude-code', session, queuedStage.env)
  shape(queued)
  assert.equal(queued.settlement.evidence.complete, true)
  assert.equal(queued.settlement.evidence.queuedTurns.length, 1)
  assert.equal(queued.settlement.state, 'in-flight')
  assert.equal(queued.inFlight, true)

  const { env } = await stageJsonl('claude-code', session, 'claude-code/queued-turn.jsonl')
  const nextTurn = await answers('claude-code', session, env)
  shape(nextTurn)
  assert.deepEqual(nextTurn.settlement.evidence.queuedTurns, [])
  assert.equal(nextTurn.settlement.evidence.complete, false)
  assert.equal(nextTurn.settlement.state, 'in-flight')
  assert.equal(nextTurn.inFlight, true, 'the dequeued user turn is now open')
})

test('completion/claude-code: the captured interrupt cancels; compaction keeps prior answers', async () => {
  const session = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
  const interruptedStage = await stageJsonl('claude-code', session, 'claude-code/interrupted.jsonl')
  const interrupted = await answers('claude-code', session, interruptedStage.env)
  shape(interrupted)
  assert.equal(interrupted.cancelled, true)
  assert.equal(interrupted.inFlight, false)
  assert.equal(interrupted.settlement.state, 'settled')
  assert.equal(interrupted.settlement.boundary, 'user.request_interrupted')
  assert.equal(readiness(interrupted).state, 'ready')

  const quotedStage = await stageJsonl('claude-code', session, 'claude-code/interrupted.jsonl', {
    mutate(records) {
      records[0].uuid = 'ordinary-user-quoted-marker'
      records[0].message.content[0].text =
        'Please quote "[Request interrupted by user]" in the report.'
      delete records[0].interruptedMessageId
      return records
    },
  })
  const quoted = await answers('claude-code', session, quotedStage.env)
  shape(quoted)
  assert.equal(quoted.cancelled, false)
  assert.equal(quoted.settlement.state, 'in-flight')
  assertNotReady(quoted)

  const compactedStage = await stageJsonl('claude-code', session, 'claude-code/compaction.jsonl')
  const compacted = await answers('claude-code', session, compactedStage.env)
  shape(compacted)
  assert.ok(compacted.items.some((item) => item.id === 'msg_011CeLQKZ3EKM5fCw4temirx'))
  assert.equal(compacted.items.find((item) => item.role === 'assistant').settled, true)
  assert.equal(compacted.inFlight, true, 'the post-compaction user turn is open')
})

test('completion/claude-code: popAll consumes every popped item and later queue history reconciles', async () => {
  const session = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
  const poppedStage = await stageJsonl('claude-code', session, 'claude-code/queue-pop-all.jsonl', {
    take: 5,
  })
  const popped = await answers('claude-code', session, poppedStage.env)
  shape(popped)
  assert.deepEqual(popped.settlement.evidence.queuedTurns, ['queue:1', 'queue:2'])
  assertNotReady(popped)

  const consumedStage = await stageJsonl(
    'claude-code',
    session,
    'claude-code/queue-pop-all.jsonl',
    { take: 8 },
  )
  const consumed = await answers('claude-code', session, consumedStage.env)
  shape(consumed)
  assert.deepEqual(consumed.settlement.evidence.queuedTurns, [])
  assertNotReady(consumed)

  const finalStage = await stageJsonl('claude-code', session, 'claude-code/queue-pop-all.jsonl')
  const final = await answers('claude-code', session, finalStage.env)
  shape(final)
  assert.deepEqual(final.settlement.evidence.queuedTurns, [])
  assert.equal(final.settlement.boundary, 'system.stop_hook_summary')
  assert.equal(final.settlement.state, 'settled')
  assert.equal(readiness(final).state, 'ready')
})

test('completion/claude-code: native API error settles incomplete as failure, not cancellation', async () => {
  const session = '33383216-87a0-4e6d-a273-07c4b229cdb1'
  const { env } = await stageJsonl('claude-code', session, 'claude-code/provider-429.jsonl')
  const result = await answers('claude-code', session, env)
  shape(result)

  assert.equal(result.cancelled, false)
  assert.equal(result.failed, true)
  assert.match(result.failure, /429|rate limit/i)
  assert.equal(result.inFlight, false)
  assert.equal(result.items.at(-1).complete, false)
  assert.equal(result.items.at(-1).settled, true)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'native')
  assert.equal(result.settlement.evidence.complete, false)
  assert.equal(result.settlement.boundary, 'assistant.api_error')
})

// -------------------------------------------------------------------- pi

test('completion/pi: a turn stays open between tool results and the next assistant step', async () => {
  const session = 'hazy-ridge'
  const betweenStage = await stageJsonl('pi', session, 'pi/between-tool-steps.jsonl', { take: 5 })
  const between = await answers('pi', session, betweenStage.env)
  shape(between)
  assert.deepEqual(between.settlement.evidence.openTools, [])
  assert.equal(between.settlement.state, 'in-flight')
  assert.equal(between.inFlight, true)
  assert.equal(between.items.filter((item) => item.role === 'tool').length, 2)

  const { env } = await stageJsonl('pi', session, 'pi/between-tool-steps.jsonl')
  const next = await answers('pi', session, env)
  shape(next)
  assert.equal(next.inFlight, true)
  assert.equal(next.settlement.evidence.openTools.length, 2)
})

test('completion/pi: toolCallId closes the loop and a 120-second quiet window derives settlement', async () => {
  const { env } = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    ageMs: PI_QUIET_MS + 1_000,
  })
  const result = await answers('pi', 'hazy-ridge', env)
  shape(result)
  assert.deepEqual(
    result.items.map((item) => item.role),
    ['user', 'assistant', 'tool', 'tool', 'assistant'],
  )
  const final = result.items.at(-1)
  assert.equal(final.text.length, 1878)
  assert.equal(final.complete, true)
  assert.equal(final.settled, true)
  assert.equal(result.inFlight, false)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'derived')
  assert.equal(result.settlement.boundary, 'session.quiet_window')
  assert.equal(readiness(result, undefined, 'pi').state, 'unknown')
})

test('completion/pi: matching settlement evidence promotes the native boundary, mismatches stay derived', async () => {
  const settledStage = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    settlement: { launchId: 'launch-pi-1', frontierId: '3f9b029e' },
  })
  const settled = await answers('pi', 'hazy-ridge', settledStage.env)
  shape(settled)
  assert.equal(settled.settlement.state, 'settled')
  assert.equal(settled.settlement.provenance, 'native')
  assert.equal(settled.settlement.boundary, 'agent_settled')
  assert.equal(settled.settlement.evidence.complete, true)

  const wrongFrontierStage = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    settlement: { launchId: 'launch-pi-1', frontierId: 'not-the-leaf' },
  })
  const wrongFrontier = await answers('pi', 'hazy-ridge', wrongFrontierStage.env)
  assert.equal(wrongFrontier.settlement.provenance, 'derived')

  const wrongLaunchStage = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    settlement: {
      launchId: 'launch-pi-1',
      expectedLaunchId: 'another-launch',
      frontierId: '3f9b029e',
    },
  })
  const wrongLaunch = await answers('pi', 'hazy-ridge', wrongLaunchStage.env)
  assert.equal(wrongLaunch.settlement.provenance, 'derived')
})

test('completion/pi: every retry prefix stays unready until success plus the real quiet boundary', async () => {
  const session = 'triton-jade-fern'
  for (const take of [2, 3, 4, 5]) {
    const stage = await stageJsonl('pi', session, 'pi/provider-429.jsonl', { take })
    const prefix = await answers('pi', session, stage.env)
    shape(prefix)
    assert.equal(prefix.settlement.provenance, 'derived')
    assert.notEqual(prefix.settlement.state, 'settled', `record prefix ${take} settled early`)
    assertNotReady(prefix)
  }

  const exhaustedStage = await stageJsonl('pi', session, 'pi/provider-429.jsonl', {
    take: 4,
    ageMs: PI_QUIET_MS + 1_000,
  })
  const exhausted = await answers('pi', session, exhaustedStage.env)
  shape(exhausted)
  assert.equal(exhausted.cancelled, false)
  assert.equal(exhausted.failed, true)
  assert.match(exhausted.failure, /429|rate limit/i)
  assert.equal(exhausted.settlement.state, 'unknown')
  assert.equal(exhausted.settlement.provenance, 'derived')
  assertNotReady(exhausted)

  const settledStage = await stageJsonl('pi', session, 'pi/provider-429.jsonl', {
    ageMs: PI_QUIET_MS + 1_000,
  })
  const settled = await answers('pi', session, settledStage.env)
  shape(settled)
  assert.equal(settled.cancelled, false)
  assert.equal(settled.failed, false)
  assert.equal(settled.items.at(-1).id, '465fb416')
  assert.equal(settled.settlement.state, 'settled')
  assert.equal(settled.settlement.provenance, 'derived')
  assert.equal(settled.settlement.boundary, 'session.quiet_window')
  assert.equal(readiness(settled, undefined, 'pi').state, 'unknown')
})

// ------------------------------------------------------------------ kimi

test('completion/pi: historical finals remain readable during the next turn, with open tools excluded', async () => {
  for (const openTool of [false, true]) {
    const staged = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
      mutate: (rows) => [
        ...rows.filter((row) => !openTool || row.message?.role !== 'toolResult'),
        {
          type: 'message',
          id: 'next-user',
          timestamp: new Date().toISOString(),
          message: { role: 'user', content: [{ type: 'text', text: 'Continue working' }] },
        },
      ],
    })
    try {
      const result = await answers('pi', 'hazy-ridge', staged.env)
      assert.equal(result.inFlight, true)
      assert.equal(result.settlement.state, 'in-flight')
      assert.equal(result.items.find((item) => item.id === '3f9b029e').settled, !openTool)
      assert.equal(result.items.find((item) => item.id === '4cea719d').settled, false)
    } finally {
      await fs.rm(staged.root, { recursive: true, force: true })
    }
  }
})

test('completion/kimi: a new turn preserves earlier completed results, never unfinished tools', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  for (const openTool of [false, true]) {
    const staged = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl', {
      mutate: (rows) => [
        ...rows.filter((row) => !openTool || row.event?.type !== 'tool.result'),
        { type: 'prompt.accepted', promptId: 'next-prompt' },
        { type: 'context.append_loop_event', event: { type: 'step.begin', turnId: 'next-turn' } },
      ],
    })
    try {
      const result = await answers('kimi', session, staged.env)
      assert.equal(result.inFlight, true)
      assert.equal(
        result.items.find((item) => item.id === 'e7b213db-15f9-49f4-bd2a-bffee2d1791d').settled,
        !openTool,
      )
      assert.equal(
        result.items.find((item) => item.id === 'd26c913f-c98b-4262-8340-06a147aa7937').settled,
        false,
      )
    } finally {
      await fs.rm(staged.root, { recursive: true, force: true })
    }
  }
})

test('completion/kimi: parentUuid/toolCallId closes the originating turn and emits tool output', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const afterResultStage = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl', { take: 8 })
  const afterResult = await answers('kimi', session, afterResultStage.env)
  shape(afterResult)
  assert.deepEqual(afterResult.settlement.evidence.openTools, [])
  assert.equal(afterResult.inFlight, true, 'a tool result does not end the turn')

  const { env } = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl')
  const result = await answers('kimi', session, env)
  shape(result)
  const tool = result.items.find((item) => item.role === 'tool')
  assert.equal(tool.id, '8a2310db-75a0-4e59-bbde-274f2845eede')
  assert.match(tool.text, /public https:\/\/github.com\/ngvoicu\/consensflow/)
  const assistants = result.items.filter((item) => item.role === 'assistant')
  assert.equal(assistants[0].id, 'd26c913f-c98b-4262-8340-06a147aa7937')
  assert.equal(assistants.at(-1).id, 'e7b213db-15f9-49f4-bd2a-bffee2d1791d')
  assert.equal(assistants.at(-1).complete, true)
  assert.equal(assistants.at(-1).settled, true)
  assert.equal(result.inFlight, false)
  assert.equal(result.version, '1.5')
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'native')
  assert.equal(result.settlement.boundary, 'turn.ended')
  assert.equal(readiness(result).state, 'ready')
})

test('completion/kimi: history extraction preserves the latest turn queued-admission guard', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const staged = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl', {
    mutate: (rows) => [
      ...rows.slice(0, -1),
      { type: 'prompt.accepted', promptId: 'queued-during-turn' },
      rows.at(-1),
    ],
  })
  try {
    const result = await answers('kimi', session, staged.env)
    assert.equal(result.settlement.state, 'in-flight')
    assert.equal(result.items.at(-1).complete, true)
    assert.equal(result.items.at(-1).settled, false)
  } finally {
    await fs.rm(staged.root, { recursive: true, force: true })
  }
})

test('completion/kimi: prompt.accepted invalidates a prior turn and supplies the following prompt id', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const admittedStage = await stageJsonl('kimi', session, 'kimi/admitted-prompt.jsonl', {
    take: 6,
  })
  const admitted = await answers('kimi', session, admittedStage.env)
  shape(admitted)
  assert.equal(admitted.settlement.state, 'in-flight')
  assert.deepEqual(admitted.settlement.evidence.queuedTurns, ['msg_01M0TAMK4JC65YQKSQCQ5F0A1Q'])
  assertNotReady(admitted)

  const promptStage = await stageJsonl('kimi', session, 'kimi/admitted-prompt.jsonl')
  const prompt = await answers('kimi', session, promptStage.env)
  shape(prompt)
  assert.equal(prompt.settlement.state, 'in-flight')
  assert.deepEqual(prompt.settlement.evidence.queuedTurns, [])
  assert.equal(prompt.items.at(-1).id, 'msg_01M0TAMK4JC65YQKSQCQ5F0A1Q')
  assertNotReady(prompt)
})

test('completion/kimi: content-part identity supports growth and repeated equal text', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const { env } = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl', {
    mutate(records) {
      const firstPartIndex = records.findIndex(
        (record) => record.event?.type === 'content.part' && record.event?.part?.type === 'text',
      )
      const first = records[firstPartIndex]
      first.event.part.text = 'A'
      const grown = structuredClone(first)
      grown.event.part.text = 'AB'
      const repeated = structuredClone(grown)
      repeated.event.uuid = 'distinct-equal-content-part'
      records.splice(firstPartIndex + 1, 0, grown, repeated)
      return records
    },
  })
  const result = await answers('kimi', session, env)
  shape(result)
  assert.equal(
    result.items.find((item) => item.id === 'd26c913f-c98b-4262-8340-06a147aa7937').text,
    'ABAB',
  )
})

test('completion/kimi: context message id survives an earlier-prompt rewrite', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const baseStage = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl')
  const base = await answers('kimi', session, baseStage.env)
  const original = base.items.find((item) => item.role === 'user')

  const earlier = {
    type: 'turn.prompt',
    agentId: 'main',
    input: [{ type: 'text', text: 'Earlier prompt' }],
    origin: { kind: 'user' },
    time: 1787000000000,
  }
  const earlierMessage = {
    type: 'context.append_message',
    agentId: 'main',
    message: {
      role: 'user',
      content: [{ type: 'text', text: 'Earlier prompt' }],
      toolCalls: [],
      origin: { kind: 'user' },
      id: 'msg_earlier_native_prompt',
    },
    time: 1787000000001,
  }
  const rewrittenStage = await stageJsonl('kimi', session, 'kimi/tool-result.jsonl', {
    prepend: [earlier, earlierMessage],
  })
  const rewritten = await answers('kimi', session, rewrittenStage.env)
  shape(rewritten)
  assert.equal(original.id, 'msg_01M0TA752ZAJKC8PRMYSJE95WY')
  assert.ok(rewritten.items.some((item) => item.id === original.id))
})

test('completion/kimi: an older superseded tool cannot poison a later native settlement', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const { env } = await stageJsonl('kimi', session, 'kimi/superseded-tool.jsonl')
  const result = await answers('kimi', session, env)
  shape(result)

  assert.equal(result.inFlight, false)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.boundary, 'turn.ended')
  assert.deepEqual(result.settlement.evidence.openTools, [])
})

test('completion/kimi: native provider failure settles incomplete and is never cancellation', async () => {
  const session = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const { env } = await stageJsonl('kimi', session, 'kimi/provider-429.jsonl')
  const result = await answers('kimi', session, env)
  shape(result)

  assert.equal(result.cancelled, false)
  assert.equal(result.failed, true)
  assert.match(result.failure, /429|overloaded/i)
  assert.equal(result.inFlight, false)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'native')
  assert.equal(result.settlement.evidence.complete, false)
  assert.equal(result.settlement.boundary, 'turn.ended')
})

test('completion/kimi: real protocol 1.4 without turn.ended is unsupported, not forever busy', async () => {
  const session = 'session_159aa36f-e114-4bef-a9d2-144efdb84c10'
  const { env } = await stageJsonl('kimi', session, 'kimi/protocol-1.4-no-turn-ended.jsonl')
  const result = await answers('kimi', session, env)
  assert.equal(result.unknown, true)
  assert.match(result.reason, /unsupported version 1\.4/i)
})

// --------------------------------------------------------------- opencode

test('completion/opencode: step-finish is in-flight until native time.completed appears', async () => {
  const session = 'ses_f88c0c7cdffeANJRVLwiBceADi'
  const before = await answers(
    'opencode',
    session,
    await stageOpencode('opencode/completion-window.json', { snapshot: 'before' }),
  )
  shape(before)
  assert.equal(before.items.find((item) => item.role === 'assistant').complete, false)
  assert.equal(before.inFlight, true)
  assert.equal(before.settlement.state, 'in-flight')

  const after = await answers(
    'opencode',
    session,
    await stageOpencode('opencode/completion-window.json', { snapshot: 'after' }),
  )
  shape(after)
  assert.equal(after.items.find((item) => item.role === 'assistant').complete, true)
  assert.equal(after.inFlight, false)
  assert.equal(after.settlement.state, 'settled')
  assert.equal(after.settlement.provenance, 'native')
  assert.equal(after.settlement.boundary, 'message.time.completed')
  assert.ok(before.cursor < after.cursor)
  assert.equal(after.items[0].seq, before.items[0].seq)
  assert.ok(after.items[0].seq < after.settlement.cursor)
  assert.equal(after.settlement.cursor, after.cursor)
  assert.equal(readiness(after).state, 'ready')
})

test('completion/opencode: native length and APIError are settled incomplete, failure is separate', async () => {
  const length = await answers(
    'opencode',
    'ses_f886ed7dbffe1myK161SPisoR2',
    await stageOpencode('opencode/finish-length.json'),
  )
  shape(length)
  assert.equal(length.items.at(-1).complete, false)
  assert.equal(length.failed, false)
  assert.equal(length.cancelled, false)
  assert.equal(length.inFlight, false)
  assert.equal(length.settlement.state, 'settled')
  assert.equal(length.settlement.evidence.complete, false)

  const failed = await answers(
    'opencode',
    'ses_f9905d94effe57fADEYMVwfKVF',
    await stageOpencode('opencode/api-error.json'),
  )
  shape(failed)
  assert.equal(failed.failed, true)
  assert.equal(failed.cancelled, false)
  assert.match(failed.failure, /18\+ age confirmation/)
  assert.equal(failed.inFlight, false)
  assert.equal(failed.settlement.state, 'settled')
  assert.equal(failed.settlement.provenance, 'native')
})

test('completion/opencode: MessageAbortedError is a failure, never cancellation without a native fixture', async () => {
  const env = await stageOpencode('opencode/api-error.json', {
    mutate({ messages }) {
      const data = JSON.parse(messages[0].data)
      data.error.name = 'MessageAbortedError'
      messages[0].data = JSON.stringify(data)
    },
  })
  const result = await answers('opencode', 'ses_f9905d94effe57fADEYMVwfKVF', env)
  shape(result)
  assert.equal(result.cancelled, false)
  assert.equal(result.failed, true)
  assert.match(result.failure, /18\+ age confirmation/)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.settlement.provenance, 'native')
})

test('completion/opencode: tool output and long final text are emitted whole from one snapshot', async () => {
  const session = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'
  const result = await answers(
    'opencode',
    session,
    await stageOpencode('opencode/tool-result.json'),
  )
  shape(result)
  const tool = result.items.find((item) => item.role === 'tool')
  assert.equal(tool.id, 'prt_07834de0e001LFavSKJTkpG9xh')
  assert.match(tool.text, /Build fixtures/)
  const final = result.items.find((item) => item.id === 'msg_07834e4450017pCEw2eVdPoUxQ')
  assert.equal(final.text.length, 4515)
  assert.equal(final.complete, true)
  assert.equal(result.settlement.state, 'settled')
  assert.equal(readiness(result).state, 'ready')
})

test('completion/opencode: metadata updates preserve native admission order and settlement', async () => {
  const session = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'
  const before = await answers(
    'opencode',
    session,
    await stageOpencode('opencode/tool-result.json', { eventThrough: 2362 }),
  )
  const after = await answers('opencode', session, await stageOpencode('opencode/tool-result.json'))
  shape(before)
  shape(after)

  const expectedIds = [
    'msg_0781dd0a1001NxYXxddKs7UGGM',
    'prt_07834de0e001LFavSKJTkpG9xh',
    'msg_07834cf2a001ZxKIEEils8Twxd',
    'msg_07834e4450017pCEw2eVdPoUxQ',
  ]
  assert.deepEqual(
    before.items.map((item) => item.id),
    expectedIds,
  )
  assert.deepEqual(
    after.items.map((item) => item.id),
    expectedIds,
  )
  assert.equal(after.settlement.state, 'settled')
  assert.equal(readiness(after).state, 'ready')
  assert.equal(after.settlement.cursor, before.settlement.cursor)
  assert.ok(after.cursor > before.cursor, 'the metadata event advances only the snapshot frontier')
  assert.equal(
    after.items[0].seq,
    before.items[0].seq,
    'the original prompt keeps its admission position',
  )
  assert.deepEqual(
    completion.itemsAfterCursor('opencode', after.items, before.cursor),
    [],
    'old prompt text is not newly eligible after the completed snapshot cursor',
  )
})

test('completion/opencode: native event seq is the opaque cursor and orders equal timestamps', async () => {
  const session = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'
  const env = await stageOpencode('opencode/tool-result.json', {
    mutate({ messages, parts }) {
      for (const row of [...messages, ...parts]) {
        row.time_created = 777
        row.time_updated = 777
        const data = JSON.parse(row.data)
        if (data.time) {
          for (const key of Object.keys(data.time)) data.time[key] = 777
        }
        if (data.state?.time) {
          for (const key of Object.keys(data.state.time)) data.state.time[key] = 777
        }
        row.data = JSON.stringify(data)
      }
    },
  })
  const result = await answers('opencode', session, env)
  shape(result)
  assert.ok(result.cursor > result.settlement.cursor)
  assert.deepEqual(
    result.items.map((item) => item.id),
    [
      'msg_0781dd0a1001NxYXxddKs7UGGM',
      'prt_07834de0e001LFavSKJTkpG9xh',
      'msg_07834cf2a001ZxKIEEils8Twxd',
      'msg_07834e4450017pCEw2eVdPoUxQ',
    ],
  )
  assert.ok(result.items[0].seq < result.items[1].seq)
  assert.ok(result.items[1].seq < result.items[2].seq)
  assert.ok(result.items[2].seq < result.items[3].seq)
  const intermediate = result.items.find((item) => item.id === 'msg_07834cf2a001ZxKIEEils8Twxd')
  assert.deepEqual(
    completion.itemsAfterCursor('opencode', result.items, intermediate.seq).map((item) => item.id),
    ['msg_07834e4450017pCEw2eVdPoUxQ'],
  )
  assert.deepEqual(completion.itemsAfterCursor('opencode', result.items, result.cursor), [])
})

test('completion/itemsAfterCursor: an unrecognised cursor returns null, not verified empty', async () => {
  const result = await answers(
    'opencode',
    'ses_f87e22f72ffewC2qJ2dAyyfPe1',
    await stageOpencode('opencode/tool-result.json'),
  )
  shape(result)
  assert.equal(completion.itemsAfterCursor('opencode', result.items, 2353), null)
})

test('completion/itemsAfterCursor: a cursor minted by another harness returns null', async () => {
  const opencode = await answers(
    'opencode',
    'ses_f87e22f72ffewC2qJ2dAyyfPe1',
    await stageOpencode('opencode/tool-result.json'),
  )
  const kimiSession = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const kimiStage = await stageJsonl('kimi', kimiSession, 'kimi/tool-result.jsonl')
  const kimi = await answers('kimi', kimiSession, kimiStage.env)
  shape(opencode)
  shape(kimi)
  assert.equal(completion.itemsAfterCursor('opencode', opencode.items, kimi.cursor), null)
})

test('completion/itemsAfterCursor: unparseable items return null, not verified empty', async () => {
  const result = await answers(
    'opencode',
    'ses_f87e22f72ffewC2qJ2dAyyfPe1',
    await stageOpencode('opencode/tool-result.json'),
  )
  shape(result)
  assert.equal(completion.itemsAfterCursor('opencode', undefined, result.cursor), null)
  assert.equal(
    completion.itemsAfterCursor('opencode', [{ id: 'broken', seq: 'not-a-cursor' }], result.cursor),
    null,
  )
  assert.equal(
    completion.itemsAfterCursor('opencode', [{ seq: result.items[0].seq }], result.cursor),
    null,
    'a valid cursor cannot make a malformed item readable',
  )
  const arrayItem = Object.assign([], result.items[0])
  assert.equal(completion.itemsAfterCursor('opencode', [arrayItem], result.cursor), null)
  assert.equal(completion.itemsAfterCursor('__proto__', [], 0), null)
  assert.equal(completion.itemsAfterCursor('toString', [], 0), null)
})

test('completion/settledAfter: readiness delegates cursor freshness to the owning adapter', async () => {
  const piStage = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    ageMs: PI_QUIET_MS + 1_000,
  })
  const pi = await answers('pi', 'hazy-ridge', piStage.env)
  const claudeSession = '15fba934-d727-4777-8791-123675a63649'
  const claudeStage = await stageJsonl('claude-code', claudeSession, 'claude-code/fragments.jsonl')
  const claude = await answers('claude-code', claudeSession, claudeStage.env)
  shape(pi)
  shape(claude)

  assert.equal(completion.settledAfter('pi', pi.settlement, pi.items[0].seq), true)
  assert.equal(completion.settledAfter('pi', pi.settlement, pi.settlement.cursor), false)
  assert.equal(completion.settledAfter('pi', pi.settlement, claude.settlement.cursor), null)

  const common = { answers: pi, kind: 'pi', draftLatched: false, epoch: 17 }
  assert.equal(leadReady({ ...common, sinceCursor: pi.items[0].seq }).state, 'unknown')
  assert.equal(leadReady({ ...common, sinceCursor: pi.settlement.cursor }).state, 'unknown')
  assert.equal(leadReady({ ...common, sinceCursor: claude.settlement.cursor }).state, 'unknown')
})

test('completion/opencode: one read transaction rejects a competing writer from its snapshot', async () => {
  const session = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'
  const env = await stageOpencode('opencode/tool-result.json')
  const dbFile = path.join(env.XDG_DATA_HOME, 'opencode', 'opencode.db')
  let writerRan = false
  const first = await answers('opencode', session, env, {
    betweenOpenCodeSnapshotReads() {
      const writer = new DatabaseSync(dbFile)
      const partId = 'prt_07834fd70001aAGLSN74ke5QX3'
      const row = writer.prepare('select * from part where id = ?').get(partId)
      const data = JSON.parse(row.data)
      data.text = 'written between snapshot reads'
      const seq = writer.prepare('select max(seq) as seq from event').get().seq + 1
      const eventData = JSON.stringify({
        sessionID: session,
        part: { id: partId, sessionID: session, messageID: row.message_id, ...data },
        time: 778,
      })
      writer.exec('begin immediate')
      writer
        .prepare('update part set data = ?, time_updated = ? where id = ?')
        .run(JSON.stringify(data), 778, partId)
      writer
        .prepare('insert into event (id, aggregate_id, seq, type, data) values (?, ?, ?, ?, ?)')
        .run('event-from-competing-writer', session, seq, 'message.part.updated.1', eventData)
      writer.exec('commit')
      writer.close()
      writerRan = true
    },
  })
  shape(first)
  assert.equal(writerRan, true)
  assert.equal(
    first.items.find((item) => item.id === 'msg_07834e4450017pCEw2eVdPoUxQ').text.length,
    4515,
  )

  const second = await answers('opencode', session, env)
  shape(second)
  assert.equal(
    second.items.find((item) => item.id === 'msg_07834e4450017pCEw2eVdPoUxQ').text,
    'written between snapshot reads',
  )
  assert.ok(second.cursor > first.cursor)
})

test('completion: every positive fixture keeps exact native identity across repeated reads', async () => {
  const cases = [
    {
      kind: 'codex',
      session: '01a074ec-7aff-74b0-8cf6-aa00d8e451cb',
      env: (
        await stageJsonl('codex', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb', 'codex/completed.jsonl')
      ).env,
      ids: [
        'msg_01a077c2-5dda-7d83-95a9-f770fa2023b1',
        '01a077c2-5ddd-76f0-aa8a-875af8f520ae',
        'msg_04d9db96cf5fb8de016a9da268ba0c87d296ccd58a43338425',
      ],
    },
    {
      kind: 'codex',
      session: '01a077fa-5968-7b62-8fdd-043410a3d4b9',
      env: (await stageJsonl('codex', '01a077fa-5968-7b62-8fdd-043410a3d4b9', 'codex/forked.jsonl'))
        .env,
      ids: [
        'msg_01a077f8-bf56-7ae2-ae08-6700ad4bb8f1',
        'msg_015cc9d48025f610016a9db15cf11887d293d9a0435bc9a8b8',
      ],
      readyState: 'unknown',
    },
    {
      kind: 'claude-code',
      session: '15fba934-d727-4777-8791-123675a63649',
      env: (
        await stageJsonl(
          'claude-code',
          '15fba934-d727-4777-8791-123675a63649',
          'claude-code/fragments.jsonl',
        )
      ).env,
      ids: ['srvtoolu_01EDse4eJ8ri24eacy6VeNmi', 'msg_011CeTZ4moLoyUafGxCpFzhW'],
    },
    {
      kind: 'claude-code',
      session: '1b09fb15-feb1-4595-9f47-5eb9ff768191',
      env: (
        await stageJsonl(
          'claude-code',
          '1b09fb15-feb1-4595-9f47-5eb9ff768191',
          'claude-code/queue-pop-all.jsonl',
        )
      ).env,
      ids: [
        'msg_011CeMhojBgvhpsANRYPv7wM',
        'msg_011CeMib8DXcSqLRwtK8cgv6',
        '21d31b27-c132-458d-82ff-f027b56d8c27',
        'msg_011CeN4TSS1YDxWFLbmQK18Z',
      ],
    },
    {
      kind: 'pi',
      session: 'hazy-ridge',
      env: (
        await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
          ageMs: PI_QUIET_MS + 1_000,
        })
      ).env,
      ids: ['cf0dfe1b', '4cea719d', 'e4c09431', '5f1ed4e9', '3f9b029e'],
    },
    {
      kind: 'pi',
      session: 'triton-jade-fern',
      env: (
        await stageJsonl('pi', 'triton-jade-fern', 'pi/provider-429.jsonl', {
          ageMs: PI_QUIET_MS + 1_000,
        })
      ).env,
      ids: ['5a83763c', '51cb4790', '8b4c70fe', '465fb416'],
    },
    {
      kind: 'kimi',
      session: 'session_11c123b3-dd33-4f21-8862-beabdc50cd18',
      env: (
        await stageJsonl(
          'kimi',
          'session_11c123b3-dd33-4f21-8862-beabdc50cd18',
          'kimi/tool-result.jsonl',
        )
      ).env,
      ids: [
        'msg_01M0TA752ZAJKC8PRMYSJE95WY',
        '8a2310db-75a0-4e59-bbde-274f2845eede',
        'd26c913f-c98b-4262-8340-06a147aa7937',
        'e7b213db-15f9-49f4-bd2a-bffee2d1791d',
      ],
    },
    {
      kind: 'kimi',
      session: 'session_11c123b3-dd33-4f21-8862-beabdc50cd18',
      env: (
        await stageJsonl(
          'kimi',
          'session_11c123b3-dd33-4f21-8862-beabdc50cd18',
          'kimi/superseded-tool.jsonl',
        )
      ).env,
      ids: [
        'msg_01M0T5V7XT3MA4EW8X1VS9GR0X',
        'msg_01M0TA752ZAJKC8PRMYSJE95WY',
        '8a2310db-75a0-4e59-bbde-274f2845eede',
        'd26c913f-c98b-4262-8340-06a147aa7937',
        'e7b213db-15f9-49f4-bd2a-bffee2d1791d',
      ],
    },
    {
      kind: 'opencode',
      session: 'ses_f87e22f72ffewC2qJ2dAyyfPe1',
      env: await stageOpencode('opencode/tool-result.json'),
      ids: [
        'msg_0781dd0a1001NxYXxddKs7UGGM',
        'prt_07834de0e001LFavSKJTkpG9xh',
        'msg_07834cf2a001ZxKIEEils8Twxd',
        'msg_07834e4450017pCEw2eVdPoUxQ',
      ],
    },
    {
      kind: 'opencode',
      session: 'ses_f88c0c7cdffeANJRVLwiBceADi',
      env: await stageOpencode('opencode/completion-window.json', { snapshot: 'after' }),
      ids: ['msg_0773f385a001oy2xD1d5J3DNge'],
    },
  ]

  for (const fixture of cases) {
    const first = await answers(fixture.kind, fixture.session, fixture.env)
    const second = await answers(fixture.kind, fixture.session, fixture.env)
    shape(first)
    shape(second)
    assert.deepEqual(
      first.items.map((item) => item.id),
      fixture.ids,
      fixture.kind,
    )
    assert.deepEqual(
      second.items.map(({ id, role }) => ({ id, role })),
      first.items.map(({ id, role }) => ({ id, role })),
      `${fixture.kind} identity changed without a native rewrite`,
    )
    assert.equal(
      readiness(first).state,
      fixture.readyState ?? 'ready',
      `${fixture.kind} positive fixture`,
    )
  }
})

test('completion: every adapter returns a 60,000-character native text leaf whole', async () => {
  const longText = 'L'.repeat(60000)

  const claudeSession = '15fba934-d727-4777-8791-123675a63649'
  const claudeStage = await stageJsonl(
    'claude-code',
    claudeSession,
    'claude-code/fragments.jsonl',
    {
      mutate(records) {
        records[4].message.content[0].text = longText
        return records
      },
    },
  )
  const claude = await answers('claude-code', claudeSession, claudeStage.env)
  assert.ok(claude.items.find((item) => item.role === 'assistant').text.endsWith(longText))

  const piStage = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    mutate(records) {
      records.at(-1).message.content[0].text = longText
      return records
    },
  })
  const pi = await answers('pi', 'hazy-ridge', piStage.env)
  assert.equal(pi.items.at(-1).text, longText)

  const kimiSession = 'session_11c123b3-dd33-4f21-8862-beabdc50cd18'
  const kimiStage = await stageJsonl('kimi', kimiSession, 'kimi/tool-result.jsonl', {
    mutate(records) {
      records.find(
        (record) =>
          record.event?.stepUuid === 'e7b213db-15f9-49f4-bd2a-bffee2d1791d' &&
          record.event?.part?.type === 'text',
      ).event.part.text = longText
      return records
    },
  })
  const kimi = await answers('kimi', kimiSession, kimiStage.env)
  assert.equal(
    kimi.items.find((item) => item.id === 'e7b213db-15f9-49f4-bd2a-bffee2d1791d').text,
    longText,
  )

  const opencodeSession = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'
  const opencodeEnv = await stageOpencode('opencode/tool-result.json', {
    mutate({ parts }) {
      const part = parts.find((candidate) => candidate.id === 'prt_07834fd70001aAGLSN74ke5QX3')
      part.data = JSON.stringify({ ...JSON.parse(part.data), text: longText })
    },
  })
  const opencode = await answers('opencode', opencodeSession, opencodeEnv)
  assert.equal(
    opencode.items.find((item) => item.id === 'msg_07834e4450017pCEw2eVdPoUxQ').text,
    longText,
  )
})

// ------------------------------------------------ versions, corruption, guards

test('completion: every adapter rejects an undeclared protocol or schema version', async () => {
  const codexSession = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const codexStage = await stageJsonl('codex', codexSession, 'codex/completed.jsonl', {
    mutate(records) {
      records[0].payload.cli_version = '0.154.0'
      return records
    },
  })
  const claudeSession = '15fba934-d727-4777-8791-123675a63649'
  const claudeStage = await stageJsonl(
    'claude-code',
    claudeSession,
    'claude-code/fragments.jsonl',
    {
      mutate(records) {
        for (const record of records) record.version = '2.2.0'
        return records
      },
    },
  )
  const piStage = await stageJsonl('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
    mutate(records) {
      records[0].version = 4
      return records
    },
  })

  const cases = [
    await answers('codex', codexSession, codexStage.env),
    await answers('claude-code', claudeSession, claudeStage.env),
    await answers('pi', 'hazy-ridge', piStage.env),
    await answers(
      'opencode',
      'ses_f88c0c7cdffeANJRVLwiBceADi',
      await stageOpencode('opencode/completion-window.json', { version: '1.19.0' }),
    ),
  ]
  for (const result of cases) {
    assert.equal(result.unknown, true)
    assert.match(result.reason, /unsupported version/i)
  }
})

test('completion: JSONL streams tolerate only an incomplete final append', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const incompleteStage = await stageJsonl('codex', session, 'codex/completed.jsonl', {
    finalAppend: '\n{"type":"event_msg"',
  })
  const incomplete = await answers('codex', session, incompleteStage.env)
  shape(incomplete)
  assert.equal(incomplete.settlement.state, 'settled')

  const malformedStage = await stageJsonl('codex', session, 'codex/completed.jsonl')
  const raw = await fs.readFile(malformedStage.file, 'utf8')
  const lines = raw.trimEnd().split('\n')
  lines.splice(2, 0, '{"malformed":')
  await fs.writeFile(malformedStage.file, `${lines.join('\n')}\n`)
  const malformed = await answers('codex', session, malformedStage.env)
  assert.equal(malformed.unknown, true)
  assert.match(malformed.reason, /malformed JSONL.*record 2/i)

  for (const finalAppend of ['\ndefinitely-not-json', '\n{"type":!}']) {
    const invalidStage = await stageJsonl('codex', session, 'codex/completed.jsonl', {
      finalAppend,
    })
    const invalid = await answers('codex', session, invalidStage.env)
    assert.equal(invalid.unknown, true, `${finalAppend} is invalid, not incomplete`)
    assert.match(invalid.reason, /malformed JSONL/i)
  }

  for (const [name, whitespace] of [
    ['vertical tab', '\v'],
    ['no-break space', '\u00a0'],
    ['form feed', '\f'],
  ]) {
    for (const finalAppend of [`\n${whitespace}`, `\n{"type":${whitespace}`]) {
      const invalidStage = await stageJsonl('codex', session, 'codex/completed.jsonl', {
        finalAppend,
      })
      const invalid = await answers('codex', session, invalidStage.env)
      assert.equal(invalid.unknown, true, `${name} is not JSON whitespace`)
      assert.match(invalid.reason, /malformed JSONL/i)
    }
  }
})

test('completion fixtures: Claude compaction names the real source transcript', async () => {
  const readme = await fs.readFile(path.join(FIX, 'README.md'), 'utf8')
  const start = readme.indexOf('- `compaction.jsonl`')
  const end = readme.indexOf('\n\n## Pi', start)
  const entry = readme.slice(start, end)
  assert.match(entry, /1b09fb15-feb1-4595-9f47-5eb9ff768191\.jsonl/)
  assert.doesNotMatch(entry, /the same source/)
})

test('completion: env is mandatory and never defaults to the ambient process', async () => {
  assert.equal(answers.length, 3)
  const result = await answers('codex', 'no-ambient-env')
  assert.equal(result.unknown, true)
  assert.match(result.reason, /explicit env/i)
})

test('completion: readable corrupted storage and absent storage fail closed', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-completion-corrupt-'))
  const dir = path.join(root, 'sessions')
  await fs.mkdir(dir, { recursive: true })
  await fs.writeFile(path.join(dir, 'rollout-corrupt.jsonl'), '{bad}\n')
  const corrupt = await answers('codex', 'corrupt', { CODEX_HOME: root })
  assert.equal(corrupt.unknown, true)
  assert.match(corrupt.reason, /malformed JSONL/)

  const empty = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-completion-empty-'))
  for (const kind of ['codex', 'claude-code', 'pi', 'kimi', 'opencode']) {
    const result = await answers(kind, 'no-such-session', { HOME: empty, XDG_DATA_HOME: empty })
    assert.deepEqual(Object.keys(result).sort(), ['reason', 'unknown'])
  }
})

test('completion: fixtures retain the decisive native source fields', async () => {
  const claude = (await fs.readFile(path.join(FIX, 'claude-code/fragments.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
  assert.equal(new Set(claude.slice(0, 5).map((record) => record.message.id)).size, 1)
  assert.ok(claude.some((record) => record.message?.content?.[0]?.type === 'server_tool_use'))
  const queue = (await fs.readFile(path.join(FIX, 'claude-code/queue-pop-all.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
  assert.equal(queue.filter((record) => record.operation === 'popAll').length, 2)
  assert.equal(queue.at(-1).subtype, 'stop_hook_summary')

  const pi = (await fs.readFile(path.join(FIX, 'pi/provider-429.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
  assert.equal(pi.filter((record) => record.message?.stopReason === 'error').length, 2)
  assert.match(
    pi.find((record) => record.message?.stopReason === 'error').message.errorMessage,
    /429|rate limit/i,
  )
  assert.equal(pi.at(-1).message.stopReason, 'stop')

  const admitted = (await fs.readFile(path.join(FIX, 'kimi/admitted-prompt.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
  assert.equal(admitted.at(-3).type, 'prompt.accepted')
  assert.equal(admitted.at(-3).promptId, 'msg_01M0TAMK4JC65YQKSQCQ5F0A1Q')
  assert.equal(admitted.at(-1).message.id, 'msg_01M0TAMK4JC65YQKSQCQ5F0A1Q')

  const kimi = (await fs.readFile(path.join(FIX, 'kimi/tool-result.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
    .find((record) => record.event?.type === 'tool.result')
  assert.ok(kimi.event.parentUuid)
  assert.ok(kimi.event.toolCallId)
  assert.equal(kimi.event.turnId, undefined)

  const opencode = JSON.parse(
    await fs.readFile(path.join(FIX, 'opencode/completion-window.json'), 'utf8'),
  )
  const before = JSON.parse(opencode.event.find((event) => event.seq === 13).data).info
  const after = JSON.parse(opencode.event.find((event) => event.seq === 15).data).info
  assert.equal(before.finish, 'stop')
  assert.equal(before.time.completed, undefined)
  assert.equal(after.time.completed, 1788707030665)
  const nativeEvents = JSON.parse(
    await fs.readFile(path.join(FIX, 'opencode/native-events.json'), 'utf8'),
  ).event
  assert.ok(nativeEvents.length > 0)
  assert.ok(
    nativeEvents.every(
      (event) =>
        typeof event.id === 'string' &&
        typeof event.aggregate_id === 'string' &&
        Number.isInteger(event.seq) &&
        typeof event.type === 'string' &&
        typeof event.data === 'string',
    ),
  )
  assert.deepEqual(
    nativeEvents
      .filter((event) => event.aggregate_id === 'ses_f87e22f72ffewC2qJ2dAyyfPe1')
      .filter((event) => event.seq >= 2361)
      .map((event) => event.seq),
    [2361, 2362, 2363, 2364],
  )

  const fork = (await fs.readFile(path.join(FIX, 'codex/forked.jsonl'), 'utf8'))
    .trim()
    .split('\n')
    .map(JSON.parse)
  assert.ok(fork.length > 1)
  assert.ok(fork.some((record) => record.payload?.item?.type === 'AgentMessage'))
})

test('completion: extraction is streamed, snapshot-consistent, and never display-normalised', async () => {
  const source = await fs.readFile(
    fileURLToPath(new URL('../../hosts/lib/completion.js', import.meta.url)),
    'utf8',
  )
  assert.match(source, /createReadStream/)
  assert.ok(!/adaptLine\s*\(/.test(source))
  assert.ok(!/from\s+['"][^'"]*transcript-events['"]/.test(source))
})
