import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import test from 'node:test'
import { answers } from '../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../hosts/lib/runners.js'
import { launchConfiguration, send } from '../src/channels.js'
import * as devinInstall from '../src/devin-install.js'
import { endLaunch, receiverEnv } from '../src/launch.js'
import { HARNESSES, harnessForKind } from '../src/roster.js'
import { LEAD_HARNESSES } from '../src/tabs.js'
import { tempEnv } from './helpers.mjs'

async function stage(t, mutate = () => {}, name = 'native-tui') {
  const f = tempEnv()
  t.after(f.cleanup)
  f.env.XDG_DATA_HOME = path.join(f.root, 'data')
  const fixture = JSON.parse(
    await fs.readFile(new URL(`./engine/fixtures/completion/devin/${name}.json`, import.meta.url)),
  )
  mutate(fixture)
  const file = path.join(f.env.XDG_DATA_HOME, 'devin', 'cli', 'sessions.db')
  await fs.mkdir(path.dirname(file), { recursive: true })
  const db = new DatabaseSync(file)
  db.exec(
    'CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id INTEGER); CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, node_id INTEGER, parent_node_id INTEGER, chat_message TEXT, created_at TEXT)',
  )
  db.prepare('INSERT INTO sessions VALUES (?, ?)').run(
    fixture.session.id,
    fixture.session.main_chain_id,
  )
  const insert = db.prepare('INSERT INTO message_nodes VALUES (?, ?, ?, ?, ?, ?)')
  for (const r of fixture.rows)
    insert.run(r.row_id, r.session_id, r.node_id, r.parent_node_id, r.chat_message, r.created_at)
  db.close()
  const wire = path.join(
    f.env.CONSENSFLOW_HOME,
    'integrations',
    'devin',
    'native-launch',
    'wire.jsonl',
  )
  await fs.mkdir(path.dirname(wire), { recursive: true })
  await fs.writeFile(wire, fixture.wire.map(JSON.stringify).join('\n') + '\n')
  return { ...f, session: fixture.session.id, file, wire }
}

test('Devin completion reads only the native main chain and confirms complete turns by native UUID', async (t) => {
  const f = await stage(t)
  const result = await answers('devin', f.session, f.env)
  assert.equal(result.unknown, undefined, result.reason)
  const completed = result.items.filter((r) => r.role === 'assistant' && r.complete)
  assert.deepEqual(
    completed.map((r) => r.text),
    ['LOCAL_NATIVE_REPLY_3', 'LOCAL_NATIVE_REPLY_4'],
  )
  assert.equal(new Set(result.items.map((r) => r.id)).size, result.items.length)
  for (const marker of ['PRODUCT_RESULT_1', 'PRODUCT_RESULT_2', 'PRODUCT_RESULT_3', 'PRODUCT_LATE'])
    assert.equal(
      result.items.filter((r) => ['user', 'custom'].includes(r.role) && r.text.includes(marker))
        .length,
      1,
    )
  assert.equal(result.settlement.state, 'settled')
  assert.equal(result.inFlight, false)
})

test('Devin cancelled native turns never become completed worker answers', async (t) => {
  const f = await stage(t, (fixture) => {
    for (const event of fixture.wire) if (event.cause === 'complete') event.cause = 'cancelled'
  })
  const result = await answers('devin', f.session, f.env)
  assert.equal(result.unknown, undefined, result.reason)
  assert.equal(result.items.filter((r) => r.role === 'assistant' && r.complete).length, 0)
  assert.equal(result.cancelled, true)
})

test('Devin text without a native completion boundary remains incomplete, including a partial log tail', async (t) => {
  const f = await stage(t, (fixture) => {
    fixture.wire = fixture.wire.filter((r) => !r.cause)
  })
  await fs.appendFile(f.wire, '{"cause":')
  const result = await answers('devin', f.session, f.env)
  assert.equal(result.unknown, undefined, result.reason)
  assert.equal(result.items.filter((r) => r.role === 'assistant' && r.complete).length, 0)
  await fs.appendFile(f.wire, 'invalid}\n')
  assert.equal((await answers('devin', f.session, f.env)).unknown, true)
})

test('Devin completion refuses a missing chain ancestor instead of returning a partial report', async (t) => {
  const f = await stage(t, (fixture) => {
    fixture.rows = fixture.rows.filter((r) => r.node_id !== fixture.session.main_chain_id)
  })
  const result = await answers('devin', f.session, f.env)
  assert.equal(result.unknown, true)
  assert.match(result.reason, /chain|ancestor|head/)
})

test('Devin is a selectable native TUI harness for workers and both coordinators', () => {
  assert.ok(HARNESSES.includes('devin'))
  assert.ok(LEAD_HARNESSES.includes('devin'))
  assert.equal(harnessForKind('devin'), 'devin')
  const start = interactiveStart({ kind: 'devin', model: 'default' }, null)
  assert.equal(start.command, 'devin')
  assert.deepEqual(start.args, [])
  const resume = interactiveResume({ kind: 'devin' }, 'exact-native-session')
  assert.deepEqual(resume.args, ['--resume', 'exact-native-session'])
  assert.equal(resume.args.includes('--acp'), false)
  const env = receiverEnv({
    tab: 't',
    pane: 'p',
    launch: 'devin-test',
    generation: 1,
    kind: 'devin',
    app: { url: 'http://127.0.0.1:9' },
  })
  assert.equal(JSON.parse(env.CF_RESULT_RECEIVER).kind, 'devin')
  endLaunch('devin-test')
})

test('Devin launch installs private hooks and writes the task to a private prompt file', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  const executable = path.join(f.root, 'devin')
  await fs.writeFile(executable, '#!/bin/sh\necho "Devin CLI 3000.10.21"\n', { mode: 0o700 })
  const configuration = await launchConfiguration('devin', {
    launchId: 'launch-one',
    workspace: f.root,
    executable,
    node: process.execPath,
    env: f.env,
  })
  assert.equal(configuration.channel.kind, 'devin-tui')
  const task = 'Exact task\n' + '漢'.repeat(50_000)
  const invocation = await devinInstall.prepareDevinPrompt(
    interactiveStart({ kind: 'devin' }, null, task),
    configuration,
  )
  assert.equal(invocation.args[0], '--prompt-file')
  assert.ok(invocation.args[1].startsWith(f.env.CONSENSFLOW_HOME + path.sep))
  assert.equal(await fs.readFile(invocation.args[1], 'utf8'), task)
  assert.equal(invocation.prompt, undefined)
  await fs.writeFile(executable, '#!/bin/sh\necho "Devin CLI 3000.6.14"\n')
  await assert.rejects(
    launchConfiguration('devin', {
      launchId: 'launch-old',
      workspace: f.root,
      executable,
      node: process.execPath,
      env: f.env,
    }),
    /3000.10.21/,
  )
})

test('Devin explicit follow-ups check the selected native conversation and retain the input epoch guard', async (t) => {
  const f = await stage(t)
  const calls = []
  const target = {
    session: 'wrong-native-session',
    pane: 'p-1',
    generation: 1,
    epoch: 42,
    channel: { wire: f.wire },
    bridge: {
      request: async (...args) => {
        calls.push(args)
        return { ok: true }
      },
    },
  }
  assert.equal((await send('devin-tui', target, 'follow up')).admitted, false)
  assert.equal(calls.length, 0)
  target.session = f.session
  assert.equal((await send('devin-tui', target, 'follow up')).ok, true)
  assert.deepEqual(calls, [
    ['pane.write_paste', { id: 'p-1', generation: 1, epoch: 42, body: 'follow up' }],
  ])
})

test('a native complete boundary with the wrong request identity cannot credit matching text', async (t) => {
  const f = await stage(t, (fixture) => {
    for (const event of fixture.wire)
      if (event.turnClientMessageId) event.turnClientMessageId = 'unrelated-request'
  })
  const result = await answers('devin', f.session, f.env)
  assert.equal(
    result.unknown === true || result.items.every((r) => r.role !== 'assistant' || !r.complete),
    true,
  )
})

test('Devin discovery requires both its launch wire selection and the exact opening launch marker', async (t) => {
  const f = await stage(t)
  const { discoverSessionWithEvidence } = await import('../hosts/lib/harness-transcript.js')
  const db = new DatabaseSync(f.file)
  const rows = db.prepare('SELECT row_id, chat_message FROM message_nodes ORDER BY row_id').all()
  for (const row of rows) {
    const message = JSON.parse(row.chat_message)
    if (message.role !== 'user') continue
    message.content = '[consensflow launch native-launch]\nReview this task'
    db.prepare('UPDATE message_nodes SET chat_message = ? WHERE row_id = ?').run(
      JSON.stringify(message),
      row.row_id,
    )
  }
  db.close()
  const discover = (nonce) => discoverSessionWithEvidence('devin', f.root, 0, f.env, { nonce })
  assert.equal((await discover('native-launch'))?.sessionId, f.session)
  assert.equal(await discover('wrong-launch'), null)
  await fs.appendFile(
    f.wire,
    JSON.stringify({
      sessionId: 'other-session',
      update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] },
    }) + '\n',
  )
  assert.equal(await discover('native-launch'), null)
})

test('native Devin first, second and resumed third replies are indexed once; cancelled and independent replies stay out', async (t) => {
  const f = await stage(t, () => {}, 'worker-tui')
  const { Store } = await import('../src/store.js')
  const { Tabs } = await import('../src/tabs.js')
  const { Watcher } = await import('../src/delivery-watch.js')
  const store = new Store(f.env.CONSENSFLOW_HOME)
  await store.open()
  const tabs = new Tabs(store),
    owner = await tabs.create(f.root, 'devin')
  await store.mutate(f.root, 'test.worker', async (io) =>
    io.writeThreads({
      worker: {
        agent: 'devin',
        kind: 'devin',
        lead: owner.leadId,
        sessionId: f.session,
        binding: { launchId: 'native-launch' },
      },
    }),
  )
  const errors = [],
    watcher = new Watcher({
      store,
      tabs,
      env: f.env,
      onError: (error) => errors.push(error.message),
    })
  try {
    await watcher.reconcile()
    await watcher.reconcile()
    const state = await store.readInbox(f.root)
    assert.deepEqual(
      Object.values(state.results).map((r) => r.answer),
      ['LOCAL_NATIVE_REPLY_1', 'LOCAL_NATIVE_REPLY_2', 'LOCAL_NATIVE_REPLY_4'],
    )
    assert.deepEqual(errors, [])
    const native = await answers('devin', f.session, f.env)
    assert.equal(native.cancelled, true)
    assert.equal(native.inFlight, false)
  } finally {
    await watcher.close()
    await store.close()
  }
})
