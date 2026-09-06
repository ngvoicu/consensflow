import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { leadId } from '../hosts/lib/threads.js'
import { Store } from '../src/store.js'
import { leadIdentity, paneIdentity, samePane, Tabs } from '../src/tabs.js'

/**
 * The tab store (TEST-PANE-13) rides the app store's ONE queue: every tab
 * write is a `store.mutate(dir, 'tab.*', …)`, so the serialization proofs
 * in tests/store.test.mjs cover these mutations too. What is left to prove
 * here is the tab semantics: identities, panes and their order, and the
 * restart rule.
 */
async function withTabs(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-tabs-'))
  const store = new Store(path.join(dir, 'consensflow'))
  await store.open()
  const tabs = new Tabs(store)
  try {
    return await fn({ dir, store, tabs })
  } finally {
    await store.close()
    await rm(dir, { recursive: true, force: true })
  }
}

test('tabs: concurrent creates in different directories all land with distinct identities', async () => {
  await withTabs(async ({ dir, tabs }) => {
    // Real creates, not raw fixtures: the counter and the tab list share
    // the store's one queue, so concurrent mints cannot collide.
    const [a, b, c] = await Promise.all([
      tabs.create(path.join(dir, 'a'), 'pi'),
      tabs.create(path.join(dir, 'b'), 'codex'),
      tabs.create(path.join(dir, 'a'), 'pi'),
    ])
    assert.equal(new Set([a.id, b.id, c.id]).size, 3)
    assert.equal(new Set([a.leadId, b.leadId, c.leadId]).size, 3)
    const panes = (await tabs.list()).flatMap((tab) => tab.panes.map((pane) => pane.id))
    assert.equal(panes.length, 3)
    assert.equal(new Set(panes).size, 3, 'every lead pane minted a distinct id')
  })
})

test('tabs: tab.create returns {id, generation: 1, leadId: "tab:<id>:1"}', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const created = await tabs.create(path.join(dir, 'project'), 'claude-code')

    assert.match(created.id, /^t-\d+$/)
    assert.equal(created.generation, 1)
    assert.equal(created.leadId, `tab:${created.id}:1`)

    // A tab is a directory, one lead pane and a policy — created whole.
    const tab = await tabs.get(created.id)
    assert.equal(tab.closed, false)
    assert.equal(tab.directory, path.join(dir, 'project'))
    assert.equal(tab.lead.harness, 'claude-code')
    assert.equal(tab.policy, 'auto')
    assert.equal(leadIdentity(tab), created.leadId)
    assert.equal(tab.panes.length, 1)
    assert.equal(tab.panes[0].kind, 'lead')

    // Failure branches: an image preset cannot hold a pane, an unknown tab
    // is nobody's.
    await assert.rejects(() => tabs.create(dir, 'image'), /harness/)
    await assert.rejects(() => tabs.create(dir, ''), /harness/)
    assert.equal(await tabs.get('t-999'), null)
  })
})

test('tabs: two tabs may share a directory without sharing a lead', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const shared = path.join(dir, 'shared')
    const one = await tabs.create(shared, 'pi')
    const two = await tabs.create(shared, 'pi')

    assert.notEqual(one.id, two.id)
    assert.notEqual(one.leadId, two.leadId, 'same directory, two distinct leads')

    const listed = await tabs.list()
    assert.equal(listed.length, 2)
    assert.deepEqual(
      listed.map((tab) => tab.directory),
      [shared, shared],
    )
  })
})

test('tabs: pane ids are app-wide unique — two tabs never share a pane identity', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const one = await tabs.create(path.join(dir, 'a'), 'pi')
    const two = await tabs.create(path.join(dir, 'b'), 'pi')
    const leadOne = (await tabs.get(one.id)).panes[0]
    const leadTwo = (await tabs.get(two.id)).panes[0]

    assert.notEqual(leadOne.id, leadTwo.id, 'every lead pane is minted, none is a hardcoded p-1')
    assert.equal(samePane(leadOne, leadTwo), false)

    const workerOne = await tabs.addPane(one.id, { kind: 'worker' })
    const workerTwo = await tabs.addPane(two.id, { kind: 'worker' })
    const ids = [leadOne.id, leadTwo.id, workerOne.id, workerTwo.id]
    assert.equal(new Set(ids).size, ids.length, 'no pane identity repeats across tabs')
  })
})

test('tabs: removing the highest pane never recycles its id', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const { id } = await tabs.create(dir, 'pi')
    const first = await tabs.addPane(id, { kind: 'shell' })
    const highest = await tabs.addPane(id, { kind: 'shell' })

    assert.equal(await tabs.removePane(id, highest.id, highest.generation), true)
    const late = await tabs.addPane(id, { kind: 'shell' })
    assert.notEqual(late.id, highest.id, 'the minter never hands out a removed id')
    assert.notEqual(late.id, first.id)
  })
})

test('tabs: an explicit duplicate pane id is refused, in any tab', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const one = await tabs.create(path.join(dir, 'a'), 'pi')
    const two = await tabs.create(path.join(dir, 'b'), 'pi')

    await tabs.addPane(one.id, { id: 'custom-1', kind: 'shell' })
    await assert.rejects(
      () => tabs.addPane(one.id, { id: 'custom-1', kind: 'shell' }),
      /duplicate pane id/,
    )
    await assert.rejects(
      () => tabs.addPane(two.id, { id: 'custom-1', kind: 'shell' }),
      /duplicate pane id/,
      'uniqueness is app-wide, not per tab',
    )
    await assert.rejects(() => tabs.addPane(one.id, { id: 'p-5', kind: 'shell' }), /p-\d+/i)
  })
})

test('tabs: removePane with a stale generation refuses and keeps the replacement', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const { id } = await tabs.create(dir, 'pi')
    const before = (await tabs.get(id)).panes.find((pane) => pane.kind === 'lead')

    // The lead pane is reopened under the same id with a new generation;
    // a delayed removal naming the old generation must not take it.
    await tabs.suspend(id)
    await tabs.resume(id)
    await assert.rejects(() => tabs.removePane(id, before.id, before.generation), /generation/)
    const after = (await tabs.get(id)).panes.find((pane) => pane.kind === 'lead')
    assert.equal(after.generation, before.generation + 1, 'the replacement survived')
  })
})

test('tabs: addPane appends, removePane drops one, and order is kept', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const { id } = await tabs.create(dir, 'pi')
    const leadPane = (await tabs.get(id)).panes[0]

    const shell = await tabs.addPane(id, { kind: 'shell' })
    const worker = await tabs.addPane(id, { kind: 'worker', conversation: 'ares-bubble-sky' })
    const extra = await tabs.addPane(id, { kind: 'shell' })
    let panes = (await tabs.get(id)).panes
    assert.deepEqual(
      panes.map((pane) => pane.id),
      [leadPane.id, shell.id, worker.id, extra.id],
    )
    assert.deepEqual(
      panes.map((pane) => pane.order),
      [0, 1, 2, 3],
    )
    assert.equal(worker.conversation, 'ares-bubble-sky')

    await assert.rejects(() => tabs.addPane(id, { kind: 'boss' }), /kind/)

    assert.equal(await tabs.removePane(id, shell.id, shell.generation), true)
    assert.equal(await tabs.removePane(id, 'never-existed', 1), false)
    panes = (await tabs.get(id)).panes
    assert.deepEqual(
      panes.map((pane) => pane.id),
      [leadPane.id, worker.id, extra.id],
      'the survivors keep their order',
    )
    assert.deepEqual(
      panes.map((pane) => pane.order),
      [0, 2, 3],
    )

    const late = await tabs.addPane(id, { kind: 'shell' })
    const after = (await tabs.get(id)).panes
    assert.equal(after[after.length - 1].id, late.id)
    assert.equal(after[after.length - 1].order, 4, 'a new pane appends after the survivors')

    await assert.rejects(
      () => tabs.removePane(id, leadPane.id, leadPane.generation),
      /lead/,
      'the lead pane is the tab itself — suspend the tab instead',
    )
  })
})

test('tabs: suspend closes a tab, resume mints generation + 1 and a new leadId', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const created = await tabs.create(dir, 'codex')

    const suspended = await tabs.suspend(created.id)
    assert.equal(suspended.closed, true)
    await tabs.suspend(created.id)

    const resumed = await tabs.resume(created.id)
    assert.equal(resumed.generation, 2)
    assert.equal(resumed.leadId, `tab:${created.id}:2`)
    assert.notEqual(resumed.leadId, created.leadId)
    assert.equal((await tabs.get(created.id)).closed, false)

    await assert.rejects(
      () => tabs.resume(created.id),
      /not suspended/,
      'resuming a live tab is a caller bug, not a silent no-op',
    )
    await assert.rejects(() => tabs.suspend('t-999'), /no tab/)
  })
})

test('tabs: a restart reads every tab closed', async () => {
  await withTabs(async ({ dir, store, tabs }) => {
    await tabs.create(path.join(dir, 'a'), 'pi')
    const two = await tabs.create(path.join(dir, 'b'), 'codex')
    await tabs.suspend(two.id)

    await store.close()
    const restarted = new Store(store.root)
    await restarted.open()
    const after = new Tabs(restarted)

    const listed = await after.list()
    assert.equal(listed.length, 2)
    for (const tab of listed) {
      assert.equal(tab.closed, true, `${tab.id} reads closed after the restart`)
    }
    await restarted.close()
  })
})

test('tabs: a pane id with a new generation is not the old pane', async () => {
  await withTabs(async ({ dir, tabs }) => {
    const { id } = await tabs.create(dir, 'pi')
    const before = (await tabs.get(id)).panes.find((pane) => pane.kind === 'lead')

    await tabs.suspend(id)
    await tabs.resume(id)

    const after = (await tabs.get(id)).panes.find((pane) => pane.kind === 'lead')
    assert.equal(after.id, before.id, 'the pane id is reused for the reopened lead')
    assert.equal(after.generation, before.generation + 1, 'the generation is new')
    assert.equal(paneIdentity(before) === paneIdentity(after), false)
    assert.equal(samePane(before, after), false, 'same id, new generation: not the old pane')
    assert.ok(samePane(before, { ...before }), 'a pane is still itself')
  })
})

test('tabs: the app lead identity is first in LEAD_KEYS and wins over the harness session', () => {
  assert.equal(leadId({ CONSENSFLOW_LEAD_ID: 'tab:1:2' }), 'tab:1:2')
  assert.equal(
    leadId({ CONSENSFLOW_LEAD_ID: 'tab:1:2', CLAUDE_CODE_SESSION_ID: 'sess-9' }),
    'tab:1:2',
    'the app minted this lead; the harness session inside its pane does not replace it',
  )
  assert.equal(
    leadId({ CLAUDE_CODE_SESSION_ID: 'sess-9' }),
    'sess-9',
    "outside the app, today's rule is unchanged",
  )
  assert.equal(
    leadId({ CONSENSFLOW_LEAD_ID: '', CLAUDE_CODE_SESSION_ID: 'sess-9' }),
    'sess-9',
    'a blank app identity is nobody',
  )
})

test('tabs: an explicit pane id is never issued twice at the same generation', async () => {
  await withTabs(async ({ dir, store, tabs }) => {
    const { id } = await tabs.create(dir, 'pi')
    await tabs.addPane(id, { id: 'custom-1', kind: 'shell', generation: 1 })
    assert.equal(await tabs.removePane(id, 'custom-1', 1), true)

    // Removing a pane does not give its identity back. A pane is
    // (id, generation): reissuing the whole pair would make every record
    // that still names the old pane — a delayed removal, a binding — name
    // the replacement instead.
    await assert.rejects(
      () => tabs.addPane(id, { id: 'custom-1', kind: 'shell', generation: 1 }),
      /generation/,
    )
    const replacement = await tabs.addPane(id, { id: 'custom-1', kind: 'shell', generation: 2 })
    assert.equal(replacement.generation, 2)

    // The removal that was in flight when the identity was reissued names
    // the old generation, and takes nothing.
    await assert.rejects(() => tabs.removePane(id, 'custom-1', 1), /generation/)
    assert.ok(
      (await tabs.get(id)).panes.some((pane) => pane.id === 'custom-1' && pane.generation === 2),
      'the replacement survived',
    )

    // What was issued is durable: a restart does not forget it.
    await store.close()
    const restarted = new Store(store.root)
    await restarted.open()
    const after = new Tabs(restarted)
    assert.equal(await after.removePane(id, 'custom-1', 2), true)
    await assert.rejects(
      () => after.addPane(id, { id: 'custom-1', kind: 'shell', generation: 2 }),
      /generation/,
    )
    await after.addPane(id, { id: 'custom-1', kind: 'shell', generation: 3 })
    await restarted.close()
  })
})

test('tabs: a __proto__ pane id is refused, and never reaches the registry', async () => {
  await withTabs(async ({ dir, store, tabs }) => {
    const { id } = await tabs.create(dir, 'pi')

    // `__proto__` is not an identity: on an ordinary object it reads the
    // prototype and writes through a setter, so a registry that took it
    // would remember nothing and hand the same pane out for ever.
    await assert.rejects(
      () => tabs.addPane(id, { id: '__proto__', kind: 'shell', generation: 1 }),
      /__proto__|pane id/,
    )
    assert.equal(
      (await tabs.get(id)).panes.some((pane) => pane.id === '__proto__'),
      false,
      'nothing was added',
    )
    // The refusal is not a prototype write dressed up as one.
    assert.equal(Object.hasOwn(Object.prototype, 'consensflow'), false)
    assert.equal({}.generation, undefined, 'no generation leaked onto every object')

    // An ordinary id still keeps its history, across a restart — the
    // registry is a map of identities, not a prototype chain.
    await tabs.addPane(id, { id: 'custom-1', kind: 'shell', generation: 1 })
    assert.equal(await tabs.removePane(id, 'custom-1', 1), true)
    await store.close()
    const restarted = new Store(store.root)
    await restarted.open()
    const after = new Tabs(restarted)
    await assert.rejects(
      () => after.addPane(id, { id: 'custom-1', kind: 'shell', generation: 1 }),
      /generation/,
    )
    await assert.rejects(
      () => after.addPane(id, { id: '__proto__', kind: 'shell', generation: 2 }),
      /__proto__|pane id/,
    )
    await restarted.close()
  })
})
