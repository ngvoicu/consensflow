import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { digest, envelope } from '../hosts/lib/deliveries.js'
import { beginInsertion, claimNext, registerReceiver, resultStatus } from '../hosts/lib/inbox.js'
import { launchConfiguration } from '../src/channels.js'
import { Watcher } from '../src/delivery-watch.js'
import { Page } from '../src/page.js'
import { Store } from '../src/store.js'
import { Tabs } from '../src/tabs.js'
import { tempEnv } from './helpers.mjs'

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

async function setup(t) {
  const f = tempEnv(),
    cwd = path.join(f.root, 'project')
  await fs.mkdir(cwd)
  const store = new Store(f.env.CONSENSFLOW_HOME)
  await store.open()
  const tabs = new Tabs(store)
  const lead = await tabs.create(cwd, 'codex')
  const pm = await tabs.create(cwd, 'claude-code', { parentTabId: lead.id })
  await store.mutate(cwd, 'test.seed', async (io) => {
    await io.writeThreads({
      worker: {
        agent: 'zeus',
        kind: 'codex',
        lead: lead.leadId,
        sessionId: 'native-worker',
        binding: { launchId: 'worker-launch' },
      },
      advisor: {
        agent: 'zeus',
        kind: 'codex',
        role: 'advisor',
        lead: pm.leadId,
        sessionId: 'native-advisor',
        binding: { launchId: 'advisor-launch' },
      },
    })
    const all = await io.readTabs()
    for (const tab of all) {
      tab.closed = true
      tab.policy = 'manual'
    }
    await io.writeTabs(all)
  })
  const errors = [],
    requests = []
  const watcher = new Watcher({
    store,
    tabs,
    env: f.env,
    floorMs: 60_000,
    onError: (error) => errors.push(error),
  })
  watcher.attachBridge({
    request: (...args) => {
      requests.push(args)
      throw Error('old sender must never be invoked')
    },
    onEvent: () => () => {},
  })
  t.after(async () => {
    await watcher.close()
    await store.close()
    f.cleanup()
  })
  return { f, cwd, store, tabs, lead, pm, watcher, errors, requests }
}

test('scanner indexes every completed reply for closed/manual/unbound coordinators and separates PM ownership', async (t) => {
  const s = await setup(t)
  await writeCodexSession(
    s.f.env,
    'native-worker',
    Array.from({ length: 40 }, (_, n) => ({ answer: `reply ${n}`, answerId: `answer-${n}` })),
  )
  await writeCodexSession(s.f.env, 'native-advisor', [
    { answer: 'advisor findings', answerId: 'advice' },
  ])
  await s.watcher.reconcile()
  const state = await s.store.readInbox(s.cwd)
  assert.equal(Object.values(state.results).filter((r) => r.owner === s.lead.id).length, 40)
  assert.equal(Object.values(state.results).filter((r) => r.owner === s.pm.id).length, 1)
  await s.watcher.reconcile()
  assert.equal(Object.keys((await s.store.readInbox(s.cwd)).results).length, 41)
  assert.deepEqual(s.requests, [])
  assert.deepEqual(s.errors, [])
  assert.deepEqual(await fs.readdir(s.cwd), [])
})

test('the scanner never reads a conversation deleted from its session', async (t) => {
  // 153 of 168 conversations the live daemon re-parsed every second were ones
  // Gabriel had deleted from the session (2026-09-19 diagnosis).
  const s = await setup(t)
  await s.store.mutate(s.cwd, 'test.delete', async (io) => {
    const all = await io.readTabs()
    all.find((tab) => tab.id === s.lead.id).deletedConversations = ['worker']
    await io.writeTabs(all)
  })
  await writeCodexSession(s.f.env, 'native-worker', [{ answer: 'deleted reply', answerId: 'gone' }])
  await writeCodexSession(s.f.env, 'native-advisor', [{ answer: 'kept', answerId: 'kept' }])
  await s.watcher.reconcile()
  const results = Object.values((await s.store.readInbox(s.cwd)).results)
  assert.deepEqual(
    results.map((r) => r.answer),
    ['kept'],
  )
  assert.deepEqual(s.errors, [])
})

test('stored results stay readable while an unrelated background scan is stalled', async (t) => {
  const s = await setup(t)
  await writeCodexSession(s.f.env, 'native-worker', [
    { answer: 'Full saved report', answerId: 'saved' },
  ])
  await s.watcher.reconcile()
  const gate = Promise.withResolvers()
  const entered = Promise.withResolvers()
  const list = s.tabs.list.bind(s.tabs)
  s.tabs.list = async () => {
    entered.resolve()
    await gate.promise
    return list()
  }
  const scanning = s.watcher.reconcile()
  await entered.promise
  let timer
  try {
    const result = await Promise.race([
      Promise.all([
        s.watcher.readResult(s.lead.id, 'worker', 'saved'),
        s.watcher.results(s.lead.id),
      ]),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('stored read waited on scanning')), 500)
      }),
    ])
    assert.equal(result[0].answer, 'Full saved report')
    assert.equal(result[1][0].results[0].id, 'saved')
  } finally {
    clearTimeout(timer)
    gate.resolve()
    await scanning
  }
})

test('repeated scan requests share the in-progress scan instead of creating a backlog', async (t) => {
  const s = await setup(t)
  const gate = Promise.withResolvers()
  const entered = Promise.withResolvers()
  const list = s.tabs.list.bind(s.tabs)
  let scans = 0
  s.tabs.list = async () => {
    scans++
    entered.resolve()
    await gate.promise
    return list()
  }
  const first = s.watcher.reconcile()
  await entered.promise
  const queued = Array.from({ length: 50 }, () => s.watcher.reconcile())
  gate.resolve()
  await Promise.all([first, ...queued])
  assert.equal(scans, 1)
  await s.watcher.reconcile()
  assert.equal(scans, 2, 'a later request still refreshes the inbox')
})

test('scanner migrates accepted and uncertain historical copies once and preserves evidence with missing native history', async (t) => {
  const s = await setup(t)
  const target = {
    tab: s.lead.id,
    leadId: s.lead.leadId,
    pane: 'p-1',
    generation: 1,
    session: 'old-lead',
  }
  const records = plan({
    items: [1, 2].map((n) => ({
      id: `answer-${n}`,
      role: 'assistant',
      complete: true,
      settled: true,
      text: `stored answer ${n}`,
    })),
    conversation: 'worker',
    agent: 'zeus',
    kind: 'codex',
    target,
    now: 1,
    workspace: s.cwd,
    newId: (() => {
      let id = 100
      return () => `d-${id++}`
    })(),
  })
  records[0].state = 'accepted'
  records[0].evidenceIds = ['native-proof']
  records[1].state = 'uncertain'
  records[1].attempts = 1
  records[1].submittedAt = 2
  const duplicate = {
    ...resend(records[1], { id: 'd-102', now: 3, workspace: s.cwd }),
    manualRead: true,
    state: 'submitting',
  }
  await s.store.mutate(s.cwd, 'test.legacy', (io) =>
    io.writeDeliveries(Object.fromEntries([...records, duplicate].map((r) => [r.id, r]))),
  )
  await s.watcher.reconcile()
  const migrated = Object.values((await s.store.readInbox(s.cwd)).results)
  assert.equal(migrated.length, 2)
  assert.deepEqual(migrated.map(resultStatus).sort(), ['received', 'uncertain'])
  assert.equal(migrated.find((r) => r.answerId === 'answer-2').legacy.length, 2)
  assert.equal((await s.store.readDeliveries(s.cwd))['d-100'].evidenceIds[0], 'native-proof')
  await s.watcher.reconcile()
  assert.equal(Object.keys((await s.store.readInbox(s.cwd)).results).length, 2)
  assert.deepEqual(s.requests, [])
})

test('scanner reconciles exact native receipts after restart without submitting or guessing the selected session', async (t) => {
  const s = await setup(t)
  await writeCodexSession(s.f.env, 'native-worker', [
    { answer: 'complete answer', answerId: 'one' },
  ])
  await s.watcher.reconcile()
  let selected
  await s.store.mutate(s.cwd, 'test.claim', async (io) => {
    const state = await io.readInbox()
    registerReceiver(state, {
      owner: s.lead.id,
      launch: 'lead-launch',
      pane: 'p-1',
      generation: 1,
      kind: 'codex',
      session: 'native-old',
      lease: 'lease',
      previous: null,
      now: 1,
    })
    selected = claimNext(state, { owner: s.lead.id, lease: 'lease', id: 'claim', now: 2 })
    beginInsertion(state, {
      owner: s.lead.id,
      lease: 'lease',
      result: selected.result,
      claim: selected.id,
      now: 3,
    })
    await io.writeInbox(state)
  })
  await writeCodexSession(s.f.env, 'native-old', [
    { user: selected.text, answer: 'received', answerId: 'lead-response' },
  ])
  await s.watcher.start()
  assert.equal(resultStatus((await s.store.readInbox(s.cwd)).results[selected.result]), 'received')
  assert.deepEqual(s.requests, [])
})

test('Pi integration runtime paths use configured private home and never create project bookkeeping', async (t) => {
  const s = await setup(t)
  const launch = await launchConfiguration('pi', {
    launchId: 'private-launch',
    workspace: s.cwd,
    env: s.f.env,
  })
  for (const name of ['inbox', 'ack', 'settled', 'quarantine', 'expired'])
    assert.ok(
      launch.channel[name].startsWith(s.f.env.CONSENSFLOW_HOME + path.sep),
      launch.channel[name],
    )
  assert.deepEqual(await fs.readdir(s.cwd), [])
})

test('superseded automatic dispatch and per-channel delivery exports are absent', async () => {
  for (const file of [
    'channels.js',
    'channels/claude-peer.js',
    'channels/codex.js',
    'channels/opencode.js',
    'channels/pi.js',
    'channels/pty.js',
  ]) {
    const module = await import(`../src/${file}`)
    assert.equal(module.deliver, undefined, file)
  }
})

test('migration preserves cancelled offers and parent reads; rediscovery never churns result IDs', async (t) => {
  const s = await setup(t)
  const [old] = plan({
    items: [
      { id: 'legacy-one', role: 'assistant', complete: true, settled: true, text: 'retained body' },
    ],
    conversation: 'worker',
    agent: 'zeus',
    kind: 'codex',
    now: 1,
    newId: () => 'd-900',
    target: {
      tab: s.lead.id,
      leadId: s.lead.leadId,
      pane: 'p-1',
      generation: 1,
      session: 'old-lead',
    },
  })
  old.state = 'cancelled'
  const parent = {
    ...resend(old, { id: 'd-901', now: 2 }),
    sourceParent: s.lead.id,
    manual: true,
    target: { ...old.target, tab: s.pm.id },
  }
  await s.store.mutate(s.cwd, 'test.legacy', (io) =>
    io.writeDeliveries({ [old.id]: old, [parent.id]: parent }),
  )
  await s.watcher.reconcile()
  let results = Object.values((await s.store.readInbox(s.cwd)).results)
  assert.equal(resultStatus(results.find((r) => r.owner === s.lead.id)), 'cancelled')
  assert.equal(results.find((r) => r.owner === s.pm.id).manualOnly, true)
  await writeCodexSession(s.f.env, 'native-worker', [
    { answer: old.answer, answerId: old.answerId },
  ])
  await s.watcher.reconcile()
  let allocations = 0
  const allocate = s.store.allocateDeliveryId.bind(s.store)
  s.store.allocateDeliveryId = () => {
    allocations++
    return allocate()
  }
  await s.watcher.reconcile()
  await s.watcher.reconcile()
  assert.equal(allocations, 0)
  results = Object.values((await s.store.readInbox(s.cwd)).results)
  assert.equal(results.length, 2)
  assert.deepEqual(s.errors, [])
})

function plan({ items, newId, conversation, agent, kind, target, now }) {
  return items.map((item) => {
    const record = {
      id: newId(),
      conversation,
      agent,
      kind,
      target,
      createdAt: now,
      answerId: item.id,
      answer: item.text,
      channel: 'pty-inline',
      state: 'pending',
    }
    record.digest = digest(envelope(record))
    return record
  })
}
function resend(record, { id, now }) {
  const next = { ...record, id, state: 'pending', createdAt: now, resendOf: record.id }
  next.digest = digest(envelope(next))
  return next
}

test('scanner recovers a historical native receipt while preserving the original uncertain attempt', async (t) => {
  const s = await setup(t)
  const { answers } = await import('../hosts/lib/completion.js')
  await writeCodexSession(s.f.env, 'native-old', [
    { user: 'initial question', answer: 'initial reply' },
  ])
  const before = await answers('codex', 'native-old', s.f.env)
  const old = {
    id: 'd-100',
    conversation: 'worker',
    agent: 'zeus',
    answerId: 'old-answer',
    answer: 'Legacy exact whole answer',
    kind: 'codex',
    channel: 'pty-inline',
    state: 'uncertain',
    attempts: 1,
    createdAt: 1,
    submittedAt: 2,
    target: { tab: s.lead.id, session: 'native-old' },
    snapshot: { targetSession: 'native-old', cursor: before.items.at(-1).seq },
    nativeSubmissionId: 'legacy-user-id',
  }
  old.digest = digest(envelope(old))
  await s.store.mutate(s.cwd, 'test.legacy', (io) => io.writeDeliveries({ [old.id]: old }))
  await s.watcher.reconcile()
  assert.equal(
    resultStatus(Object.values((await s.store.readInbox(s.cwd)).results)[0]),
    'uncertain',
  )
  await writeCodexSession(s.f.env, 'native-old', [
    { user: 'initial question', answer: 'initial reply' },
    { user: envelope(old), userId: 'legacy-user-id', answer: 'received' },
  ])
  await s.watcher.reconcile()
  const result = Object.values((await s.store.readInbox(s.cwd)).results)[0]
  assert.equal(resultStatus(result), 'received')
  assert.equal(result.legacy[0].state, 'uncertain')
  assert.deepEqual(result.legacyEvidence[0].ids, ['legacy-user-id'])
  assert.deepEqual((await s.store.readDeliveries(s.cwd))['d-100'], old)
  assert.equal(s.requests.length, 0)
})

test('pane activity follows native task state, current receiver and exact worker generation without changing receipts', async (t) => {
  const s = await setup(t)
  const worker = await s.tabs.addPane(s.lead.id, { kind: 'worker', conversation: 'worker' })
  const advisor = await s.tabs.addPane(s.pm.id, { kind: 'worker', conversation: 'advisor' })
  const shell = await s.tabs.addPane(s.lead.id, { kind: 'shell' })
  await s.store.mutate(s.cwd, 'test.live', async (io) => {
    const all = await io.readTabs()
    const state = await io.readInbox()
    for (const tab of all) {
      tab.closed = false
      tab.lead.harness = 'codex'
      tab.lead.nativeSession = 'obsolete-lead'
      tab.lead.reserved = { launchId: `${tab.id}-launch`, resolvedAt: 'now' }
      registerReceiver(state, {
        owner: tab.id,
        launch: tab.lead.reserved.launchId,
        pane: tab.panes[0].id,
        generation: 1,
        kind: 'codex',
        session: `${tab.id}-current`,
        lease: `${tab.id}-lease`,
        previous: null,
        now: 1,
      })
    }
    const rows = await io.readThreads()
    for (const [name, pane, owner] of [
      ['worker', worker, s.lead.id],
      ['advisor', advisor, s.pm.id],
    ]) {
      rows[name].binding.generation = pane.generation
      rows[name].reserved = {
        tab: owner,
        pane: pane.id,
        generation: pane.generation,
        launchId: rows[name].binding.launchId,
        resolvedAt: 'now',
      }
    }
    await io.writeThreads(rows)
    await io.writeTabs(all)
    await io.writeInbox(state)
  })
  const page = new Page({
    store: s.store,
    tabs: s.tabs,
    watcher: s.watcher,
    agents: { names: () => [], row: () => null },
  })
  const activity = async (owner, id) =>
    (await page.state()).tabs.find((tab) => tab.id === owner).panes.find((pane) => pane.id === id)
      .activity.state
  const leadPane = (await s.tabs.get(s.lead.id)).panes[0]
  const pmPane = (await s.tabs.get(s.pm.id)).panes[0]
  assert.equal(await activity(s.lead.id, leadPane.id), 'unknown')
  await writeCodexSession(s.f.env, 'obsolete-lead', [{ answer: 'old answer' }])
  await writeCodexSession(s.f.env, `${s.lead.id}-current`, [{}])
  await writeCodexSession(s.f.env, `${s.pm.id}-current`, [{ answer: 'pm answer' }])
  await writeCodexSession(s.f.env, 'native-worker', [{}])
  await writeCodexSession(s.f.env, 'native-advisor', [{ answer: 'advice' }])
  await s.watcher.reconcile()
  assert.equal(await activity(s.lead.id, leadPane.id), 'working')
  assert.equal(await activity(s.pm.id, pmPane.id), 'idle')
  assert.equal(await activity(s.lead.id, worker.id), 'working')
  assert.equal(await activity(s.pm.id, advisor.id), 'idle')
  assert.equal(await activity(s.lead.id, shell.id), 'open')
  const before = JSON.stringify(await s.store.readInbox(s.cwd))
  await page.state()
  assert.equal(JSON.stringify(await s.store.readInbox(s.cwd)), before)

  await writeCodexSession(s.f.env, 'native-worker', [{ answer: 'finished' }])
  await s.watcher.reconcile()
  assert.equal(await activity(s.lead.id, worker.id), 'idle')
  await writeCodexSession(s.f.env, 'native-worker', [{ answer: 'finished' }, {}])
  await s.watcher.reconcile()
  assert.equal(await activity(s.lead.id, worker.id), 'working', 'every follow-up counts')
  s.watcher.now = () => Date.now() + 11_000
  assert.equal(
    await activity(s.lead.id, worker.id),
    'unknown',
    'a stalled scan cannot leave stale Working visible',
  )
  s.watcher.now = Date.now

  await s.store.mutate(s.cwd, 'test.new-session', async (io) => {
    const state = await io.readInbox()
    registerReceiver(state, {
      ...state.receivers[s.lead.id],
      session: 'selected-new',
      lease: 'new-lease',
      previous: state.receivers[s.lead.id].lease,
      now: 2,
    })
    await io.writeInbox(state)
    const all = await io.readTabs()
    all.find((tab) => tab.id === s.lead.id).panes.find((pane) => pane.id === worker.id).generation++
    await io.writeTabs(all)
  })
  assert.equal(
    await activity(s.lead.id, leadPane.id),
    'unknown',
    'old selected session does not leak into /new',
  )
  assert.equal(
    await activity(s.lead.id, worker.id),
    'unknown',
    'old binding cannot label a replacement pane',
  )
  await writeCodexSession(s.f.env, 'selected-new', [{ answer: 'new answer' }])
  await s.watcher.reconcile()
  assert.equal(await activity(s.lead.id, leadPane.id), 'idle')
  const file = await writeCodexSession(s.f.env, 'selected-new', [])
  await fs.rm(file)
  await s.watcher.reconcile()
  assert.equal(
    await activity(s.lead.id, leadPane.id),
    'unknown',
    'unreadable native state is not Idle',
  )
  await s.store.mutate(s.cwd, 'test.close', async (io) => {
    const all = await io.readTabs()
    all.find((tab) => tab.id === s.lead.id).closed = true
    const pm = all.find((tab) => tab.id === s.pm.id)
    delete pm.lead.reserved.resolvedAt
    pm.panes.find((pane) => pane.id === advisor.id).failure = { message: 'exited' }
    await io.writeTabs(all)
  })
  assert.equal(await activity(s.lead.id, leadPane.id), 'closed')
  assert.equal(await activity(s.pm.id, pmPane.id), 'starting')
  assert.equal(await activity(s.pm.id, advisor.id), 'failed')
  assert.deepEqual(s.errors, [])
})
