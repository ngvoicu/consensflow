import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { OVERDUE_MS, openLedger, SCHEMA_VERSION, verdictOf } from '../src/ledger/index.js'
import { MIGRATIONS } from '../src/ledger/schema.js'

/**
 * The ledger (TEST-BDC-01): one SQLite file in the home that holds every
 * project, participant, task and inbox message. Each test gets a throwaway
 * directory and a clock that moves one second per reading.
 */
const LEDGER = fileURLToPath(new URL('../src/ledger/index.js', import.meta.url))

async function withDir(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-ledger-'))
  try {
    return await fn(dir)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

function clock() {
  let at = Date.parse('2026-09-19T10:00:00.000Z')
  return () => {
    at += 1000
    return new Date(at)
  }
}

async function withLedger(fn) {
  return withDir(async (dir) => {
    const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: clock() })
    try {
      return await fn(ledger, dir)
    } finally {
      ledger.close()
    }
  })
}

/** A project with a lead and two workers and no review gate, the shape most tests start from. */
function team(ledger) {
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    lead: { harness: 'claude-code' },
    review: 'none',
  })
  ledger.addMember(project.id, {
    agent: 'zeus',
    harness: 'claude-code',
    role: 'worker',
    tier: 'standard',
  })
  ledger.addMember(project.id, {
    agent: 'diana',
    harness: 'codex',
    role: 'worker',
    tier: 'standard',
  })
  const id = (handle) => ledger.project(project.id).participants.find((p) => p.handle === handle).id
  return { project, id }
}

/** Delivers a message the way the dispatcher will: begin, then confirm. */
function deliver(ledger, message) {
  ledger.beginDelivery(message.id)
  return ledger.confirmDelivery(message.id, { evidence: `native-${message.id}` })
}

/** Runs a Node child that imports the ledger; resolves with its stdout. */
function child(source, { killAfterMs, readyLine } = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn(process.execPath, ['--input-type=module', '-e', source], {
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let out = ''
    let err = ''
    proc.stdout.on('data', (chunk) => {
      out += chunk
      if (readyLine && out.includes(readyLine) && killAfterMs !== undefined) {
        setTimeout(() => proc.kill('SIGKILL'), killAfterMs)
        readyLine = null
      }
    })
    proc.stderr.on('data', (chunk) => {
      err += chunk
    })
    proc.on('error', reject)
    proc.on('close', (code, signal) => resolve({ out, err, code, signal }))
  })
}

describe('opening the ledger', () => {
  it('creates the schema at the current version and keeps its data across a reopen', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const first = openLedger(file, { now: clock() })
      const created = first.createProject({
        directory: '/work/app',
        name: 'app',
        lead: { harness: 'opencode' },
      })
      first.close()

      const raw = new DatabaseSync(file, { readOnly: true })
      assert.equal(raw.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION)
      assert.equal(raw.prepare('PRAGMA journal_mode').get().journal_mode, 'wal')
      raw.close()

      const second = openLedger(file, { now: clock() })
      try {
        assert.deepEqual(
          second.projects().map((project) => [project.id, project.name, project.state]),
          [[created.id, 'app', 'open']],
        )
      } finally {
        second.close()
      }
    })
  })

  it('refuses a second open while the first holds the file, in this process and in another', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const holder = openLedger(file)
      try {
        assert.throws(() => openLedger(file), { code: 'ledger-locked' })
        const other = await child(
          `import { openLedger } from ${JSON.stringify(LEDGER)}
           try { openLedger(${JSON.stringify(file)}); console.log('opened') }
           catch (error) { console.log(error.code) }`,
        )
        assert.equal(other.out.trim(), 'ledger-locked', other.err)
      } finally {
        holder.close()
      }
      openLedger(file).close()
    })
  })

  it('is free again once a holder is killed', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const killed = await child(
        `import { openLedger } from ${JSON.stringify(LEDGER)}
         openLedger(${JSON.stringify(file)}); console.log('ready'); setInterval(() => {}, 1000)`,
        { readyLine: 'ready', killAfterMs: 0 },
      )
      assert.equal(killed.signal, 'SIGKILL', killed.err)
      openLedger(file).close()
    })
  })

  it('refuses a database written by a newer ConsensFlow', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const raw = new DatabaseSync(file)
      raw.exec(`PRAGMA user_version = ${SCHEMA_VERSION + 1}`)
      raw.close()
      assert.throws(() => openLedger(file), { code: 'ledger-newer' })
    })
  })

  it('names a file that is not a ledger instead of failing somewhere inside SQLite', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      await writeFile(file, 'this is not a database, it is a text file '.repeat(200))
      assert.throws(() => openLedger(file), { code: 'ledger-unreadable' })
    })
  })

  it('keeps every task with its first message when the process is killed mid-write', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const crashed = await child(
        `import { openLedger } from ${JSON.stringify(LEDGER)}
         const ledger = openLedger(${JSON.stringify(file)})
         const project = ledger.createProject({ directory: '/w', name: 'w', lead: { harness: 'pi' } })
         ledger.addMember(project.id, { agent: 'zeus', harness: 'pi', role: 'worker', tier: 'standard' })
         console.log('ready')
         for (let n = 0; ; n++) ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'task ' + n })`,
        { readyLine: 'ready', killAfterMs: 150 },
      )
      assert.equal(crashed.signal, 'SIGKILL', crashed.err)
      const ledger = openLedger(file)
      try {
        assert.equal(ledger.integrity(), 'ok')
        const [project] = ledger.projects()
        const tasks = ledger.board(project.id).lanes.flatMap((lane) => lane.tasks)
        assert.ok(tasks.length > 0, 'the child wrote tasks before it was killed')
        for (const task of tasks) {
          const first = ledger
            .task(project.id, task.number)
            .messages.filter((m) => m.kind === 'task')
          assert.equal(first.length, 1, `T-${task.number} has exactly one task message`)
        }
      } finally {
        ledger.close()
      }
    })
  })
})

describe('upgrading a home', () => {
  it('gives a home written by the first schema the role sets, the unreviewed reason and the question options', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const old = new DatabaseSync(file)
      old.exec(MIGRATIONS[0])
      old.exec('PRAGMA user_version = 1')
      const at = '2026-09-19T21:00:00.000Z'
      old
        .prepare(
          `INSERT INTO project (id, directory, name, state, review, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 'members', ?, ?)`,
        )
        .run(at, at)
      old
        .prepare(
          `INSERT INTO participant (project_id, handle, role, agent, harness, tier, created_at) VALUES
           (1, 'human', 'human', NULL, NULL, NULL, ?),
           (1, 'lead', 'lead', NULL, 'claude-code', NULL, ?),
           (1, 'zeus', 'worker', 'zeus', 'claude-code', 'standard', ?),
           (1, 'hera', 'reviewer', 'hera', 'codex', 'standard', ?)`,
        )
        .run(at, at, at, at)
      old.close()
      const ledger = openLedger(file)
      try {
        const roles = Object.fromEntries(
          ledger.project(1).participants.map((p) => [p.handle, p.roles]),
        )
        assert.deepEqual(roles, { human: [], lead: [], zeus: ['worker'], hera: ['reviewer'] })
        assert.deepEqual(
          ledger.members(1, 'reviewer').map((m) => m.handle),
          ['hera'],
          'the review policy still has its reviewer',
        )
        const question = ledger.ask(1, { from: 'lead', to: 'human', body: 'Still here?' })
        assert.equal(question.questions, null)
      } finally {
        ledger.close()
      }
      const upgraded = new DatabaseSync(file)
      assert.equal(upgraded.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION)
      upgraded.close()
    })
  })
})

describe('projects and participants', () => {
  it('starts a project with the human and its lead, each with a lane', async () => {
    await withLedger((ledger) => {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        lead: { harness: 'claude-code' },
      })
      assert.equal(project.state, 'open')
      assert.equal(project.resumeOnStart, false)
      assert.deepEqual(
        project.participants.map((p) => [p.handle, p.role, p.harness]),
        [
          ['human', 'human', null],
          ['lead', 'lead', 'claude-code'],
        ],
      )
    })
  })

  it('lets a member hold several roles, and asks for members by any of them', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      const both = ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        roles: ['worker', 'reviewer'],
        tier: 'standard',
        tags: [],
      })
      assert.deepEqual(
        [both.role, both.roles],
        ['worker', ['worker', 'reviewer']],
        'the first role leads',
      )
      assert.ok(ledger.members(project.id, 'reviewer').some((m) => m.handle === 'hera'))
      assert.ok(ledger.members(project.id, 'worker').some((m) => m.handle === 'hera'))
      assert.deepEqual(
        ledger.project(project.id).participants.find((p) => p.handle === 'zeus').roles,
        ['worker'],
        'one role given as before reads as a set of one',
      )
      const changed = ledger.setRoles(project.id, 'hera', ['reviewer'])
      assert.deepEqual([changed.role, changed.roles], ['reviewer', ['reviewer']])
      assert.ok(!ledger.members(project.id, 'worker').some((m) => m.handle === 'hera'))
      assert.throws(() => ledger.setRoles(project.id, 'hera', []), { code: 'invalid-role' })
      assert.throws(() => ledger.setRoles(project.id, 'hera', ['lead']), { code: 'invalid-role' })
      assert.throws(() => ledger.setRoles(project.id, 'lead', ['worker']), { code: 'not-a-member' })
      assert.deepEqual(ledger.lastTeam().find((m) => m.agent === 'hera').roles, ['reviewer'])
    })
  })

  it('requires a reviewer on the team for any review policy, and keeps the last one', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      assert.equal(ledger.project(project.id).review, 'none', 'no reviewer, so nothing is reviewed')
      assert.throws(() => ledger.setReview(project.id, 'members'), { code: 'no-reviewer' })
      assert.throws(
        () =>
          ledger.createProject({
            directory: '/work/other',
            name: 'other',
            lead: { harness: 'pi' },
            review: 'all',
            team: [{ agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' }],
          }),
        { code: 'no-reviewer' },
      )
      const withReviewer = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        lead: { harness: 'pi' },
        team: [{ agent: 'hera', harness: 'pi', roles: ['worker', 'reviewer'], tier: 'standard' }],
      })
      assert.equal(
        withReviewer.review,
        'members',
        'a reviewer on the team: members are reviewed by default',
      )
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'reviewer',
        tier: 'standard',
      })
      ledger.setReview(project.id, 'members')
      assert.throws(() => ledger.removeMember(project.id, 'hera'), { code: 'last-reviewer' })
      assert.throws(() => ledger.setRoles(project.id, 'hera', ['worker']), {
        code: 'last-reviewer',
      })
      ledger.setReview(project.id, 'none')
      assert.equal(ledger.removeMember(project.id, 'hera').member.leftAt !== null, true)
    })
  })

  it('tells no coordinator about a member joining, only a requester about work cancelled by one leaving', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const lead =
        ledger.currentConversation(id('lead')) ??
        ledger.startConversation(id('lead'), { harness: 'claude-code' })
      assert.ok(lead)
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      assert.equal(ledger.inbox(id('lead')).length, 0, 'no joining note')
      ledger.removeMember(project.id, 'hera')
      assert.equal(ledger.inbox(id('lead')).length, 0, 'nothing cancelled, nothing to say')
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      const given = ledger.createTask(project.id, { from: 'lead', to: 'hera', body: 'Lexer' })
      deliver(ledger, given.message)
      ledger.removeMember(project.id, 'hera')
      const notes = ledger.inbox(id('lead')).filter((m) => m.kind === 'note')
      assert.equal(notes.length, 1)
      assert.match(
        notes[0].body,
        /^@hera left the team; it takes no more tasks\. Cancelled with it: T-1\.$/,
      )
    })
  })

  it('adds members once each, and a PM as the second coordinator', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'zeus',
            harness: 'pi',
            role: 'worker',
            tier: 'standard',
          }),
        { code: 'member-exists' },
      )
      assert.throws(
        () => ledger.addMember(project.id, { agent: 'hera', harness: 'pi', role: 'lead' }),
        { code: 'invalid-role' },
      )
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'hera',
            harness: 'emacs',
            role: 'worker',
            tier: 'standard',
          }),
        { code: 'invalid-harness' },
      )
      const pm = ledger.addPm(project.id, { harness: 'codex' })
      assert.deepEqual([pm.handle, pm.role, pm.harness], ['pm', 'pm', 'codex'])
      assert.throws(() => ledger.addPm(project.id, { harness: 'pi' }), { code: 'member-exists' })
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'opencode',
        role: 'advisor',
        tier: 'standard',
      })
      assert.deepEqual(
        ledger.project(project.id).participants.map((p) => p.handle),
        ['human', 'lead', 'zeus', 'diana', 'pm', 'athena'],
      )
    })
  })

  it('reuses the previous project team for the next project', async () => {
    await withLedger((ledger) => {
      team(ledger)
      assert.deepEqual(ledger.lastTeam(), [
        { agent: 'zeus', harness: 'claude-code', role: 'worker', roles: ['worker'] },
        { agent: 'diana', harness: 'codex', role: 'worker', roles: ['worker'] },
      ])
    })
  })

  it('marks the projects that were open for resume after a restart, once', async () => {
    await withLedger((ledger) => {
      const open = team(ledger).project
      const suspended = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        lead: { harness: 'pi' },
      })
      ledger.setProjectState(suspended.id, 'suspended')

      assert.deepEqual(
        ledger.suspendForRestart().map((project) => project.id),
        [open.id],
      )
      assert.deepEqual(
        ledger.projects().map((s) => [s.id, s.state, s.resumeOnStart]),
        [
          [open.id, 'suspended', true],
          [suspended.id, 'suspended', false],
        ],
      )
      ledger.setProjectState(open.id, 'open')
      assert.equal(ledger.project(open.id).resumeOnStart, false)
      ledger.setProjectState(open.id, 'suspended')
      assert.equal(ledger.project(open.id).resumeOnStart, false)
    })
  })

  it('deletes a project with everything in it', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      ledger.deleteProject(project.id)
      assert.equal(ledger.project(project.id), null)
      assert.deepEqual(ledger.projects(), [])
      assert.deepEqual(ledger.events(project.id), [])
    })
  })
})

describe('the project team', () => {
  const notes = (ledger, participantId) =>
    ledger
      .inbox(participantId)
      .filter((message) => message.kind === 'note')
      .map((message) => [message.sender, message.body])
      .reverse()

  it('starts a project with the team it is given, or not at all', async () => {
    await withLedger((ledger) => {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        lead: { harness: 'pi' },
        team: [
          { agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' },
          { agent: 'athena', harness: 'opencode', role: 'advisor', tier: 'standard' },
        ],
      })
      assert.deepEqual(
        project.participants.map((p) => [p.handle, p.role, p.harness]),
        [
          ['human', 'human', null],
          ['lead', 'lead', 'pi'],
          ['zeus', 'worker', 'claude-code'],
          ['athena', 'advisor', 'opencode'],
        ],
      )
      assert.throws(
        () =>
          ledger.createProject({
            directory: '/work/other',
            name: 'other',
            lead: { harness: 'pi' },
            team: [{ agent: 'hera', harness: 'pi', role: 'lead' }],
          }),
        { code: 'invalid-role' },
      )
      assert.deepEqual(
        ledger.projects().map((s) => s.name),
        ['app'],
      )
    })
  })

  it('cancels the open work of a member who leaves, and nothing new reaches it', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const zeus = id('zeus')
      const first = ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      deliver(ledger, first.message)
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Lexer' })
      ledger.createTask(project.id, { from: 'lead', to: 'diana', body: 'Docs' })
      const note = ledger.note(project.id, { from: 'lead', to: 'zeus', body: 'Mind the tests' })
      ledger.beginDelivery(note.id)
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'human',
        body: 'Which parser?',
        task: 1,
      })

      const { member, cancelled } = ledger.removeMember(project.id, 'zeus')

      assert.equal(member.handle, 'zeus')
      assert.notEqual(member.leftAt, null)
      assert.deepEqual(cancelled, [1, 2])
      assert.deepEqual(
        ledger
          .board(project.id)
          .lanes.map((lane) => [lane.participant.handle, lane.tasks.map((task) => task.state)]),
        [
          ['human', []],
          ['lead', []],
          ['diana', ['queued']],
        ],
      )
      assert.deepEqual(
        [1, 2].map((number) => ledger.task(project.id, number).state),
        ['cancelled', 'cancelled'],
      )
      assert.deepEqual(
        ledger.inbox(zeus).map((message) => [message.kind, message.state]),
        [
          ['note', 'cancelled'],
          ['task', 'cancelled'],
          ['task', 'delivered'],
        ],
      )
      assert.equal(ledger.message(question.id).state, 'cancelled')
      assert.throws(
        () => ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'More' }),
        { code: 'member-left' },
      )
      assert.throws(() => ledger.note(project.id, { from: 'lead', to: 'zeus', body: 'Hi' }), {
        code: 'member-left',
      })
      assert.deepEqual(ledger.lastTeam(), [
        { agent: 'diana', harness: 'codex', role: 'worker', roles: ['worker'] },
      ])
      assert.deepEqual(
        ledger
          .events(project.id)
          .filter((event) => event.kind === 'member.left')
          .map((event) => event.data),
        [{ handle: 'zeus', cancelled: [1, 2] }],
      )
    })
  })

  it('answers and follow-ups never go to a member who left', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      const task = ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      deliver(ledger, task.message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      const question = ledger.ask(project.id, { from: 'zeus', to: 'lead', body: 'More?' })
      deliver(ledger, question)
      ledger.removeMember(project.id, 'zeus')

      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Again' }), {
        code: 'member-left',
      })
      assert.throws(() => ledger.answer(question.id, { from: 'lead', body: 'No' }), {
        code: 'member-left',
      })
      assert.throws(() => ledger.removeMember(project.id, 'zeus'), { code: 'member-left' })
    })
  })

  it('only team members leave', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      ledger.addPm(project.id, { harness: 'pi' })
      for (const handle of ['human', 'lead', 'pm']) {
        assert.throws(() => ledger.removeMember(project.id, handle), { code: 'not-a-member' })
      }
      assert.throws(() => ledger.removeMember(project.id, 'nobody'), {
        code: 'unknown-participant',
      })
    })
  })

  it('takes a member back in the role and harness it rejoins with', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const before = id('zeus')
      ledger.removeMember(project.id, 'zeus')
      const back = ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'pi',
        role: 'reviewer',
        tier: 'standard',
      })

      assert.deepEqual(
        [back.id, back.role, back.harness, back.leftAt],
        [before, 'reviewer', 'pi', null],
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Review' }).task.assignee,
        'zeus',
      )
      assert.deepEqual(
        ledger
          .events(project.id)
          .filter((event) => event.kind === 'member.added' && event.data.handle === 'zeus')
          .map((event) => event.data.rejoined ?? false),
        [false, true],
      )
    })
  })

  it('tells a running coordinator who joined or left, and whose tasks went with them', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const pm = ledger.addPm(project.id, { harness: 'pi' })
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'pi',
        role: 'advisor',
        tier: 'standard',
      })
      assert.deepEqual(notes(ledger, id('lead')), [], 'no window yet: its launch reads the team')
      assert.deepEqual(notes(ledger, pm.id), [])

      ledger.startConversation(id('lead'), { harness: 'claude-code' })
      ledger.startConversation(pm.id, { harness: 'pi' })
      ledger.addMember(project.id, {
        agent: 'apollo',
        harness: 'codex',
        role: 'worker',
        tier: 'standard',
      })
      ledger.addMember(project.id, {
        agent: 'metis',
        harness: 'codex',
        role: 'advisor',
        tier: 'standard',
      })
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      ledger.createTask(project.id, { from: 'pm', to: 'zeus', body: 'Estimate' })
      ledger.createTask(project.id, { from: 'human', to: 'zeus', body: 'Logo' })
      ledger.removeMember(project.id, 'zeus')

      assert.deepEqual(notes(ledger, id('lead')), [
        [null, '@zeus left the team; it takes no more tasks. Cancelled with it: T-1, T-2, T-3.'],
      ])
      assert.deepEqual(notes(ledger, pm.id), [
        [null, '@zeus left the team; it takes no more tasks. Cancelled with it: T-1, T-2, T-3.'],
      ])
      assert.deepEqual(notes(ledger, id('human')), [])
    })
  })
})

describe('conversations', () => {
  it('gives a participant one current conversation, and a native session to one conversation', async () => {
    await withLedger((ledger) => {
      const { id } = team(ledger)
      const first = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      const second = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      assert.notEqual(first.id, second.id)
      assert.equal(ledger.currentConversation(id('zeus')).id, second.id)

      ledger.bindConversation(second.id, 'native-1')
      assert.equal(ledger.currentConversation(id('zeus')).nativeSession, 'native-1')
      // Unique within a harness: two harnesses may mint the same string.
      const lead = ledger.startConversation(id('lead'), { harness: 'claude-code' })
      assert.throws(() => ledger.bindConversation(lead.id, 'native-1'), {
        code: 'native-session-taken',
      })
      const codex = ledger.startConversation(id('diana'), { harness: 'codex' })
      assert.equal(ledger.bindConversation(codex.id, 'native-1').nativeSession, 'native-1')
      ledger.endConversation(second.id)
      assert.equal(ledger.currentConversation(id('zeus')), null)
    })
  })
})

describe('tasks and the inbox queue', () => {
  it('creates a task and queues it for its assignee in one step', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      const { task, message } = ledger.createTask(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Write the parser\nwith tests',
      })
      assert.deepEqual(
        [task.number, task.title, task.state, task.requester, task.assignee],
        [1, 'Write the parser', 'queued', 'lead', 'zeus'],
      )
      assert.deepEqual(
        [message.kind, message.state, message.recipient, message.sender, message.taskNumber],
        ['task', 'queued', 'zeus', 'lead', 1],
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'lead', to: 'diana', body: 'Docs' }).task.number,
        2,
      )
    })
  })

  it('refuses a task for someone outside the project and writes nothing', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      const before = ledger.events(project.id).length
      assert.throws(
        () => ledger.createTask(project.id, { from: 'lead', to: 'ghost', body: 'Boo' }),
        { code: 'unknown-participant' },
      )
      assert.throws(() => ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: '  ' }), {
        code: 'invalid-text',
      })
      assert.deepEqual(
        ledger.board(project.id).lanes.flatMap((lane) => lane.tasks),
        [],
      )
      assert.equal(ledger.events(project.id).length, before)
    })
  })

  it('delivers one message at a time per recipient, oldest first', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const first = ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'One' }).message
      const second = ledger.createTask(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Two',
      }).message
      assert.equal(ledger.nextDelivery(id('zeus')).id, first.id)
      ledger.beginDelivery(first.id)
      assert.equal(ledger.nextDelivery(id('zeus')), null, 'one delivery at a time')
      assert.throws(() => ledger.beginDelivery(second.id), { code: 'recipient-busy' })
      const confirmed = ledger.confirmDelivery(first.id, { evidence: 'native-1' })
      assert.equal(confirmed.state, 'delivered')
      assert.deepEqual(confirmed.receipt, { evidence: 'native-1' })
      assert.equal(ledger.task(project.id, 1).state, 'working')
      assert.equal(ledger.nextDelivery(id('zeus')), null, 'a second task waits for the first')
    })
  })

  it('hands a coordinator a new task while an earlier one is still open', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship v2' }).message,
      )
      const second = ledger.createTask(project.id, {
        from: 'human',
        to: 'lead',
        body: 'Also fix the docs',
      })
      assert.equal(ledger.nextDelivery(id('lead')).id, second.message.id)
      deliver(ledger, second.message)
      assert.equal(ledger.task(project.id, 2).state, 'working')
    })
  })

  it('still delivers answers and notes about its task to a worker busy with it, and nothing about no task', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'One' }).message,
      )
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Two' })
      const stray = ledger.note(project.id, { from: 'lead', to: 'zeus', body: 'Hello there' })
      assert.equal(ledger.nextDelivery(id('zeus')), null, "a member's session is its task's")
      const note = ledger.note(project.id, { from: 'lead', to: 'zeus', task: 1, body: 'Use JSON' })
      assert.equal(ledger.nextDelivery(id('zeus')).id, note.id)
      assert.equal(ledger.message(stray.id).state, 'queued')
    })
  })

  it('retries a delivery, then fails it and its task', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const { message } = ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'One' })
      ledger.beginDelivery(message.id)
      const retried = ledger.retryDelivery(message.id, 'harness not idle')
      assert.deepEqual(
        [retried.state, retried.attempts, retried.reason],
        ['queued', 1, 'harness not idle'],
      )
      assert.equal(ledger.nextDelivery(id('zeus')).id, message.id)
      ledger.beginDelivery(message.id)
      const failed = ledger.failDelivery(message.id, 'no receipt')
      assert.deepEqual([failed.state, failed.attempts], ['failed', 2])
      assert.equal(ledger.task(project.id, 1).state, 'failed')
      assert.equal(ledger.nextDelivery(id('zeus')), null)
    })
  })

  it('finishes a task with its result and queues the result for the requester', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      const { task, message } = ledger.recordResult(project.id, 1, { body: 'Parser done' })
      assert.equal(task.state, 'done')
      assert.deepEqual(
        [message.kind, message.recipient, message.sender, message.body, message.taskNumber],
        ['result', 'lead', 'zeus', 'Parser done', 1],
      )
      assert.equal(ledger.nextDelivery(id('lead')).id, message.id)
      assert.throws(() => ledger.recordResult(project.id, 1, { body: 'again' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('makes a task wait for a question and resumes it when the answer is delivered', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'lead',
        task: 1,
        body: 'Which format?',
      })
      assert.deepEqual([question.kind, question.recipient], ['question', 'lead'])
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      deliver(ledger, question)
      const answer = ledger.answer(question.id, { from: 'lead', body: 'JSON' })
      assert.deepEqual(
        [answer.kind, answer.recipient, answer.replyTo, answer.taskNumber],
        ['answer', 'zeus', question.id, 1],
      )
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      deliver(ledger, answer)
      assert.equal(ledger.task(project.id, 1).state, 'working')
      assert.throws(() => ledger.answer(answer.id, { from: 'zeus', body: 'thanks' }), {
        code: 'not-a-question',
      })
      assert.throws(() => ledger.ask(project.id, { to: 'lead', body: 'Who asks?' }), {
        code: 'unknown-participant',
      })
    })
  })

  const OPTIONS = [
    {
      question: 'Which colour?',
      header: 'Colour',
      options: [{ label: 'red', description: 'Warm' }, { label: 'blue' }],
    },
    { question: 'Ship it?', header: 'Ship', options: [{ label: 'yes' }, { label: 'no' }] },
  ]
  const NORMALIZED = [
    {
      question: 'Which colour?',
      header: 'Colour',
      options: [
        { label: 'red', description: 'Warm' },
        { label: 'blue', description: null },
      ],
      multiple: false,
    },
    {
      question: 'Ship it?',
      header: 'Ship',
      options: [
        { label: 'yes', description: null },
        { label: 'no', description: null },
      ],
      multiple: false,
    },
  ]

  /** Zeus on T-1, asking the lead a question with options. */
  function asked(ledger, questions = OPTIONS) {
    const { project, id } = team(ledger)
    deliver(
      ledger,
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
    )
    const question = ledger.ask(project.id, { from: 'zeus', to: 'lead', task: 1, questions })
    deliver(ledger, question)
    return { project, id, question }
  }

  it('asks with options: the question reads as text, keeps the options, and a bad shape is refused', async () => {
    await withLedger((ledger) => {
      const { project, question } = asked(ledger)
      assert.deepEqual(question.questions, NORMALIZED)
      assert.equal(
        question.body,
        'Colour: Which colour?\n- red: Warm\n- blue\n\nShip: Ship it?\n- yes\n- no',
      )
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      for (const bad of [
        [],
        [{ question: 'x' }],
        [{ question: 'x', header: 'h', options: 'none' }],
        [{ question: 'x', header: 'h', options: [{ description: 'no label' }] }],
        Array.from({ length: 5 }, () => OPTIONS[0]),
      ]) {
        assert.throws(
          () => ledger.ask(project.id, { from: 'zeus', to: 'lead', task: 1, questions: bad }),
          { code: 'bad-questions' },
        )
      }
    })
  })

  it('answers a question with options by choice: the answer is read at once, never delivered, and the task resumes', async () => {
    await withLedger((ledger) => {
      const { project, id, question } = asked(ledger)
      assert.equal(ledger.answerTo(question.id), null)
      const answer = ledger.answer(question.id, { from: 'lead', choices: [['blue'], ['yes']] })
      assert.deepEqual(
        [answer.kind, answer.recipient, answer.replyTo, answer.state, answer.choices, answer.body],
        ['answer', 'zeus', question.id, 'read', [['blue'], ['yes']], 'Colour: blue\nShip: yes'],
      )
      assert.equal(ledger.task(project.id, 1).state, 'working', 'the door resumes the task')
      assert.equal(ledger.nextDelivery(id('zeus')), null, 'nothing is pasted into the window')
      assert.deepEqual(ledger.answerTo(question.id).choices, [['blue'], ['yes']])
      assert.throws(
        () => ledger.answer(question.id, { from: 'lead', choices: [['red'], ['no']] }),
        {
          code: 'already-answered',
        },
      )
    })
  })

  it('keeps a question asked before the task arrived on that task, which then arrives waiting', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const { message } = ledger.createTask(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Parser',
      })
      assert.equal(
        ledger.activeTask(id('zeus'), { queued: true }).number,
        1,
        'the window may be up',
      )
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'lead',
        task: 1,
        questions: OPTIONS,
      })
      assert.equal(ledger.task(project.id, 1).state, 'queued')
      deliver(ledger, message)
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      ledger.answer(question.id, { from: 'lead', choices: [['red'], ['no']] })
      assert.equal(ledger.task(project.id, 1).state, 'working')
    })
  })

  it('lets the asker answer its own question with options, when its window answered first', async () => {
    await withLedger((ledger) => {
      const { project, question } = asked(ledger)
      const answer = ledger.answer(question.id, { from: 'zeus', choices: [['red'], ['no']] })
      assert.deepEqual([answer.sender, answer.recipient, answer.state], ['zeus', 'zeus', 'read'])
      assert.equal(ledger.task(project.id, 1).state, 'working')
      const plain = ledger.ask(project.id, { from: 'zeus', to: 'lead', task: 1, body: 'Plain?' })
      assert.throws(() => ledger.answer(plain.id, { from: 'zeus', body: 'Me' }), {
        code: 'not-your-question',
      })
    })
  })

  it('maps a text answer onto the options, one line per question, and keeps free text', async () => {
    await withLedger((ledger) => {
      const { question } = asked(ledger)
      assert.throws(() => ledger.answer(question.id, { from: 'lead', body: 'blue' }), {
        code: 'bad-choices',
      })
      const answer = ledger.answer(question.id, { from: 'lead', body: 'BLUE\nmaybe later' })
      assert.deepEqual(answer.choices, [['blue'], ['maybe later']])
      assert.equal(answer.body, 'Colour: blue\nShip: maybe later')
    })
  })

  it('takes several labels for a question that allows them', async () => {
    await withLedger((ledger) => {
      const { question } = asked(ledger, [
        {
          question: 'Which?',
          header: 'Which',
          options: [{ label: 'a' }, { label: 'b' }, { label: 'c' }],
          multiple: true,
        },
      ])
      assert.throws(
        () => ledger.answer(question.id, { from: 'lead', choices: [['a'], ['b']] }),
        { code: 'bad-choices' },
        'one array of labels per question',
      )
      const answer = ledger.answer(question.id, { from: 'lead', body: 'a, C' })
      assert.deepEqual([answer.choices, answer.body], [[['a', 'c']], 'Which: a, c'])
    })
  })

  it("lets the human answer any question, and shows a coordinator's unanswered question as overdue", async () => {
    await withDir((dir) => {
      let at = Date.parse('2026-09-19T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: () => new Date(at) })
      try {
        const { project } = team(ledger)
        deliver(
          ledger,
          ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
        )
        const question = ledger.ask(project.id, {
          from: 'zeus',
          to: 'lead',
          task: 1,
          body: 'Which format?',
        })
        at += OVERDUE_MS - 1000
        assert.deepEqual(ledger.board(project.id).overdue, [])
        at += 2000
        assert.deepEqual(
          ledger.board(project.id).overdue.map((m) => [m.id, m.recipient]),
          [[question.id, 'lead']],
        )
        const answer = ledger.answer(question.id, { from: 'human', body: 'JSON' })
        assert.deepEqual(
          [answer.recipient, answer.state, answer.choices],
          ['zeus', 'queued', null],
          'a plain question is answered in text, delivered as before',
        )
        assert.deepEqual(ledger.board(project.id).overdue, [])
        assert.throws(() => ledger.answer(question.id, { from: 'diana', body: 'CSV' }), {
          code: 'not-your-question',
        })
      } finally {
        ledger.close()
      }
    })
  })

  it('keeps messages for the human in the human inbox until they are read', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const question = ledger.ask(project.id, { from: 'lead', to: 'human', body: 'Deploy now?' })
      assert.equal(ledger.nextDelivery(id('human')), null, 'the human reads in the app')
      assert.throws(() => ledger.beginDelivery(question.id), { code: 'human-reads-in-app' })
      assert.deepEqual(
        ledger.inbox(id('human')).map((m) => [m.id, m.state]),
        [[question.id, 'queued']],
      )
      assert.equal(ledger.markRead(question.id).state, 'read')
      const answer = ledger.answer(question.id, { from: 'human', body: 'Yes' })
      assert.equal(ledger.nextDelivery(id('lead')).id, answer.id)
    })
  })

  it('lets the human take a task by reading it and finish it with a result', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      const { message } = ledger.createTask(project.id, {
        from: 'lead',
        to: 'human',
        body: 'Try the login',
      })
      ledger.markRead(message.id)
      assert.equal(ledger.task(project.id, 1).state, 'working')
      assert.equal(ledger.recordResult(project.id, 1, { body: 'Works' }).message.recipient, 'lead')
    })
  })

  it('follows the task state machine for accept, reopen, cancel and fail', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      ledger.recordResult(project.id, 1, { body: 'done' })
      const reopened = ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Add tests' })
      assert.equal(reopened.task.state, 'queued')
      assert.deepEqual([reopened.message.kind, reopened.message.recipient], ['task', 'zeus'])
      deliver(ledger, reopened.message)
      ledger.recordResult(project.id, 1, { body: 'tests added' })
      assert.equal(ledger.acceptTask(project.id, 1, { by: 'lead' }).state, 'accepted')
      assert.throws(() => ledger.acceptTask(project.id, 1, { by: 'lead' }), {
        code: 'invalid-transition',
      })
      assert.throws(() => ledger.cancelTask(project.id, 1, { by: 'lead' }), {
        code: 'invalid-transition',
      })

      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Lexer' })
      assert.equal(ledger.cancelTask(project.id, 2, { by: 'lead' }).state, 'cancelled')
      assert.equal(ledger.nextDelivery(id('zeus')), null, 'a cancelled task is never delivered')

      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'diana', body: 'Docs' }).message,
      )
      assert.equal(ledger.failTask(project.id, 3, { reason: 'pane exited' }).state, 'failed')
      assert.equal(
        ledger.reopenTask(project.id, 3, { by: 'lead', body: 'Retry' }).task.state,
        'queued',
      )
    })
  })

  it('writes every change into the project log, in order', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      ledger.recordResult(project.id, 1, { body: 'done' })
      const kinds = ledger.events(project.id).map((event) => event.kind)
      const expected = [
        'project.created',
        'member.added',
        'task.created',
        'delivery.begun',
        'delivery.confirmed',
        'task.state',
        'task.state',
      ]
      let at = 0
      for (const kind of kinds) if (kind === expected[at]) at += 1
      assert.equal(at, expected.length, `log order: ${kinds.join(', ')}`)
      const after = ledger.events(project.id).at(-2).id
      assert.deepEqual(
        ledger.events(project.id, { after }).map((event) => event.kind),
        ['task.state'],
      )
    })
  })
})

describe('views', () => {
  it('draws a lane per participant, in join order, with the tasks assigned to it', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship v2' })
      const board = ledger.board(project.id)
      assert.equal(board.project.id, project.id)
      assert.deepEqual(
        board.lanes.map((lane) => [lane.participant.handle, lane.tasks.map((t) => t.number)]),
        [
          ['human', []],
          ['lead', [2]],
          ['zeus', [1]],
          ['diana', []],
        ],
      )
    })
  })

  it('shows a task with its whole thread, oldest first', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'lead',
        task: 1,
        body: 'Format?',
      })
      ledger.answer(question.id, { from: 'lead', body: 'JSON' })
      assert.deepEqual(
        ledger.task(project.id, 1).messages.map((m) => [m.kind, m.sender, m.recipient]),
        [
          ['task', 'lead', 'zeus'],
          ['question', 'zeus', 'lead'],
          ['answer', 'lead', 'zeus'],
        ],
      )
      assert.equal(ledger.task(project.id, 99), null)
    })
  })

  it('names the task a participant is on, with its thread', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      assert.equal(ledger.activeTask(id('zeus')), null)
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'One' })
      assert.equal(ledger.activeTask(id('zeus')), null, 'a queued task is not started')
      assert.equal(ledger.activeTask(id('zeus'), { queued: true }).state, 'queued')
      deliver(ledger, ledger.task(project.id, 1).messages[0])
      assert.deepEqual(
        [ledger.activeTask(id('zeus')).number, ledger.activeTask(id('zeus')).messages.length],
        [1, 1],
      )
      ledger.ask(project.id, { from: 'zeus', to: 'lead', task: 1, body: 'Format?' })
      assert.equal(ledger.activeTask(id('zeus')).state, 'waiting')
    })
  })

  it('lists an inbox newest first', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const one = ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'one' })
      const two = ledger.note(project.id, { from: 'diana', to: 'lead', body: 'two' })
      assert.deepEqual(
        ledger.inbox(id('lead')).map((m) => m.id),
        [two.id, one.id],
      )
    })
  })
})

describe('tiered dispatch: open tasks the daemon assigns', () => {
  /** A lead with two standard workers, a light worker, a PM and a complex advisor. */
  function tiered(ledger) {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      lead: { harness: 'claude-code' },
    })
    const add = (agent, harness, role, tier, tags) =>
      ledger.addMember(project.id, { agent, harness, role, tier, tags })
    add('zeus', 'claude-code', 'worker', 'standard', ['coding', 'rust'])
    add('diana', 'codex', 'worker', 'standard', ['coding'])
    add('hera', 'pi', 'worker', 'light', [])
    ledger.addPm(project.id, { harness: 'pi' })
    add('athena', 'opencode', 'advisor', 'complex', ['research'])
    add('nemesis', 'pi', 'reviewer', 'standard', [])
    const id = (handle) =>
      ledger.project(project.id).participants.find((p) => p.handle === handle).id
    return { project, id }
  }
  const openTask = (ledger, project, extra = {}) =>
    ledger.createTask(project.id, {
      from: 'lead',
      pool: 'worker',
      tier: 'standard',
      body: 'Write the parser',
      ...extra,
    })

  it("records each member's tier and tags, and refuses a member without a tier", async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      assert.deepEqual(
        ledger
          .project(project.id)
          .participants.filter((p) => p.role !== 'human')
          .map((p) => [p.handle, p.tier, p.tags]),
        [
          ['lead', null, []],
          ['zeus', 'standard', ['coding', 'rust']],
          ['diana', 'standard', ['coding']],
          ['hera', 'light', []],
          ['pm', null, []],
          ['athena', 'complex', ['research']],
          ['nemesis', 'standard', []],
        ],
      )
      assert.throws(
        () => ledger.addMember(project.id, { agent: 'apollo', harness: 'pi', role: 'worker' }),
        { code: 'invalid-tier' },
      )
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'apollo',
            harness: 'pi',
            role: 'worker',
            tier: 'max',
          }),
        { code: 'invalid-tier' },
      )
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'apollo',
            harness: 'pi',
            role: 'worker',
            tier: 'light',
            tags: 'coding',
          }),
        { code: 'invalid-tags' },
      )
      const next = ledger.createProject({
        directory: '/work/api',
        name: 'api',
        lead: { harness: 'pi' },
        team: [
          { agent: 'zeus', harness: 'pi', role: 'reviewer', tier: 'critical', tags: ['review'] },
        ],
      })
      assert.deepEqual(
        next.participants.at(-1) && [next.participants.at(-1).tier, next.participants.at(-1).tags],
        ['critical', ['review']],
      )
    })
  })

  it("opens a task for a tier instead of a member; it sits in nobody's lane", async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      const { task, message } = openTask(ledger, project, { tags: ['rust'] })
      assert.deepEqual(
        [task.state, task.assignee, task.pool, task.tier, task.tags, task.requester, message],
        ['open', null, 'worker', 'standard', ['rust'], 'lead', null],
      )
      const board = ledger.board(project.id)
      assert.deepEqual(
        board.open.map((t) => t.number),
        [1],
      )
      assert.deepEqual(
        board.lanes.flatMap((lane) => lane.tasks),
        [],
      )
      assert.equal(ledger.task(project.id, 1).messages.length, 0)
      assert.deepEqual(ledger.events(project.id).at(-1).data, {
        task: 1,
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        tags: ['rust'],
      })
    })
  })

  it('refuses what no member could take: a bad tier or pool, an empty tier, critical work without its purpose', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      const refuses = (extra, code) =>
        assert.throws(() => openTask(ledger, project, extra), { code })
      refuses({ tier: 'max' }, 'invalid-tier')
      refuses({ pool: 'lead' }, 'invalid-pool')
      refuses({ tier: 'critical', purpose: 'architecture' }, 'no-member-of-tier')
      refuses({ pool: 'worker', tier: 'complex' }, 'no-member-of-tier')
      refuses({ tags: ['ok', 'not a tag'] }, 'invalid-tags')
      ledger.addMember(project.id, {
        agent: 'calliope',
        harness: 'claude-code',
        role: 'worker',
        tier: 'critical',
        tags: ['architecture'],
      })
      refuses({ tier: 'critical' }, 'purpose-required')
      refuses({ tier: 'critical', purpose: 'coding' }, 'purpose-required')
      const { task } = openTask(ledger, project, { tier: 'critical', purpose: 'architecture' })
      assert.deepEqual([task.tier, task.purpose], ['critical', 'architecture'])
      assert.deepEqual(
        ledger.board(project.id).open.map((t) => t.number),
        [task.number],
        'the refusals created nothing',
      )
    })
  })

  it('lets only coordinators and the human create tasks', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      assert.throws(() => openTask(ledger, project, { from: 'zeus' }), {
        code: 'not-a-coordinator',
      })
      assert.throws(
        () => ledger.createTask(project.id, { from: 'zeus', to: 'lead', body: 'Do it' }),
        { code: 'not-a-coordinator' },
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'pm', to: 'lead', body: 'Plan' }).task.assignee,
        'lead',
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'lead', to: 'lead', body: 'My own' }).task.assignee,
        'lead',
      )
      assert.equal(
        ledger.createTask(project.id, {
          from: 'human',
          pool: 'advisor',
          tier: 'complex',
          body: 'Look',
        }).task.pool,
        'advisor',
      )
    })
  })

  it('counts a member as holding its work from assignment to the verdict, busy to the assigner meanwhile', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      ledger.setReview(project.id, 'members')
      openTask(ledger, project)
      const busy = () => ledger.members(project.id, 'worker').find((m) => m.handle === 'zeus').busy
      assert.deepEqual([ledger.holdsWork(id('zeus')), busy()], [false, false], 'nothing yet')
      ledger.assignTask(project.id, 1, id('zeus'))
      assert.deepEqual([ledger.holdsWork(id('zeus')), busy()], [true, true], 'queued')
      const brief = ledger.task(project.id, 1).messages.find((m) => m.kind === 'task')
      ledger.beginDelivery(brief.id)
      ledger.confirmDelivery(brief.id, { item: 'i-1' })
      assert.deepEqual([ledger.holdsWork(id('zeus')), busy()], [true, true], 'working')
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      assert.equal(ledger.task(project.id, 1).state, 'review')
      assert.deepEqual([ledger.holdsWork(id('zeus')), busy()], [true, true], 'under review')
      ledger.skipReview(project.id, 1, { reason: 'no reviewer' })
      assert.equal(ledger.task(project.id, 1).state, 'done')
      assert.deepEqual([ledger.holdsWork(id('zeus')), busy()], [false, false], 'done')
    })
  })

  it('lists the members a task may go to, with their tags and how many tasks each has taken', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      assert.deepEqual(
        ledger
          .candidates(project.id, 1)
          .map((c) => [c.handle, c.tier, c.tags, c.taken, c.outUntil]),
        [
          ['zeus', 'standard', ['coding', 'rust'], 0, null],
          ['diana', 'standard', ['coding'], 0, null],
        ],
      )
      ledger.assignTask(project.id, 1, id('zeus'))
      openTask(ledger, project)
      ledger.markOut(id('diana'), { until: '2026-09-19T18:00:00.000Z', reason: 'out of quota' })
      assert.deepEqual(
        ledger.candidates(project.id, 2).map((c) => [c.handle, c.taken, c.outUntil]),
        [
          ['zeus', 1, null],
          ['diana', 0, '2026-09-19T18:00:00.000Z'],
        ],
      )
      assert.equal(
        ledger.project(project.id).participants.find((p) => p.handle === 'diana').outUntil,
        '2026-09-19T18:00:00.000Z',
      )
      ledger.removeMember(project.id, 'zeus')
      assert.deepEqual(
        ledger.candidates(project.id, 2).map((c) => c.handle),
        ['diana'],
      )
    })
  })

  it("lists a role's members with what the daemon ranks them by, busy or not", async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      ledger.assignTask(project.id, 1, id('zeus'))
      assert.deepEqual(
        ledger.members(project.id, 'worker').map((m) => [m.handle, m.tier, m.busy, m.taken]),
        [
          ['zeus', 'standard', true, 1],
          ['diana', 'standard', false, 0],
          ['hera', 'light', false, 0],
        ],
      )
      assert.deepEqual(
        ledger.members(project.id, 'advisor').map((m) => m.handle),
        ['athena'],
      )
      assert.deepEqual(
        ledger.members(project.id, 'reviewer').map((m) => m.handle),
        ['nemesis'],
      )
      deliver(ledger, ledger.task(project.id, 1).messages[0])
      ledger.setReview(project.id, 'members')
      ledger.recordResult(project.id, 1, { body: 'Done' })
      assert.equal(
        ledger.members(project.id, 'worker')[0].busy,
        true,
        'work under review is still on its hands',
      )
      ledger.skipReview(project.id, 1, { reason: 'no reviewer' })
      assert.equal(ledger.members(project.id, 'worker')[0].busy, false, 'finished work is not busy')
    })
  })

  it('withdraws a review so another reviewer can take it', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      ledger.setReview(project.id, 'members')
      ledger.addMember(project.id, {
        agent: 'apollo',
        harness: 'pi',
        role: 'reviewer',
        tier: 'standard',
      })
      openTask(ledger, project)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      const review = ledger.createReview(project.id, 1, { reviewer: id('apollo') })
      deliver(ledger, review.message)
      const withdrawn = ledger.withdrawReview(project.id, review.task.number, {
        reason: "@apollo's window closed",
      })
      assert.deepEqual([withdrawn.state, ledger.task(project.id, 1).state], ['cancelled', 'review'])
      assert.deepEqual(
        ledger.reviewsPending(project.id).map((t) => t.number),
        [1],
      )
      assert.throws(() => ledger.withdrawReview(project.id, 1, { reason: 'x' }), {
        code: 'not-a-review',
      })
      assert.throws(() => ledger.withdrawReview(project.id, review.task.number, { reason: 'x' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('assigns an open task: it is queued for the member and the log says who was picked', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      const { task, message } = ledger.assignTask(project.id, 1, id('zeus'))
      assert.deepEqual([task.state, task.assignee], ['queued', 'zeus'])
      assert.deepEqual(
        [message.recipient, message.kind, message.state, message.body],
        ['zeus', 'task', 'queued', 'Write the parser'],
      )
      assert.equal(ledger.nextDelivery(id('zeus')).id, message.id)
      assert.deepEqual(ledger.events(project.id).at(-1), {
        ...ledger.events(project.id).at(-1),
        kind: 'task.assigned',
        data: { task: 1, from: 'open', to: 'queued', assignee: 'zeus', message: message.id },
      })
      assert.throws(() => ledger.assignTask(project.id, 1, id('diana')), {
        code: 'invalid-transition',
      })
      openTask(ledger, project, { tier: 'light' })
      assert.throws(() => ledger.assignTask(project.id, 2, id('zeus')), { code: 'not-a-candidate' })
      assert.throws(() => ledger.assignTask(project.id, 2, id('athena')), {
        code: 'not-a-candidate',
      })
      assert.equal(ledger.assignTask(project.id, 2, id('hera')).task.assignee, 'hera')
      const board = ledger.board(project.id)
      assert.deepEqual(board.open, [])
      assert.deepEqual(
        board.lanes.find((lane) => lane.participant.handle === 'hera').tasks.map((t) => t.number),
        [2],
      )
    })
  })

  it('leads critical work with its purpose when it is delivered', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      ledger.addMember(project.id, {
        agent: 'calliope',
        harness: 'claude-code',
        role: 'worker',
        tier: 'critical',
        tags: [],
      })
      openTask(ledger, project, {
        tier: 'critical',
        purpose: 'hard-problem',
        body: 'Why is it slow?',
      })
      const { message } = ledger.assignTask(project.id, 1, id('calliope'))
      assert.match(
        message.body,
        /^Critical work: hard-problem\. No coding or implementation edits\./,
      )
      assert.match(message.body, /\n\nWhy is it slow\?$/)
    })
  })

  it('takes a task back from a member that ran out, tells the requester, and hands the next member a warning', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      const first = ledger.assignTask(project.id, 1, id('zeus'))
      deliver(ledger, first.message)
      assert.equal(ledger.task(project.id, 1).state, 'working')
      const note = ledger.note(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Mind the tests',
        task: 1,
      })

      const { task } = ledger.releaseTask(project.id, 1, {
        because: 'ran out of quota after starting',
      })
      assert.deepEqual([task.state, task.assignee], ['open', null])
      assert.equal(ledger.message(note.id).state, 'cancelled')
      assert.equal(ledger.message(first.message.id).state, 'delivered', 'history stays')
      assert.deepEqual(ledger.events(project.id).at(-2).data, {
        task: 1,
        from: 'working',
        to: 'open',
        member: 'zeus',
        because: 'ran out of quota after starting',
      })
      const told = ledger.inbox(id('lead'))[0]
      assert.deepEqual(
        [told.kind, told.taskNumber, told.body],
        [
          'note',
          1,
          'T-1 was taken back from @zeus (ran out of quota after starting) and waits for another standard worker.',
        ],
      )
      assert.equal(ledger.activeTask(id('zeus')), null)

      const second = ledger.assignTask(project.id, 1, id('diana'))
      assert.equal(
        second.message.body,
        'Write the parser\n\nReassigned from @zeus, which ran out of quota after starting; check the working tree for partial changes.',
      )
      deliver(ledger, second.message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      assert.throws(() => ledger.releaseTask(project.id, 1, { because: 'x' }), {
        code: 'invalid-transition',
      })
      const direct = ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship' })
      assert.throws(() => ledger.releaseTask(project.id, direct.task.number, { because: 'x' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('cancels an open task; a task queued by handle still follows the old path', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      assert.equal(ledger.cancelTask(project.id, 1, { by: 'lead' }).state, 'cancelled')
      assert.equal(ledger.task(project.id, 1).messages.length, 0)
      const direct = ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship it' })
      assert.deepEqual(
        [direct.task.state, direct.task.pool, direct.task.tier],
        ['queued', null, null],
      )
      assert.equal(ledger.nextDelivery(id('lead')).id, direct.message.id)
    })
  })
})

describe('tiered dispatch: the review gate', () => {
  /** A lead, a standard worker and two reviewers; the daemon would pick the reviewer. */
  function reviewed(ledger, review = 'members') {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      lead: { harness: 'claude-code' },
    })
    const add = (agent, harness, role) =>
      ledger.addMember(project.id, { agent, harness, role, tier: 'standard', tags: [] })
    add('zeus', 'claude-code', 'worker')
    add('diana', 'codex', 'reviewer')
    add('calliope', 'claude-code', 'reviewer')
    // The policy comes once someone can review: without a reviewer it is refused.
    ledger.setReview(project.id, review)
    const id = (handle) =>
      ledger.project(project.id).participants.find((p) => p.handle === handle).id
    return { project, id }
  }
  /** A worker's task from open to its result: assigned, delivered, answered. */
  function finished(ledger, project, id, body = 'Parser done') {
    ledger.createTask(project.id, {
      from: 'lead',
      pool: 'worker',
      tier: 'standard',
      body: 'Parser',
    })
    const number = ledger.board(project.id).open.at(-1).number
    deliver(ledger, ledger.assignTask(project.id, number, id('zeus')).message)
    return { number, ...ledger.recordResult(project.id, number, { body }) }
  }
  const queued = (ledger, participantId) =>
    ledger
      .inbox(participantId)
      .filter((m) => m.state === 'queued')
      .reverse()
      .map((m) => [m.kind, m.taskNumber, m.body])

  it('reads the verdict through the emphasis a harness wraps it in', () => {
    for (const [body, expected] of [
      ['Fine.\n\nVERDICT: pass', 'pass'],
      ['Fine.\n\n**VERDICT: pass**', 'pass'],
      ['Fine.\n\nVerdict: **changes**', 'changes'],
      ['Fine.\n\n- VERDICT — changes.', 'changes'],
      ['Fine.\n\nverdict: passable', null],
      ['VERDICT: pass\n\nBut then more prose.', null],
    ])
      assert.equal(verdictOf(body), expected, body)
  })

  it("holds a member's result for review when the project asks, and releases it with the review on pass", async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger)
      assert.equal(ledger.project(project.id).review, 'members', 'the default')
      assert.throws(() => ledger.setReview(project.id, 'twice'), { code: 'invalid-review' })

      const { number, task, message } = finished(ledger, project, id)
      assert.deepEqual([task.state, task.round, message.state], ['review', 0, 'held'])
      assert.equal(ledger.nextDelivery(id('lead')), null, 'nothing reaches the requester yet')
      assert.deepEqual(
        ledger.reviewsPending(project.id).map((t) => t.number),
        [number],
      )

      const review = ledger.createReview(project.id, number, { reviewer: id('diana') })
      assert.deepEqual(
        [
          review.task.number,
          review.task.kind,
          review.task.reviewOf,
          review.task.assignee,
          review.task.requester,
          review.task.state,
          review.task.title,
        ],
        [2, 'review', 1, 'diana', 'lead', 'queued', 'Review T-1'],
      )
      assert.match(review.message.body, /^Review T-1 \(round 1\) by @zeus\./)
      assert.match(review.message.body, /The task:\nParser\n/)
      assert.match(review.message.body, /The result:\nParser done\n/)
      assert.match(review.message.body, /VERDICT: pass or VERDICT: changes\.$/)
      assert.deepEqual(ledger.reviewsPending(project.id), [])
      assert.throws(() => ledger.createReview(project.id, number, { reviewer: id('calliope') }), {
        code: 'invalid-transition',
      })

      deliver(ledger, review.message)
      assert.equal(ledger.activeTask(id('diana')).number, 2)
      const verdict = ledger.recordVerdict(project.id, 2, { body: 'Looks right.\n\nVERDICT: pass' })
      assert.deepEqual(
        [verdict.verdict, verdict.task.state, verdict.task.round, verdict.review.state],
        ['pass', 'done', 1, 'done'],
      )
      assert.equal(verdict.review.verdict, 'pass')
      assert.deepEqual(queued(ledger, id('lead')), [['result', 1, 'Parser done']], 'one delivery')
      assert.deepEqual(ledger.reviewsOf(project.id, 1), [
        {
          number: 2,
          round: 1,
          reviewer: 'diana',
          state: 'done',
          verdict: 'pass',
          findings: 'Looks right.\n\nVERDICT: pass',
        },
      ])
      assert.equal(
        ledger.task(project.id, 2).messages.find((m) => m.kind === 'result').state,
        'read',
        'the findings stay on the review task, for the board',
      )
      assert.deepEqual(ledger.events(project.id).find((e) => e.kind === 'review.verdict').data, {
        task: 1,
        review: 2,
        verdict: 'pass',
        round: 1,
      })
    })
  })

  it('refuses a reviewer that is not one, or one that wrote the work', async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger)
      const { number } = finished(ledger, project, id)
      assert.throws(() => ledger.createReview(project.id, number, { reviewer: id('zeus') }), {
        code: 'not-a-reviewer',
      })
      assert.throws(() => ledger.createReview(project.id, number, { reviewer: id('lead') }), {
        code: 'not-a-reviewer',
      })
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'reviewer',
        tier: 'light',
      })
      ledger.removeMember(project.id, 'hera')
      assert.throws(
        () => ledger.createReview(project.id, number, { reviewer: id('lead') - 0 + 99 }),
        {
          code: 'unknown-participant',
        },
      )
    })
  })

  it('sends the work back with the findings, and after a second round lets the requester decide', async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger)
      const { number, message: held } = finished(ledger, project, id)
      const first = ledger.createReview(project.id, number, { reviewer: id('diana') })
      deliver(ledger, first.message)
      const back = ledger.recordVerdict(project.id, first.task.number, {
        body: 'Missing tests.\n\nVERDICT: changes',
      })
      assert.deepEqual(
        [back.verdict, back.task.state, back.task.assignee, back.task.round, back.review.state],
        ['changes', 'queued', 'zeus', 1, 'done'],
      )
      assert.equal(ledger.message(held.id).state, 'cancelled', 'the first result is superseded')
      const followUp = ledger.nextDelivery(id('zeus'))
      assert.deepEqual(
        [followUp.kind, followUp.sender, followUp.taskNumber, followUp.body],
        [
          'task',
          'diana',
          number,
          'Review round 1 by @diana asks for changes:\n\nMissing tests.\n\nVERDICT: changes',
        ],
      )
      assert.deepEqual(queued(ledger, id('lead')), [], 'the requester sees nothing yet')
      assert.deepEqual(
        ledger.task(project.id, first.task.number).messages.map((m) => [m.kind, m.state]),
        [
          ['task', 'delivered'],
          ['result', 'read'],
        ],
        'its findings stay on the review task, undelivered',
      )

      deliver(ledger, followUp)
      const again = ledger.recordResult(project.id, number, { body: 'Tests added' })
      assert.deepEqual(
        [again.task.state, again.task.round, again.message.state],
        ['review', 1, 'held'],
      )
      assert.deepEqual(
        ledger.reviewsPending(project.id).map((t) => t.number),
        [number],
      )
      const second = ledger.createReview(project.id, number, { reviewer: id('calliope') })
      assert.match(second.message.body, /^Review T-1 \(round 2\) by @zeus\./)
      deliver(ledger, second.message)
      const decide = ledger.recordVerdict(project.id, second.task.number, {
        body: 'Still wrong.\n\nVERDICT: changes',
      })
      assert.deepEqual(
        [decide.verdict, decide.task.state, decide.task.round],
        ['changes', 'done', 2],
      )
      assert.deepEqual(
        queued(ledger, id('lead')),
        [['result', number, 'Tests added']],
        'one delivery',
      )
      assert.deepEqual(
        ledger.reviewsOf(project.id, number).map((r) => [r.round, r.verdict, r.findings]),
        [
          [1, 'changes', 'Missing tests.\n\nVERDICT: changes'],
          [2, 'changes', 'Still wrong.\n\nVERDICT: changes'],
        ],
      )
    })
  })

  it("reviews coordinators' own work only under the all policy, and nothing under none", async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger, 'all')
      const own = ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship it' })
      deliver(ledger, own.message)
      const done = ledger.recordResult(project.id, own.task.number, { body: 'Shipped' })
      assert.deepEqual([done.task.state, done.message.state], ['review', 'held'])

      ledger.setReview(project.id, 'none')
      assert.equal(ledger.project(project.id).review, 'none')
      const plain = finished(ledger, project, id)
      assert.deepEqual([plain.task.state, plain.message.state], ['done', 'queued'])

      ledger.setReview(project.id, 'members')
      const lead = ledger.createTask(project.id, { from: 'lead', to: 'lead', body: 'My plan' })
      deliver(ledger, lead.message)
      assert.equal(
        ledger.recordResult(project.id, lead.task.number, { body: 'Planned' }).task.state,
        'done',
      )
      assert.deepEqual(
        ledger
          .events(project.id)
          .filter((e) => e.kind === 'project.review')
          .map((e) => e.data),
        [
          { from: 'none', to: 'all' },
          { from: 'all', to: 'none' },
          { from: 'none', to: 'members' },
        ],
      )
    })
  })

  it('lets a coordinator ask for a review by hand, and skips a review nobody can do', async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger, 'none')
      const { number } = finished(ledger, project, id)
      assert.throws(() => ledger.requestReview(project.id, number, { by: 'zeus' }), {
        code: 'not-a-coordinator',
      })
      const asked = ledger.requestReview(project.id, number, { by: 'lead' })
      assert.equal(asked.state, 'review')
      assert.throws(() => ledger.requestReview(project.id, number, { by: 'lead' }), {
        code: 'invalid-transition',
      })
      assert.deepEqual(
        ledger.reviewsPending(project.id).map((t) => t.number),
        [number],
      )
      const skipped = ledger.skipReview(project.id, number, {
        reason: 'no independent reviewer on the team',
      })
      assert.equal(skipped.state, 'done')
      assert.deepEqual(queued(ledger, id('lead')), [['result', number, 'Parser done']])
      assert.equal(
        ledger.task(project.id, number).unreviewed,
        'no independent reviewer on the team',
      )

      ledger.setReview(project.id, 'members')
      const held = finished(ledger, project, id, 'Lexer done')
      assert.equal(held.message.state, 'held')
      ledger.skipReview(project.id, held.number, { reason: 'no independent reviewer on the team' })
      assert.equal(ledger.message(held.message.id).state, 'queued', 'the held result is released')
      assert.throws(() => ledger.skipReview(project.id, held.number, { reason: 'x' }), {
        code: 'invalid-transition',
      })
    })
  })

  it("gives the board each task's result line, and a task its reviews for the drawer", async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger)
      const zeusCard = () =>
        ledger.board(project.id).lanes.find((l) => l.participant.handle === 'zeus').tasks[0]
      const { number } = finished(
        ledger,
        project,
        id,
        '  \nParser done: 14 tests.\nMore lines follow.',
      )
      assert.equal(
        zeusCard().result,
        'Parser done: 14 tests.',
        'the first line that says something',
      )
      const review = ledger.createReview(project.id, number, { reviewer: id('diana') })
      deliver(ledger, review.message)
      ledger.recordVerdict(project.id, review.task.number, {
        body: 'Looks right.\n\nVERDICT: pass',
      })
      assert.deepEqual(ledger.task(project.id, number).reviews, [
        {
          number: review.task.number,
          round: 1,
          reviewer: 'diana',
          state: 'done',
          verdict: 'pass',
          findings: 'Looks right.\n\nVERDICT: pass',
        },
      ])
      assert.deepEqual(
        ledger.task(project.id, review.task.number).reviews,
        [],
        'a review has none of its own',
      )
      const open = ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Lexer',
      })
      assert.equal(
        ledger.board(project.id).open.find((t) => t.number === open.task.number).result,
        null,
      )
    })
  })

  it('reads a review with no verdict line as a pass, and says so', async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger)
      const { number } = finished(ledger, project, id)
      const review = ledger.createReview(project.id, number, { reviewer: id('diana') })
      deliver(ledger, review.message)
      const verdict = ledger.recordVerdict(project.id, review.task.number, { body: 'Fine by me.' })
      assert.deepEqual(
        [verdict.verdict, verdict.task.state, verdict.review.verdict],
        [null, 'done', null],
      )
      assert.equal(
        queued(ledger, id('lead')).at(-1)[1],
        number,
        'the work, not the review, is delivered',
      )
      assert.equal(
        ledger.reviewsOf(project.id, number)[0].findings,
        'No VERDICT line; read as pass.\n\nFine by me.',
      )
    })
  })

  it('takes an open review with its task when the task is cancelled, and finds another reviewer when one leaves', async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger)
      const { number } = finished(ledger, project, id)
      const review = ledger.createReview(project.id, number, { reviewer: id('diana') })
      assert.equal(ledger.cancelTask(project.id, number, { by: 'lead' }).state, 'cancelled')
      assert.deepEqual(
        [
          ledger.task(project.id, review.task.number).state,
          ledger.message(review.message.id).state,
        ],
        ['cancelled', 'cancelled'],
      )

      const next = finished(ledger, project, id, 'Lexer done')
      const again = ledger.createReview(project.id, next.number, { reviewer: id('diana') })
      ledger.removeMember(project.id, 'diana')
      assert.equal(ledger.task(project.id, again.task.number).state, 'cancelled')
      assert.equal(
        ledger.task(project.id, next.number).state,
        'review',
        'the work still waits for a review',
      )
      assert.deepEqual(
        ledger.reviewsPending(project.id).map((t) => t.number),
        [next.number],
      )
      assert.equal(
        ledger.createReview(project.id, next.number, { reviewer: id('calliope') }).task.assignee,
        'calliope',
      )
    })
  })
})
