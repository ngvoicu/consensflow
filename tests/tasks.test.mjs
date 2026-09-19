import assert from 'node:assert/strict'
import test from 'node:test'
import { taskWithWorkPolicy } from '../hosts/lib/presets.js'
import { Store } from '../src/store.js'
import { Tabs } from '../src/tabs.js'
import { recordAssignment, Tasks } from '../src/tasks.js'
import { tempEnv } from './helpers.mjs'

async function fixture(t) {
  const tmp = tempEnv()
  const store = await new Store(tmp.env.CONSENSFLOW_HOME).open()
  t.after(async () => {
    await store.close()
    tmp.cleanup()
  })
  const tabs = new Tabs(store)
  const lead = await tabs.create('/project/shared', 'claude-code')
  const pm = await tabs.createPm(lead.id, 'pi')
  const other = await tabs.create('/project/shared', 'codex')
  return { store, tabs, lead: lead.id, pm: pm.id, other: other.id, tasks: new Tasks(store) }
}
const add = (tasks, owner, title, extra = {}) =>
  tasks.change(owner, { action: 'add', title, ...extra })

test('automatic card title describes the assignment after the app critical-work preamble', () => {
  const tab = { id: 't-1' }
  const prompt = taskWithWorkPolicy(
    { model: 'gpt-6-astra', effort: 'ultra', workTier: 'critical' },
    'Review the session boundaries',
    'critical-review',
  )
  assert.ok(prompt.startsWith('Critical work:'))
  recordAssignment(tab, { conversation: 'review-worker', task: prompt, opId: 'initial' })
  assert.equal(tab.tasks[0].title, 'Review the session boundaries')
  assert.equal(tab.tasks[0].description, prompt)
})

test('task edits persist through store restart and never overwrite stale revisions', async (t) => {
  const { store, lead, tasks } = await fixture(t)
  const first = await add(tasks, lead, 'Fix receipt routing', {
    description: 'Evidence and checks',
    kind: 'implementation',
  })
  const next = await tasks.change(lead, {
    action: 'update',
    id: first.id,
    revision: first.revision,
    status: 'review',
    note: 'Implementation ready',
  })
  await assert.rejects(
    tasks.change(lead, {
      action: 'update',
      id: first.id,
      revision: first.revision,
      title: 'stale',
    }),
    /changed/,
  )
  await store.close()
  await store.open()
  const saved = await tasks.get(lead, first.id)
  assert.equal(saved.title, first.title)
  assert.equal(saved.status, 'review')
  assert.equal(saved.revision, next.revision)
  assert.equal(saved.history.at(-1).note, 'Implementation ready')
})

test('combined session query includes PM only for a human and never merges shared directories', async (t) => {
  const { tasks, lead, pm, other } = await fixture(t)
  await add(tasks, lead, 'Implementation')
  await add(tasks, pm, 'Specification', { kind: 'specification' })
  await add(tasks, other, 'Unrelated session')
  assert.deepEqual(
    (await tasks.list(lead)).tasks.map((x) => x.title),
    ['Implementation'],
  )
  assert.equal((await tasks.list(lead, { combined: true })).total, 2)
  assert.deepEqual(
    new Set((await tasks.list(pm, { combined: true })).owners.map((x) => x.id)),
    new Set([lead, pm]),
  )
  const foreign = (await tasks.list(pm)).tasks[0]
  await assert.rejects(tasks.get(lead, foreign.id), /not found/)
  await assert.rejects(
    tasks.change(lead, {
      action: 'update',
      id: foreign.id,
      revision: foreign.revision,
      status: 'accepted',
    }),
    /not found/,
  )
})

test('links validate ownership and cycles without corrupting the task ledger', async (t) => {
  const { tasks, lead, pm } = await fixture(t)
  const a = await add(tasks, lead, 'Implementation')
  const b = await add(tasks, lead, 'Independent review', { kind: 'review', reviewOf: a.id })
  const p = await add(tasks, pm, 'PM work')
  for (const patch of [
    { dependsOn: [b.id] },
    { reviewOf: a.id },
    { dependsOn: [p.id] },
    { dependsOn: ['missing'] },
  ])
    await assert.rejects(
      tasks.change(lead, { action: 'update', id: a.id, revision: a.revision, ...patch }),
      /cycle|own group|not found/,
    )
  assert.deepEqual((await tasks.get(lead, a.id)).dependsOn, [])
})

test('questions have stable IDs; human answers preserve newer questions and do not auto-accept', async (t) => {
  const { tasks, lead } = await fixture(t)
  let item = await add(tasks, lead, 'Choose storage')
  item = await tasks.change(lead, {
    action: 'update',
    id: item.id,
    revision: item.revision,
    question: 'SQLite or JSON?',
  })
  assert.equal(item.status, 'blocked')
  const question = item.questions[0].id
  const stale = item.revision
  item = await tasks.change(lead, {
    action: 'update',
    id: item.id,
    revision: item.revision,
    question: 'Keep history?',
  })
  await assert.rejects(
    tasks.change(
      lead,
      { action: 'answer', id: item.id, revision: stale, question, answer: 'JSON' },
      'human',
    ),
    /changed/,
  )
  await assert.rejects(
    tasks.change(lead, {
      action: 'answer',
      id: item.id,
      revision: item.revision,
      question,
      answer: 'JSON',
    }),
    /human/,
  )
  item = await tasks.change(
    lead,
    { action: 'answer', id: item.id, revision: item.revision, question, answer: 'JSON' },
    'human',
  )
  assert.equal(item.questions.length, 2)
  assert.equal(item.questions[0].answer, 'JSON')
  assert.equal(item.questions[1].answer, undefined)
  assert.equal(item.status, 'blocked')
  assert.equal(item.history.at(-1).actor, 'human')
})

test('automatic assignment captures initial text and followups once; a new dispatch reopens accepted work', () => {
  const tab = { id: 't-1', role: 'lead', tasks: [] }
  const input = {
    conversation: 'worker-one',
    task: 'Implement the parser',
    agent: 'alpha',
    opId: 'op-1',
  }
  recordAssignment(tab, input, 100)
  recordAssignment(tab, input, 101)
  assert.equal(tab.tasks.length, 1)
  assert.equal(tab.tasks[0].history.length, 1)
  tab.tasks[0].status = 'accepted'
  recordAssignment(tab, { ...input, task: 'Now test malformed input', opId: 'op-2' }, 200)
  assert.equal(tab.tasks[0].description, 'Implement the parser')
  assert.equal(tab.tasks[0].history.at(-1).note, 'Now test malformed input')
  assert.equal(tab.tasks[0].status, 'active')
  assert.equal(tab.tasks[0].source, 'assignment')
})

test('historical assignments are honest, editable, and survive a later pane deletion', async (t) => {
  const { store, tabs, lead, tasks } = await fixture(t)
  const pane = await tabs.addPane(lead, { kind: 'worker', conversation: 'old-worker' })
  await store.conversationCreate('/project/shared', {
    name: 'old-worker',
    agent: 'alpha',
    kind: 'codex',
    lead: `tab:${lead}:1`,
    pane: { tab: lead, id: pane.id, generation: pane.generation },
  })
  const row = (await tasks.list(lead)).tasks[0]
  assert.equal(row.source, 'historical')
  assert.equal(row.title, 'old-worker')
  assert.equal(row.status, 'unknown')
  const full = await tasks.get(lead, row.id)
  assert.equal(full.description, '')
  const saved = await tasks.change(
    lead,
    { action: 'update', id: row.id, revision: 0, status: 'review', title: 'Recovered review' },
    'human',
  )
  await tabs.removePane(lead, pane.id, pane.generation)
  assert.equal((await tasks.get(lead, row.id)).id, saved.id)
})

test('automatic and historical assignments cannot reassign their conversation', async (t) => {
  const { store, tabs, lead, tasks } = await fixture(t)
  const pane = await tabs.addPane(lead, { kind: 'worker', conversation: 'old-worker' })
  await store.conversationCreate('/project/shared', {
    name: 'old-worker',
    agent: 'alpha',
    kind: 'codex',
    lead: `tab:${lead}:1`,
    pane: { tab: lead, id: pane.id, generation: pane.generation },
  })
  for (const source of ['historical', 'assignment']) {
    if (source === 'assignment')
      await store.mutate(null, 'test.assignment', async (io) => {
        const rows = await io.readTabs()
        recordAssignment(
          rows.find((tab) => tab.id === lead),
          { conversation: 'new-worker', task: 'Original work', opId: 'first' },
        )
        await io.writeTabs(rows)
      })
    const task = (await tasks.list(lead)).tasks.find((task) => task.source === source)
    await assert.rejects(
      tasks.change(lead, {
        action: 'update',
        id: task.id,
        revision: task.revision,
        conversation: null,
      }),
      /cannot change/i,
    )
    assert.ok((await tasks.get(lead, task.id)).conversation)
  }
})

test('first recorded followup retains missing-original provenance and only appends history', () => {
  const tab = { id: 't-1', tasks: [] }
  recordAssignment(tab, {
    conversation: 'old-worker',
    task: 'One more review',
    agent: 'alpha',
    opId: 'second',
    initial: false,
  })
  assert.equal(tab.tasks[0].source, 'historical')
  assert.equal(tab.tasks[0].title, 'old-worker')
  assert.equal(tab.tasks[0].description, '')
  assert.equal(tab.tasks[0].history[0].note, 'One more review')
})

test('review links retain virtual historical targets after permanent pane deletion', async (t) => {
  const { store, tabs, lead, tasks } = await fixture(t)
  const pane = await tabs.addPane(lead, { kind: 'worker', conversation: 'old-worker' })
  await store.conversationCreate('/project/shared', {
    name: 'old-worker',
    agent: 'alpha',
    kind: 'codex',
    lead: `tab:${lead}:1`,
    pane: { tab: lead, id: pane.id, generation: pane.generation },
  })
  const original = (await tasks.list(lead)).tasks[0]
  const review = await add(tasks, lead, 'Review old work', {
    kind: 'review',
    reviewOf: original.id,
  })
  await tabs.beginPaneDelete(lead, pane.id, pane.generation)
  await tabs.removePane(lead, pane.id, pane.generation)
  assert.equal((await tasks.get(lead, original.id)).source, 'historical')
  await tasks.change(lead, {
    action: 'update',
    id: review.id,
    revision: review.revision,
    status: 'accepted',
  })
})

test('aggregate escaped task bytes are bounded before mutation so the complete reply fits the bridge', async (t) => {
  const { tasks, lead } = await fixture(t)
  const control = '\u0001'.repeat(1000)
  let item = await add(tasks, lead, 'Large history', { description: '\u0001'.repeat(32768) })
  let refused = false
  for (let i = 0; i < 64; i++) {
    const before = await tasks.get(lead, item.id)
    try {
      item = await tasks.change(lead, {
        action: 'update',
        id: item.id,
        revision: item.revision,
        question: control,
        note: control,
      })
      item = await tasks.change(
        lead,
        {
          action: 'answer',
          id: item.id,
          revision: item.revision,
          question: item.questions.at(-1).id,
          answer: control,
        },
        'human',
      )
    } catch (error) {
      assert.match(error.message, /task.*size|task.*large/i)
      const saved = await tasks.get(lead, item.id)
      assert.equal(saved.revision, item.revision)
      assert.ok(saved.revision >= before.revision)
      refused = true
      break
    }
  }
  assert.equal(refused, true)
  assert.ok(Buffer.byteLength(JSON.stringify(await tasks.get(lead, item.id))) < 600000)
})

test('summaries paginate without full prompts/history and details stay bounded', async (t) => {
  const { tasks, lead } = await fixture(t)
  const prompt = 'large task '.repeat(2500)
  const first = await add(tasks, lead, 'Large task', { description: prompt })
  for (let i = 0; i < 4; i++) await add(tasks, lead, `Task ${i}`)
  const page = await tasks.list(lead, { limit: 2 })
  assert.equal(page.total, 5)
  assert.equal(page.tasks.length, 2)
  assert.equal(page.next, 2)
  assert.ok(page.tasks.every((x) => x.description === undefined && x.history === undefined))
  assert.equal((await tasks.get(lead, first.id)).description, prompt)
  assert.equal((await tasks.list(lead, { offset: 4, limit: 2 })).next, null)
  await assert.rejects(tasks.list(lead, { offset: -1 }), /offset/)
  await assert.rejects(add(tasks, lead, 'Bad', { description: 'x'.repeat(40000) }), /description/)
})

test('invalid states, fields, conversation ownership and lifecycle fences fail before writing', async (t) => {
  const { tasks, store, lead, pm } = await fixture(t)
  const one = await add(tasks, lead, 'Task')
  await assert.rejects(
    tasks.change(lead, { action: 'update', id: one.id, revision: one.revision, state: 'received' }),
    /field/,
  )
  await assert.rejects(
    tasks.change(lead, {
      action: 'update',
      id: one.id,
      revision: one.revision,
      status: 'received',
    }),
    /status/,
  )
  await store.conversationCreate('/project/shared', {
    name: 'advisor-one',
    agent: 'alpha',
    kind: 'pi',
    lead: `tab:${pm}:1`,
  })
  await assert.rejects(
    add(tasks, lead, 'Invalid assignment', { conversation: 'advisor-one' }),
    /own group/,
  )
  await store.mutate(null, 'test.deleting', async (io) => {
    const tabs = await io.readTabs()
    tabs.find((x) => x.id === lead).deleting = true
    await io.writeTabs(tabs)
  })
  await assert.rejects(
    tasks.change(lead, {
      action: 'update',
      id: one.id,
      revision: one.revision,
      status: 'accepted',
    }),
    /delet/,
  )
})
