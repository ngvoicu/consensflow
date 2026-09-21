import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { OVERDUE_MS, openLedger, SCHEMA_VERSION, TRANSCRIPT_ITEM_MAX } from '../src/ledger/index.js'

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

/** Session names in a fixed order, so a test can say `zeus-amber-pine` and mean the first one. */
function names() {
  const list = [
    'amber-pine',
    'brisk-birch',
    'calm-brook',
    'coral-canyon',
    'crisp-cedar',
    'dusky-cliff',
  ]
  let at = 0
  return () => list[at++ % list.length]
}

async function withLedger(fn) {
  return withDir(async (dir) => {
    const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: clock(), names: names() })
    try {
      return await fn(ledger, dir)
    } finally {
      ledger.close()
    }
  })
}

/** A project with a lead and two workers, the shape most tests start from. */
function team(ledger) {
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    lead: { harness: 'claude-code' },
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

/** The id of a session (or any participant) by handle. */
const sessionId = (ledger, projectId, handle) =>
  ledger.project(projectId).participants.find((p) => p.handle === handle).id

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

describe('deleting a project', () => {
  it('deletes a closed project with everything in it, and refuses an open one', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const { message } = ledger.createTask(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      ledger.ask(project.id, { from: 'zeus', to: 'lead', task: 1, body: 'Which?' })
      const other = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        lead: { harness: 'pi' },
      })
      const leadId = id('lead')
      assert.throws(() => ledger.deleteProject(project.id), { code: 'project-open' })
      ledger.setProjectState(project.id, 'suspended')
      assert.deepEqual(ledger.deleteProject(project.id), { id: project.id, name: 'app' })
      assert.deepEqual(
        ledger.projects().map((p) => p.name),
        ['other'],
        'the other project is untouched',
      )
      assert.equal(ledger.project(project.id), null)
      assert.throws(() => ledger.deleteProject(project.id), { code: 'unknown-project' })
      assert.equal(ledger.events(other.id).length > 0, true)
      assert.deepEqual(ledger.events(project.id), [], 'nothing of it is left')
      assert.equal(ledger.task(project.id, 1), null)
      assert.equal(ledger.inbox(leadId).length, 0, 'its messages went with it')
    })
  })
})

describe('the schema', () => {
  it('refuses what the model never holds, and keeps every reference whole', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedger(file, { now: clock(), names: names() })
      const { project, id } = team(ledger)
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      const lead = id('lead')
      const zeus = id('zeus')
      ledger.close()
      const raw = new DatabaseSync(file)
      raw.exec('PRAGMA foreign_keys = ON')
      const at = '2026-09-21T10:00:00.000Z'
      assert.throws(() => raw.prepare('UPDATE project SET gate = 2').run(), /CHECK/)
      assert.throws(() => raw.prepare("UPDATE task SET pool = 'judge'").run(), /CHECK/)
      assert.throws(() => raw.prepare("UPDATE task SET state = 'review'").run(), /CHECK/)
      assert.throws(() => raw.prepare('INSERT INTO task_need VALUES (1, 1)').run(), /CHECK/)
      assert.throws(() => raw.prepare('INSERT INTO task_need VALUES (1, 99)').run(), /FOREIGN KEY/)
      const insert = raw.prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
         VALUES (?, ?, ?, 'note', ?, ?, ?)`,
      )
      assert.throws(() => insert.run(project.id, lead, zeus, 'Held', 'held', at), /CHECK/)
      insert.run(project.id, lead, zeus, 'One', 'delivering', at)
      assert.throws(
        () => insert.run(project.id, lead, zeus, 'Two', 'delivering', at),
        /UNIQUE constraint failed: message.recipient_id/,
        'one delivery at a time per recipient',
      )
      assert.equal(raw.prepare('PRAGMA foreign_key_check').all().length, 0, 'nothing dangles')
      raw.close()
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

  it('adds members once each', async () => {
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
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'opencode',
        role: 'advisor',
        tier: 'standard',
      })
      assert.deepEqual(
        ledger.project(project.id).participants.map((p) => p.handle),
        ['human', 'lead', 'zeus', 'diana', 'athena'],
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
      for (const handle of ['human', 'lead']) {
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

  it('tells a running lead who joined or left, and whose tasks went with them', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
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

      ledger.startConversation(id('lead'), { harness: 'claude-code' })
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
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Estimate' })
      ledger.createTask(project.id, { from: 'human', to: 'zeus', body: 'Logo' })
      ledger.removeMember(project.id, 'zeus')

      assert.deepEqual(notes(ledger, id('lead')), [
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
  /** A lead with two standard workers, a light worker, a complex advisor and a reviewer. */
  function tiered(ledger) {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      lead: { harness: 'claude-code' },
    })
    const add = (agent, harness, role, tier) =>
      ledger.addMember(project.id, { agent, harness, role, tier })
    add('zeus', 'claude-code', 'worker', 'standard')
    add('diana', 'codex', 'worker', 'standard')
    add('hera', 'pi', 'worker', 'light')
    add('athena', 'opencode', 'advisor', 'complex')
    add('nemesis', 'pi', 'reviewer', 'standard')
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

  it("records each member's tier, and refuses a member without a tier", async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      assert.deepEqual(
        ledger
          .project(project.id)
          .participants.filter((p) => p.role !== 'human')
          .map((p) => [p.handle, p.tier]),
        [
          ['lead', null],
          ['zeus', 'standard'],
          ['diana', 'standard'],
          ['hera', 'light'],
          ['athena', 'complex'],
          ['nemesis', 'standard'],
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
      const next = ledger.createProject({
        directory: '/work/api',
        name: 'api',
        lead: { harness: 'pi' },
        team: [{ agent: 'zeus', harness: 'pi', role: 'reviewer', tier: 'critical' }],
      })
      assert.deepEqual(
        next.participants.at(-1) && [next.participants.at(-1).tier, next.participants.at(-1).roles],
        ['critical', ['reviewer']],
      )
    })
  })

  it("opens a task for a tier instead of a member; it sits in nobody's lane", async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      const { task, message } = openTask(ledger, project)
      assert.deepEqual(
        [task.state, task.assignee, task.pool, task.tier, task.requester, message],
        ['open', null, 'worker', 'standard', 'lead', null],
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
      ledger.addMember(project.id, {
        agent: 'calliope',
        harness: 'claude-code',
        role: 'worker',
        tier: 'critical',
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

  it('opens an image task for the designer with no tier, and hands it to a free designer', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      assert.throws(
        () => ledger.createTask(project.id, { from: 'lead', pool: 'designer', body: 'A logo' }),
        {
          code: 'no-member-of-tier',
        },
      )
      ledger.addMember(project.id, {
        agent: 'pygmalion',
        harness: 'image',
        role: 'designer',
        tier: 'light',
      })
      const { task } = ledger.createTask(project.id, {
        from: 'lead',
        pool: 'designer',
        tier: 'critical',
        body: 'A logo: a compass rose, teal on white; save it as images/logo.png',
      })
      assert.deepEqual(
        [task.pool, task.tier, task.state],
        ['designer', null, 'open'],
        'no tier: any designer',
      )
      assert.deepEqual(
        ledger.candidates(project.id, task.number).map((c) => c.handle),
        ['pygmalion'],
      )
      const { message } = ledger.assignTask(project.id, task.number, id('pygmalion'))
      deliver(ledger, message)
      const done = ledger.recordResult(project.id, task.number, {
        body: '/work/app/images/logo.png',
      })
      assert.deepEqual(
        [done.task.state, done.message.recipient],
        ['done', 'lead'],
        'the drawing goes to the lead',
      )
    })
  })

  it('counts a member for every role it holds when a tier is checked, not only its first', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      assert.throws(() => openTask(ledger, project, { pool: 'advisor', tier: 'light' }), {
        code: 'no-member-of-tier',
      })
      ledger.setRoles(project.id, 'hera', ['worker', 'advisor'])
      const { task } = openTask(ledger, project, { pool: 'advisor', tier: 'light' })
      assert.deepEqual([task.pool, task.tier, task.state], ['advisor', 'light', 'open'])
    })
  })

  it('lets only the lead and the human create tasks', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      assert.throws(() => openTask(ledger, project, { from: 'zeus' }), {
        code: 'not-a-coordinator',
      })
      assert.throws(
        () => ledger.createTask(project.id, { from: 'zeus', to: 'lead', body: 'Do it' }),
        { code: 'not-a-coordinator' },
      )
      assert.throws(
        () => ledger.createTask(project.id, { from: 'athena', to: 'lead', body: 'Plan' }),
        { code: 'not-a-coordinator' },
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'lead', to: 'lead', body: 'My own' }).task.assignee,
        'lead',
      )
      assert.throws(
        () =>
          ledger.createTask(project.id, {
            from: 'human',
            pool: 'advisor',
            tier: 'complex',
            body: 'Look',
          }),
        { code: 'advice-for-the-lead' },
        'advice is for the lead alone to ask',
      )
      assert.equal(
        ledger.createTask(project.id, {
          from: 'human',
          pool: 'worker',
          tier: 'standard',
          body: 'Look',
        }).task.pool,
        'worker',
      )
    })
  })

  it('counts a member as holding its work from assignment to its result, busy to the assigner meanwhile', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      // The work is on the session's hands; the member counts its sessions at work.
      const sessions = () =>
        ledger.members(project.id, 'worker').find((m) => m.handle === 'zeus').sessions
      assert.deepEqual([ledger.holdsWork(id('zeus')), sessions()], [false, 0], 'nothing yet')
      ledger.assignTask(project.id, 1, id('zeus'))
      const session = sessionId(ledger, project.id, 'zeus-amber-pine')
      assert.deepEqual([ledger.holdsWork(session), sessions()], [true, 1], 'queued')
      const brief = ledger.task(project.id, 1).messages.find((m) => m.kind === 'task')
      ledger.beginDelivery(brief.id)
      ledger.confirmDelivery(brief.id, { item: 'i-1' })
      assert.deepEqual([ledger.holdsWork(session), sessions()], [true, 1], 'working')
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      assert.equal(ledger.task(project.id, 1).state, 'done')
      assert.deepEqual([ledger.holdsWork(session), sessions()], [false, 0], 'done')
    })
  })

  it('lists the members a task may go to, with how many tasks each has taken', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      assert.deepEqual(
        ledger.candidates(project.id, 1).map((c) => [c.handle, c.tier, c.taken, c.outUntil]),
        [
          ['zeus', 'standard', 0, null],
          ['diana', 'standard', 0, null],
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
        ledger.members(project.id, 'worker').map((m) => [m.handle, m.tier, m.sessions, m.taken]),
        [
          ['zeus', 'standard', 1, 1],
          ['diana', 'standard', 0, 0],
          ['hera', 'light', 0, 0],
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
      ledger.recordResult(project.id, 1, { body: 'Done' })
      assert.equal(ledger.members(project.id, 'worker')[0].sessions, 0, 'finished work is not busy')
    })
  })

  it('opens a review for a reviewer of a tier: a task like any other, its findings the result', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      assert.equal(ledger.task(project.id, 1).state, 'done', 'nothing is reviewed on its own')
      const { task } = openTask(ledger, project, {
        pool: 'reviewer',
        body: 'Review T-1: the parser in src/parse.js',
      })
      assert.deepEqual(
        [task.number, task.pool, task.tier, task.state],
        [2, 'reviewer', 'standard', 'open'],
      )
      assert.deepEqual(
        ledger.candidates(project.id, 2).map((c) => c.handle),
        ['nemesis'],
      )
      assert.throws(() => openTask(ledger, project, { pool: 'reviewer', tier: 'complex' }), {
        code: 'no-member-of-tier',
      })
      deliver(ledger, ledger.assignTask(project.id, 2, id('nemesis')).message)
      const done = ledger.recordResult(project.id, 2, { body: 'No test for empty input.' })
      assert.deepEqual(
        [done.task.state, done.message.recipient, done.message.body],
        ['done', 'lead', 'No test for empty input.'],
      )
      assert.equal(ledger.task(project.id, 1).state, 'done', 'the lead decides the work')
    })
  })

  it('assigns an open task: it is queued for the member and the log says who was picked', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      const { task, message } = ledger.assignTask(project.id, 1, id('zeus'))
      assert.deepEqual([task.state, task.assignee], ['queued', 'zeus-amber-pine'])
      assert.deepEqual(
        [message.recipient, message.kind, message.state, message.body],
        ['zeus-amber-pine', 'task', 'queued', 'Write the parser'],
      )
      assert.equal(
        ledger.nextDelivery(sessionId(ledger, project.id, 'zeus-amber-pine')).id,
        message.id,
      )
      assert.deepEqual(ledger.events(project.id).at(-1), {
        ...ledger.events(project.id).at(-1),
        kind: 'task.assigned',
        data: {
          task: 1,
          from: 'open',
          to: 'queued',
          assignee: 'zeus-amber-pine',
          member: 'zeus',
          message: message.id,
        },
      })
      assert.throws(() => ledger.assignTask(project.id, 1, id('diana')), {
        code: 'invalid-transition',
      })
      openTask(ledger, project, { tier: 'light' })
      assert.throws(() => ledger.assignTask(project.id, 2, id('zeus')), { code: 'not-a-candidate' })
      assert.throws(() => ledger.assignTask(project.id, 2, id('athena')), {
        code: 'not-a-candidate',
      })
      assert.equal(ledger.assignTask(project.id, 2, id('hera')).task.assignee, 'hera-brisk-birch')
      const board = ledger.board(project.id)
      assert.deepEqual(board.open, [])
      assert.deepEqual(
        board.lanes
          .find((lane) => lane.participant.handle === 'hera-brisk-birch')
          .tasks.map((t) => t.number),
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
        to: 'zeus-amber-pine',
        body: 'Mind the tests',
        task: 1,
      })

      const { task } = ledger.releaseTask(project.id, 1, {
        because: 'ran out of quota after starting',
      })
      assert.deepEqual([task.state, task.assignee], ['open', null])
      assert.equal(ledger.message(note.id).state, 'cancelled')
      assert.equal(ledger.message(first.message.id).state, 'delivered', 'history stays')
      assert.deepEqual(ledger.events(project.id).findLast((e) => e.kind === 'task.released').data, {
        task: 1,
        from: 'working',
        to: 'open',
        member: 'zeus-amber-pine',
        because: 'ran out of quota after starting',
      })
      assert.ok(
        ledger.project(project.id).participants.some((p) => p.handle === 'zeus-amber-pine'),
        'the session that lost its work stays for the human',
      )
      const told = ledger.inbox(id('lead'))[0]
      assert.deepEqual(
        [told.kind, told.taskNumber, told.body],
        [
          'note',
          1,
          'T-1 was taken back from @zeus-amber-pine (ran out of quota after starting) and waits for another standard worker.',
        ],
      )
      assert.equal(ledger.activeTask(id('zeus')), null)

      const second = ledger.assignTask(project.id, 1, id('diana'))
      assert.equal(
        second.message.body,
        'Write the parser\n\nReassigned from @zeus-amber-pine, which ran out of quota after starting; check the working tree for partial changes.',
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

describe("sessions: a member's named windows", () => {
  /** A team with a tiered task open for a standard worker. */
  function opened(ledger, body = 'Write the parser') {
    const { project, id } = team(ledger)
    ledger.createTask(project.id, { from: 'lead', pool: 'worker', tier: 'standard', body })
    return { project, id }
  }
  const sessionOf = (ledger, projectId, handle) =>
    ledger.project(projectId).participants.find((p) => p.handle === handle)

  it("assigns a task to a new session of the member, named after it, with the member's shape", async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      const { task, message } = ledger.assignTask(project.id, 1, id('zeus'))
      assert.equal(task.assignee, 'zeus-amber-pine')
      assert.equal(message.recipient, 'zeus-amber-pine')
      const session = sessionOf(ledger, project.id, 'zeus-amber-pine')
      assert.deepEqual(
        [
          session.memberId,
          session.member,
          session.session,
          session.role,
          session.roles,
          session.tier,
          session.agent,
          session.harness,
        ],
        [id('zeus'), 'zeus', 'amber-pine', 'worker', ['worker'], 'standard', 'zeus', 'claude-code'],
      )
      assert.deepEqual(
        ledger.members(project.id, 'worker').map((m) => [m.handle, m.sessions]),
        [
          ['zeus', 1],
          ['diana', 0],
        ],
        'members stay members; a session is counted, not listed',
      )
      assert.equal(ledger.holdsWork(session.id), true)
      assert.equal(ledger.holdsWork(id('zeus')), false)
      assert.equal(ledger.task(project.id, 1).session, 'zeus-amber-pine')
    })
  })

  it('runs any number of sessions of one member at once: no cap, no expiry', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      for (const body of ['Lexer', 'Docs', 'Tests']) {
        ledger.createTask(project.id, { from: 'lead', pool: 'worker', tier: 'standard', body })
      }
      assert.equal(ledger.assignTask(project.id, 1, id('zeus')).task.assignee, 'zeus-amber-pine')
      assert.equal(ledger.assignTask(project.id, 2, id('zeus')).task.assignee, 'zeus-brisk-birch')
      assert.equal(ledger.assignTask(project.id, 3, id('zeus')).task.assignee, 'zeus-calm-brook')
      assert.equal(ledger.assignTask(project.id, 4, id('zeus')).task.assignee, 'zeus-coral-canyon')
      assert.equal(
        ledger.members(project.id, 'worker').find((m) => m.handle === 'zeus').sessions,
        4,
      )
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'More',
      })
      assert.throws(
        () => ledger.assignTask(project.id, 5, sessionOf(ledger, project.id, 'zeus-amber-pine').id),
        { code: 'not-a-member' },
        'a task is assigned to a member, never to a session by hand',
      )
    })
  })

  it('continues a session with --after: the follow-up goes to the same window, alive and free', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      assert.throws(
        () => ledger.createTask(project.id, { from: 'lead', after: 1, body: 'Also the lexer' }),
        { code: 'session-busy' },
      )
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const { task, message } = ledger.createTask(project.id, {
        from: 'lead',
        after: 1,
        body: 'Now the lexer, in the same style',
      })
      assert.deepEqual(
        [task.number, task.assignee, task.state, task.pool, message.recipient],
        [2, 'zeus-amber-pine', 'queued', null, 'zeus-amber-pine'],
      )
      assert.equal(
        ledger.nextDelivery(sessionOf(ledger, project.id, 'zeus-amber-pine').id).id,
        message.id,
      )
      deliver(ledger, message)
      ledger.recordResult(project.id, 2, { body: 'Lexer done' })
      ledger.acceptTask(project.id, 1, { by: 'lead' })
      assert.ok(
        sessionOf(ledger, project.id, 'zeus-amber-pine'),
        'the session stays while T-2 is unaccepted',
      )
      ledger.acceptTask(project.id, 2, { by: 'lead' })
      assert.ok(
        sessionOf(ledger, project.id, 'zeus-amber-pine'),
        'accepted work keeps the session: only the human ends it',
      )
      const more = ledger.createTask(project.id, { from: 'lead', after: 1, body: 'More' })
      assert.equal(more.task.assignee, 'zeus-amber-pine', 'and a follow-up still finds it')
      ledger.cancelTask(project.id, more.task.number, { by: 'lead' })
      ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
      assert.equal(sessionOf(ledger, project.id, 'zeus-amber-pine'), undefined)
      assert.throws(
        () => ledger.createTask(project.id, { from: 'lead', after: 1, body: 'More' }),
        (error) =>
          error.code === 'session-ended' && /open the task for its tier/.test(error.message),
      )
      assert.throws(
        () => ledger.createTask(project.id, { from: 'zeus-amber-pine', after: 1, body: 'x' }),
        { code: 'member-left' },
        'an ended session is nobody',
      )
    })
  })

  it("folds an ended session's tasks into its member's lane, and keeps its cards", async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      let board = ledger.board(project.id)
      assert.deepEqual(
        board.lanes.map((l) => [
          l.participant.handle,
          l.participant.member ?? null,
          l.tasks.map((t) => t.number),
        ]),
        [
          ['human', null, []],
          ['lead', null, []],
          ['zeus', null, []],
          ['diana', null, []],
          ['zeus-amber-pine', 'zeus', [1]],
        ],
      )
      ledger.acceptTask(project.id, 1, { by: 'lead' })
      ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
      board = ledger.board(project.id)
      assert.deepEqual(
        board.lanes.map((l) => [
          l.participant.handle,
          l.tasks.map((t) => [t.number, t.state, t.session]),
        ]),
        [
          ['human', []],
          ['lead', []],
          ['zeus', [[1, 'accepted', 'zeus-amber-pine']]],
          ['diana', []],
        ],
      )
      assert.deepEqual(
        ledger.members(project.id, 'worker').map((m) => [m.handle, m.taken]),
        [
          ['zeus', 1],
          ['diana', 0],
        ],
        'a member counts the tasks its sessions took',
      )
    })
  })

  it('reopens a done task on its own session; cancelling its last work leaves the session for the human', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const { task } = ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Handle comments too' })
      assert.deepEqual([task.assignee, task.state], ['zeus-amber-pine', 'queued'])
      ledger.cancelTask(project.id, 1, { by: 'lead' })
      assert.ok(sessionOf(ledger, project.id, 'zeus-amber-pine'), 'the session stays')
      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Again' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('ends a session only when the human says so, and not while it holds work', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      assert.throws(() => ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' }), {
        code: 'session-busy',
      })
      assert.throws(() => ledger.endSession(project.id, 'zeus', { by: 'human' }), {
        code: 'not-a-session',
      })
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
      assert.equal(sessionOf(ledger, project.id, 'zeus-amber-pine'), undefined)
      assert.deepEqual(ledger.events(project.id).at(-1).data, {
        handle: 'zeus-amber-pine',
        member: 'zeus',
        reason: 'ended by @human',
      })
      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Again' }), {
        code: 'session-ended',
      })
      assert.equal(ledger.task(project.id, 1).state, 'done', 'the task itself is untouched')
      const lane = ledger.board(project.id).lanes.find((l) => l.participant.handle === 'zeus')
      assert.deepEqual(
        lane.tasks.map((t) => t.number),
        [1],
        "its work folds into the member's lane",
      )
    })
  })

  it('gives a review task its own reviewer session, kept after its result', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      ledger.addMember(project.id, {
        agent: 'nemesis',
        harness: 'pi',
        roles: ['reviewer'],
        tier: 'standard',
      })
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'reviewer',
        tier: 'standard',
        body: 'Review T-1',
      })
      const review = ledger.assignTask(project.id, 2, id('nemesis'))
      assert.deepEqual(
        [
          review.task.assignee,
          review.message.recipient,
          sessionOf(ledger, project.id, 'nemesis-brisk-birch').role,
        ],
        ['nemesis-brisk-birch', 'nemesis-brisk-birch', 'reviewer'],
      )
      deliver(ledger, review.message)
      ledger.recordResult(project.id, 2, { body: 'Fine.' })
      assert.ok(
        sessionOf(ledger, project.id, 'nemesis-brisk-birch'),
        'its result leaves the reviewer session for the human',
      )
    })
  })

  it("refuses a session's handle where a member's is meant", async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      ledger.assignTask(project.id, 1, id('zeus'))
      assert.throws(() => ledger.removeMember(project.id, 'zeus-amber-pine'), {
        code: 'not-a-member',
      })
      assert.throws(() => ledger.setRoles(project.id, 'zeus-amber-pine', ['reviewer']), {
        code: 'not-a-member',
      })
      assert.ok(sessionOf(ledger, project.id, 'zeus-amber-pine'), 'the session is untouched')
    })
  })

  it('removes a member with its live sessions, cancelling their work', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.removeMember(project.id, 'zeus')
      assert.equal(sessionOf(ledger, project.id, 'zeus-amber-pine'), undefined)
      assert.equal(ledger.task(project.id, 1).state, 'cancelled', 'its work went with it')
      assert.deepEqual(
        ledger.members(project.id, 'worker').map((m) => m.handle),
        ['diana'],
      )
    })
  })
})

describe('human approval required: the gate', () => {
  /** A lead, a standard worker and a reviewer on another model, with the gate on. */
  function gated(ledger) {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      lead: { harness: 'claude-code' },
      gate: true,
    })
    const add = (agent, harness, role) =>
      ledger.addMember(project.id, { agent, harness, role, tier: 'standard' })
    add('zeus', 'claude-code', 'worker')
    add('diana', 'codex', 'reviewer')
    const id = (handle) =>
      ledger.project(project.id).participants.find((p) => p.handle === handle).id
    return { project, id }
  }
  /** The lead's task for a worker, assigned by the daemon: the task and its brief. */
  function briefed(ledger, project, id) {
    ledger.createTask(project.id, {
      from: 'lead',
      pool: 'worker',
      tier: 'standard',
      body: 'Parser',
    })
    const number = ledger.board(project.id).open.at(-1).number
    return { number, ...ledger.assignTask(project.id, number, id('zeus')) }
  }
  /** A worker's task from its brief to its window: approved by the human and delivered. */
  function working(ledger, project, id) {
    const { number, message } = briefed(ledger, project, id)
    deliver(ledger, ledger.approveMessage(message.id, { by: 'human' }))
    return { number, message, session: message.recipient }
  }
  const gatedIds = (ledger, project) => ledger.board(project.id).gated.map((m) => m.id)
  const noteTo = (ledger, participantId) =>
    ledger
      .inbox(participantId)
      .filter((m) => m.kind === 'note')
      .map((m) => [m.sender, m.taskNumber, m.state, m.body])

  it('is off unless asked for, set by hand, and refused when not a yes or no', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      assert.equal(ledger.project(project.id).gate, false)
      assert.equal(ledger.setGate(project.id, true).gate, true)
      assert.equal(ledger.setGate(project.id, true).gate, true, 'a second yes changes nothing')
      assert.deepEqual(
        ledger
          .events(project.id)
          .filter((e) => e.kind === 'project.gate')
          .map((e) => e.data),
        [{ from: false, to: true }],
      )
      assert.throws(() => ledger.setGate(project.id, 'yes'), { code: 'invalid-gate' })
      assert.throws(
        () =>
          ledger.createProject({
            directory: '/work/x',
            name: 'x',
            lead: { harness: 'pi' },
            gate: 1,
          }),
        { code: 'invalid-gate' },
      )
      assert.equal(ledger.projects().length, 1, 'nothing was created')
    })
  })

  it("holds the lead's brief for the human, who passes it on: nothing reaches the worker before", async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, task, message } = briefed(ledger, project, id)
      assert.deepEqual([task.state, message.state], ['queued', 'gated'])
      assert.equal(ledger.nextDelivery(message.recipientId), null, 'no window opens for it')
      assert.deepEqual(gatedIds(ledger, project), [message.id])
      assert.equal(ledger.inbox(message.recipientId).length, 0, 'the worker cannot see it')
      assert.equal(
        ledger.task(project.id, number).messages[0].state,
        'gated',
        'the human sees it on the card',
      )
      const approved = ledger.approveMessage(message.id, { by: 'human' })
      assert.equal(approved.state, 'queued')
      assert.equal(ledger.nextDelivery(message.recipientId).id, message.id)
      assert.deepEqual(gatedIds(ledger, project), [])
      assert.deepEqual(
        ledger
          .events(project.id)
          .filter((e) => e.kind === 'message.approved')
          .map((e) => e.data),
        [{ message: message.id, by: 'human' }],
      )
      assert.throws(() => ledger.approveMessage(message.id, { by: 'human' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('declines a brief: the task is cancelled, its session kept, and the lead told why', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, message } = briefed(ledger, project, id)
      const declined = ledger.declineMessage(message.id, { by: 'human' })
      assert.deepEqual([declined.state, declined.reason], ['cancelled', 'declined by @human'])
      assert.equal(ledger.task(project.id, number).state, 'cancelled')
      assert.deepEqual(noteTo(ledger, id('lead')), [
        ['human', number, 'queued', '@human declined T-1 (Parser). It is cancelled.'],
      ])
      assert.equal(ledger.nextDelivery(id('lead')).kind, 'note', 'the note goes without the gate')
      assert.equal(
        ledger.project(project.id).participants.some((p) => p.member === 'zeus'),
        true,
        'the session that never opened stays for the human',
      )
      assert.deepEqual(gatedIds(ledger, project), [])
      // Without a reason, the note says only what happened.
      const again = briefed(ledger, project, id)
      ledger.declineMessage(again.message.id, { by: 'human' })
      assert.equal(
        noteTo(ledger, id('lead'))[0][3],
        `@human declined T-${again.number} (Parser). It is cancelled.`,
      )
      assert.throws(() => ledger.declineMessage(again.message.id, { by: 'human' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('holds a result for the human, who passes it on, sends it back or accepts it, never declines it', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const first = working(ledger, project, id)
      const { task, message } = ledger.recordResult(project.id, first.number, { body: 'Done' })
      assert.deepEqual([task.state, message.state], ['done', 'gated'])
      assert.equal(ledger.nextDelivery(id('lead')), null, 'the lead waits')
      assert.throws(() => ledger.declineMessage(message.id, { by: 'human' }), {
        code: 'not-declinable',
        message: /a result is passed on, not declined/,
      })
      assert.equal(ledger.approveMessage(message.id, { by: 'human' }).state, 'queued')
      assert.equal(ledger.nextDelivery(id('lead')).id, message.id)

      const second = working(ledger, project, id)
      const held = ledger.recordResult(project.id, second.number, { body: 'Half done' }).message
      const back = ledger.reopenTask(project.id, second.number, { by: 'human', body: 'Add tests' })
      assert.deepEqual(
        [ledger.message(held.id).state, ledger.message(held.id).reason],
        ['cancelled', 'sent back by @human'],
      )
      assert.deepEqual([back.task.state, back.message.state], ['queued', 'queued'])
      assert.equal(back.message.sender, 'human', "the human's own follow-up passes the gate")

      const third = working(ledger, project, id)
      const done = ledger.recordResult(project.id, third.number, { body: 'All done' }).message
      ledger.acceptTask(project.id, third.number, { by: 'human' })
      assert.deepEqual(
        [ledger.message(done.id).state, ledger.message(done.id).reason],
        ['cancelled', 'accepted by @human'],
      )
      assert.deepEqual(gatedIds(ledger, project), [])
    })
  })

  it("holds a worker's question for the human, who passes it to the lead or answers it", async () => {
    await withDir(async (dir) => {
      let at = Date.parse('2026-09-20T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: () => {
          at += 1000
          return new Date(at)
        },
        names: names(),
      })
      try {
        const { project, id } = gated(ledger)
        const { number, session } = working(ledger, project, id)
        const question = ledger.ask(project.id, {
          from: session,
          to: 'lead',
          task: number,
          body: 'Which colour?',
        })
        assert.equal(question.state, 'gated')
        assert.equal(ledger.task(project.id, number).state, 'waiting')
        assert.equal(ledger.nextDelivery(id('lead')), null)
        at += OVERDUE_MS
        assert.deepEqual(
          ledger.board(project.id).overdue,
          [],
          'gated is not overdue: it is in the bay',
        )
        assert.throws(() => ledger.declineMessage(question.id, { by: 'human' }), {
          code: 'not-declinable',
          message: /a question is passed on, not declined/,
        })
        ledger.approveMessage(question.id, { by: 'human' })
        assert.equal(ledger.nextDelivery(id('lead')).id, question.id)
        assert.deepEqual(
          ledger.board(project.id).overdue.map((m) => m.id),
          [question.id],
          'and overdue once on its way to the lead',
        )

        const other = ledger.ask(project.id, {
          from: session,
          to: 'lead',
          task: number,
          body: 'Which size?',
        })
        const answer = ledger.answer(other.id, { from: 'human', body: 'Large' })
        assert.deepEqual([answer.state, answer.recipient], ['queued', question.sender])
        assert.deepEqual(
          [ledger.message(other.id).state, ledger.message(other.id).reason],
          ['cancelled', 'answered by @human'],
          'the lead never gets a question the human answered',
        )
        assert.equal(ledger.answerTo(other.id).id, answer.id)
        assert.deepEqual(gatedIds(ledger, project), [])
      } finally {
        ledger.close()
      }
    })
  })

  it("holds the lead's answer for the human, who passes it on or declines it for another", async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, session } = working(ledger, project, id)
      const question = ledger.ask(project.id, {
        from: session,
        to: 'lead',
        task: number,
        body: 'Which colour?',
      })
      deliver(ledger, ledger.approveMessage(question.id, { by: 'human' }))
      const answer = ledger.answer(question.id, { from: 'lead', body: 'Blue' })
      assert.equal(answer.state, 'gated')
      assert.equal(ledger.answerTo(question.id), null, 'the door keeps waiting')
      assert.throws(() => ledger.answer(question.id, { from: 'lead', body: 'Red' }), {
        code: 'already-answered',
      })
      const declined = ledger.declineMessage(answer.id, { by: 'human' })
      assert.equal(declined.state, 'cancelled')
      assert.equal(
        noteTo(ledger, id('lead'))[0][3],
        `@human declined your answer to m-${question.id}. Answer it again: cf answer m-${question.id} "…"`,
      )
      const again = ledger.answer(question.id, { from: 'lead', body: 'Red' })
      assert.equal(again.state, 'gated', 'the question was open for another answer')
      assert.equal(ledger.approveMessage(again.id, { by: 'human' }).state, 'queued')
      assert.equal(ledger.answerTo(question.id).id, again.id)
      assert.equal(
        ledger.nextDelivery(again.recipientId).id,
        again.id,
        'into the window that asked',
      )
    })
  })

  it('holds a choice answer too, and lands it read for the door once approved', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, session } = working(ledger, project, id)
      const question = ledger.ask(project.id, {
        from: session,
        to: 'lead',
        task: number,
        questions: [{ question: 'Colour?', header: 'Colour', options: [{ label: 'red' }] }],
      })
      deliver(ledger, ledger.approveMessage(question.id, { by: 'human' }))
      const answer = ledger.answer(question.id, { from: 'lead', choices: [['red']] })
      assert.deepEqual([answer.state, answer.choices], ['gated', [['red']]])
      assert.equal(ledger.task(project.id, number).state, 'waiting', 'the task waits on')
      assert.equal(ledger.answerTo(question.id), null)
      const approved = ledger.approveMessage(answer.id, { by: 'human' })
      assert.equal(approved.state, 'read', 'collected by the door, never pasted')
      assert.equal(ledger.task(project.id, number).state, 'working')
      assert.equal(ledger.nextDelivery(answer.recipientId), null)
      assert.equal(ledger.answerTo(question.id).id, answer.id)
    })
  })

  it("lets the human's own messages, an agent's word to itself and ConsensFlow's notes pass", async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const byHand = ledger.createTask(project.id, { from: 'human', to: 'zeus', body: 'Parser' })
      assert.equal(byHand.message.state, 'queued', 'the human gave it')
      const own = ledger.createTask(project.id, { from: 'lead', to: 'lead', body: 'Plan' })
      assert.equal(own.message.state, 'queued', "the lead's own work")
      assert.equal(
        ledger.ask(project.id, { from: 'lead', to: 'human', body: 'Ship it?' }).state,
        'queued',
        'a question for the human',
      )
      assert.equal(
        ledger.note(project.id, { to: 'lead', body: 'T-9 waits for a free worker.' }).state,
        'queued',
        "ConsensFlow's own note",
      )
      ledger.setGate(project.id, false)
      const open = briefed(ledger, project, id)
      assert.equal(open.message.state, 'queued', 'gate off: straight through')
    })
  })

  it('drops a gated message with its task, or with the member who leaves', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const first = briefed(ledger, project, id)
      ledger.cancelTask(project.id, first.number, { by: 'lead' })
      assert.equal(ledger.message(first.message.id).state, 'cancelled')
      const second = briefed(ledger, project, id)
      ledger.removeMember(project.id, 'zeus')
      assert.equal(ledger.message(second.message.id).state, 'cancelled')
      assert.deepEqual(gatedIds(ledger, project), [])
    })
  })
})

describe('a plan on the board: needs', () => {
  /** A lead and two standard workers, with T-1 open for a worker. */
  function planned(ledger) {
    const { project, id } = team(ledger)
    ledger.createTask(project.id, { from: 'lead', pool: 'worker', tier: 'standard', body: 'Lexer' })
    return { project, id }
  }
  const open = (ledger, project, body, extra = {}) =>
    ledger.createTask(project.id, {
      from: 'lead',
      pool: 'worker',
      tier: 'standard',
      body,
      ...extra,
    }).task
  /** A worker's task through to done: assigned, delivered, answered. */
  function finish(ledger, project, id, number) {
    deliver(ledger, ledger.assignTask(project.id, number, id('zeus')).message)
    ledger.recordResult(project.id, number, { body: 'Done' })
  }

  it('waits for the tasks it needs until each is accepted, and says which block it', async () => {
    await withLedger((ledger) => {
      const { project, id } = planned(ledger)
      const parser = open(ledger, project, 'Parser', { needs: [1] })
      assert.deepEqual([parser.needs, parser.blockedBy], [[{ number: 1, state: 'open' }], [1]])
      assert.deepEqual(
        ledger.board(project.id).open.map((t) => [t.number, t.blockedBy]),
        [
          [1, []],
          [2, [1]],
        ],
        'both wait on the board; only the first may go',
      )
      finish(ledger, project, id, 1)
      assert.deepEqual(ledger.task(project.id, 2).blockedBy, [1], 'done is not accepted')
      ledger.acceptTask(project.id, 1, { by: 'lead' })
      const freed = ledger.task(project.id, 2)
      assert.deepEqual([freed.needs, freed.blockedBy], [[{ number: 1, state: 'accepted' }], []])
      assert.deepEqual(
        ledger.events(project.id).find((e) => e.kind === 'task.opened' && e.data.task === 2).data,
        { task: 2, from: 'lead', pool: 'worker', tier: 'standard', needs: [1] },
      )
      // A need already accepted blocks nothing; one named twice counts once.
      const cli = open(ledger, project, 'CLI', { needs: [1, 2, 2] })
      assert.deepEqual(cli.blockedBy, [2])
      assert.equal(id('lead') > 0, true)
    })
  })

  it('puts a new task before tasks still on the board, and refuses one already in a window', async () => {
    await withLedger((ledger) => {
      const { project, id } = planned(ledger)
      open(ledger, project, 'Parser', { needs: [1] })
      const fix = open(ledger, project, 'Fix the lexer bug', { before: [1, 2] })
      assert.deepEqual(fix.blockedBy, [])
      assert.deepEqual(ledger.task(project.id, 1).blockedBy, [3])
      assert.deepEqual(ledger.task(project.id, 2).blockedBy, [1, 3])
      assert.deepEqual(
        ledger.events(project.id).find((e) => e.kind === 'task.opened' && e.data.task === 3).data
          .before,
        [1, 2],
      )
      finish(ledger, project, id, 3)
      ledger.acceptTask(project.id, 3, { by: 'lead' })
      assert.deepEqual(ledger.task(project.id, 1).blockedBy, [])
      ledger.assignTask(project.id, 1, id('zeus'))
      assert.throws(() => open(ledger, project, 'Too late', { before: [1] }), {
        code: 'not-on-the-board',
        message: /T-1 is queued/,
      })
      assert.equal(ledger.task(project.id, 4), null, 'nothing of the refused task is left')
    })
  })

  it('refuses a need that does not exist or is cancelled, and needs on a task that is not on the board', async () => {
    await withLedger((ledger) => {
      const { project } = planned(ledger)
      assert.throws(() => open(ledger, project, 'Parser', { needs: [9] }), { code: 'unknown-task' })
      assert.throws(() => open(ledger, project, 'Parser', { needs: ['T-1'] }), {
        code: 'invalid-needs',
      })
      ledger.cancelTask(project.id, 1, { by: 'lead' })
      assert.throws(() => open(ledger, project, 'Parser', { needs: [1] }), {
        code: 'need-cancelled',
      })
      for (const address of [{ to: 'lead' }, { to: 'zeus' }]) {
        assert.throws(
          () =>
            ledger.createTask(project.id, { from: 'lead', ...address, body: 'Plan', needs: [1] }),
          { code: 'needs-on-the-board' },
        )
      }
      assert.equal(ledger.board(project.id).open.length, 0)
    })
  })

  it('refuses a circle: a task cannot come before what it waits for, near or far', async () => {
    await withLedger((ledger) => {
      const { project } = planned(ledger)
      assert.throws(() => open(ledger, project, 'Loop', { needs: [1], before: [1] }), {
        code: 'circular-needs',
        message: 'T-1 is already what T-2 waits for: a plan has no circles',
      })
      open(ledger, project, 'Parser', { needs: [1] })
      assert.throws(() => open(ledger, project, 'Far loop', { needs: [2], before: [1] }), {
        code: 'circular-needs',
        message: 'T-1 is already what T-3 waits for: a plan has no circles',
      })
      assert.equal(ledger.task(project.id, 3), null, 'nothing of the refused task is left')
      assert.deepEqual(ledger.task(project.id, 1).blockedBy, [], 'and T-1 is untouched')
      const fine = open(ledger, project, 'Between', { needs: [1], before: [2] })
      assert.deepEqual([fine.blockedBy, ledger.task(project.id, 2).blockedBy], [[1], [1, 3]])
    })
  })

  it('keeps a task blocked by a need that was cancelled, and says so', async () => {
    await withLedger((ledger) => {
      const { project } = planned(ledger)
      open(ledger, project, 'Parser', { needs: [1] })
      ledger.cancelTask(project.id, 1, { by: 'lead' })
      const parser = ledger.task(project.id, 2)
      assert.deepEqual([parser.needs, parser.blockedBy], [[{ number: 1, state: 'cancelled' }], [1]])
      ledger.cancelTask(project.id, 2, { by: 'lead' })
      assert.equal(ledger.task(project.id, 2).state, 'cancelled', 'the lead decides')
    })
  })
})

describe('the transcript copy', () => {
  const item = (id, role, text, extra = {}) => ({ id, role, text, complete: true, ...extra })
  /** A worker's task assigned to a session with a conversation of its own. */
  function windowed(ledger) {
    const { project, id } = team(ledger)
    ledger.createTask(project.id, {
      from: 'lead',
      pool: 'worker',
      tier: 'standard',
      body: 'Parser',
    })
    const { message } = ledger.assignTask(project.id, 1, id('zeus'))
    const conversation = ledger.startConversation(message.recipientId, { harness: 'claude-code' })
    return { project, id, session: message.recipientId, conversation }
  }

  it('copies what is new, brings an item still being written up to date, and reads it by task', async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      assert.deepEqual(ledger.transcript(project.id, 1), { items: [], total: 0 })
      const first = [
        item('u1', 'user', '[ConsensFlow m-1 · T-1 · task from @lead]\nParser'),
        item('a1', 'assistant', 'On it', { complete: false, at: '2026-09-21T08:00:00.000Z' }),
      ]
      assert.equal(ledger.copyTranscript(conversation.id, first), 2)
      assert.equal(
        ledger.copyTranscript(conversation.id, first),
        0,
        'nothing changed: nothing written',
      )
      const then = [
        item('a1', 'assistant', 'On it. Parser done', { at: '2026-09-21T08:00:00.000Z' }),
        item('t1', 'tool', 'ok\n', { at: 7 }),
      ]
      assert.equal(ledger.copyTranscript(conversation.id, then, { from: 1 }), 2)
      const { items, total } = ledger.transcript(project.id, 1)
      assert.equal(total, 3)
      assert.deepEqual(
        items.map((i) => [i.id, i.role, i.text, i.complete, i.at]),
        [
          ['u1', 'user', '[ConsensFlow m-1 · T-1 · task from @lead]\nParser', true, null],
          ['a1', 'assistant', 'On it. Parser done', true, '2026-09-21T08:00:00.000Z'],
          ['t1', 'tool', 'ok\n', true, null],
        ],
      )
      const page = ledger.transcript(project.id, 1, { limit: 2 })
      assert.deepEqual(
        [page.total, page.items.map((i) => i.id)],
        [3, ['a1', 't1']],
        'the last ones',
      )
    })
  })

  it('cuts an item longer than it keeps, files an unknown role as custom, and skips what has no id', async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      const long = 'x'.repeat(TRANSCRIPT_ITEM_MAX + 5)
      ledger.copyTranscript(conversation.id, [
        item('big', 'tool', long),
        item('odd', 'system', 'hm'),
        { role: 'user', text: 'no id' },
        item('none', 'assistant', undefined),
      ])
      const { items } = ledger.transcript(project.id, 1)
      assert.deepEqual(
        items.map((i) => [i.id, i.role, i.text.length]),
        [
          ['big', 'tool', TRANSCRIPT_ITEM_MAX + `\n… (${long.length} characters; cut here)`.length],
          ['odd', 'custom', 2],
          ['none', 'assistant', 0],
        ],
      )
      assert.match(items[0].text, /… \(64005 characters; cut here\)$/)
      assert.throws(() => ledger.copyTranscript(999, [item('a', 'user', 'x')]), {
        code: 'unknown-conversation',
      })
      assert.throws(() => ledger.copyTranscript(conversation.id, 'items'), {
        code: 'invalid-items',
      })
    })
  })

  it("follows a task's window across a continued conversation, and shows nothing for a task on the board", async () => {
    await withLedger((ledger) => {
      const { project, id, session, conversation } = windowed(ledger)
      ledger.copyTranscript(conversation.id, [item('a1', 'assistant', 'Parser done')])
      deliver(ledger, ledger.task(project.id, 1).messages[0])
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const again = ledger.createTask(project.id, { from: 'lead', after: 1, body: 'Now the lexer' })
      assert.equal(again.task.assignee, ledger.task(project.id, 1).assignee)
      ledger.copyTranscript(conversation.id, [item('a2', 'assistant', 'Lexer done')], { from: 1 })
      assert.deepEqual(
        ledger.transcript(project.id, 2).items.map((i) => i.text),
        ['Parser done', 'Lexer done'],
        'the follow-up shows the same window',
      )
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Tests',
      })
      assert.deepEqual(ledger.transcript(project.id, 3), { items: [], total: 0 })
      assert.throws(() => ledger.transcript(project.id, 9), { code: 'unknown-task' })
      assert.equal(session > 0 && id('lead') > 0, true)
    })
  })
})

describe('pause and resume', () => {
  /** A worker's tiered task delivered into its session window. */
  function running(ledger) {
    const { project, id } = team(ledger)
    ledger.createTask(project.id, {
      from: 'lead',
      pool: 'worker',
      tier: 'standard',
      body: 'Parser',
    })
    const { message } = ledger.assignTask(project.id, 1, id('zeus'))
    deliver(ledger, message)
    return { project, id, session: message.recipient, sessionId: message.recipientId }
  }

  it('pauses work on the board or in a window, drops what was on its way, and keeps the session', async () => {
    await withLedger((ledger) => {
      const { project, id, session, sessionId } = running(ledger)
      const question = ledger.ask(project.id, {
        from: session,
        to: 'lead',
        task: 1,
        body: 'Which?',
      })
      const paused = ledger.pauseTask(project.id, 1, { by: 'lead' })
      assert.deepEqual([paused.state, paused.assignee], ['paused', session])
      assert.equal(ledger.message(question.id).state, 'cancelled', 'nothing of it goes on')
      assert.equal(ledger.holdsWork(sessionId), true, 'the window stays for the resumption')
      assert.equal(ledger.pausedTask(sessionId).number, 1)
      assert.equal(ledger.activeTask(sessionId), null)
      assert.equal(ledger.nextDelivery(sessionId), null)
      assert.deepEqual(ledger.events(project.id).at(-1).data, {
        task: 1,
        from: 'waiting',
        to: 'paused',
        by: 'lead',
      })
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Lexer',
      })
      assert.equal(
        ledger.pauseTask(project.id, 2).state,
        'paused',
        'from the board, by ConsensFlow',
      )
      assert.deepEqual(ledger.events(project.id).at(-1).data, {
        task: 2,
        from: 'open',
        to: 'paused',
        by: null,
      })
      assert.deepEqual(ledger.board(project.id).open, [], 'a paused task is not given out')
      assert.throws(() => ledger.pauseTask(project.id, 2, { by: 'lead' }), {
        code: 'invalid-transition',
      })
      assert.equal(id('lead') > 0, true)
    })
  })

  it("refuses to pause the lead's own work or finished work", async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      const own = ledger.createTask(project.id, { from: 'lead', to: 'lead', body: 'Plan' })
      assert.throws(() => ledger.pauseTask(project.id, own.task.number, { by: 'lead' }), {
        code: 'own-work',
      })
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      deliver(ledger, ledger.assignTask(project.id, 2, id('zeus')).message)
      ledger.recordResult(project.id, 2, { body: 'Done' })
      assert.throws(() => ledger.pauseTask(project.id, 2, { by: 'lead' }), {
        code: 'invalid-transition',
      })
      assert.throws(() => ledger.pauseTask(project.id, 9, { by: 'lead' }), { code: 'unknown-task' })
    })
  })

  it('resumes into the same window with the words, with the brief first when it never arrived', async () => {
    await withLedger((ledger) => {
      const { project, session } = running(ledger)
      ledger.pauseTask(project.id, 1, { by: 'lead' })
      const { task, message } = ledger.resumeTask(project.id, 1, { by: 'lead', body: 'Go on' })
      assert.deepEqual(
        [task.state, task.assignee, message.recipient, message.kind, message.state],
        ['queued', session, session, 'task', 'queued'],
      )
      assert.equal(message.body, 'Resumed: Go on')
      assert.throws(() => ledger.resumeTask(project.id, 1, { by: 'lead', body: 'Again' }), {
        code: 'invalid-transition',
      })
      assert.throws(() => ledger.resumeTask(project.id, 1, { by: 'lead', body: '' }), {
        code: 'invalid-text',
      })
      // Paused before its brief was delivered: the brief goes in with the words.
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Lexer',
      })
      const { message: brief } = ledger.assignTask(
        project.id,
        2,
        ledger.project(project.id).participants.find((p) => p.handle === 'diana').id,
      )
      ledger.pauseTask(project.id, 2, { by: 'human' })
      assert.equal(ledger.message(brief.id).state, 'cancelled')
      const again = ledger.resumeTask(project.id, 2, { by: 'human', body: 'Start now' })
      assert.equal(again.message.body, 'Lexer\n\nResumed: Start now')
      assert.equal(again.task.state, 'queued')
    })
  })

  it('resumes a task paused on the board back onto the board, and one whose session ended too', async () => {
    await withDir(async (dir) => {
      let at = Date.parse('2026-09-21T09:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: () => {
          at += 1000
          return new Date(at)
        },
        names: names(),
      })
      try {
        const { project, id } = team(ledger)
        ledger.createTask(project.id, {
          from: 'lead',
          pool: 'worker',
          tier: 'standard',
          body: 'Parser',
        })
        ledger.pauseTask(project.id, 1, { by: 'lead' })
        const opened = ledger.resumeTask(project.id, 1, { by: 'lead', body: 'When you can' })
        assert.deepEqual(
          [opened.task.state, opened.task.assignee, opened.message],
          ['open', null, null],
        )
        assert.equal(opened.task.body, 'Parser\n\nResumed: When you can')

        deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
        ledger.pauseTask(project.id, 1, { by: 'lead' })
        ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
        const fresh = ledger.resumeTask(project.id, 1, { by: 'lead', body: 'Try again' })
        assert.deepEqual([fresh.task.state, fresh.task.assignee], ['open', null])
        assert.match(fresh.task.body, /Resumed after a pause, in a fresh window .*: Try again$/)
        assert.equal(ledger.board(project.id).open.length, 1, 'the daemon gives it out again')

        const named = ledger.createTask(project.id, { from: 'human', to: 'diana', body: 'By name' })
        deliver(ledger, named.message)
        ledger.pauseTask(project.id, 2, { by: 'human' })
        ledger.removeMember(project.id, 'diana')
        assert.throws(() => ledger.resumeTask(project.id, 2, { by: 'human', body: 'Go' }), {
          code: 'session-ended',
        })
        assert.equal(ledger.cancelTask(project.id, 2, { by: 'human' }).state, 'cancelled')
      } finally {
        ledger.close()
      }
    })
  })
})
