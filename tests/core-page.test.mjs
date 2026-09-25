import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { CATALOG } from '../src/catalog.js'
import { pageOperations } from '../src/core/page.js'
import { openLedger } from '../src/ledger/index.js'
import { addAgent, removeAgent } from '../src/roster.js'

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
        chief: { harness: request.harness },
        staff: request.staff,
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
          gate: undefined,
          staff: [],
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

  it('lists the saved agents for the staff picker and adds one with its own harness', async () => {
    await withPage(async ({ operations }) => {
      const { agents } = await operations['agents.list']({})
      // Every catalog agent is on offer, as the catalog has it: the file's
      // copies of zeus and diana change nothing.
      const mine = agents.filter((a) => ['zeus', 'diana'].includes(a.name))
      assert.deepEqual(
        mine.map((a) => [a.name, a.harness, a.model]),
        [
          ['diana', 'codex', 'gpt-5.6-luna'],
          ['zeus', 'claude', 'claude-opus-5-5'],
        ],
        'in the catalog’s order',
      )
      assert.equal(
        agents.length,
        Object.values(CATALOG).flat().length,
        'the whole catalog is on offer',
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

  it('starts a new project with the last staff, as the saved agents are now', async () => {
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
      const { agents } = await operations['agents.list']({})
      const zeus = agents.find((a) => a.name === 'zeus')
      const diana = agents.find((a) => a.name === 'diana')
      // zeus is the human's own opencode agent now; diana is the catalog's, as it has her.
      assert.deepEqual(opened[1].staff, [
        { roles: ['worker'], agent: 'zeus', harness: 'opencode', tier: zeus.profile.workTier },
        { roles: ['reviewer'], agent: 'diana', harness: 'codex', tier: diana.profile.workTier },
      ])
      assert.ok(zeus.profile.workTier, 'an agent always has a tier')
      assert.deepEqual(
        next.project.participants.map((p) => p.handle),
        ['human', 'chief', 'zeus', 'diana'],
      )
    })
  })

  it('marks a member whose agent is gone on the board, and nobody else', async () => {
    await withPage(async ({ operations, env }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      addAgent({ name: 'mine', harness: 'codex', model: 'gpt-6-astra', effort: 'low' }, env)
      await operations['member.add']({ project: project.id, agent: 'mine' })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      removeAgent('mine', env)
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual(
        board.lanes.filter((lane) => lane.agentMissing).map((lane) => lane.participant.handle),
        ['mine'],
      )
    })
  })

  it('takes a member off the staff through the dispatcher, which closes its window', async () => {
    await withPage(async ({ ledger, operations, removed, kicks }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Parser',
      })
      ledger.assignTask(project.id, 1, artemis.id)
      const before = kicks()
      const { member, cancelled } = await operations['member.remove']({
        project: project.id,
        agent: 'artemis',
      })
      assert.deepEqual(removed, [[project.id, 'artemis']])
      assert.deepEqual([member.handle, cancelled], ['artemis', [1]])
      assert.equal(kicks(), before + 1)
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual(
        board.lanes.map((lane) => lane.participant.handle),
        ['human', 'chief'],
      )
    })
  })

  it('reassigns a task given by tier back to the board, and nothing given by name', async () => {
    await withPage(async ({ ledger, operations, kicks }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Joke',
      })
      ledger.assignTask(project.id, 1, artemis.id)
      const before = kicks()
      const { task } = await operations['task.reassign']({ project: project.id, task: 1 })
      assert.deepEqual([task.state, task.assignee], ['open', null])
      assert.match(task.body, /Reassigned from @artemis-[a-z]+-[a-z]+ \(by @human\)/)
      assert.ok(kicks() > before, 'the daemon looks at once')
      const own = ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Plan' })
      await assert.rejects(
        operations['task.reassign']({ project: project.id, task: own.task.number }),
        /given by name, not by tier/,
      )
    })
  })

  it('offers the human no review and no new task: the chief adds both, a review as a task', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'diana', roles: ['reviewer'] })
      const diana = ledger.project(project.id).participants.find((p) => p.handle === 'diana')
      const { task } = ledger.createTask(project.id, {
        from: 'chief',
        pool: 'reviewer',
        tier: diana.tier,
        body: 'Review the docs',
      })
      assert.deepEqual([task.state, task.assignee, task.pool], ['open', null, 'reviewer'])
      for (const name of ['task.add', 'task.review', 'project.review']) {
        assert.equal(operations[name], undefined, `no ${name} for the human`)
      }
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual(
        board.open.map((open) => open.number),
        [1],
      )
      assert.equal(board.project.review, undefined, 'a project has no review policy')
    })
  })

  it('opens a project with human approval required, sets it by hand, and approves or declines what waits', async () => {
    await withPage(async ({ ledger, operations, opened, kicks }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
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
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'One',
      })
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Two',
      })
      const first = ledger.assignTask(project.id, 1, artemis.id).message
      const second = ledger.assignTask(project.id, 2, artemis.id).message
      const { board } = await operations['board.get']({ project: project.id })
      assert.deepEqual(
        board.gated.map((m) => [m.id, m.kind, m.sender, m.taskNumber]),
        [
          [first.id, 'task', 'chief', 1],
          [second.id, 'task', 'chief', 2],
        ],
      )
      const before = kicks()
      const approved = await operations['message.approve']({ message: first.id })
      assert.equal(approved.message.state, 'queued')
      const declined = await operations['message.decline']({ message: second.id })
      assert.deepEqual(
        [declined.message.state, declined.message.reason],
        ['cancelled', 'declined by @human'],
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
      })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, artemis.id)
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
      assert.deepEqual([result.project.id, result.project.name], [project.id, 'app'])
      assert.deepEqual((await operations['projects.list']({})).projects, [])
    })
  })

  it('lets the human cancel tasks, and leaves handing out, sending back and accepting to the chief', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      const { task } = ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Write the parser',
      })
      assert.deepEqual([task.number, task.requester, task.assignee], [1, 'chief', null])
      ledger.assignTask(project.id, 1, artemis.id)
      const message = ledger.task(project.id, 1).messages[0]
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, {})
      // The human writes to no agent from the board: sending back is the chief's, in its terminal.
      assert.equal(operations['task.reopen'], undefined)
      const cancelled = await operations['task.cancel']({ project: project.id, task: 1 })
      assert.equal(cancelled.task.state, 'cancelled')
      const { task: thread } = await operations['task.get']({ project: project.id, task: 1 })
      assert.deepEqual(
        thread.messages.map((m) => [m.kind, m.sender.replace(/^artemis-.*$/, 'artemis-session')]),
        [['task', 'chief']],
      )
      await assert.rejects(operations['task.get']({ project: project.id, task: 9 }), /no task T-9/)
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Second',
      })
      ledger.assignTask(project.id, 2, artemis.id)
      const second = ledger.task(project.id, 2).messages[0]
      ledger.beginDelivery(second.id)
      ledger.confirmDelivery(second.id, {})
      ledger.recordResult(project.id, 2, { body: 'ok' })
      assert.equal(
        operations['task.accept'],
        undefined,
        "accepting is the chief's, in its terminal",
      )
    })
  })

  it('lets the human pause a task and resume it without writing to the agent', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, artemis.id)
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, {})
      const paused = await operations['task.pause']({ project: project.id, task: 1 })
      assert.equal(paused.task.state, 'paused')
      const { task, message: words } = await operations['task.resume']({
        project: project.id,
        task: 1,
      })
      assert.deepEqual(
        [task.state, words.body, words.sender],
        ['queued', 'Resumed: Go on where you stopped.', 'human'],
        'the human writes nothing: the same words every time',
      )
    })
  })

  it("reads a task's transcript copy, the last items first when asked for fewer", async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        harness: 'pi',
      })
      await operations['member.add']({ project: project.id, agent: 'artemis' })
      const artemis = ledger.project(project.id).participants.find((p) => p.handle === 'artemis')
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: artemis.tier,
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, artemis.id)
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
      const question = ledger.ask(project.id, { from: 'chief', to: 'human', body: 'Deploy now?' })
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
      assert.deepEqual([message.kind, message.recipient], ['answer', 'chief'])
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
          ['chief', 'idle', null],
        ],
      )
    })
  })
})
