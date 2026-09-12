import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { emptyInbox, indexResult } from '../hosts/lib/inbox.js'
import { workspaceKey } from '../hosts/lib/state.js'
import { leadEnv, receiverEnv } from '../src/launch.js'
import { startUiServer } from '../src/ui.js'
import { tempEnv } from './helpers.mjs'

async function setup(t) {
  const fixture = tempEnv()
  const server = await startUiServer(fixture.env)
  t.after(async () => {
    await server.close()
    fixture.cleanup()
  })
  const cwd = path.join(fixture.root, 'project')
  const tab = {
    id: 't-1',
    directory: cwd,
    role: 'lead',
    closed: false,
    lead: {
      harness: 'codex',
      generation: 1,
      nativeSession: null,
      reserved: { launchId: 'launch-one', pane: 'p-1', generation: 1 },
    },
    panes: [{ id: 'p-1', kind: 'lead', generation: 1 }],
  }
  const tabsFile = path.join(fixture.env.CONSENSFLOW_HOME, 'app', 'tabs.json')
  await fs.writeFile(
    tabsFile,
    JSON.stringify({ nextTab: 2, nextPane: 2, nextDelivery: 2, issued: { 'p-1': 1 }, tabs: [tab] }),
  )
  const inbox = emptyInbox()
  indexResult(inbox, {
    id: 'd-1',
    owner: 't-1',
    conversation: 'worker',
    agent: 'zeus',
    kind: 'codex',
    session: 'worker-native',
    answerId: 'answer-one',
    answer: 'Exact complete answer.',
    now: 1,
  })
  const inboxFile = path.join(
    fixture.env.CONSENSFLOW_HOME,
    'workspaces',
    workspaceKey(cwd),
    'inbox.json',
  )
  await fs.mkdir(path.dirname(inboxFile), { recursive: true })
  await fs.writeFile(inboxFile, JSON.stringify(inbox))
  const config = JSON.parse(
    receiverEnv({
      app: server,
      tab: tab.id,
      pane: 'p-1',
      launch: 'launch-one',
      generation: 1,
      kind: 'codex',
    }).CF_RESULT_RECEIVER,
  )
  const post = (op, body = {}, token = config.token) =>
    fetch(`${server.url}/api/receiver/${op}`, {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify(body),
    })
  return { fixture, server, tab, tabsFile, inboxFile, post }
}

test('receiver API authenticates owner/launch and fences a claimed part across native new', async (t) => {
  const s = await setup(t)
  const ordinary = leadEnv({
    app: s.server,
    tab: 't-1',
    pane: 'p-1',
    leadId: 'tab:t-1:1',
    node: '/bundle/node',
  })
  assert.equal(
    (await s.post('register', { session: 'native-a' }, ordinary.CONSENSFLOW_APP_TOKEN)).status,
    403,
  )
  assert.equal((await s.post('register', { owner: 'other', session: 'native-a' })).status, 400)
  const response = await s.post('register', { session: 'native-a', previous: null })
  assert.equal(response.status, 200)
  const first = await response.json()
  const selected = await (await s.post('claim', { lease: first.lease })).json()
  assert.equal(selected.result, 'd-1')
  assert.match(selected.text, /Exact complete answer/)
  const next = await (
    await s.post('register', { session: 'native-b', previous: first.lease })
  ).json()
  assert.notEqual(next.lease, first.lease)
  assert.equal(
    (await s.post('begin', { lease: first.lease, result: selected.result, claim: selected.id }))
      .status,
    409,
  )
  assert.equal(
    (await s.post('register', { session: 'native-c', previous: first.lease })).status,
    409,
  )
  const second = await (await s.post('claim', { lease: next.lease })).json()
  assert.equal(second.result, 'd-1')
  assert.equal(
    (await s.post('begin', { lease: next.lease, result: second.result, claim: second.id })).status,
    200,
  )
  // A caller's success assertion is not a native receipt; no native transcript exists.
  await s.post('receipt', {
    lease: next.lease,
    result: second.result,
    claim: second.id,
    received: true,
  })
  const saved = JSON.parse(await fs.readFile(s.inboxFile, 'utf8'))
  assert.equal(saved.results['d-1'].claims.at(-1).state, 'submitting')
  const tabs = JSON.parse(await fs.readFile(s.tabsFile, 'utf8'))
  tabs.tabs[0].lead.reserved.launchId = 'another-launch'
  await fs.writeFile(s.tabsFile, JSON.stringify(tabs))
  assert.equal((await s.post('claim', { lease: next.lease })).status, 403)
})

test('manual policy retains inbox visibility and forbids automatic claims', async (t) => {
  const s = await setup(t)
  const tabs = JSON.parse(await fs.readFile(s.tabsFile, 'utf8'))
  tabs.tabs[0].policy = 'manual'
  await fs.writeFile(s.tabsFile, JSON.stringify(tabs))
  const registered = await (
    await s.post('register', { session: 'native-a', previous: null })
  ).json()
  assert.equal(await (await s.post('claim', { lease: registered.lease })).json(), null)
  const saved = JSON.parse(await fs.readFile(s.inboxFile, 'utf8'))
  assert.equal(saved.results['d-1'].claims.length, 0)
  assert.equal(saved.results['d-1'].answer, 'Exact complete answer.')
})

test('native home and shutdown retire claims and resume of the same session gets fresh authority', async (t) => {
  const s = await setup(t)
  const first = await (await s.post('register', { session: 'native-a' })).json()
  const selected = await (await s.post('claim', { lease: first.lease })).json()
  assert.equal((await s.post('retire', { lease: first.lease })).status, 200)
  assert.equal(
    (await s.post('begin', { lease: first.lease, result: selected.result, claim: selected.id }))
      .status,
    409,
  )
  assert.equal((await s.post('claim', { lease: first.lease })).status, 409)
  const resumed = await (
    await s.post('register', { session: 'native-a', previous: first.lease })
  ).json()
  assert.notEqual(resumed.lease, first.lease)
  const next = await (await s.post('claim', { lease: resumed.lease })).json()
  assert.equal(next.result, selected.result)
  await s.post('begin', { lease: resumed.lease, result: next.result, claim: next.id })
  await s.post('retire', { lease: resumed.lease })
  const saved = JSON.parse(await fs.readFile(s.inboxFile, 'utf8'))
  assert.equal(saved.results['d-1'].claims.at(-1).state, 'uncertain')
})

test('native wake contains no body and is deduplicated until the current prompt collects', async (t) => {
  const s = await setup(t)
  const current = await (await s.post('register', { session: 'native-a' })).json()
  assert.deepEqual(await (await s.post('wake', { lease: current.lease })).json(), { wake: true })
  assert.deepEqual(await (await s.post('wake', { lease: current.lease })).json(), { wake: false })
  assert.equal((await s.post('wake', { lease: 'retired' })).status, 409)
  await s.post('claim', { lease: current.lease })
  assert.deepEqual(await (await s.post('wake', { lease: current.lease })).json(), { wake: false })
})
