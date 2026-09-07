import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { watch } from 'node:fs'
import fs from 'node:fs/promises'
import { createServer } from 'node:http'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { PassThrough } from 'node:stream'
import test from 'node:test'
import { answers } from '../hosts/lib/completion.js'
import { envelope, plan, pointer } from '../hosts/lib/deliveries.js'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { Bridge } from '../src/bridge.js'
import { launchConfiguration } from '../src/channels.js'
import { bookkeepingItems, HELD_ACTION, Watcher } from '../src/delivery-watch.js'
import { Store } from '../src/store.js'
import { leadIdentity, Tabs } from '../src/tabs.js'
import { tempEnv } from './helpers.mjs'

const FLOOR_MS = 20
const NEVER_FLOOR_MS = 60_000

function waitFor(predicate, timeoutMs = 4_000) {
  const started = Date.now()
  return new Promise((resolve, reject) => {
    const tick = async () => {
      try {
        if (await predicate()) return resolve()
      } catch (cause) {
        return reject(cause)
      }
      if (Date.now() - started > timeoutMs) return reject(new Error('timed out waiting'))
      setTimeout(tick, 5)
    }
    tick()
  })
}

function record(type, payload, ordinal) {
  return {
    timestamp: `2026-09-07T00:00:${String(ordinal).padStart(2, '0')}.000Z`,
    ordinal,
    type,
    payload,
  }
}

/** A small supported Codex store; completion.js, not a test adapter, reads it. */
function codexRecords(session, exchanges = [], { replaced = false } = {}) {
  let ordinal = 0
  const rows = [
    record(
      'session_meta',
      {
        session_id: session,
        id: replaced ? `${session}-replacement` : session,
        ...(replaced ? { forked_from_id: session } : {}),
        cli_version: '0.153.4',
      },
      ordinal++,
    ),
  ]
  exchanges.forEach((exchange, index) => {
    const turn = `${session}-turn-${index + 1}`
    rows.push(record('event_msg', { type: 'task_started', turn_id: turn }, ordinal++))
    rows.push(
      record(
        'response_item',
        {
          type: 'message',
          id: exchange.userId ?? `${turn}-user`,
          role: 'user',
          content: [{ type: 'input_text', text: exchange.user ?? `question ${index + 1}` }],
          internal_chat_message_metadata_passthrough: { turn_id: turn },
        },
        ordinal++,
      ),
    )
    if (exchange.tool !== undefined) {
      rows.push(
        record(
          'response_item',
          {
            type: 'function_call_output',
            id: exchange.toolId ?? `${turn}-tool`,
            call_id: `${turn}-call`,
            output: exchange.tool,
            internal_chat_message_metadata_passthrough: { turn_id: turn },
          },
          ordinal++,
        ),
      )
    }
    if (exchange.answer === undefined) return
    const answerId = exchange.answerId ?? `${turn}-answer`
    rows.push(
      record(
        'event_msg',
        {
          type: 'item_completed',
          turn_id: turn,
          item: {
            type: 'AgentMessage',
            id: answerId,
            content: [{ type: 'Text', text: exchange.answer }],
            phase: 'final_answer',
          },
        },
        ordinal++,
      ),
    )
    rows.push(
      record(
        'event_msg',
        { type: 'task_complete', turn_id: turn, last_agent_message: exchange.answer },
        ordinal++,
      ),
    )
  })
  return rows
}

async function writeCodexSession(env, session, exchanges, options) {
  const directory = path.join(env.CODEX_HOME, 'sessions', '2026', '09', '07')
  await fs.mkdir(directory, { recursive: true })
  const file = path.join(directory, `rollout-${session}.jsonl`)
  const rows = codexRecords(session, exchanges, options)
  await fs.writeFile(file, `${rows.map(JSON.stringify).join('\n')}\n`)
  return file
}

async function copyPiSession(env, session = 'hazy-ridge') {
  const source = new URL('./engine/fixtures/completion/pi/tool-loop.jsonl', import.meta.url)
  const directory = path.join(env.HOME, '.pi', 'agent', 'sessions', 'watcher')
  await fs.mkdir(directory, { recursive: true })
  const file = path.join(directory, `2026-09-07T00-00-00-000Z_${session}.jsonl`)
  await fs.copyFile(source, file)
  const fresh = new Date()
  await fs.utimes(file, fresh, fresh)
  return file
}

async function writePiSession(env, session, exchanges) {
  const directory = path.join(env.HOME, '.pi', 'agent', 'sessions', 'watcher')
  await fs.mkdir(directory, { recursive: true })
  const file = path.join(directory, `2026-09-07T00-00-00-000Z_${session}.jsonl`)
  const rows = [
    {
      type: 'session',
      version: 3,
      id: session,
      timestamp: '2026-09-07T00:00:00.000Z',
      cwd: '/workspace',
    },
  ]
  for (const [index, exchange] of exchanges.entries()) {
    rows.push({
      type: 'message',
      id: `${session}-user-${index + 1}`,
      timestamp: `2026-09-07T00:00:0${index + 1}.000Z`,
      message: {
        role: 'user',
        content: [{ type: 'text', text: exchange.user }],
      },
    })
    rows.push({
      type: 'message',
      id: exchange.answerId,
      timestamp: `2026-09-07T00:00:1${index + 1}.000Z`,
      message: {
        role: 'assistant',
        content: [{ type: 'text', text: exchange.answer }],
        stopReason: 'stop',
      },
    })
  }
  await fs.writeFile(file, `${rows.map(JSON.stringify).join('\n')}\n`)
  const old = new Date(Date.now() - 120_001)
  await fs.utimes(file, old, old)
  return file
}

async function writePiSettlement(env, launchId, session, frontierId) {
  const directory = path.join(env.CONSENSFLOW_HOME, 'pi-settled')
  await fs.mkdir(directory, { recursive: true })
  await fs.writeFile(
    path.join(directory, `${launchId}.json`),
    `${JSON.stringify({
      launchId,
      sessionId: session,
      frontier: { id: frontierId },
      settledAt: Date.now(),
    })}\n`,
  )
  return directory
}

async function writeOpenCodeSession(env, session, exchanges) {
  env.XDG_DATA_HOME ??= path.join(env.HOME, '.local', 'share')
  const directory = path.join(env.XDG_DATA_HOME, 'opencode')
  await fs.mkdir(directory, { recursive: true })
  const file = path.join(directory, 'opencode.db')
  await fs.rm(file, { force: true })
  const db = new DatabaseSync(file)
  db.exec(`
    create table session (id text primary key, version text not null);
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
  const insert = (table, row) => {
    const columns = Object.keys(row)
    db.prepare(
      `insert into ${table} (${columns.join(', ')}) values (${columns.map(() => '?').join(', ')})`,
    ).run(...columns.map((column) => row[column]))
  }
  insert('session', { id: session, version: '1.18.29' })
  let sequence = 0
  let time = 1_788_700_000_000
  const event = (type, data) => {
    sequence += 1
    insert('event', {
      id: `${session}-event-${sequence}`,
      aggregate_id: session,
      seq: sequence,
      type,
      data: JSON.stringify(data),
    })
  }
  const message = (role, id, text, complete) => {
    const messageCreated = time++
    const partId = `${id}-text`
    const data = {
      role,
      time: {
        created: messageCreated,
        ...(complete ? { completed: messageCreated + 1 } : {}),
      },
      ...(role === 'assistant' && complete ? { finish: 'stop' } : {}),
    }
    insert('message', {
      id,
      session_id: session,
      time_created: messageCreated,
      time_updated: messageCreated + 1,
      data: JSON.stringify(data),
    })
    insert('part', {
      id: partId,
      message_id: id,
      session_id: session,
      time_created: messageCreated,
      time_updated: messageCreated,
      data: JSON.stringify({ type: 'text', text }),
    })
    event('message.updated.1', {
      info: { id, role, time: { created: messageCreated } },
    })
    event('message.part.updated.1', { part: { id: partId, type: 'text', text } })
    if (complete) {
      event('message.updated.1', {
        info: {
          id,
          role,
          finish: role === 'assistant' ? 'stop' : undefined,
          time: { created: messageCreated, completed: messageCreated + 1 },
        },
      })
    }
  }
  for (const [index, exchange] of exchanges.entries()) {
    message('user', `${session}-user-${index + 1}`, exchange.user, false)
    if (exchange.answer !== undefined) {
      message('assistant', exchange.answerId, exchange.answer, true)
    }
  }
  db.close()
  return file
}

async function nativeLead(s, kind, admitted = true, ackTimeoutMs = 1_000, beforeResponse = null) {
  const state = { admitted, received: [], authorization: [] }
  s.setNow(Date.now())
  let close
  let channel
  if (kind === 'pi') {
    const inbox = path.join(s.temporary.root, 'native-pi-inbox')
    const ack = path.join(s.temporary.root, 'native-pi-ack')
    await fs.mkdir(inbox)
    await fs.mkdir(ack)
    const extension = watch(inbox, { persistent: false }, async () => {
      const files = (await fs.readdir(inbox)).filter((name) => name.endsWith('.json'))
      for (const name of files) {
        const received = JSON.parse(await fs.readFile(path.join(inbox, name), 'utf8'))
        if (state.received.some((entry) => entry.id === received.id)) continue
        state.received.push(received)
        await beforeResponse?.(received)
        const acknowledgement = { id: received.id, admitted: state.admitted }
        if (state.admitted !== true) {
          acknowledgement.reason = state.admitted === null ? 'admission-unknown' : 'not admitted'
        }
        await fs.writeFile(
          path.join(ack, `${received.id}.json`),
          `${JSON.stringify(acknowledgement)}\n`,
        )
      }
    })
    channel = { kind: 'pi-extension', inbox, ack, ackTimeoutMs }
    close = () => extension.close()
    await writePiSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-pi' },
    ])
    channel.launchId = 'watcher-native-lead'
    channel.settled = await writePiSettlement(
      s.temporary.env,
      channel.launchId,
      s.leadSession,
      'lead-ready-pi',
    )
  } else {
    const server = createServer(async (request, response) => {
      const chunks = []
      for await (const chunk of request) chunks.push(chunk)
      state.authorization.push(request.headers.authorization)
      state.received.push(JSON.parse(Buffer.concat(chunks).toString('utf8')))
      await beforeResponse?.()
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(
        JSON.stringify({
          admitted: state.admitted,
          ...(state.admitted === null ? { reason: 'admission-unknown' } : {}),
        }),
      )
    })
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
    const { port } = server.address()
    channel = {
      kind: 'opencode-server',
      endpoint: `http://127.0.0.1:${port}`,
      password: 'watcher-test-password',
      ackTimeoutMs,
    }
    close = async () =>
      await new Promise((resolve, reject) =>
        server.close((cause) => (cause === undefined ? resolve() : reject(cause))),
      )
    await writeOpenCodeSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-opencode' },
    ])
  }
  await s.store.mutate(s.workspace, 'test.native-lead', async (io) => {
    const tabs = await io.readTabs()
    const tab = tabs.find((candidate) => candidate.id === s.tab.id)
    tab.lead.harness = kind
    tab.lead.reserved = { channel }
    await io.writeTabs(tabs)
  })
  return { ...state, state, close }
}

function rustPair() {
  const nodeToRust = new PassThrough()
  const rustToNode = new PassThrough()
  const node = new Bridge({
    input: rustToNode,
    output: nodeToRust,
    idPrefix: 'n-',
    peerIdPrefix: 'r-',
  })
  const rust = new Bridge({
    input: nodeToRust,
    output: rustToNode,
    idPrefix: 'r-',
    peerIdPrefix: 'n-',
  })
  const state = {
    epoch: 7,
    draft: false,
    snapshots: [],
    writes: [],
    claims: [],
    claim: null,
    beforeSnapshot: null,
    write: null,
  }
  rust.on('pane.snapshot', async (request) => {
    state.snapshots.push(request)
    await state.beforeSnapshot?.(request)
    return {
      ok: true,
      generation: request.generation,
      inputEpoch: state.epoch,
      draftLatched: state.draft,
      pasteInFlight: false,
      inputFailed: false,
      queuedHumanBytes: 0,
    }
  })
  rust.on('pane.write_paste', async (request) => {
    state.writes.push(request)
    if (state.write !== null) return await state.write(request, { rustToNode })
    return { ok: true }
  })
  rust.on('pane.claim_epoch', async (request) => {
    state.claims.push(request)
    if (state.claim !== null) return await state.claim(request)
    return { ok: true }
  })
  return {
    node,
    rust,
    state,
    close() {
      node.close()
      rust.close()
    },
  }
}

async function system({ workerCount = 1, floorMs = NEVER_FLOOR_MS, watcherOptions = {} } = {}) {
  const temporary = tempEnv()
  const workspace = path.join(temporary.root, 'workspace')
  await fs.mkdir(workspace, { recursive: true })
  const store = new Store(temporary.env.CONSENSFLOW_HOME)
  await store.open()
  const tabs = new Tabs(store)
  const created = await tabs.create(workspace, 'codex')
  let tab = await tabs.get(created.id)
  const leadSession = 'lead-session'
  const workers = []
  for (let index = 0; index < workerCount; index++) {
    const name = `worker-${index + 1}`
    const session = `worker-session-${index + 1}`
    const pane = await tabs.addPane(tab.id, {
      kind: 'worker',
      conversation: name,
      generation: 1,
    })
    workers.push({ name, session, pane })
  }
  tab = await tabs.get(tab.id)
  const leadId = leadIdentity(tab)
  await store.mutate(workspace, 'test.bind', async (io) => {
    const storedTabs = await io.readTabs()
    const stored = storedTabs.find((candidate) => candidate.id === tab.id)
    stored.lead.nativeSession = leadSession
    await io.writeTabs(storedTabs)
    const threads = await io.readThreads()
    for (const worker of workers) {
      threads[worker.name] = {
        agent: `agent-${worker.name}`,
        kind: 'codex',
        lead: leadId,
        sessionId: worker.session,
        binding: { evidence: 'test', generation: 1, launchId: `launch-${worker.name}` },
        sent: [],
        seen: { [leadId]: [] },
      }
    }
    await io.writeThreads(threads)
  })
  await writeCodexSession(temporary.env, leadSession, [
    { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
  ])
  for (const worker of workers) {
    await writeCodexSession(temporary.env, worker.session, [
      { user: 'work', userId: `${worker.name}-user-1` },
    ])
  }

  const pipe = rustPair()
  let now = 1_000
  const watcher = new Watcher({
    store,
    tabs,
    env: temporary.env,
    now: () => now,
    floorMs,
    receiptMs: 50,
    ...watcherOptions,
  })
  watcher.attachBridge(pipe.node)
  const result = {
    temporary,
    workspace,
    store,
    tabs,
    tab,
    leadId,
    leadSession,
    workers,
    pipe,
    watcher,
    setNow(value) {
      now = value
    },
    async deliveries() {
      return Object.values(await store.readDeliveries(workspace))
    },
    async threads() {
      return await store.readThreads(workspace)
    },
    async close() {
      await result.watcher.close()
      assert.equal(result.watcher.timer, null)
      assert.deepEqual(result.watcher.unsubscribe, [])
      pipe.close()
      await store.close()
      temporary.cleanup()
    },
  }
  return result
}

async function resumeLead(s, session = 'lead-session-2') {
  await s.tabs.suspend(s.tab.id)
  await s.tabs.resume(s.tab.id)
  await s.store.mutate(s.workspace, 'test.resume-lead', async (io) => {
    const tabs = await io.readTabs()
    tabs.find((tab) => tab.id === s.tab.id).lead.nativeSession = session
    await io.writeTabs(tabs)
  })
  await writeCodexSession(s.temporary.env, session, [
    { user: 'resume', answer: 'new lead ready', answerId: `${session}-ready` },
  ])
}

test('bookkeeping projection excludes tool items for planning and every contiguous seen walk', () => {
  const items = [
    { id: 'u-1', role: 'user' },
    { id: 't-1', role: 'tool' },
    { id: 'a-1', role: 'assistant' },
  ]
  assert.deepEqual(bookkeepingItems(items), [items[0], items[2]])
  assert.deepEqual(bookkeepingItems(null), [])
})

test('restart plans a completed answer from a bound conversation whose pane is already closed', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await s.tabs.removePane(s.tab.id, worker.pane.id, worker.pane.generation)
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'finished while closing', answerId: 'closed-answer-1' },
    ])

    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(delivery.conversation, worker.name)
    assert.equal(delivery.answerId, 'closed-answer-1')
  } finally {
    await s.close()
  }
})

test('restart, pane.idle, pane.exit and the floor reconcile every bound conversation, live or not', async () => {
  const s = await system({ workerCount: 3, floorMs: FLOOR_MS })
  try {
    await s.tabs.removePane(s.tab.id, s.workers[1].pane.id, 1)
    await s.watcher.start()
    assert.equal((await s.deliveries()).length, 0, 'restart read incomplete bound stores')

    for (const worker of s.workers.slice(0, 2)) {
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'work', answer: `answer from ${worker.name}`, answerId: `${worker.name}-a1` },
      ])
    }
    s.pipe.rust.event('pane.idle', { id: s.workers[0].pane.id, generation: 1 })
    await waitFor(async () => (await s.deliveries()).length === 2)
    assert.deepEqual(
      (await s.deliveries()).map((entry) => entry.conversation).sort(),
      ['worker-1', 'worker-2'],
      'one idle hint scans both the live and ended bound conversations',
    )

    await writeCodexSession(s.temporary.env, s.workers[2].session, [
      { user: 'work', answer: 'finished before exit', answerId: 'worker-3-a1' },
    ])
    s.pipe.rust.event('pane.exit', { id: s.workers[2].pane.id, generation: 1 })
    await waitFor(async () => (await s.deliveries()).length === 3)

    await writeCodexSession(s.temporary.env, s.workers[0].session, [
      { user: 'work', answer: 'first', answerId: 'worker-1-a1' },
      { user: 'more', answer: 'observed by floor', answerId: 'worker-1-a2' },
    ])
    await waitFor(async () => (await s.deliveries()).length === 4)
  } finally {
    await s.close()
  }
})

test('auto writes only on ready, passes the epoch and requires a fresh settled turn next', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'one', answer: 'answer one', answerId: 'answer-1' },
      { user: 'two', answer: 'answer two', answerId: 'answer-2' },
    ])
    await s.watcher.start()
    assert.equal(s.pipe.state.writes.length, 1, 'one readiness boundary admits only one answer')
    assert.equal(s.pipe.state.writes[0].epoch, 7)
    assert.match(s.pipe.state.writes[0].body, /answer one/)
    let deliveries = await s.deliveries()
    assert.equal(deliveries.find((entry) => entry.answerId === 'answer-1').state, 'submitting')
    assert.match(
      deliveries.find((entry) => entry.answerId === 'answer-2').reason,
      /no settlement newer/,
    )

    const first = deliveries.find((entry) => entry.answerId === 'answer-1')
    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
      { user: envelope(first), answer: 'receipt settled', answerId: 'lead-ready-2' },
    ])
    await s.watcher.reconcile('test.receipt')
    deliveries = await s.deliveries()
    assert.equal(deliveries.find((entry) => entry.answerId === 'answer-1').state, 'accepted')
    assert.equal(s.pipe.state.writes.length, 2, 'fresh settlement releases the next answer')
    assert.match(s.pipe.state.writes[1].body, /answer two/)
  } finally {
    await s.close()
  }
})

for (const refusal of ['Stale', 'Draft']) {
  test(`${refusal} keeps the automatic record pending with a reason`, async () => {
    const s = await system()
    try {
      const worker = s.workers[0]
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'work', answer: 'answer', answerId: 'answer-1' },
      ])
      s.pipe.state.write = async () => ({ ok: false, error: refusal })
      await s.watcher.start()
      const [delivery] = await s.deliveries()
      assert.equal(delivery.state, 'pending')
      assert.match(delivery.reason, new RegExp(refusal, 'i'))
    } finally {
      await s.close()
    }
  })
}

test('policy is re-read immediately before write and manual cancels queued automatic work', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'answer', answerId: 'answer-1' },
    ])
    s.pipe.state.beforeSnapshot = async () => {
      s.pipe.state.beforeSnapshot = null
      await s.store.policySet({ tab: s.tab.id, value: 'manual' })
    }
    await s.watcher.start()
    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'cancelled')
    assert.match(delivery.reason, /manual/)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    await s.close()
  }
})

test('planning waits for a just-sent question to reach the worker transcript or for grace to expire', async () => {
  const s = await system({ workerCount: 2 })
  try {
    const sentAt = Date.parse('2026-09-07T00:00:05.000Z')
    s.temporary.env.CONSENSFLOW_WAIT_GRACE_MS = '4000'
    s.setNow(sentAt + 100)
    for (const worker of s.workers) {
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'first question', answer: 'standing answer', answerId: `${worker.name}-a1` },
      ])
    }
    await s.store.mutate(s.workspace, 'test.just-sent-questions', async (io) => {
      const threads = await io.readThreads()
      for (const worker of s.workers) {
        threads[worker.name].sent.push({
          kind: 'say',
          opId: `say-${worker.name}`,
          at: new Date(sentAt).toISOString(),
        })
      }
      await io.writeThreads(threads)
    })

    await s.watcher.start()
    assert.equal((await s.deliveries()).length, 0, 'the standing answers are still stale')

    const arrived = s.workers[0]
    await writeCodexSession(s.temporary.env, arrived.session, [
      { user: 'first question', answer: 'standing answer', answerId: `${arrived.name}-a1` },
      { user: 'the just-sent question' },
    ])
    await s.watcher.reconcile('sent-question-arrived')
    assert.deepEqual(
      (await s.deliveries()).map((delivery) => delivery.conversation),
      [arrived.name],
      'the native user turn releases only its own conversation inside the grace',
    )

    s.setNow(sentAt + 4_000)
    await s.watcher.reconcile('sent-question-grace-expired')
    assert.deepEqual(
      (await s.deliveries()).map((delivery) => delivery.conversation).sort(),
      s.workers.map((worker) => worker.name).sort(),
    )
  } finally {
    await s.close()
  }
})

test('submission admission rechecks wait grace against a concurrently recorded question', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  const sentAt = Date.parse('2026-09-07T00:00:05.000Z')
  let raced = false
  try {
    s.temporary.env.CONSENSFLOW_WAIT_GRACE_MS = '4000'
    s.setNow(sentAt + 100)
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'first question', answer: 'standing answer', answerId: 'answer-1' },
    ])
    s.store.mutate = async (workspace, name, operation) => {
      if (name === 'delivery.submit' && !raced) {
        raced = true
        await originalMutate(workspace, 'test.sent-during-admission', async (io) => {
          const threads = await io.readThreads()
          threads[worker.name].sent.push({
            kind: 'say',
            opId: 'say-during-admission',
            at: new Date(sentAt).toISOString(),
          })
          await io.writeThreads(threads)
        })
      }
      return await originalMutate(workspace, name, operation)
    }

    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(raced, true)
    assert.equal(delivery.state, 'pending')
    assert.match(delivery.reason, /question.*grace/i)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    s.store.mutate = originalMutate
    await s.close()
  }
})

test('manual policy and submission admission are one queued decision', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'answer', answerId: 'answer-1' },
    ])
    let release
    let entered
    const held = new Promise((resolve) => {
      release = resolve
    })
    const active = new Promise((resolve) => {
      entered = resolve
    })
    let queued
    s.pipe.state.beforeSnapshot = async () => {
      s.pipe.state.beforeSnapshot = null
      const blocker = s.store.mutate(s.workspace, 'test.hold-admission', async () => {
        entered()
        await held
      })
      await active
      const manual = s.store.policySet({ tab: s.tab.id, value: 'manual' })
      queued = Promise.all([blocker, manual])
      setTimeout(release, 100)
    }
    await s.watcher.start()
    await queued

    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'cancelled')
    assert.match(delivery.reason, /manual/)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    await s.close()
  }
})

test('bridge EOF after write admission is uncertain and never automatically replayed', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'maybe landed', answerId: 'answer-1' },
    ])
    s.pipe.state.write = async (_request, { rustToNode }) => {
      rustToNode.end()
      await new Promise(() => {})
    }
    await s.watcher.start()
    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'uncertain')
    assert.match(delivery.reason, /EOF/i)
    await s.watcher.reconcile('after.eof')
    assert.equal(s.pipe.state.writes.length, 1, 'uncertain is never replayed automatically')
  } finally {
    await s.close()
  }
})

test('a confirmed zero-byte refusal retries without waiting for fresh lead settlement', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'retry me', answerId: 'answer-1' },
    ])
    let writes = 0
    s.pipe.state.write = async () => {
      writes += 1
      return writes === 1 ? { ok: false, error: 'not admitted', bytesWritten: 0 } : { ok: true }
    }
    await s.watcher.start()
    assert.equal((await s.deliveries())[0].state, 'failed')

    await s.watcher.reconcile('retry-confirmed-failure')
    const deliveries = await s.deliveries()
    assert.equal(s.pipe.state.writes.length, 2)
    assert.equal(deliveries.length, 2)
    assert.equal(deliveries[1].state, 'submitting')
  } finally {
    await s.close()
  }
})

test('a resumed lead holds old-generation records and explicit held-send creates new targets', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'held answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    const [old] = await s.deliveries()
    assert.equal(old.state, 'pending')
    assert.equal(old.target.generation, 1)

    await resumeLead(s)
    await s.watcher.reconcile('resumed')

    const held = await s.watcher.held(s.tab.id)
    assert.equal(held.action, HELD_ACTION)
    assert.deepEqual(
      held.records.map((entry) => entry.id),
      [old.id],
    )
    assert.equal(s.pipe.state.writes.length, 0, 'a generation that did not ask gets nothing')

    const [fresh] = await s.watcher.sendHeld(s.tab.id)
    assert.notEqual(fresh.id, old.id)
    assert.equal(fresh.heldOf, old.id)
    assert.equal(fresh.target.generation, 2)
    assert.equal(fresh.target.session, 'lead-session-2')
    assert.equal((await s.deliveries()).length, 2)
  } finally {
    await s.close()
  }
})

test('a closed lead keeps pending answers visible as held until resume', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'held while closed', answerId: 'closed-held-answer' },
    ])
    await s.watcher.start()
    const [pending] = await s.deliveries()
    assert.equal(pending.state, 'pending')

    await s.tabs.suspend(s.tab.id)
    const held = await s.watcher.held(s.tab.id)
    assert.deepEqual(
      held.records.map((record) => record.id),
      [pending.id],
    )
    await assert.rejects(
      () => s.watcher.sendHeld(s.tab.id),
      /has no live lead to receive held answers/,
    )
  } finally {
    await s.close()
  }
})

test('an answer first discovered after resume stays held for the generation that asked', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'late old answer', answerId: 'late-answer' },
    ])
    await resumeLead(s)
    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(delivery.target.leadId, s.leadId)
    assert.equal(delivery.target.generation, 1)
    assert.match(delivery.reason, /held/)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    await s.close()
  }
})

test('held-send is idempotent for the held set already copied to the current lead', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'held answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    await resumeLead(s)
    await s.watcher.reconcile('resumed')

    assert.equal((await s.watcher.sendHeld(s.tab.id)).length, 1)
    assert.deepEqual(await s.watcher.sendHeld(s.tab.id), [])
    assert.equal((await s.deliveries()).length, 2)
    assert.deepEqual((await s.watcher.held(s.tab.id)).records, [])
  } finally {
    await s.close()
  }
})

test('held-send creates one current-generation copy across the full answer chain', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'held answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()

    await resumeLead(s, 'lead-session-2')
    await s.watcher.reconcile('generation-2')
    const [second] = await s.watcher.sendHeld(s.tab.id)

    await resumeLead(s, 'lead-session-3')
    await s.watcher.reconcile('generation-3')
    const [third] = await s.watcher.sendHeld(s.tab.id)
    assert.equal(third.heldOf, second.id)

    assert.deepEqual(await s.watcher.sendHeld(s.tab.id), [])
    assert.deepEqual((await s.watcher.held(s.tab.id)).records, [])
    assert.equal((await s.deliveries()).length, 3)
  } finally {
    await s.close()
  }
})

test('a replaced worker session invalidates its binding and pending decision', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'do not deliver after replacement', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    await writeCodexSession(
      s.temporary.env,
      worker.session,
      [{ user: 'fork', answer: 'different session', answerId: 'fork-answer' }],
      { replaced: true },
    )
    await s.watcher.reconcile('replacement')

    const row = (await s.threads())[worker.name]
    assert.equal(row.sessionId, null)
    assert.equal(row.binding, undefined)
    assert.match(row.replaced.reason, /replaced/i)
    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'pending')
    assert.match(delivery.reason, /replaced/i)
    assert.equal(delivery.suspended, true)
  } finally {
    await s.close()
  }
})

test('submission admission atomically invalidates a worker replacement discovered after planning', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  let replaced = false
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'old answer', answerId: 'old-a1' },
    ])
    s.store.mutate = async (workspace, name, operation) => {
      if (name === 'delivery.submit' && !replaced) {
        replaced = true
        await writeCodexSession(
          s.temporary.env,
          worker.session,
          [{ user: 'fork', answer: 'replacement answer', answerId: 'replacement-a1' }],
          { replaced: true },
        )
      }
      return await originalMutate(workspace, name, operation)
    }

    await s.watcher.start()

    const row = (await s.threads())[worker.name]
    assert.equal(replaced, true)
    assert.equal(row.sessionId, null)
    assert.equal(row.binding, undefined)
    assert.match(row.replaced.reason, /replaced/i)
    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'pending')
    assert.equal(delivery.suspended, true)
    assert.match(delivery.reason, /replaced/i)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    s.store.mutate = originalMutate
    await s.close()
  }
})

test('a persisted worker invalidation suspends an unsuspended pending record after restart', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'must remain suspended', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    await s.store.mutate(s.workspace, 'test.interrupted-worker-invalidation', async (io) => {
      const threads = await io.readThreads()
      const row = threads[worker.name]
      row.replaced = {
        at: '2026-09-07T00:00:00.000Z',
        reason: 'replaced while suspension write was interrupted',
        previousSession: row.sessionId,
      }
      row.sessionId = null
      delete row.binding
      await io.writeThreads(threads)
    })
    await s.watcher.close()
    s.pipe.state.draft = false
    s.watcher = new Watcher({
      store: s.store,
      tabs: s.tabs,
      env: s.temporary.env,
      floorMs: NEVER_FLOOR_MS,
    })
    s.watcher.attachBridge(s.pipe.node)
    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'pending')
    assert.equal(delivery.suspended, true)
    assert.match(delivery.reason, /replaced/)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    await s.close()
  }
})

test('an interrupted worker invalidation cannot later deliver from that session', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'first answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    await writeCodexSession(
      s.temporary.env,
      worker.session,
      [{ user: 'fork', answer: 'replacement answer', answerId: 'replacement-answer' }],
      { replaced: true },
    )

    let interrupted = false
    s.store.mutate = async (workspace, name, operation) => {
      if (name !== 'delivery.session-replaced' || interrupted) {
        return await originalMutate(workspace, name, operation)
      }
      interrupted = true
      return await originalMutate(workspace, name, async (io) => {
        let deliveriesWritten = false
        return await operation({
          ...io,
          writeDeliveries: async (...arguments_) => {
            await io.writeDeliveries(...arguments_)
            deliveriesWritten = true
          },
          writeThreads: async () => {
            assert.equal(deliveriesWritten, true)
            throw new Error('injected worker invalidation interruption')
          },
        })
      })
    }
    await s.watcher.reconcile('interrupted-worker-invalidation')
    s.store.mutate = originalMutate

    s.pipe.state.draft = false
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'first answer', answerId: 'answer-1' },
      { user: 'more', answer: 'second answer', answerId: 'answer-2' },
    ])
    await s.watcher.reconcile('after-interrupted-worker-invalidation')

    const row = (await s.threads())[worker.name]
    assert.equal(row.sessionId, null)
    assert.equal(row.binding, undefined)
    assert.equal((await s.deliveries()).length, 1)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    s.store.mutate = originalMutate
    await s.close()
  }
})

test('a replaced lead is durably invalidated and cannot resume on a later clean read', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'do not retarget', answerId: 'answer-1' },
    ])
    await writeCodexSession(
      s.temporary.env,
      s.leadSession,
      [{ user: 'fork', answer: 'replacement lead', answerId: 'lead-replacement' }],
      { replaced: true },
    )
    await s.watcher.start()

    let tab = await s.tabs.get(s.tab.id)
    let [delivery] = await s.deliveries()
    assert.equal(tab.lead.nativeSession, null)
    assert.match(tab.lead.replaced.reason, /replaced/i)
    assert.equal(delivery.suspended, true)
    assert.equal(s.pipe.state.writes.length, 0)

    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'clean again', answer: 'must not revive', answerId: 'lead-clean' },
    ])
    await s.store.mutate(s.workspace, 'test.same-generation-lead-rebind', async (io) => {
      const tabs = await io.readTabs()
      tabs.find((candidate) => candidate.id === s.tab.id).lead.nativeSession = s.leadSession
      await io.writeTabs(tabs)
    })
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'do not retarget', answerId: 'answer-1' },
      { user: 'more', answer: 'still invalidated', answerId: 'answer-2' },
    ])
    await s.watcher.reconcile('lead-clean-after-replacement')
    tab = await s.tabs.get(s.tab.id)
    ;[delivery] = await s.deliveries()
    assert.equal(tab.lead.generation, 1)
    assert.equal(delivery.suspended, true)
    assert.equal((await s.deliveries()).length, 1)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    await s.close()
  }
})

test('an interrupted lead invalidation cannot later deliver to that generation', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'first answer', answerId: 'answer-1' },
    ])
    await writeCodexSession(
      s.temporary.env,
      s.leadSession,
      [{ user: 'fork', answer: 'replacement lead', answerId: 'lead-replacement' }],
      { replaced: true },
    )

    let interrupted = false
    s.store.mutate = async (workspace, name, operation) => {
      if (name !== 'delivery.lead-replaced' || interrupted) {
        return await originalMutate(workspace, name, operation)
      }
      interrupted = true
      return await originalMutate(workspace, name, async (io) => {
        let deliveriesWritten = false
        return await operation({
          ...io,
          writeDeliveries: async (...arguments_) => {
            await io.writeDeliveries(...arguments_)
            deliveriesWritten = true
          },
          writeTabs: async () => {
            assert.equal(deliveriesWritten, true)
            throw new Error('injected lead invalidation interruption')
          },
        })
      })
    }
    await s.watcher.start()
    s.store.mutate = originalMutate

    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'clean again', answer: 'must not revive', answerId: 'lead-clean' },
    ])
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'first answer', answerId: 'answer-1' },
      { user: 'more', answer: 'second answer', answerId: 'answer-2' },
    ])
    await s.watcher.reconcile('after-interrupted-lead-invalidation')

    const tab = await s.tabs.get(s.tab.id)
    assert.equal(tab.lead.nativeSession, null)
    assert.equal(tab.lead.replaced.generation, 1)
    assert.equal((await s.deliveries()).length, 1)
    assert.equal(s.pipe.state.writes.length, 0)
  } finally {
    s.store.mutate = originalMutate
    await s.close()
  }
})

test('a submitting record durably carries interrupted lead invalidation across restart', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'first answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    assert.equal(s.pipe.state.writes.length, 1)
    await writeCodexSession(
      s.temporary.env,
      s.leadSession,
      [{ user: 'fork', answer: 'replacement lead', answerId: 'lead-replacement' }],
      { replaced: true },
    )

    let interrupted = false
    s.store.mutate = async (workspace, name, operation) => {
      if (name !== 'delivery.lead-replaced' || interrupted) {
        return await originalMutate(workspace, name, operation)
      }
      interrupted = true
      return await originalMutate(workspace, name, async (io) => {
        let deliveriesWritten = false
        return await operation({
          ...io,
          writeDeliveries: async (...arguments_) => {
            await io.writeDeliveries(...arguments_)
            deliveriesWritten = true
          },
          writeTabs: async () => {
            assert.equal(deliveriesWritten, true)
            throw new Error('injected submitting lead invalidation interruption')
          },
        })
      })
    }
    await s.watcher.reconcile('interrupted-submitting-lead-invalidation')
    s.store.mutate = originalMutate
    await s.watcher.close()

    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
      { user: 'later turn', answer: 'newer clean settlement', answerId: 'lead-ready-2' },
    ])
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'first answer', answerId: 'answer-1' },
      { user: 'more', answer: 'second answer', answerId: 'answer-2' },
    ])
    s.watcher = new Watcher({
      store: s.store,
      tabs: s.tabs,
      env: s.temporary.env,
      floorMs: NEVER_FLOOR_MS,
    })
    s.watcher.attachBridge(s.pipe.node)
    await s.watcher.start()

    const tab = await s.tabs.get(s.tab.id)
    assert.equal(tab.lead.nativeSession, null)
    assert.equal(tab.lead.replaced.generation, 1)
    assert.equal((await s.deliveries()).length, 1)
    assert.equal(s.pipe.state.writes.length, 1)
  } finally {
    s.store.mutate = originalMutate
    await s.close()
  }
})

test('accepted bookkeeping skips tool items so its contiguous seen walk reaches the answer', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      {
        user: 'work',
        userId: 'worker-user',
        tool: 'tool output',
        toolId: 'worker-tool',
        answer: 'finished',
        answerId: 'worker-answer',
      },
    ])
    await s.store.seenSet(s.workspace, {
      name: worker.name,
      lead: s.leadId,
      items: ['worker-user'],
    })
    await s.watcher.start()
    const [submitted] = await s.deliveries()
    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
      { user: envelope(submitted), answer: 'received', answerId: 'lead-ready-2' },
    ])
    await s.watcher.reconcile('receipt')

    const row = (await s.threads())[worker.name]
    assert.deepEqual(row.seen[s.leadId], ['worker-user', 'worker-answer'])
    assert.ok(!row.seen[s.leadId].includes('worker-tool'))
  } finally {
    await s.close()
  }
})

test('restart repairs a worker seen mark after the accepted receipt was already saved', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', userId: 'worker-user', answer: 'finished', answerId: 'worker-answer' },
    ])
    await s.store.seenSet(s.workspace, {
      name: worker.name,
      lead: s.leadId,
      items: ['worker-user'],
    })
    await s.watcher.start()
    const [submitted] = await s.deliveries()
    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
      { user: envelope(submitted), answer: 'received', answerId: 'lead-ready-2' },
    ])
    await s.watcher.reconcile('receipt')
    await s.store.mutate(s.workspace, 'test.interrupted-seen-write', async (io) => {
      const threads = await io.readThreads()
      threads[worker.name].seen[s.leadId] = ['worker-user']
      await io.writeThreads(threads)
    })
    await s.watcher.close()
    s.watcher = new Watcher({
      store: s.store,
      tabs: s.tabs,
      env: s.temporary.env,
      floorMs: NEVER_FLOOR_MS,
    })
    s.watcher.attachBridge(s.pipe.node)
    await s.watcher.start()

    assert.deepEqual((await s.threads())[worker.name].seen[s.leadId], [
      'worker-user',
      'worker-answer',
    ])
  } finally {
    await s.close()
  }
})

test('equal submission timestamps still fence on the most recent submission cursor', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'one', answer: 'answer one', answerId: 'answer-1' },
      { user: 'two', answer: 'answer two', answerId: 'answer-2' },
      { user: 'three', answer: 'answer three', answerId: 'answer-3' },
    ])
    await s.watcher.start()
    const first = (await s.deliveries()).find((entry) => entry.answerId === 'answer-1')
    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
      { user: envelope(first), answer: 'first receipt', answerId: 'lead-ready-2' },
    ])
    await s.watcher.reconcile('first-receipt')
    assert.equal(s.pipe.state.writes.length, 2)

    s.setNow(1_100)
    await s.watcher.reconcile('second-timeout')
    const deliveries = await s.deliveries()
    assert.equal(deliveries.find((entry) => entry.answerId === 'answer-2').state, 'uncertain')
    assert.equal(deliveries.find((entry) => entry.answerId === 'answer-3').state, 'pending')
    assert.match(
      deliveries.find((entry) => entry.answerId === 'answer-3').reason,
      /no settlement newer/,
    )
    assert.equal(s.pipe.state.writes.length, 2)
  } finally {
    await s.close()
  }
})

test('delivery channel and part budgets are selected by the receiving lead harness', async () => {
  const piBudget = { bytes: 256, lines: 20 }
  const s = await system({
    watcherOptions: {
      inlineBudget: { codex: 10_000, pi: 50 },
      partBudget: { codex: { bytes: 2_048, lines: 100 }, pi: piBudget },
    },
  })
  try {
    const worker = s.workers[0]
    await s.store.mutate(s.workspace, 'test.lead-harness', async (io) => {
      const tabs = await io.readTabs()
      tabs.find((tab) => tab.id === s.tab.id).lead.harness = 'pi'
      await io.writeTabs(tabs)
    })
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'x'.repeat(100), answerId: 'answer-1' },
    ])
    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(delivery.kind, 'pi')
    assert.equal(delivery.channel, 'cf-read')
    assert.deepEqual(delivery.partBudget, piBudget)
  } finally {
    await s.close()
  }
})

test('Pi and OpenCode native channels preserve inline versus cf-read receipt semantics', async () => {
  for (const kind of ['pi', 'opencode']) {
    for (const recordChannel of ['pty-inline', 'cf-read']) {
      const s = await system({
        watcherOptions: {
          inlineBudget: { [kind]: recordChannel === 'pty-inline' ? 10_000 : 1 },
        },
      })
      let native
      try {
        native = await nativeLead(s, kind)
        const worker = s.workers[0]
        await writeCodexSession(s.temporary.env, worker.session, [
          { user: 'work', answer: `${kind} ${recordChannel} answer`, answerId: 'answer-1' },
        ])
        await s.watcher.start()

        let [delivery] = await s.deliveries()
        assert.equal(delivery.channel, recordChannel)
        const receivedText =
          kind === 'pi' ? native.state.received[0].text : native.state.received[0].parts[0].text
        assert.equal(
          receivedText,
          recordChannel === 'cf-read' ? pointer(delivery) : envelope(delivery),
        )
        assert.equal(
          delivery.snapshot.evidenceType,
          recordChannel === 'cf-read' ? 'tool-result' : 'user-item',
        )

        const receiptExchange = [
          { user: 'lead question', answer: 'lead ready', answerId: `lead-ready-${kind}` },
          { user: envelope(delivery), answer: 'user echo settled', answerId: 'lead-after-echo' },
        ]
        if (kind === 'pi') {
          await writePiSession(s.temporary.env, s.leadSession, receiptExchange)
        } else {
          await writeOpenCodeSession(s.temporary.env, s.leadSession, receiptExchange)
        }
        await s.watcher.reconcile('native-user-echo')
        delivery = (await s.deliveries()).find((candidate) => candidate.id === delivery.id)
        assert.equal(
          delivery.state,
          recordChannel === 'cf-read' ? 'submitting' : 'accepted',
          'a user envelope is never file coverage',
        )
      } finally {
        await s.close()
        await native?.close()
      }
    }
  }
})

test('OpenCode native delivery preserves the reservation authentication', async () => {
  const s = await system()
  let native
  try {
    native = await nativeLead(s, 'opencode')
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'authenticated answer', answerId: 'answer-1' },
    ])

    await s.watcher.start()

    assert.deepEqual(native.state.authorization, [
      `Basic ${Buffer.from('opencode:watcher-test-password').toString('base64')}`,
    ])
  } finally {
    await s.close()
    await native?.close()
  }
})

test('each native submission carries one absolute expiry and becomes uncertain at it', async () => {
  for (const kind of ['pi', 'opencode']) {
    const s = await system({ watcherOptions: { receiptMs: 10_000 } })
    let native
    try {
      native = await nativeLead(s, kind, true, 10_000)
      const worker = s.workers[0]
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'work', answer: `${kind} expiring answer`, answerId: 'answer-1' },
      ])

      await s.watcher.start()

      let [delivery] = await s.deliveries()
      assert.equal(delivery.state, 'submitting')
      assert.equal(delivery.expiresAt, delivery.submittedAt + 10_000)
      if (kind === 'pi') assert.equal(native.state.received[0].expiresAt, delivery.expiresAt)

      s.setNow(delivery.expiresAt)
      await s.watcher.reconcile('native-expiry')

      delivery = (await s.deliveries()).find((candidate) => candidate.id === delivery.id)
      assert.equal(delivery.state, 'uncertain')
      assert.match(delivery.reason, /expir/i)
      assert.equal(native.state.received.length, 1, 'an expired submission is never replayed')
    } finally {
      await s.close()
      await native?.close()
    }
  }
})

test('a native response received at the absolute expiry is uncertain', async () => {
  for (const kind of ['pi', 'opencode']) {
    const s = await system()
    let native
    try {
      native = await nativeLead(s, kind, false, 1_000, async () => {
        const [submitted] = await s.deliveries()
        assert.equal(submitted.state, 'submitting')
        s.setNow(submitted.expiresAt)
      })
      const worker = s.workers[0]
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'work', answer: `${kind} late response`, answerId: 'answer-1' },
      ])

      await s.watcher.start()

      const [delivery] = await s.deliveries()
      assert.equal(delivery.state, 'uncertain')
      assert.match(delivery.reason, /expir/i)
      assert.equal(native.state.received.length, 1)
    } finally {
      await s.close()
      await native?.close()
    }
  }
})

test('a native Stale claim at expiry is a replayable zero-byte failure', async () => {
  const s = await system()
  let native
  try {
    native = await nativeLead(s, 'opencode', true, 1_000)
    s.pipe.state.claim = async () => {
      const [submitted] = await s.deliveries()
      assert.equal(submitted.state, 'submitting')
      s.setNow(submitted.expiresAt)
      return { ok: false, error: 'Stale' }
    }
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'one-expiry answer', answerId: 'answer-1' },
    ])

    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(delivery.state, 'failed')
    assert.match(delivery.reason, /Stale|zero-byte/i)
    assert.ok(Number.isFinite(delivery.expiresAt))
    const expiresAt = delivery.expiresAt
    assert.equal(native.state.received.length, 0)

    s.pipe.state.claim = () => ({ ok: true })
    await s.watcher.reconcile('stale-retry')

    const deliveries = await s.deliveries()
    assert.equal(deliveries.length, 2)
    assert.equal(deliveries.find((candidate) => candidate.id === delivery.id).state, 'failed')
    const replay = deliveries.find((candidate) => candidate.id !== delivery.id)
    assert.equal(replay.state, 'submitting')
    assert.ok(replay.expiresAt > expiresAt)
    assert.equal(native.state.received.length, 1)
  } finally {
    await s.close()
    await native?.close()
  }
})

for (const matching of [true, false]) {
  test(`Pi settlement comes from the target launch, matching=${matching} (TEST-PANE-65)`, async () => {
    const s = await system()
    let native
    try {
      native = await nativeLead(s, 'pi')
      if (!matching) {
        s.temporary.env.CF_DELIVERY_SETTLED = path.join(
          s.temporary.env.CONSENSFLOW_HOME,
          'pi-settled',
        )
        s.temporary.env.CF_DELIVERY_LAUNCH_ID = 'watcher-native-lead'
        await s.store.mutate(s.workspace, 'test.other-pi-launch', async (io) => {
          const tabs = await io.readTabs()
          tabs.find((t) => t.id === s.tab.id).lead.reserved.channel.launchId = 'other-launch'
          await io.writeTabs(tabs)
        })
      }
      await writeCodexSession(s.temporary.env, s.workers[0].session, [
        { user: 'work', answer: 'target-scoped native evidence', answerId: 'answer-1' },
      ])
      await s.watcher.start()
      const [delivery] = await s.deliveries()
      assert.equal(delivery.state, matching ? 'submitting' : 'pending')
      assert.equal(native.state.received.length, matching ? 1 : 0)
    } finally {
      await s.close()
      await native?.close()
    }
  })
}

test('Pi worker settlement uses its bound launch without editor environment hints (TEST-PANE-65)', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await copyPiSession(s.temporary.env)
    const launchId = 'worker-native-probe'
    const { channel } = await launchConfiguration('pi', { launchId, workspace: s.workspace })
    await fs.mkdir(channel.settled, { recursive: true })
    await fs.writeFile(
      path.join(channel.settled, `${launchId}.json`),
      JSON.stringify({
        launchId,
        sessionId: 'hazy-ridge',
        frontier: { id: '3f9b029e' },
        settledAt: Date.now(),
      }),
    )
    await s.store.mutate(s.workspace, 'test.pi-native-worker', async (io) => {
      const threads = await io.readThreads()
      Object.assign(threads[worker.name], { kind: 'pi', sessionId: 'hazy-ridge' })
      threads[worker.name].binding.launchId = launchId
      await io.writeThreads(threads)
    })
    await s.watcher.start()
    assert.equal((await s.deliveries()).length, 1, 'native worker settlement plans immediately')
    assert.equal(s.pipe.state.writes.length, 1)
  } finally {
    await s.close()
  }
})

test('a watcher cf-read delivery reaches an already-idle real Pi extension as its pointer', async () => {
  const s = await system({ watcherOptions: { inlineBudget: { pi: 1 } } })
  const inbox = path.join(s.temporary.root, 'real-pi-inbox')
  const ack = path.join(s.temporary.root, 'real-pi-ack')
  const quarantine = path.join(s.temporary.root, 'real-pi-quarantine')
  const handlers = new Map()
  const sent = []
  const context = { isIdle: () => true }
  const pi = {
    on(event, handler) {
      handlers.set(event, handler)
    },
    sendUserMessage(text) {
      sent.push(text)
      queueMicrotask(() =>
        handlers.get('message_start')(
          { message: { role: 'user', content: [{ type: 'text', text }] } },
          context,
        ),
      )
    },
  }
  createDeliveryExtension(pi, {
    inbox,
    ack,
    quarantine,
    ackTimeoutMs: 100,
    logger: { error: () => {} },
  })
  await handlers.get('session_start')({}, context)
  try {
    s.setNow(Date.now())
    await s.store.mutate(s.workspace, 'test.real-pi-lead', async (io) => {
      const tabs = await io.readTabs()
      const tab = tabs.find((candidate) => candidate.id === s.tab.id)
      tab.lead.harness = 'pi'
      tab.lead.reserved = {
        channel: {
          kind: 'pi-extension',
          inbox,
          ack,
          quarantine,
          ackTimeoutMs: 200,
          launchId: 'watcher-real-extension',
          settled: path.join(s.temporary.env.CONSENSFLOW_HOME, 'pi-settled'),
        },
      }
      await io.writeTabs(tabs)
    })
    await writePiSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-pi' },
    ])
    await writePiSettlement(
      s.temporary.env,
      'watcher-real-extension',
      s.leadSession,
      'lead-ready-pi',
    )
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'file-backed answer', answerId: 'answer-1' },
    ])

    await s.watcher.start()

    const [delivery] = await s.deliveries()
    assert.equal(delivery.channel, 'cf-read')
    assert.deepEqual(sent, [pointer(delivery)])
    assert.equal(delivery.state, 'submitting')
  } finally {
    await handlers.get('session_shutdown')()
    await s.close()
  }
})

test('Pi and OpenCode negative acknowledgements fail with zero bytes and remain replayable', async () => {
  for (const kind of ['pi', 'opencode']) {
    const s = await system()
    let native
    try {
      native = await nativeLead(s, kind, false)
      const worker = s.workers[0]
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'work', answer: `${kind} rejected answer`, answerId: 'answer-1' },
      ])
      await s.watcher.start()

      let deliveries = await s.deliveries()
      assert.equal(deliveries.length, 1)
      assert.equal(deliveries[0].state, 'failed')
      assert.match(deliveries[0].reason, /not admitted|declined|refused/i)

      native.state.admitted = true
      await s.watcher.reconcile('retry-after-negative-ack')
      deliveries = await s.deliveries()
      assert.equal(deliveries.length, 2, 'affirmative non-admission is replayable')
      assert.equal(deliveries.find((delivery) => delivery.state === 'failed').answerId, 'answer-1')
      assert.equal(
        deliveries.find((delivery) => delivery.state === 'submitting').answerId,
        'answer-1',
      )
    } finally {
      await s.close()
      await native?.close()
    }
  }
})

test('a native admitted:null acknowledgement is uncertain and never replayed', async () => {
  for (const kind of ['pi', 'opencode']) {
    const s = await system()
    let native
    try {
      native = await nativeLead(s, kind, null)
      const worker = s.workers[0]
      await writeCodexSession(s.temporary.env, worker.session, [
        {
          user: 'work',
          answer: `${kind} admission was not observed`,
          answerId: 'answer-1',
        },
      ])

      await s.watcher.start()

      const [delivery] = await s.deliveries()
      assert.equal(delivery.state, 'uncertain')
      assert.match(delivery.reason, /uncertain|unknown|not observed|admission/i)
      assert.equal(native.state.received.length, 1)

      await s.watcher.reconcile('after-null-admission')
      const [after] = await s.deliveries()
      assert.equal(after.state, 'uncertain')
      assert.equal(native.state.received.length, 1)
    } finally {
      await s.close()
      await native?.close()
    }
  }
})

test('delivery uses the lead reservation channel object instead of writing the bridge directly', async () => {
  const s = await system()
  const inbox = path.join(s.temporary.root, 'pi-inbox')
  const ack = path.join(s.temporary.root, 'pi-ack')
  await fs.mkdir(inbox)
  await fs.mkdir(ack)
  let received
  const extension = watch(inbox, { persistent: false }, async () => {
    const files = (await fs.readdir(inbox)).filter((name) => name.endsWith('.json'))
    if (files.length === 0) return
    received = JSON.parse(await fs.readFile(path.join(inbox, files[0]), 'utf8'))
    await fs.writeFile(
      path.join(ack, `${received.id}.json`),
      `${JSON.stringify({ id: received.id, admitted: true })}\n`,
    )
  })
  try {
    s.setNow(Date.now())
    const worker = s.workers[0]
    await s.store.mutate(s.workspace, 'test.lead-channel', async (io) => {
      const tabs = await io.readTabs()
      const tab = tabs.find((candidate) => candidate.id === s.tab.id)
      tab.lead.harness = 'pi'
      tab.lead.reserved = {
        channel: {
          kind: 'pi-extension',
          extensionPath: '/repo/hosts/pi-extension/consensflow-delivery.mjs',
          launchId: 'watcher-reserved-channel',
          settled: path.join(s.temporary.env.CONSENSFLOW_HOME, 'pi-settled'),
          inbox,
          ack,
          ackTimeoutMs: 1_000,
        },
      }
      await io.writeTabs(tabs)
    })
    await writePiSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-pi' },
    ])
    await writePiSettlement(
      s.temporary.env,
      'watcher-reserved-channel',
      s.leadSession,
      'lead-ready-pi',
    )
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'native channel answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()

    assert.equal(s.pipe.state.writes.length, 0)
    assert.equal(received.id, (await s.deliveries())[0].id)
    assert.equal(received.answer, 'native channel answer')
  } finally {
    extension.close()
    await s.close()
  }
})

test('submission dispatches through the reservation channel captured inside admission', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  const inbox = path.join(s.temporary.root, 'raced-pi-inbox')
  const ack = path.join(s.temporary.root, 'raced-pi-ack')
  await fs.mkdir(inbox)
  await fs.mkdir(ack)
  let received
  const extension = watch(inbox, { persistent: false }, async () => {
    const files = (await fs.readdir(inbox)).filter((name) => name.endsWith('.json'))
    if (files.length === 0) return
    received = JSON.parse(await fs.readFile(path.join(inbox, files[0]), 'utf8'))
    await fs.writeFile(
      path.join(ack, `${received.id}.json`),
      `${JSON.stringify({ id: received.id, admitted: true })}\n`,
    )
  })
  try {
    s.setNow(Date.now())
    const worker = s.workers[0]
    let changedRoute = false
    s.store.mutate = async (workspace, name, operation) => {
      if (name === 'delivery.submit' && !changedRoute) {
        changedRoute = true
        await originalMutate(workspace, 'test.route-before-admission', async (io) => {
          const tabs = await io.readTabs()
          const tab = tabs.find((candidate) => candidate.id === s.tab.id)
          tab.lead.harness = 'pi'
          tab.lead.reserved = {
            channel: {
              kind: 'pi-extension',
              extensionPath: '/repo/hosts/pi-extension/consensflow-delivery.mjs',
              inbox,
              ack,
              ackTimeoutMs: 1_000,
            },
          }
          await io.writeTabs(tabs)
        })
      }
      return await originalMutate(workspace, name, operation)
    }
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'routed after admission', answerId: 'answer-1' },
    ])
    await s.watcher.start()

    assert.equal(s.pipe.state.writes.length, 0)
    assert.equal(received.id, (await s.deliveries())[0].id)
  } finally {
    s.store.mutate = originalMutate
    extension.close()
    await s.close()
  }
})

test('Pi derived-only settlement plans no automatic record but a manual record still delivers', async () => {
  const s = await system({ floorMs: FLOOR_MS })
  try {
    const worker = s.workers[0]
    const file = await copyPiSession(s.temporary.env)
    await s.store.mutate(s.workspace, 'test.pi-binding', async (io) => {
      const threads = await io.readThreads()
      threads[worker.name].kind = 'pi'
      threads[worker.name].sessionId = 'hazy-ridge'
      await io.writeThreads(threads)
    })
    await s.watcher.start()
    assert.equal((await s.deliveries()).length, 0, 'a fresh Pi append has not settled')

    const old = new Date(Date.now() - 120_001)
    await fs.utimes(file, old, old)
    const completion = await answers('pi', 'hazy-ridge', s.temporary.env)
    assert.equal(completion.settlement.state, 'settled')
    assert.equal(completion.settlement.provenance, 'derived')
    await s.watcher.reconcile('pi-derived-only')
    assert.equal((await s.deliveries()).length, 0, 'derived evidence never plans automatic work')

    const item = completion.items.find(
      (candidate) => candidate.role === 'assistant' && candidate.complete === true,
    )
    const tab = await s.tabs.get(s.tab.id)
    const leadPane = tab.panes.find((pane) => pane.kind === 'lead')
    const row = (await s.threads())[worker.name]
    const manualId = await s.store.allocateDeliveryId()
    const [manual] = plan({
      items: [item],
      policy: { mode: 'manual' },
      kind: tab.lead.harness,
      conversation: worker.name,
      agent: row.agent,
      target: {
        leadId: leadIdentity(tab),
        session: tab.lead.nativeSession,
        tab: tab.id,
        pane: leadPane.id,
        generation: tab.lead.generation,
      },
      newId: () => manualId,
      now: 1_000,
      workspace: s.workspace,
      manual: true,
    })
    await s.store.deliveryUpsert(s.workspace, manual)
    await s.watcher.reconcile('deliver.now')

    const [delivery] = await s.deliveries()
    assert.equal(delivery.manual, true)
    assert.equal(delivery.state, 'submitting')
    assert.equal(s.pipe.state.writes.length, 1)
  } finally {
    await s.close()
  }
})

test('deliver.now passes manual purpose so a derived-only Pi lead can receive it', async () => {
  const s = await system({ workerCount: 2 })
  try {
    await s.store.mutate(s.workspace, 'test.derived-pi-lead', async (io) => {
      const tabs = await io.readTabs()
      tabs.find((candidate) => candidate.id === s.tab.id).lead.harness = 'pi'
      await io.writeTabs(tabs)
    })
    await writePiSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'derived lead ready', answerId: 'lead-derived-ready' },
    ])
    const leadCompletion = await answers('pi', s.leadSession, s.temporary.env)
    assert.equal(leadCompletion.settlement.state, 'settled')
    assert.equal(leadCompletion.settlement.provenance, 'derived')

    const [manualWorker, automaticWorker] = s.workers
    await writeCodexSession(s.temporary.env, manualWorker.session, [
      { user: 'work', answer: 'manual answer', answerId: 'manual-answer' },
    ])
    await writeCodexSession(s.temporary.env, automaticWorker.session, [
      { user: 'work', answer: 'automatic answer', answerId: 'automatic-answer' },
    ])
    const manualCompletion = await answers('codex', manualWorker.session, s.temporary.env)
    const automaticCompletion = await answers('codex', automaticWorker.session, s.temporary.env)
    const manualItem = manualCompletion.items.find(
      (candidate) => candidate.role === 'assistant' && candidate.complete === true,
    )
    const automaticItem = automaticCompletion.items.find(
      (candidate) => candidate.role === 'assistant' && candidate.complete === true,
    )
    const tab = await s.tabs.get(s.tab.id)
    const leadPane = tab.panes.find((pane) => pane.kind === 'lead')
    const rows = await s.threads()
    const target = {
      leadId: leadIdentity(tab),
      session: tab.lead.nativeSession,
      tab: tab.id,
      pane: leadPane.id,
      generation: tab.lead.generation,
    }
    const automaticId = await s.store.allocateDeliveryId()
    const [automatic] = plan({
      items: [automaticItem],
      policy: { mode: 'auto' },
      kind: tab.lead.harness,
      conversation: automaticWorker.name,
      agent: rows[automaticWorker.name].agent,
      target,
      newId: () => automaticId,
      now: 1_000,
      workspace: s.workspace,
    })
    const manualId = await s.store.allocateDeliveryId()
    const [manual] = plan({
      items: [manualItem],
      policy: { mode: 'manual' },
      kind: tab.lead.harness,
      conversation: manualWorker.name,
      agent: rows[manualWorker.name].agent,
      target,
      newId: () => manualId,
      now: 1_000,
      workspace: s.workspace,
      manual: true,
    })
    await s.store.deliveryUpsert(s.workspace, automatic)
    await s.store.deliveryUpsert(s.workspace, manual)

    await s.watcher.start()

    const deliveries = await s.deliveries()
    const pending = deliveries.find((delivery) => delivery.id === automatic.id)
    assert.equal(pending.state, 'pending')
    assert.match(pending.reason, /Pi.*native settlement evidence/i)
    assert.equal(deliveries.find((delivery) => delivery.id === manual.id).state, 'submitting')
    assert.equal(s.pipe.state.writes.length, 1)
  } finally {
    await s.close()
  }
})

test('one derived Pi frontier plans none of its completed historical answers', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writePiSession(s.temporary.env, 'pi-history', [
      { user: 'one', answer: 'first historical answer', answerId: 'pi-answer-1' },
      { user: 'two', answer: 'second historical answer', answerId: 'pi-answer-2' },
    ])
    await s.store.mutate(s.workspace, 'test.pi-history-binding', async (io) => {
      const threads = await io.readThreads()
      threads[worker.name].kind = 'pi'
      threads[worker.name].sessionId = 'pi-history'
      await io.writeThreads(threads)
    })
    await s.watcher.start()

    assert.deepEqual(await s.deliveries(), [])
  } finally {
    await s.close()
  }
})

test('a completed answer is planned even when a later turn remains open', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'one', answer: 'complete answer', answerId: 'answer-1' },
      { user: 'two still running' },
    ])
    await s.watcher.start()

    const deliveries = await s.deliveries()
    assert.deepEqual(
      deliveries.map((delivery) => delivery.answerId),
      ['answer-1'],
    )
    assert.equal(s.pipe.state.writes.length, 1)
  } finally {
    await s.close()
  }
})

test('sendHeld refuses after close and leaves durable delivery state unchanged', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    s.pipe.state.draft = true
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'held answer', answerId: 'answer-1' },
    ])
    await s.watcher.start()
    await resumeLead(s)
    await s.watcher.reconcile('resumed')
    const before = await s.deliveries()

    await s.watcher.close()
    assert.throws(() => s.watcher.attachBridge(s.pipe.node), /closed/)
    await assert.rejects(() => s.watcher.sendHeld(s.tab.id), /closed/)
    assert.deepEqual(await s.deliveries(), before)
  } finally {
    await s.close()
  }
})

test('close drains a submission already admitted before its transport call', async () => {
  const s = await system()
  const originalMutate = s.store.mutate.bind(s.store)
  let admittedResolve
  const admitted = new Promise((resolve) => {
    admittedResolve = resolve
  })
  let releaseResolve
  const release = new Promise((resolve) => {
    releaseResolve = resolve
  })
  try {
    s.store.mutate = async (workspace, name, operation) => {
      const result = await originalMutate(workspace, name, operation)
      if (name === 'delivery.submit' && result?.admitted === true) {
        admittedResolve()
        await release
      }
      return result
    }
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'must reach transport', answerId: 'answer-1' },
    ])

    const starting = s.watcher.start()
    await admitted
    const closing = s.watcher.close()
    releaseResolve()
    await Promise.all([starting, closing])

    assert.equal(s.pipe.state.writes.length, 1, 'close waits for the admitted transport call')
    assert.match(s.pipe.state.writes[0].body, /must reach transport/)
    assert.equal((await s.deliveries())[0].state, 'submitting')
  } finally {
    releaseResolve()
    s.store.mutate = originalMutate
    await s.close()
  }
})

test('system cleanup closes the restarted watcher with no timer or bridge listeners', async () => {
  const s = await system({ floorMs: FLOOR_MS })
  let restarted
  try {
    await s.watcher.start()
    await s.watcher.close()
    s.watcher = new Watcher({
      store: s.store,
      tabs: s.tabs,
      env: s.temporary.env,
      floorMs: FLOOR_MS,
    })
    restarted = s.watcher
    restarted.attachBridge(s.pipe.node)
    await restarted.start()

    await s.close()
    assert.equal(restarted.closed, true)
    assert.equal(restarted.timer, null)
    assert.deepEqual(restarted.unsubscribe, [])
  } finally {
    await restarted?.close()
  }
})

test('results list only whole completed answers and scope them to the requesting session (TEST-PANE-75)', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    let listed = await s.watcher.results(s.tab.id)
    assert.equal(listed[0].conversation, worker.name)
    assert.equal(listed[0].results.length, 0)
    assert.equal(listed[0].running, true)
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'complete result', answerId: 'complete-result' },
      { user: 'still working' },
    ])
    listed = await s.watcher.results(s.tab.id)
    assert.equal(listed[0].running, true)
    assert.deepEqual(
      listed[0].results.map((r) => [r.id, r.status]),
      [['complete-result', 'unread']],
    )
    const other = await s.tabs.create(s.workspace, 'codex')
    assert.deepEqual(await s.watcher.results(other.id), [])
    await assert.rejects(s.watcher.readResult(other.id, worker.name), /conversation|session/)
  } finally {
    await s.close()
  }
})

test('manual result reading uses complete framed receipts without injecting into a busy lead (TEST-PANE-75)', async () => {
  const s = await system({
    watcherOptions: { partBudget: { default: { bytes: 1_024, lines: 30 } } },
  })
  try {
    const worker = s.workers[0]
    const whole = 'Result line with detail.\n'.repeat(200)
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: whole, answerId: 'whole-result' },
    ])
    s.pipe.state.draft = true
    const read = await s.watcher.readResult(s.tab.id, worker.name)
    assert.equal(read.channel, 'cf-read')
    assert.equal(read.manualRead, true)
    assert.equal(read.state, 'submitting')
    assert.equal(read.answer, whole)
    assert.ok(read.parts.length > 1)
    assert.deepEqual(s.pipe.state.writes, [])
    const again = await s.watcher.readResult(s.tab.id, worker.name)
    assert.equal(again.id, read.id)
    const listed = await s.watcher.results(s.tab.id)
    assert.equal(listed[0].results[0].status, 'reading')

    const leadSeed = { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' }
    // A tail that retains the end marker is still not a complete part.
    await writeCodexSession(s.temporary.env, s.leadSession, [
      leadSeed,
      { user: 'reading', tool: read.parts.map((p) => p.text.slice(-50)).join('\n') },
    ])
    s.setNow(100_000)
    await s.watcher.reconcile()
    assert.equal((await s.deliveries()).find((r) => r.id === read.id).state, 'submitting')
    assert.equal((await s.watcher.results(s.tab.id))[0].results[0].status, 'reading')
    await writeCodexSession(s.temporary.env, s.leadSession, [
      leadSeed,
      { user: 'reading', tool: read.parts.map((p) => p.text).join('\n') },
    ])
    await s.watcher.reconcile()
    const accepted = (await s.deliveries()).find((r) => r.id === read.id)
    assert.equal(accepted.state, 'accepted')
    assert.equal(accepted.partCoverage.length, read.parts.length)
    assert.equal((await s.watcher.results(s.tab.id))[0].results[0].status, 'read')
    assert.deepEqual(s.pipe.state.writes, [])
  } finally {
    await s.close()
  }
})

test('manual result reads survive daemon restart and finish through durable native receipts', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'durable complete result', answerId: 'durable-result' },
    ])
    const reading = await s.watcher.readResult(s.tab.id, worker.name)
    await s.watcher.close()
    s.watcher = new Watcher({ store: s.store, tabs: s.tabs, env: s.temporary.env, floorMs: 60_000 })
    s.watcher.attachBridge(s.pipe.node)
    await s.watcher.start()
    assert.equal((await s.deliveries()).find((r) => r.id === reading.id).state, 'submitting')
    assert.equal((await s.watcher.readResult(s.tab.id, worker.name)).id, reading.id)
    await writeCodexSession(s.temporary.env, s.leadSession, [
      { user: 'lead question', answer: 'lead ready', answerId: 'lead-ready-1' },
      { user: 'reading', tool: reading.parts.map((p) => p.text).join('\n') },
    ])
    await s.watcher.reconcile()
    assert.equal((await s.deliveries()).find((r) => r.id === reading.id).state, 'accepted')
    assert.deepEqual(s.pipe.state.writes, [])
  } finally {
    await s.close()
  }
})

test('manual reads avoid an in-flight automatic copy but can claim draft-held results', async () => {
  const s = await system()
  try {
    const worker = s.workers[0]
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'result one', answerId: 'result-one' },
    ])
    await s.watcher.reconcile()
    const [automatic] = await s.deliveries()
    assert.equal(automatic.state, 'submitting')
    assert.notEqual(automatic.channel, 'cf-read')
    assert.equal((await s.watcher.results(s.tab.id))[0].results[0].status, 'delivering')
    await assert.rejects(
      s.watcher.readResult(s.tab.id, worker.name),
      /automatic delivery.*in progress/i,
    )
    assert.equal((await s.deliveries()).length, 1)
    await writeCodexSession(s.temporary.env, worker.session, [
      { user: 'work', answer: 'result one', answerId: 'result-one' },
      { user: 'continue', answer: 'result two', answerId: 'result-two' },
    ])
    s.pipe.state.draft = true
    await s.watcher.reconcile()
    assert.equal((await s.deliveries()).find((r) => r.answerId === 'result-two').state, 'pending')
    const read = await s.watcher.readResult(s.tab.id, worker.name, 'result-two')
    assert.equal(read.manualRead, true)
    assert.equal(read.state, 'submitting')
    assert.equal(s.pipe.state.writes.length, 1)
  } finally {
    await s.close()
  }
})

test('manual reading does not reserve the input channel or consume omitted discussion (TEST-PANE-75)', async () => {
  const s = await system({ workerCount: 2 })
  try {
    for (const worker of s.workers) {
      await writeCodexSession(s.temporary.env, worker.session, [
        { user: 'unread question', answer: worker.name, answerId: `${worker.name}-result` },
      ])
    }
    const read = await s.watcher.readResult(s.tab.id, s.workers[0].name)
    await s.watcher.reconcile()
    assert.equal((await s.deliveries()).find((r) => r.id === read.id).state, 'submitting')
    assert.equal(s.pipe.state.writes.length, 1)
    assert.match(s.pipe.state.writes[0].body, /worker-2/)
    assert.deepEqual((await s.threads())[s.workers[0].name].seen[s.leadId], [])
  } finally {
    await s.close()
  }
})

test('native user text and Enter hints never authorize clearing an opaque terminal draft', async () => {
  const s = await system()
  try {
    const clears = []
    s.pipe.state.draft = true
    s.pipe.rust.on('pane.enter_digests', () => ({
      entries: [{ epoch: 7, since: 0, digest: createHash('sha256').update('work').digest('hex') }],
    }))
    s.pipe.rust.on('draft.clear', (body) => {
      clears.push(body)
      return { ok: true }
    })
    await s.watcher.start()
    s.pipe.rust.event('pane.enter', { id: s.workers[0].pane.id, generation: 1, epoch: 7 })
    await s.watcher.reconcile()
    assert.deepEqual(
      clears,
      [],
      'a delayed identical native message cannot prove the editor is empty',
    )
    assert.equal(s.pipe.state.draft, true)
  } finally {
    await s.close()
  }
})
