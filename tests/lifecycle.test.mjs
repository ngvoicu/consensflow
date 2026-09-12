import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { mkdtemp, rm } from 'node:fs/promises'
import { connect } from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { PassThrough } from 'node:stream'
import test from 'node:test'
import { digest, envelope } from '../hosts/lib/deliveries.js'
import { workspaceKey } from '../hosts/lib/state.js'
import { Bridge } from '../src/bridge.js'
import { Watcher } from '../src/delivery-watch.js'
import { Page } from '../src/page.js'
import { addAgent } from '../src/roster.js'
import { Store } from '../src/store.js'
import { leadIdentity, Tabs } from '../src/tabs.js'
import { startUiServer } from '../src/ui.js'
import { tempEnv, testRoleConfiguration } from './helpers.mjs'

const WAIT_MS = 4_000

async function waitFor(read) {
  const deadline = Date.now() + WAIT_MS
  while (Date.now() < deadline) {
    const value = await read()
    if (value !== null) return value
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
  throw new Error('timed out waiting for lifecycle state')
}

async function json(response, status) {
  const body = await response.text()
  assert.equal(response.status, status, body)
  return body.length === 0 ? null : JSON.parse(body)
}

async function paneServer() {
  const fixture = tempEnv()
  addAgent({ name: 'zeus', harness: 'codex', model: 'gpt-5-codex' }, fixture.env)
  const workspace = path.join(fixture.root, 'workspace')
  const shims = path.join(fixture.root, 'shims')
  mkdirSync(workspace, { recursive: true })
  mkdirSync(shims, { recursive: true })
  for (const command of ['claude', 'codex']) {
    const file = path.join(shims, command)
    writeFileSync(file, '#!/bin/sh\nexit 0\n')
    chmodSync(file, 0o755)
  }
  fixture.env.PATH = `${shims}:${fixture.env.PATH}`

  const server = await startUiServer(fixture.env, { prepareRole: testRoleConfiguration })
  const nodeToRust = new PassThrough()
  const rustToNode = new PassThrough()
  server.attachBridge(
    new Bridge({ input: rustToNode, output: nodeToRust, idPrefix: 'n-', peerIdPrefix: 'r-' }),
  )
  const rust = new Bridge({
    input: nodeToRust,
    output: rustToNode,
    idPrefix: 'r-',
    peerIdPrefix: 'n-',
  })
  const opens = []
  const live = new Map()
  let lists = 0
  rust.on('pane.open', (request) => {
    opens.push(request)
    live.set(`${request.id}:${request.generation}`, {
      id: request.id,
      generation: request.generation,
      alive: true,
      idleMs: 0,
    })
    return { ok: true, id: request.id, generation: request.generation }
  })
  rust.on('pane.list', (body) => {
    assert.deepEqual(body, {}, 'Rust pane.list accepts an empty object body')
    lists += 1
    return { ok: true, panes: [...live.values()] }
  })
  const api = (token, pathname = '/api/panes', { method = 'GET', body } = {}) =>
    fetch(`${server.url}${pathname}`, {
      method,
      headers: {
        ...(token === null ? {} : { authorization: `Bearer ${token}` }),
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })

  return {
    workspace,
    rust,
    opens,
    live,
    listCount: () => lists,
    api,
    tabsFile: path.join(fixture.env.CONSENSFLOW_HOME, 'app', 'tabs.json'),
    threadsFile: path.join(
      fixture.env.CONSENSFLOW_HOME,
      'workspaces',
      workspaceKey(workspace),
      'threads.json',
    ),
    async close() {
      rust.close()
      await server.close()
      fixture.cleanup()
    },
  }
}

test('lead resume preserves native context and tab routing while worker capabilities rotate', async () => {
  const app = await paneServer()
  try {
    const opened = await app.rust.request('tab.open', {
      dir: app.workspace,
      harness: 'claude-code',
    })
    assert.equal(opened.ok, true)
    assert.equal(opened.outcome, 'opened')
    const frame = app.opens[0]
    const token = frame.env.CONSENSFLOW_APP_TOKEN

    const live = await json(await app.api(token), 200)
    assert.equal(live.tab, opened.tab)
    assert.equal(live.closed, false)

    app.rust.event('pane.exit', { id: frame.id, generation: frame.generation })
    const suspended = await waitFor(async () => {
      const response = await app.api(token)
      if (response.status !== 200) return null
      const body = await response.json()
      return body.closed === true ? body : null
    })
    assert.equal(suspended.tab, opened.tab, 'a closed tab still exists and remains its lead route')

    const resumed = await app.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(resumed.ok, true)
    assert.equal(resumed.pane.generation, frame.generation + 1)
    assert.equal(resumed.resumedSession, opened.nativeSession)
    assert.equal(resumed.cold, false)
    const resumeFrame = app.opens.at(-1)
    const resumeAt = resumeFrame.argv.indexOf('--resume')
    assert.equal(resumeFrame.argv[resumeAt + 1], opened.nativeSession)
    const resumedToken = resumeFrame.env.CONSENSFLOW_APP_TOKEN
    assert.notEqual(resumedToken, token)
    assert.equal(
      (await json(await app.api(token), 200)).closed,
      false,
      'a lead credential is tab-scoped and keeps routing across its tab resume',
    )
    assert.equal((await json(await app.api(resumedToken), 200)).closed, false)

    const worker = await json(
      await app.api(token, '/api/panes/consult', {
        method: 'POST',
        body: {
          tab: opened.tab,
          agent: 'zeus',
          task: 'keep this conversation',
          fresh: true,
          opId: 'lifecycle-worker-open',
        },
      }),
      200,
    )
    const workerFrame = app.opens.at(-1)
    const firstController = await json(
      await app.api(null, '/api/launch/redeem', {
        method: 'POST',
        body: { ticket: workerFrame.env.CONSENSFLOW_LAUNCH },
      }),
      200,
    )
    const nativeWorkerSession = 'worker-native-lifecycle-session'
    const workerRow = JSON.parse(readFileSync(app.threadsFile, 'utf8'))[worker.conversation]
    const bound = await json(
      await app.api(firstController.capability, '/api/panes/session.bind', {
        method: 'POST',
        body: {
          launch: firstController.launch,
          generation: worker.pane.generation,
          candidate: {
            sessionId: nativeWorkerSession,
            turn: `[consensflow launch ${workerRow.reserved.nonce}]\nkeep this conversation`,
          },
        },
      }),
      200,
    )
    assert.equal(bound.outcome, 'bound')
    const firstProgress = await json(
      await app.api(firstController.capability, '/api/panes/progress.set', {
        method: 'POST',
        body: {
          launch: firstController.launch,
          generation: worker.pane.generation,
          progress: { state: 'running', detail: 'first generation' },
        },
      }),
      200,
    )
    assert.equal(firstProgress.outcome, 'recorded')

    app.rust.event('pane.exit', { id: worker.pane.id, generation: worker.pane.generation })
    await waitFor(async () => {
      const listed = await json(await app.api(token), 200)
      return listed.panes.some((pane) => pane.id === worker.pane.id && pane.closed !== true)
        ? null
        : listed
    })
    await json(
      await app.api(firstController.capability, '/api/panes/progress.set', {
        method: 'POST',
        body: {
          launch: firstController.launch,
          generation: worker.pane.generation,
          progress: { state: 'stale' },
        },
      }),
      401,
    )

    const reopened = await json(
      await app.api(token, '/api/panes/attach', {
        method: 'POST',
        body: { tab: opened.tab, session: worker.conversation, opId: 'lifecycle-worker-reopen' },
      }),
      200,
    )
    const reopenedFrame = app.opens.at(-1)
    assert.equal(
      reopenedFrame.argv[reopenedFrame.argv.indexOf('--native-session') + 1],
      nativeWorkerSession,
    )
    const replacementController = await json(
      await app.api(null, '/api/launch/redeem', {
        method: 'POST',
        body: { ticket: reopenedFrame.env.CONSENSFLOW_LAUNCH },
      }),
      200,
    )
    assert.notEqual(replacementController.capability, firstController.capability)
    assert.equal(
      (
        await json(
          await app.api(replacementController.capability, '/api/panes/progress.set', {
            method: 'POST',
            body: {
              launch: replacementController.launch,
              generation: reopened.pane.generation,
              progress: { state: 'running', detail: 'replacement generation' },
            },
          }),
          200,
        )
      ).outcome,
      'recorded',
    )

    app.rust.event('pane.exit', { id: frame.id, generation: frame.generation })
    await new Promise((resolve) => setTimeout(resolve, 50))
    const replacement = await json(await app.api(resumedToken), 200)
    assert.equal(replacement.closed, false, 'a stale exit cannot suspend the replacement lead')
    assert.equal(
      replacement.panes.find((pane) => pane.kind === 'lead').generation,
      frame.generation + 1,
    )
    const envelope = JSON.parse(readFileSync(app.tabsFile, 'utf8'))
    envelope.tabs = envelope.tabs.filter((tab) => tab.id !== opened.tab)
    writeFileSync(app.tabsFile, `${JSON.stringify(envelope, null, 2)}\n`)
    await json(await app.api(token), 404)
    await json(await app.api(resumedToken), 404)
  } finally {
    await app.close()
  }
})

for (const role of ['lead', 'pm'])
  test(`${role} resume replaces missing owned launches once and reuses their native history`, async () => {
    const app = await paneServer()
    try {
      const parent = await app.rust.request('tab.open', {
        dir: app.workspace,
        harness: 'claude-code',
      })
      const opened =
        role === 'pm'
          ? await app.rust.request('pm.open', { tab: parent.tab, harness: 'claude-code' })
          : parent
      const leadFrame = app.opens.at(-1)
      const token = leadFrame.env.CONSENSFLOW_APP_TOKEN
      const worker = await json(
        await app.api(token, '/api/panes/consult', {
          method: 'POST',
          body: {
            tab: opened.tab,
            agent: 'zeus',
            task: 'survive restart by native session',
            fresh: true,
            opId: 'lifecycle-restart-worker',
          },
        }),
        200,
      )
      const workerFrame = app.opens.at(-1)
      const oldController = await json(
        await app.api(null, '/api/launch/redeem', {
          method: 'POST',
          body: { ticket: workerFrame.env.CONSENSFLOW_LAUNCH },
        }),
        200,
      )
      const nativeSession = 'worker-native-after-restart'
      const workerRow = JSON.parse(readFileSync(app.threadsFile, 'utf8'))[worker.conversation]
      assert.equal(
        (
          await json(
            await app.api(oldController.capability, '/api/panes/session.bind', {
              method: 'POST',
              body: {
                launch: oldController.launch,
                generation: worker.pane.generation,
                candidate: {
                  sessionId: nativeSession,
                  turn: `[consensflow launch ${workerRow.reserved.nonce}]\nsurvive restart by native session`,
                },
              },
            }),
            200,
          )
        ).outcome,
        'bound',
      )

      app.rust.event('pane.exit', {
        id: leadFrame.id,
        generation: leadFrame.generation,
      })
      await waitFor(async () => {
        const response = await app.api(token)
        return response.status === 200 && (await response.json()).closed === true ? true : null
      })
      app.live.clear()

      const listsBeforeResume = app.listCount()
      const opensBeforeResume = app.opens.length
      const resumed = await app.rust.request('tab.resume', { tab: opened.tab })
      assert.equal(resumed.ok, true, JSON.stringify(resumed))
      assert.ok(
        app.listCount() > listsBeforeResume,
        'resume asks the replacement pane host what still exists',
      )
      await json(
        await app.api(oldController.capability, '/api/panes/progress.set', {
          method: 'POST',
          body: {
            launch: oldController.launch,
            generation: worker.pane.generation,
            progress: { state: 'stale' },
          },
        }),
        401,
      )
      assert.equal(
        (await json(await app.api(token), 200)).panes.some(
          (pane) => pane.id === worker.pane.id && pane.closed !== true,
        ),
        false,
      )

      assert.equal(resumed.workers.length, 1)
      assert.equal(resumed.workers[0].outcome, 'opened')
      assert.equal(app.opens.length, opensBeforeResume + 2, 'resume opens one lead and one worker')
      const beforeAttach = app.opens.length
      const replacement = await json(
        await app.api(token, '/api/panes/attach', {
          method: 'POST',
          body: {
            tab: opened.tab,
            session: worker.conversation,
            opId: 'lifecycle-restart-reattach',
          },
        }),
        200,
      )
      assert.equal(replacement.outcome, 'live')
      assert.equal(
        app.opens.length,
        beforeAttach,
        'reattach must not duplicate the worker resumed by the app',
      )
      const replacementFrame = app.opens.at(-1)
      assert.equal(
        replacementFrame.argv[replacementFrame.argv.indexOf('--native-session') + 1],
        nativeSession,
      )
      const newController = await json(
        await app.api(null, '/api/launch/redeem', {
          method: 'POST',
          body: { ticket: replacementFrame.env.CONSENSFLOW_LAUNCH },
        }),
        200,
      )
      assert.notEqual(newController.capability, oldController.capability)
      assert.equal(
        (
          await json(
            await app.api(newController.capability, '/api/panes/progress.set', {
              method: 'POST',
              body: {
                launch: newController.launch,
                generation: replacement.pane.generation,
                progress: { state: 'running', detail: 'reconciled after restart' },
              },
            }),
            200,
          )
        ).outcome,
        'recorded',
      )
    } finally {
      await app.close()
    }
  })

test('resume fails closed before generation changes when the pane host cannot list workers', async () => {
  const app = await paneServer()
  try {
    const opened = await app.rust.request('tab.open', {
      dir: app.workspace,
      harness: 'claude-code',
    })
    const leadFrame = app.opens.at(-1)
    assert.equal((await app.rust.request('shell.open', { tab: opened.tab })).ok, true)
    app.rust.event('pane.exit', {
      id: leadFrame.id,
      generation: leadFrame.generation,
    })
    await waitFor(async () => {
      const envelope = JSON.parse(readFileSync(app.tabsFile, 'utf8'))
      return envelope.tabs.find((tab) => tab.id === opened.tab)?.closed === true ? true : null
    })

    app.rust.on('pane.list', () => ({ ok: false, error: 'host-list-unavailable' }))
    const refused = await app.rust.request('tab.resume', { tab: opened.tab })
    assert.equal(refused.ok, false, JSON.stringify(refused))
    const unchanged = JSON.parse(readFileSync(app.tabsFile, 'utf8')).tabs.find(
      (tab) => tab.id === opened.tab,
    )
    assert.equal(unchanged.closed, true)
    assert.equal(unchanged.lead.generation, leadFrame.generation)
  } finally {
    await app.close()
  }
})

test('lead pane.open carries each interactive harness billing guard', async () => {
  const app = await paneServer()
  try {
    for (const [harness, dropEnv] of [
      ['claude-code', ['ANTHROPIC_API_KEY']],
      ['codex', ['OPENAI_API_KEY']],
    ]) {
      const before = app.opens.length
      const opened = await app.rust.request('tab.open', { dir: app.workspace, harness })
      assert.equal(opened.ok, true, JSON.stringify(opened))
      assert.deepEqual(app.opens[before].dropEnv, dropEnv, harness)
    }
  } finally {
    await app.close()
  }
})

test('restart closes tabs; resume preserves the bound lead and keeps old deliveries held and visible', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-lifecycle-'))
  const workspace = path.join(root, 'workspace')
  let store = new Store(path.join(root, 'consensflow'))
  let watcher = null
  try {
    await store.open()
    let tabs = new Tabs(store)
    const created = await tabs.create(workspace, 'claude-code')
    const nativeSession = '13d35bed-720e-4704-8d52-9bf2b67ba231'
    await store.mutate(workspace, 'test.bindLead', async (io) => {
      const records = await io.readTabs()
      records.find((tab) => tab.id === created.id).lead.nativeSession = nativeSession
      await io.writeTabs(records)
    })
    const before = await tabs.get(created.id)
    const leadPane = before.panes.find((pane) => pane.kind === 'lead')
    const deliveryId = await store.allocateDeliveryId()
    const historical = {
      id: deliveryId,
      state: 'pending',
      conversation: 'worker-session',
      answerId: 'answer-1',
      answer: 'the held answer',
      agent: 'zeus',
      createdAt: '2026-09-07T00:00:00.000Z',
      target: {
        leadId: leadIdentity(before),
        session: nativeSession,
        tab: before.id,
        pane: leadPane.id,
        generation: before.lead.generation,
      },
    }
    historical.digest = digest(envelope(historical))
    await seedLegacy(store, workspace, historical)

    await store.close()
    store = new Store(store.root)
    await store.open()
    tabs = new Tabs(store)
    const recovered = await tabs.get(created.id)
    assert.equal(recovered.closed, true)
    assert.equal(recovered.lead.nativeSession, nativeSession)

    const resumed = await tabs.resume(created.id)
    assert.equal(resumed.generation, created.generation + 1)
    assert.equal((await tabs.get(created.id)).lead.nativeSession, nativeSession)

    watcher = new Watcher({ store, tabs, env: {}, floorMs: 60_000 })
    await watcher.start()
    const page = new Page({
      store,
      tabs,
      agents: { names: () => [], row: () => null },
      env: {},
    })
    const firstView = await page.state()
    const secondView = await page.state()
    assert.equal(firstView.results.length, 1)
    assert.equal(firstView.results[0].tab, created.id)
    assert.equal(firstView.results[0].answerId, 'answer-1')
    assert.equal(firstView.results[0].state, 'waiting')
    assert.deepEqual(
      secondView.tabs,
      firstView.tabs,
      'page reads do not mutate pane lifecycle state',
    )
    assert.equal(watcher.started, true, 'state reads do not stop the app-lifetime watcher')

    const beforeTransfer = await store.readDeliveries(workspace)
    assert.equal(beforeTransfer[deliveryId].target.generation, created.generation)
    await watcher.reconcile()
    assert.deepEqual(await store.readDeliveries(workspace), beforeTransfer)
    assert.equal((await page.state()).results.length, 1, 'resume never clones results')
  } finally {
    await watcher?.close()
    await store.close()
    await rm(root, { recursive: true, force: true })
  }
})

test('the durable drain finishes while the HTTP server is still held open', async () => {
  // The shape this guards is the whole reason a killed pane host used to leave
  // a delivery `submitting` on disk. When the parent goes away the editor has
  // to finish what it started BEFORE the process ends, and the one thing it
  // must not queue behind is `server.close()`: a connection with a request in
  // flight holds that open for as long as the peer feels like, and the page is
  // exactly such a peer. So `drain` is its own path — the watcher's queue,
  // then the store's — and this asserts it completes while `close` cannot.
  const t = tempEnv()
  const server = await startUiServer(t.env, { prepareRole: testRoleConfiguration })

  // Half a request: connected, headers unfinished. Node keeps this connection
  // active, which is what makes `server.close()` wait. (An IDLE keep-alive
  // socket would not — Node closes those itself — so it has to be this.)
  const socket = connect(Number(new URL(server.url).port), '127.0.0.1')
  await new Promise((resolve) => socket.once('connect', resolve))
  socket.write('GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n')
  await new Promise((resolve) => setTimeout(resolve, 50))

  const closing = server.close()
  const stalled = Symbol('stalled')
  assert.equal(
    await Promise.race([
      closing.then(() => 'closed'),
      new Promise((resolve) => setTimeout(() => resolve(stalled), 300)),
    ]),
    stalled,
    'the premise is gone: server.close() no longer waits on an active connection',
  )

  await server.drain()

  // The claim is the proof. The store releases its kernel lock only after
  // every admitted mutation has landed, so a second Store taking the same root
  // says the drain really finished rather than merely returned.
  const successor = new Store(t.env.CONSENSFLOW_HOME)
  await successor.open()
  await successor.close()

  socket.destroy()
  await closing
  t.cleanup()
})

async function seedLegacy(store, cwd, record) {
  return store.mutate(cwd, 'test.legacy', async (io) => {
    const records = await io.readDeliveries()
    const merged = { ...records[record.id], ...record }
    records[record.id] = merged
    await io.writeDeliveries(records)
    return merged
  })
}
