import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { pageOperations } from '../../../../src/core/page.js'
import { openLedger } from '../../../../src/ledger/index.js'
import { addAgent, setPreferences } from '../../../../src/roster.js'

/**
 * What the board page may ask of the daemon, where `core-page.test.mjs` does
 * not look (TEST-BDC-13): the operation that suite never reaches, the board
 * as it is merged with what the dispatcher knows when that is not the idle
 * default, the choices an operation has beyond the ones its first test makes.
 * The dispatcher is the stand-in of that suite, with what it answers for the
 * board set by the test; the daemon recorder takes each operation and each
 * call it made on it.
 */
async function withPage(fn) {
  const home = await mkdtemp(path.join(os.tmpdir(), 'cf-core-page-corners-'))
  const env = { HOME: home, CONSENSFLOW_HOME: path.join(home, 'consensflow') }
  await mkdir(env.CONSENSFLOW_HOME, { recursive: true })
  await writeFile(
    path.join(env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify({
      schemaVersion: 1,
      agents: [
        {
          id: 'zeus',
          preset: 'zeus',
          kind: 'claude-code',
          model: 'claude-sonnet-5',
          effort: 'high',
        },
        { id: 'diana', preset: 'diana', kind: 'codex', model: 'gpt-5.6-luna' },
      ],
    })}\n`,
  )
  const ledger = openLedger(path.join(home, 'consensflow.db'))
  const shown = { activity: {}, pane: {}, hidden: {}, switching: {}, holding: {} }
  let kicks = 0
  const dispatcher = {
    async openProject(request) {
      return ledger.createProject({
        directory: request.directory,
        name: request.name,
        chief: request.chief,
        staff: request.staff,
        ...(request.gate === undefined ? {} : { gate: request.gate }),
      })
    },
    async resumeProject(id) {
      return ledger.setProjectState(id, 'open')
    },
    async closeProject(id) {
      return ledger.setProjectState(id, 'suspended')
    },
    async removeMember(project, handle) {
      return ledger.removeMember(project, handle)
    },
    async endSession(project, handle) {
      return ledger.endSession(project, handle, { by: 'human' })
    },
    async openWindow() {
      throw 'the pane host is not there'
    },
    activity: (id) => shown.activity[id] ?? { state: 'idle' },
    pane: (id) => shown.pane[id] ?? null,
    pendingSwitch: (id) => shown.switching[id] ?? null,
    holding: (id) => shown.holding[id] ?? false,
    hidden: (id) => shown.hidden[id] ?? false,
    requireAdapter() {},
  }
  const operations = pageOperations({ ledger, dispatcher, env, kick: () => kicks++ })
  try {
    await fn({ ledger, operations, env, shown, kicks: () => kicks })
  } finally {
    ledger.close()
    await rm(home, { recursive: true, force: true })
  }
}

const open = (operations, extra = {}) =>
  operations['project.open']({ directory: '/work/app', agent: 'leto', ...extra })

describe('the page protocol of the daemon, where its first suite does not look', () => {
  it('resumes a closed project, and says so when the project is not there', async () => {
    await withPage(async ({ operations, kicks }) => {
      const { project } = await open(operations)
      await operations['project.close']({ project: project.id })
      const before = kicks()
      const resumed = await operations['project.resume']({ project: project.id })
      assert.equal(resumed.project.state, 'open')
      assert.equal(kicks(), before + 1)
      await assert.rejects(operations['project.resume']({ project: 99 }), Error)
      assert.equal(kicks(), before + 1, 'a refusal wakes nothing')
    })
  })

  it('draws the board with what the dispatcher knows of each lane, after the lane’s own fields and in this order', async () => {
    await withPage(async ({ ledger, operations, shown, env }) => {
      const { project } = await open(operations)
      addAgent({ name: 'mine', harness: 'codex', model: 'gpt-6-astra', effort: 'low' }, env)
      await operations['member.add']({
        project: project.id,
        agent: 'mine',
        roles: ['worker', 'reviewer'],
      })
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      const lane = (handle) =>
        ledger.project(project.id).participants.find((p) => p.handle === handle).id
      shown.activity[lane('chief')] = { state: 'working', since: '2026-10-05T10:00:00.000Z' }
      shown.pane[lane('chief')] = { id: 7, title: 'chief', size: [120, 40] }
      shown.hidden[lane('zeus')] = true
      shown.switching[lane('chief')] = { agent: 'diana', when: 'turn' }
      shown.holding[lane('chief')] = true
      // An agent the human removed from the file under a member who is still on the staff.
      const { removeAgent } = await import('../../../../src/roster.js')
      removeAgent('mine', env)
      const { board } = await operations['board.get']({ project: project.id })
      const chief = board.lanes.find((l) => l.participant.handle === 'chief')
      assert.deepEqual(Object.keys(chief).slice(-6), [
        'agentMissing',
        'activity',
        'pane',
        'hidden',
        'switching',
        'holding',
      ])
      assert.deepEqual(
        [chief.activity.state, chief.pane.id, chief.hidden, chief.switching.agent, chief.holding],
        ['working', 7, false, 'diana', true],
      )
      assert.deepEqual(
        board.lanes.map((l) => [l.participant.handle, l.agentMissing, l.hidden]),
        [
          ['human', false, false],
          ['chief', false, false],
          ['mine', true, false],
          ['zeus', false, true],
        ],
      )
    })
  })

  it('reads the inbox of the human, or of another participant, and says who is not in the project', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await open(operations)
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      ledger.note(project.id, { from: 'chief', to: 'human', body: 'One' })
      ledger.note(project.id, { from: 'human', to: 'chief', body: 'Two' })
      const ask = (body) => operations['inbox.get']({ project: project.id, ...body })
      assert.deepEqual(
        (await ask({})).messages.map((m) => m.recipient),
        ['human'],
      )
      assert.deepEqual(
        (await ask({ participant: 'chief' })).messages.map((m) => m.recipient),
        ['chief'],
      )
      assert.equal((await ask({ participant: 'zeus' })).total, 0)
      assert.equal((await ask({ unread: true })).total, 1)
      assert.equal((await ask({ unread: 'yes' })).total, 1, 'unread is true or it is not')
      await assert.rejects(ask({ participant: 'nobody' }), {
        message: `nobody is not in project ${project.id}`,
      })
      await assert.rejects(operations['inbox.get']({ project: 99 }), {
        message: 'human is not in project 99',
      })
    })
  })

  it('opens a project with the staff it is given, and no other, and names it when the page does', async () => {
    await withPage(async ({ operations, ledger }) => {
      const staff = [
        { agent: 'zeus', roles: ['reviewer'] },
        { agent: 'diana', roles: ['worker', 'advisor'] },
      ]
      const { project } = await open(operations, { name: 'Named', gate: true, staff })
      assert.deepEqual([project.name, project.gate], ['Named', true])
      assert.deepEqual(
        project.participants.map((p) => [p.handle, p.roles]),
        [
          ['human', []],
          ['chief', []],
          ['zeus', ['reviewer']],
          ['diana', ['worker', 'advisor']],
        ],
      )
      await assert.rejects(
        open(operations, {
          directory: '/work/api',
          staff: [{ agent: 'ghost', roles: ['worker'] }],
        }),
        { message: 'no agent named ghost in your agents' },
      )
      const empty = await open(operations, { directory: '/work/empty', staff: [] })
      assert.equal(
        empty.project.participants.length,
        2,
        'an empty staff is no staff: not the last one',
      )
      assert.equal(ledger.projects().length, 2, 'the one that was refused is not there')
    })
  })

  it('adds a member as a worker unless the page says otherwise, and takes the roles it is given as they come', async () => {
    await withPage(async ({ operations }) => {
      const { project } = await open(operations)
      const added = await operations['member.add']({ project: project.id, agent: 'zeus' })
      assert.deepEqual(added.member.roles, ['worker'])
      await operations['member.roles']({
        project: project.id,
        agent: 'zeus',
        roles: ['reviewer', 'advisor'],
      })
      await assert.rejects(
        operations['member.roles']({ project: project.id, agent: 'zeus', roles: [] }),
        Error,
      )
      await assert.rejects(
        operations['member.roles']({ project: project.id, agent: 'nobody', roles: ['worker'] }),
        Error,
      )
      await assert.rejects(
        operations['member.remove']({ project: project.id, agent: 'nobody' }),
        Error,
      )
    })
  })

  it('offers the agents of the harnesses installed here, says which are missing, and hides those a preference keeps off', async () => {
    await withPage(async ({ operations, env }) => {
      const bin = path.join(env.HOME, 'bin')
      await mkdir(bin, { recursive: true })
      // Windows finds a command by its extension: a stand-in of each name, as `core-page` writes them.
      for (const name of ['claude', 'pi', 'claude.cmd', 'pi.cmd']) {
        await writeFile(path.join(bin, name), '#!/bin/sh\n', { mode: 0o755 })
      }
      env.PATH = bin
      const { agents, missing } = await operations['agents.list']({})
      assert.deepEqual(missing, ['devin', 'codex', 'opencode'])
      assert.ok(
        agents.some((agent) => agent.harness === 'claude' && agent.notInstalled === undefined),
      )
      assert.ok(
        agents
          .filter((agent) => agent.harness === 'codex')
          .every((agent) => agent.notInstalled === true),
      )
      setPreferences({ ownHarnessOnly: true }, env)
      const again = await operations['agents.list']({})
      assert.ok(again.agents.some((agent) => agent.harness === 'pi' && agent.hidden === true))
    })
  })

  it('answers with the words of whatever a stand-in threw, a string as it is', async () => {
    await withPage(async ({ operations, ledger }) => {
      const { project } = await open(operations)
      await operations['member.add']({ project: project.id, agent: 'zeus' })
      ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Plan' })
      await assert.rejects(
        operations['session.open']({ project: project.id, handle: 'chief' }),
        (cause) => cause === 'the pane host is not there',
      )
      await assert.rejects(operations['message.read']({ message: 999 }), Error)
      await assert.rejects(operations['message.approve']({ message: 999 }), Error)
      await assert.rejects(operations['message.decline']({ message: 999 }), Error)
      await assert.rejects(operations['project.gate']({ project: 99, gate: true }), Error)
    })
  })

  it('reads a task’s transcript at the limit the page names, and the whole of it when it names none', async () => {
    await withPage(async ({ ledger, operations }) => {
      const { project } = await open(operations)
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
      ledger.copyTranscript(
        conversation.id,
        Array.from({ length: 5 }, (_, n) => ({
          id: `i${n + 1}`,
          role: 'assistant',
          text: `item ${n + 1}`,
          complete: true,
        })),
      )
      const shown = async (limit) =>
        (await operations['task.transcript']({ project: project.id, task: 1, limit })).items.map(
          (i) => i.id,
        )
      assert.deepEqual(await shown(undefined), ['i1', 'i2', 'i3', 'i4', 'i5'])
      assert.deepEqual(await shown(2), ['i4', 'i5'])
      assert.deepEqual((await shown(0)).length <= 5, true)
      assert.deepEqual(await shown(99), ['i1', 'i2', 'i3', 'i4', 'i5'])
    })
  })
})
