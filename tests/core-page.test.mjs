import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
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
  let kicks = 0
  const dispatcher = {
    async openSession(request) {
      opened.push(request)
      return ledger.createSession({
        directory: request.directory,
        name: request.name,
        lead: { harness: request.harness },
      })
    },
    async resumeSession(id) {
      return ledger.setSessionState(id, 'open')
    },
    activity: () => ({ state: 'idle' }),
    pane: () => null,
  }
  const operations = pageOperations({ ledger, dispatcher, env, kick: () => kicks++ })
  try {
    await fn({ ledger, operations, opened, kicks: () => kicks })
  } finally {
    ledger.close()
    await rm(home, { recursive: true, force: true })
  }
}

describe('the page protocol of the new core', () => {
  it('opens a session named after its folder and lists it', async () => {
    await withPage(async ({ operations, opened, kicks }) => {
      const { session } = await operations['session.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      assert.deepEqual(opened, [{ directory: '/work/app', name: 'app', harness: 'pi' }])
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
