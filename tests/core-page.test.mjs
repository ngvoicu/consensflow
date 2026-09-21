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
  const closed = []
  let kicks = 0
  const dispatcher = {
    async openProject(request) {
      opened.push(request)
      return ledger.createProject({
        directory: request.directory,
        name: request.name,
        lead: { harness: request.harness },
        team: request.team,
        ...(request.review === undefined ? {} : { review: request.review }),
        ...(request.gate === undefined ? {} : { gate: request.gate }),
      })
    },
    async removeMember(project, handle) {
      removed.push([project, handle])
      return ledger.removeMember(project, handle)
    },
    windows: [],
    async openWindow(project, handle) {
      dispatcher.windows.push(['open', handle])
      return ledger.project(project)
    },
    async closeWindow(project, handle) {
      dispatcher.windows.push(['close', handle])
      return ledger.project(project)
    },
    async endSession(project, handle) {
      dispatcher.windows.push(['end', handle])
      return ledger.endSession(project, handle, { by: 'human' })
    },
    async resumeProject(id) {
      return ledger.setProjectState(id, 'open')
    },
    async closeProject(id) {
      closed.push(id)
      return ledger.setProjectState(id, 'suspended')
    },
    async deleteProject(id) {
      return ledger.deleteProject(id)
    },
    activity: () => ({ state: 'idle' }),
    pane: () => null,
  }
  const operations = pageOperations({ ledger, dispatcher, env, kick: () => kicks++ })
  try {
    await fn({ ledger, operations, opened, removed, closed, env, kicks: () => kicks, dispatcher })
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

  it('opens a project named after its folder and lists it', async () => {
    await withPage(async ({ operations, opened, kicks }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      assert.deepEqual(opened, [
        {
          directory: '/work/app',
          name: 'app',
          harness: 'pi',
          review: undefined,
          gate: undefined,
          team: [],
        },
      ])
      assert.equal(kicks(), 1)
      const { projects } = await operations['projects.list']({})
      assert.deepEqual(
        projects.map((s) => [s.id, s.name]),
        [[project.id, 'app']],
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
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const { member } = await operations['member.add']({
        project: project.id,
        agent: 'diana',
        roles: ['reviewer'],
      })
      assert.deepEqual([member.handle, member.harness, member.role], ['diana', 'codex', 'reviewer'])
      await assert.rejects(
        operations['member.add']({ project: project.id, agent: 'ghost' }),
        /no agent named ghost/,
      )
    })
  })

  it('starts a new project with the last team, as the saved agents are now', async () => {
    await withPage(async ({ operations, opened, env }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      await operations['member.add']({ project: project.id, agent: 'diana', roles: ['reviewer'] })
      await writeFile(
        path.join(env.CONSENSFLOW_HOME, 'agents.json'),
        `${JSON.stringify({
          schemaVersion: 1,
          agents: [{ id: 'zeus', kind: 'opencode', model: 'opencode/muse-spark-1.3' }],
        })}\n`,
      )
      const next = await operations['project.open']({ directory: '/work/api', harness: 'pi' })
      const [zeus] = (await operations['agents.list']({})).agents
      assert.deepEqual(opened[1].team, [
        {
          roles: ['worker'],
          agent: 'zeus',
          harness: 'opencode',
          tier: zeus.profile.workTier,
        },
      ])
      assert.ok(zeus.profile.workTier, 'a saved agent always has a tier')
      assert.deepEqual(
        next.project.participants.map((p) => p.handle),
        ['human', 'lead', 'zeus'],
      )
    })
  })

  it('takes a member off the team through the dispatcher, which closes its window', async () => {
    await withPage(async ({ ledger, operations, removed, kicks }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Parser',
      })
      ledger.assignTask(project.id, 1, zeus.id)
      const before = kicks()
      const { member, cancelled } = await operations['member.remove']({
        project: project.id,
        agent: 'zeus',
      })
      assert.deepEqual(removed, [[project.id, 'zeus']])
      assert.deepEqual([member.handle, cancelled], ['zeus', [1]])
      assert.equal(kicks(), before + 1)
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual(
        board.lanes.map((lane) => lane.participant.handle),
        ['human', 'lead'],
      )
    })
  })

  it('puts a task on the board for a tier, asks for a review, and sets the review policy', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      await operations['member.add']({ project: project.id, agent: 'diana', roles: ['reviewer'] })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const { task } = await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Write the docs',
      })
      assert.deepEqual([task.state, task.assignee, task.pool], ['open', null, 'worker'])
      // Work goes on the board for a tier: the page names no participant.
      await assert.rejects(
        operations['task.add']({ project: project.id, to: 'lead', body: 'By name' }),
        /a pool is worker, advisor, designer/,
      )
      ledger.assignTask(project.id, 1, zeus.id)
      const message = ledger.task(project.id, 1).messages[0]
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, {})
      ledger.recordResult(project.id, 1, { body: 'done' })
      assert.equal(
        (await operations['task.review']({ project: project.id, task: 1 })).task.state,
        'review',
      )
      assert.equal(
        (await operations['project.review']({ project: project.id, review: 'members' })).project
          .review,
        'members',
      )
      await assert.rejects(
        operations['project.review']({ project: project.id, review: 'all' }),
        /a review policy is none, members/,
      )
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual([board.project.review, board.open], ['members', []])
    })
  })

  it('opens a project with human approval required, sets it by hand, and approves or declines what waits', async () => {
    await withPage(async ({ ledger, operations, opened, kicks }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
        gate: true,
      })
      assert.equal(project.gate, true)
      assert.equal(opened[0].gate, true)
      assert.equal(
        (await operations['project.gate']({ project: project.id, gate: false })).project.gate,
        false,
      )
      await assert.rejects(
        operations['project.gate']({ project: project.id, gate: 'yes' }),
        /required \(true\) or not \(false\)/,
      )
      await operations['project.gate']({ project: project.id, gate: true })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      ledger.createTask(project.id, { from: 'lead', pool: 'worker', tier: zeus.tier, body: 'One' })
      ledger.createTask(project.id, { from: 'lead', pool: 'worker', tier: zeus.tier, body: 'Two' })
      const first = ledger.assignTask(project.id, 1, zeus.id).message
      const second = ledger.assignTask(project.id, 2, zeus.id).message
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual(
        board.gated.map((m) => [m.id, m.kind, m.sender, m.taskNumber]),
        [
          [first.id, 'task', 'lead', 1],
          [second.id, 'task', 'lead', 2],
        ],
      )
      const before = kicks()
      const approved = await operations['message.approve']({ message: first.id })
      assert.equal(approved.message.state, 'queued')
      const declined = await operations['message.decline']({
        message: second.id,
        reason: 'One at a time',
      })
      assert.deepEqual(
        [declined.message.state, declined.message.reason],
        ['cancelled', 'declined by @human: One at a time'],
      )
      assert.equal(ledger.task(project.id, 2).state, 'cancelled')
      assert.equal(kicks(), before + 2, 'each decision wakes the dispatcher')
      assert.deepEqual((await operations['board.get']({ project: project.id })).board.gated, [])
    })
  })

  it("opens, closes and ends a session's window at the human's hand", async () => {
    await withPage(async ({ ledger, operations, dispatcher }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      const session = message.recipient
      await operations['session.open']({ project: project.id, handle: session })
      await operations['session.close']({ project: project.id, handle: session })
      await assert.rejects(
        operations['session.end']({ project: project.id, handle: session }),
        /still holds work/,
      )
      ledger.cancelTask(project.id, 1, { by: 'human' })
      const { project: after } = await operations['session.end']({
        project: project.id,
        handle: session,
      })
      assert.equal(
        after.participants.some((p) => p.handle === session),
        false,
      )
      assert.deepEqual(dispatcher.windows, [
        ['open', session],
        ['close', session],
        ['end', session],
        ['end', session],
      ])
    })
  })

  it('closes a project through the dispatcher and lists it as suspended', async () => {
    await withPage(async ({ operations, closed }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const result = await operations['project.close']({ project: project.id })
      assert.deepEqual([closed, result.project.state], [[project.id], 'suspended'])
      const { projects } = await operations['projects.list']({})
      assert.equal(projects[0].state, 'suspended')
    })
  })

  it('deletes a closed project and lists it no more', async () => {
    await withPage(async ({ operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await assert.rejects(operations['project.delete']({ project: project.id }), {
        code: 'project-open',
      })
      await operations['project.close']({ project: project.id })
      const result = await operations['project.delete']({ project: project.id })
      assert.deepEqual(result.project, { id: project.id, name: 'app' })
      assert.deepEqual((await operations['projects.list']({})).projects, [])
    })
  })

  it('lets the human hand out, reopen and cancel tasks, and leaves accepting to the lead', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
      })
      assert.equal(project.review, 'none')
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const { task } = await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Write the parser',
      })
      assert.deepEqual([task.number, task.requester, task.assignee], [1, 'human', null])
      ledger.assignTask(project.id, 1, zeus.id)
      const message = ledger.task(project.id, 1).messages[0]
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, {})
      ledger.recordResult(project.id, 1, { body: 'done' })
      const reopened = await operations['task.reopen']({
        project: project.id,
        task: 1,
        body: 'Add tests',
      })
      assert.equal(reopened.task.state, 'queued')
      const cancelled = await operations['task.cancel']({ project: project.id, task: 1 })
      assert.equal(cancelled.task.state, 'cancelled')
      const { task: thread } = await operations['task.get']({ project: project.id, task: 1 })
      assert.deepEqual(
        thread.messages.map((m) => [m.kind, m.sender.replace(/^zeus-.*$/, 'zeus-session')]),
        [
          ['task', 'human'],
          ['result', 'zeus-session'],
          ['task', 'human'],
        ],
      )
      await assert.rejects(operations['task.get']({ project: project.id, task: 9 }), /no task T-9/)
      await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Second',
      })
      ledger.assignTask(project.id, 2, zeus.id)
      const second = ledger.task(project.id, 2).messages[0]
      ledger.beginDelivery(second.id)
      ledger.confirmDelivery(second.id, {})
      ledger.recordResult(project.id, 2, { body: 'ok' })
      assert.equal(operations['task.accept'], undefined, "accepting is the lead's, in its terminal")
    })
  })

  it('lets the human put a task on the board that waits for others', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const first = { project: project.id, pool: 'worker', tier: zeus.tier, body: 'Lexer' }
      await operations['task.add'](first)
      const { task } = await operations['task.add']({ ...first, needs: [1], body: 'Parser' })
      assert.deepEqual([task.number, task.blockedBy], [2, [1]])
      await assert.rejects(
        operations['task.add']({ ...first, needs: [9], body: 'Nowhere' }),
        /no task T-9/,
      )
    })
  })

  it('lets the human pause a task and resume it with words', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, {})
      const paused = await operations['task.pause']({ project: project.id, task: 1 })
      assert.equal(paused.task.state, 'paused')
      const { task, message: words } = await operations['task.resume']({
        project: project.id,
        task: 1,
        body: 'Go on',
      })
      assert.deepEqual(
        [task.state, words.body, words.sender],
        ['queued', 'Resumed: Go on', 'human'],
      )
    })
  })

  it("reads a task's transcript copy, the last items first when asked for fewer", async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
        review: 'none',
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      await operations['task.add']({
        project: project.id,
        pool: 'worker',
        tier: zeus.tier,
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      const conversation = ledger.startConversation(message.recipientId, { harness: 'claude-code' })
      ledger.copyTranscript(conversation.id, [
        { id: 'u1', role: 'user', text: 'Lexer', complete: true },
        { id: 'a1', role: 'assistant', text: 'Lexer done', complete: true },
      ])
      const all = await operations['task.transcript']({ project: project.id, task: 1 })
      assert.deepEqual([all.total, all.items.map((i) => i.text)], [2, ['Lexer', 'Lexer done']])
      const last = await operations['task.transcript']({ project: project.id, task: 1, limit: 1 })
      assert.deepEqual([last.total, last.items.map((i) => i.id)], [2, ['a1']])
      await assert.rejects(
        operations['task.transcript']({ project: project.id, task: 9 }),
        /no task T-9/,
      )
    })
  })

  it("shows the human's inbox and routes an answer back to whoever asked", async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const question = ledger.ask(project.id, { from: 'lead', to: 'human', body: 'Deploy now?' })
      const { messages } = await operations['inbox.get']({ project: project.id })
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
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      const { board } = await operations['board.get']({ project: project.id })
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
