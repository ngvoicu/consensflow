import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { indexResult } from '../hosts/lib/inbox.js'
import { Page } from '../src/page.js'
import { Store } from '../src/store.js'
import { Tabs } from '../src/tabs.js'
import { tempEnv } from './helpers.mjs'

async function fixture(t) {
  const f = tempEnv(),
    store = new Store(f.env.CONSENSFLOW_HOME)
  await store.open()
  const tabs = new Tabs(store),
    cwd = path.join(f.root, 'project')
  const lead = await tabs.create(cwd, 'codex')
  const pm = await tabs.create(cwd, 'pi', { parentTabId: lead.id })
  const body = '<script>never execute</script>\nλ😀'.repeat(4000)
  await store.mutate(cwd, 'test.inbox', async (io) => {
    const state = await io.readInbox()
    for (let n = 1; n <= 41; n++)
      indexResult(state, {
        id: `d-${n}`,
        owner: n === 41 ? pm.id : lead.id,
        conversation: n === 41 ? 'advisor' : 'worker',
        agent: 'zeus',
        kind: 'codex',
        session: 'source',
        answerId: `answer-${n}`,
        answer: n === 1 ? body : `reply ${n}`,
        now: n,
      })
    state.results['d-2'].legacyReceived = true
    state.results['d-3'].legacyUncertain = true
    await io.writeInbox(state)
  })
  t.after(async () => {
    await store.close()
    f.cleanup()
  })
  return {
    store,
    tabs,
    cwd,
    lead,
    pm,
    body,
    page: new Page({ store, tabs, env: f.env, agents: { names: () => [], row: () => null } }),
  }
}
test('page projects every distinct result independently of lead generation, including unconfirmed replies', async (t) => {
  const s = await fixture(t)
  const snapshot = await s.page.state()
  assert.equal(snapshot.results.length, 41)
  assert.equal(
    snapshot.results.filter((r) => r.tab === s.lead.id && r.state !== 'received').length,
    39,
  )
  assert.equal(snapshot.results.find((r) => r.id === 'd-3').state, 'uncertain')
  assert.equal(snapshot.results.filter((r) => r.tab === s.pm.id).length, 1)
  assert.equal(snapshot.deliveries, undefined)
  assert.equal(snapshot.held, undefined)
})
test('human result viewing returns the complete paged body and never grants native receipt or changes claims', async (t) => {
  const s = await fixture(t)
  const before = JSON.stringify(await s.store.readInbox(s.cwd))
  let offset = 0,
    text = ''
  do {
    const chunk = await s.page.resultBody({ tab: s.lead.id, result: 'd-1', offset })
    text += chunk.text
    offset = chunk.next
  } while (offset !== null)
  assert.equal(text, s.body)
  assert.equal(JSON.stringify(await s.store.readInbox(s.cwd)), before)
  await assert.rejects(s.page.resultBody({ tab: s.pm.id, result: 'd-1' }), /belong/)
  await assert.rejects(s.page.resultBody({ tab: s.lead.id, result: 'd-1', offset: -1 }), /offset/)
})
test('manual collect requests one waiting result; uncertainty and receipt never trigger a resend', async (t) => {
  const s = await fixture(t)
  await s.page.collectResult({ tab: s.lead.id, result: 'd-1' })
  const saved = await s.store.readInbox(s.cwd)
  assert.equal(saved.results['d-1'].requested, true)
  assert.equal(saved.results['d-1'].claims.length, 0)
  assert.equal(Object.keys(saved.results).length, 41)
  for (const result of ['d-2', 'd-3'])
    await assert.rejects(s.page.collectResult({ tab: s.lead.id, result }), /waiting/)
  await assert.rejects(s.page.collectResult({ tab: s.pm.id, result: 'd-1' }), /belong/)
})

test('complete result viewing keeps heavily escaped control text below the bridge frame limit', async (t) => {
  const s = await fixture(t)
  const answer = '\u0000\u0001\u0002'.repeat(12000)
  await s.store.mutate(s.cwd, 'test.controls', async (io) => {
    const state = await io.readInbox()
    indexResult(state, {
      id: 'd-42',
      owner: s.lead.id,
      conversation: 'worker',
      agent: 'zeus',
      kind: 'codex',
      session: 'source',
      answerId: 'controls',
      answer,
      now: 42,
    })
    await io.writeInbox(state)
  })
  let offset = 0,
    body = ''
  do {
    const chunk = await s.page.resultBody({ tab: s.lead.id, result: 'd-42', offset })
    assert(
      Buffer.byteLength(
        JSON.stringify({ v: 1, id: 'frame', kind: 'res', body: { ok: true, ...chunk } }),
      ) <
        64 * 1024,
    )
    body += chunk.text
    offset = chunk.next
  } while (offset !== null)
  assert.equal(body, answer)
})
