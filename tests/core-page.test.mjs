import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { pageOperations } from '../src/core/page.js'
import { openLedger } from '../src/ledger/index.js'

/**
 * What the board page may ask of the new core (TEST-BDC-13's data side): each
 * operation is the human acting on the ledger or the dispatcher, and every
 * change wakes the dispatcher.
 */
async function withPage(fn) {
  const home = await mkdtemp(path.join(os.tmpdir(), 'cf-core-page-'))
  const env = { HOME: home, CONSENSFLOW_HOME: path.join(home, 'consensflow') }
  await mkdir(env.CONSENSFLOW_HOME, { recursive: true })
  await writeFile(
    path.join(env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify({
      schemaVersion: 1,
      agents: [
        { id: 'zeus', kind: 'claude-code', model: 'claude-sonnet-5', effort: 'high' },
        { id: 'diana', kind: 'codex', model: 'gpt-5.6-luna' },
      ],
    })}\n`,
  )
  const ledger = openLedger(path.join(home, 'consensflow.db'))
  const opened = []
  const removed = []
  let kicks = 0
  const dispatcher = {
    async openSession(request) {
      opened.push(request)
      return ledger.createSession({
        directory: request.directory,
        name: request.name,
        lead: { harness: request.harness },
        team: request.team,
      })
    },
    async removeMember(session, handle) {
      removed.push([session, handle])
      return ledger.removeMember(session, handle)
    },
    async resumeSession(id) {
      return ledger.setSessionState(id, 'open')
    },
    activity: () => ({ state: 'idle' }),
    pane: () => null,
  }
  const operations = pageOperations({ ledger, dispatcher, env, kick: () => kicks++ })
  try {
    await fn({ ledger, operations, opened, removed, env, kicks: () => kicks })
  } finally {
    ledger.close()
    await rm(home, { recursive: true, force: true })
  }
}

describe('the page protocol of the new core', () => {
  it('is exactly what the app forwards: the Rust allow-list names every operation', async () => {
    const source = await readFile(
      new URL('../app/src-tauri/src/commands.rs', import.meta.url),
      'utf8',
    )
    const list = source.match(/const CORE_OPERATIONS: &\[&str\] = &\[([^\]]*)\]/)[1]
    const forwarded = [...list.matchAll(/"([^"]+)"/g)].map((match) => match[1])
    await withPage(async ({ operations }) => {
      assert.deepEqual(Object.keys(operations).sort(), forwarded.sort())
    })
  })

  it('opens a session named after its folder and lists it', async () => {
    await withPage(async ({ operations, opened, kicks }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      assert.deepEqual(opened, [{ directory: '/work/app', name: 'app', harness: 'pi', team: [] }])
      assert.equal(kicks(), 1)
      const { sessions } = await operations['sessions.list']({})
      assert.deepEqual(
        sessions.map((s) => [s.id, s.name]),
        [[session.id, 'app']],
      )
    })
  })

  it('lists the saved agents for the team picker and adds one with its own harness', async () => {
    await withPage(async ({ operations }) => {
      const { agents } = await operations['agents.list']({})
      assert.deepEqual(
        agents.map((a) => [a.name, a.harness, a.model]),
        [
          ['zeus', 'claude', 'claude-sonnet-5'],
          ['diana', 'codex', 'gpt-5.6-luna'],
        ],
      )
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const { member } = await operations['member.add']({
        session: session.id,
        agent: 'diana',
        role: 'reviewer',
      })
      assert.deepEqual([member.handle, member.harness, member.role], ['diana', 'codex', 'reviewer'])
      await assert.rejects(
        operations['member.add']({ session: session.id, agent: 'ghost' }),
        /no agent named ghost/,
      )
    })
  })

  it('starts a new session with the last team, as the saved agents are now', async () => {
    await withPage(async ({ operations, opened, env }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ session: session.id, agent: 'zeus' })
      await operations['member.add']({ session: session.id, agent: 'diana', role: 'reviewer' })
      await writeFile(
        path.join(env.CONSENSFLOW_HOME, 'agents.json'),
        `${JSON.stringify({
          schemaVersion: 1,
          agents: [{ id: 'zeus', kind: 'opencode', model: 'opencode/muse-spark-1.3' }],
        })}\n`,
      )
      const next = await operations['session.open']({ directory: '/work/api', harness: 'pi' })
      assert.deepEqual(opened[1].team, [{ agent: 'zeus', harness: 'opencode', role: 'worker' }])
      assert.deepEqual(
        next.session.participants.map((p) => p.handle),
        ['human', 'lead', 'zeus'],
      )
    })
  })

  it('takes a member off the team through the dispatcher, which closes its window', async () => {
    await withPage(async ({ operations, removed, kicks }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ session: session.id, agent: 'zeus' })
      await operations['task.add']({ session: session.id, to: 'zeus', body: 'Parser' })
      const before = kicks()
      const { member, cancelled } = await operations['member.remove']({
        session: session.id,
        agent: 'zeus',
      })
      assert.deepEqual(removed, [[session.id, 'zeus']])
      assert.deepEqual([member.handle, cancelled], ['zeus', [1]])
      assert.equal(kicks(), before + 1)
      const { board } = await operations['board.get']({ session: session.id })
      assert.deepEqual(
        board.lanes.map((lane) => lane.participant.handle),
        ['human', 'lead'],
      )
    })
  })

  it('adds a PM once, on the harness the human picks', async () => {
    await withPage(async ({ operations }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const { member } = await operations['pm.add']({ session: session.id, harness: 'codex' })
      assert.deepEqual([member.handle, member.role, member.harness], ['pm', 'pm', 'codex'])
      await assert.rejects(
        operations['pm.add']({ session: session.id, harness: 'pi' }),
        /already in session/,
      )
    })
  })

  it('lets the human hand out, accept, reopen and cancel tasks', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ session: session.id, agent: 'zeus' })
      const { task } = await operations['task.add']({
        session: session.id,
        to: 'zeus',
        body: 'Write the parser',
      })
      assert.deepEqual([task.number, task.requester, task.assignee], [1, 'human', 'zeus'])
      const message = ledger.task(session.id, 1).messages[0]
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, {})
      ledger.recordResult(session.id, 1, { body: 'done' })
      const reopened = await operations['task.reopen']({
        session: session.id,
        task: 1,
        body: 'Add tests',
      })
      assert.equal(reopened.task.state, 'queued')
      const cancelled = await operations['task.cancel']({ session: session.id, task: 1 })
      assert.equal(cancelled.task.state, 'cancelled')
      const { task: thread } = await operations['task.get']({ session: session.id, task: 1 })
      assert.deepEqual(
        thread.messages.map((m) => [m.kind, m.sender]),
        [
          ['task', 'human'],
          ['result', 'zeus'],
          ['task', 'human'],
        ],
      )
      await assert.rejects(operations['task.get']({ session: session.id, task: 9 }), /no task T-9/)
      await operations['task.add']({ session: session.id, to: 'zeus', body: 'Second' })
      const second = ledger.task(session.id, 2).messages[0]
      ledger.beginDelivery(second.id)
      ledger.confirmDelivery(second.id, {})
      ledger.recordResult(session.id, 2, { body: 'ok' })
      assert.equal(
        (await operations['task.accept']({ session: session.id, task: 2 })).task.state,
        'accepted',
      )
    })
  })

  it("shows the human's inbox and routes an answer back to whoever asked", async () => {
    await withPage(async ({ ledger, operations }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const question = ledger.ask(session.id, { from: 'lead', to: 'human', body: 'Deploy now?' })
      const { messages } = await operations['inbox.get']({ session: session.id })
      assert.deepEqual(
        messages.map((m) => [m.id, m.kind, m.state]),
        [[question.id, 'question', 'queued']],
      )
      assert.equal(
        (await operations['message.read']({ message: question.id })).message.state,
        'read',
      )
      const { message } = await operations['message.answer']({ question: question.id, body: 'Yes' })
      assert.deepEqual([message.kind, message.recipient], ['answer', 'lead'])
    })
  })

  it('draws the board with each lane activity and window', async () => {
    await withPage(async ({ operations }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const { board } = await operations['board.get']({ session: session.id })
      assert.deepEqual(
        board.lanes.map((lane) => [lane.participant.handle, lane.activity.state, lane.pane]),
        [
          ['human', 'idle', null],
          ['lead', 'idle', null],
        ],
      )
    })
  })
})
