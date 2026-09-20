import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  OVERDUE_MS,
  openLedger,
  SCHEMA_VERSION,
  SESSION_IDLE_MS,
  SESSION_SLOTS,
  verdictOf,
} from '../src/ledger/index.js'
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
        review: 'none',
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

describe('upgrading a home', () => {
  it('gives a home written by the first schema the role sets and sessions, and takes its PM and tags away', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const old = new DatabaseSync(file)
      old.exec(MIGRATIONS[0])
      old.exec('PRAGMA user_version = 1')
      const at = '2026-09-19T21:00:00.000Z'
      old
        .prepare(
          `INSERT INTO project (id, directory, name, state, review, created_at, updated_at)
           VALUES (1, '/work/app', 'app', 'open', 'members', ?, ?),
                  (2, '/work/all', 'all', 'suspended', 'all', ?, ?)`,
        )
        .run(at, at, at, at)
      old
        .prepare(
          `INSERT INTO participant (project_id, handle, role, agent, harness, tier, created_at) VALUES
           (1, 'human', 'human', NULL, NULL, NULL, ?),
           (1, 'lead', 'lead', NULL, 'claude-code', NULL, ?),
           (1, 'zeus', 'worker', 'zeus', 'claude-code', 'standard', ?),
           (1, 'hera', 'reviewer', 'hera', 'codex', 'standard', ?),
           (1, 'pm', 'pm', NULL, 'codex', NULL, ?)`,
        )
        .run(at, at, at, at, at)
      old.prepare(`UPDATE participant SET tags = '["coding"]' WHERE handle = 'zeus'`).run()
      old
        .prepare(
          `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, tags, created_at, updated_at)
           VALUES (1, 1, 'Parser', 'Write the parser', 2, 3, 'working', '["rust"]', ?, ?),
                  (1, 2, 'Estimate', 'Estimate the parser', 5, 3, 'queued', '[]', ?, ?)`,
        )
        .run(at, at, at, at)
      old
        .prepare(
          `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 3, 5, 'task', 2, 'Estimate the parser', 'queued', ?),
                  (1, 3, 2, 'task', 1, 'Write the parser', 'delivering', ?)`,
        )
        .run(at, at)
      old.close()
      const ledger = openLedger(file)
      try {
        const roles = Object.fromEntries(
          ledger.project(1).participants.map((p) => [p.handle, p.roles]),
        )
        assert.deepEqual(
          roles,
          { human: [], lead: [], zeus: ['worker'], hera: ['reviewer'] },
          'the PM is gone with the concept',
        )
        assert.equal(ledger.task(1, 2), null, 'and so is the task it asked for, with its message')
        assert.deepEqual(
          ledger.inbox(3).map((m) => [m.taskNumber, m.state]),
          [[1, 'delivering']],
          "the lead's own delivery stays",
        )
        assert.equal(ledger.project(2).review, 'members', "'all work' reads as workers' work now")
        assert.deepEqual(
          ledger.members(1, 'reviewer').map((m) => m.handle),
          ['hera'],
          'the review policy still has its reviewer',
        )
        const question = ledger.ask(1, { from: 'lead', to: 'human', body: 'Still here?' })
        assert.equal(question.questions, null)
        // A task the old schema assigned to the member itself stays on its lane.
        const lane = ledger.board(1).lanes.find((l) => l.participant.handle === 'zeus')
        assert.deepEqual(
          lane.tasks.map((t) => [t.number, t.state]),
          [[1, 'working']],
          'the member keeps the task it held before sessions existed',
        )
        assert.equal(lane.participant.memberId, null)
      } finally {
        ledger.close()
      }
      const upgraded = new DatabaseSync(file)
      assert.equal(upgraded.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION)
      for (const table of ['participant', 'task']) {
        const columns = upgraded
          .prepare(`SELECT name FROM pragma_table_info(?)`)
          .all(table)
          .map((row) => row.name)
        assert.ok(!columns.includes('tags'), `${table} keeps no tags`)
      }
      // The rebuilt constraints know the designer and have forgotten the PM and the all policy.
      const at2 = '2026-09-20T12:00:00.000Z'
      upgraded
        .prepare(
          `INSERT INTO participant (project_id, handle, role, roles, agent, harness, tier, created_at)
           VALUES (1, 'pygmalion', 'designer', '["designer"]', 'pygmalion', 'image', 'light', ?)`,
        )
        .run(at2)
      assert.throws(() =>
        upgraded
          .prepare(
            `INSERT INTO participant (project_id, handle, role, roles, agent, harness, created_at)
             VALUES (1, 'pm', 'pm', '[]', NULL, 'codex', ?)`,
          )
          .run(at2),
      )
      assert.throws(() => upgraded.prepare("UPDATE project SET review = 'all' WHERE id = 1").run())
      // The gate is off for a home that never knew it, and the rebuilt message
      // table admits a gated message while still refusing a second delivery.
      assert.equal(upgraded.prepare('SELECT gate FROM project WHERE id = 1').get().gate, 0)
      assert.throws(() => upgraded.prepare('UPDATE project SET gate = 2 WHERE id = 1').run())
      upgraded
        .prepare(
          `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
           VALUES (1, 3, 2, 'note', 1, 'Held for the human', 'gated', ?)`,
        )
        .run(at2)
      assert.throws(
        () =>
          upgraded
            .prepare(
              `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at)
               VALUES (1, 3, 2, 'note', 1, 'A second delivery', 'delivering', ?)`,
            )
            .run(at2),
        /UNIQUE constraint failed: message.recipient_id/,
      )
      // The needs table came with the plan: no task needs itself or a task that is not there.
      assert.throws(() =>
        upgraded.prepare('INSERT INTO task_need (task_id, needs_id) VALUES (1, 1)').run(),
      )
      assert.throws(() =>
        upgraded.prepare('INSERT INTO task_need (task_id, needs_id) VALUES (1, 99)').run(),
      )
      assert.equal(upgraded.prepare('PRAGMA foreign_keys').get().foreign_keys, 1)
      assert.equal(upgraded.prepare('SELECT COUNT(*) AS n FROM task').get().n, 1)
      assert.equal(upgraded.prepare('PRAGMA foreign_key_check').all().length, 0, 'nothing dangles')
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
            review: 'members',
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
      ledger.setReview(project.id, 'members')
      const done = ledger.recordResult(project.id, task.number, {
        body: '/work/app/images/logo.png',
      })
      assert.deepEqual(
        [done.task.state, done.message.recipient],
        ['done', 'lead'],
        'a drawing is never reviewed',
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

  it('counts a member as holding its work from assignment to the verdict, busy to the assigner meanwhile', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      ledger.setReview(project.id, 'members')
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
      assert.equal(ledger.task(project.id, 1).state, 'review')
      assert.deepEqual([ledger.holdsWork(session), sessions()], [true, 1], 'under review')
      ledger.skipReview(project.id, 1, { reason: 'no reviewer' })
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
      ledger.setReview(project.id, 'members')
      ledger.recordResult(project.id, 1, { body: 'Done' })
      assert.equal(
        ledger.members(project.id, 'worker')[0].sessions,
        1,
        'work under review is still on its hands',
      )
      ledger.skipReview(project.id, 1, { reason: 'no reviewer' })
      assert.equal(ledger.members(project.id, 'worker')[0].sessions, 0, 'finished work is not busy')
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
      assert.equal(
        ledger.events(project.id).at(-2).kind,
        'session.ended',
        'the window that lost its work ends with it',
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

describe('tiered dispatch: the review gate', () => {
  /** A lead, a standard worker and two reviewers; the daemon would pick the reviewer. */
  function reviewed(ledger, review = 'members') {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      lead: { harness: 'claude-code' },
    })
    const add = (agent, harness, role) =>
      ledger.addMember(project.id, { agent, harness, role, tier: 'standard' })
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
        [2, 'review', 1, 'diana-brisk-birch', 'lead', 'queued', 'Review T-1'],
      )
      assert.match(review.message.body, /^Review T-1 \(round 1\) by @zeus-amber-pine\./)
      assert.match(review.message.body, /The task:\nParser\n/)
      assert.match(review.message.body, /The result:\nParser done\n/)
      assert.match(review.message.body, /VERDICT: pass or VERDICT: changes\.$/)
      assert.deepEqual(ledger.reviewsPending(project.id), [])
      assert.throws(() => ledger.createReview(project.id, number, { reviewer: id('calliope') }), {
        code: 'invalid-transition',
      })

      deliver(ledger, review.message)
      assert.equal(ledger.activeTask(sessionId(ledger, project.id, 'diana-brisk-birch')).number, 2)
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
          reviewer: 'diana-brisk-birch',
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
        ['changes', 'queued', 'zeus-amber-pine', 1, 'done'],
      )
      assert.equal(ledger.message(held.id).state, 'cancelled', 'the first result is superseded')
      const followUp = ledger.nextDelivery(sessionId(ledger, project.id, 'zeus-amber-pine'))
      assert.deepEqual(
        [followUp.kind, followUp.sender, followUp.taskNumber, followUp.body],
        [
          'task',
          'diana-brisk-birch',
          number,
          'Review round 1 by @diana-brisk-birch asks for changes:\n\nMissing tests.\n\nVERDICT: changes',
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
      assert.match(second.message.body, /^Review T-1 \(round 2\) by @zeus-amber-pine\./)
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

  it('never reviews advice, even from a member that is a worker too', async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger, 'members')
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'codex',
        roles: ['worker', 'advisor'],
        tier: 'standard',
      })
      const advice = ledger.createTask(project.id, {
        from: 'lead',
        pool: 'advisor',
        tier: 'standard',
        body: 'Which parser?',
      })
      const { task, message } = ledger.assignTask(project.id, advice.task.number, id('athena'))
      assert.equal(
        ledger.project(project.id).participants.find((p) => p.handle === task.assignee).role,
        'advisor',
        "the session plays the task's role, not the member's first one",
      )
      deliver(ledger, message)
      const done = ledger.recordResult(project.id, advice.task.number, { body: 'The second.' })
      assert.deepEqual(
        [done.task.state, done.message.state, done.message.recipient],
        ['done', 'queued', 'lead'],
        'advice goes straight to the lead, which weighs it itself',
      )
    })
  })

  it("never reviews the lead's own work by policy, and nothing under none; there is no third policy", async () => {
    await withLedger((ledger) => {
      const { project, id } = reviewed(ledger, 'members')
      const own = ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship it' })
      deliver(ledger, own.message)
      const done = ledger.recordResult(project.id, own.task.number, { body: 'Shipped' })
      assert.deepEqual([done.task.state, done.message.state], ['done', 'queued'])
      assert.throws(() => ledger.setReview(project.id, 'all'), { code: 'invalid-review' })

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
          { from: 'none', to: 'members' },
          { from: 'members', to: 'none' },
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
        ledger.board(project.id).lanes.find((l) => l.participant.handle === 'zeus-amber-pine')
          .tasks[0]
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
          reviewer: 'diana-brisk-birch',
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
        'calliope-crisp-cedar',
      )
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
        ledger.members(project.id, 'worker').map((m) => [m.handle, m.sessions, m.busy]),
        [
          ['zeus', 1, false],
          ['diana', 0, false],
        ],
        'members stay members; a session is counted, not listed',
      )
      assert.equal(ledger.holdsWork(session.id), true)
      assert.equal(ledger.holdsWork(id('zeus')), false)
      assert.equal(ledger.task(project.id, 1).session, 'zeus-amber-pine')
    })
  })

  it('runs two sessions of one member at once and no more', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Lexer',
      })
      ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Docs',
      })
      assert.equal(ledger.assignTask(project.id, 1, id('zeus')).task.assignee, 'zeus-amber-pine')
      assert.equal(ledger.assignTask(project.id, 2, id('zeus')).task.assignee, 'zeus-brisk-birch')
      assert.equal(SESSION_SLOTS, 2)
      assert.equal(ledger.members(project.id, 'worker').find((m) => m.handle === 'zeus').busy, true)
      assert.throws(() => ledger.assignTask(project.id, 3, id('zeus')), { code: 'no-free-slot' })
      assert.throws(
        () => ledger.assignTask(project.id, 3, sessionOf(ledger, project.id, 'zeus-amber-pine').id),
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
      assert.equal(
        sessionOf(ledger, project.id, 'zeus-amber-pine'),
        undefined,
        'accepted work ends the session',
      )
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

  it('reopens a done task on its own session, and cancelling its last work ends the session', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const { task } = ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Handle comments too' })
      assert.deepEqual([task.assignee, task.state], ['zeus-amber-pine', 'queued'])
      ledger.cancelTask(project.id, 1, { by: 'lead' })
      assert.equal(sessionOf(ledger, project.id, 'zeus-amber-pine'), undefined)
      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Again' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('ends a session left idle after its work, so it does not linger', async () => {
    await withDir((dir) => {
      let at = Date.parse('2026-09-20T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: () => new Date(at),
        names: names(),
      })
      try {
        const { project, id } = opened(ledger)
        deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
        ledger.recordResult(project.id, 1, { body: 'Parser done' })
        at += SESSION_IDLE_MS - 1000
        assert.deepEqual(ledger.expireSessions(project.id), [])
        at += 2000
        assert.deepEqual(ledger.expireSessions(project.id), ['zeus-amber-pine'])
        assert.equal(sessionOf(ledger, project.id, 'zeus-amber-pine'), undefined)
        assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Again' }), {
          code: 'session-ended',
        })
        assert.equal(ledger.task(project.id, 1).state, 'done', 'the task itself is untouched')
      } finally {
        ledger.close()
      }
    })
  })

  it('gives a review its own reviewer session, ended with the verdict', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      ledger.addMember(project.id, {
        agent: 'nemesis',
        harness: 'pi',
        roles: ['reviewer'],
        tier: 'standard',
      })
      ledger.setReview(project.id, 'members')
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const review = ledger.createReview(project.id, 1, { reviewer: id('nemesis') })
      assert.deepEqual(
        [review.task.assignee, review.task.kind, review.message.recipient],
        ['nemesis-brisk-birch', 'review', 'nemesis-brisk-birch'],
      )
      assert.throws(
        () =>
          ledger.createReview(project.id, 1, {
            reviewer: sessionOf(ledger, project.id, 'nemesis-brisk-birch').id,
          }),
        { code: 'not-a-member' },
      )
      deliver(ledger, review.message)
      ledger.recordVerdict(project.id, review.task.number, { body: 'Fine.\n\nVERDICT: pass' })
      assert.equal(
        sessionOf(ledger, project.id, 'nemesis-brisk-birch'),
        undefined,
        'the verdict ends the reviewer session',
      )
      assert.deepEqual(
        ledger.reviewsOf(project.id, 1).map((r) => [r.reviewer, r.verdict]),
        [['nemesis-brisk-birch', 'pass']],
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
  function gated(ledger, review = 'none') {
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
    ledger.setReview(project.id, review)
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

  it('declines a brief: the task is cancelled, its session over, and the lead told why', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, message } = briefed(ledger, project, id)
      const declined = ledger.declineMessage(message.id, { by: 'human', reason: 'Not now' })
      assert.deepEqual(
        [declined.state, declined.reason],
        ['cancelled', 'declined by @human: Not now'],
      )
      assert.equal(ledger.task(project.id, number).state, 'cancelled')
      assert.deepEqual(noteTo(ledger, id('lead')), [
        ['human', number, 'queued', '@human declined T-1 (Parser): Not now. It is cancelled.'],
      ])
      assert.equal(ledger.nextDelivery(id('lead')).kind, 'note', 'the note goes without the gate')
      assert.equal(
        ledger.project(project.id).participants.some((p) => p.member === 'zeus'),
        false,
        'the session that never opened is over',
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
        message: /passed on or sent back/,
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

  it('holds a review brief before the review and the reviewed result after its verdict', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger, 'members')
      const { number } = working(ledger, project, id)
      const result = ledger.recordResult(project.id, number, { body: 'Done' }).message
      assert.equal(result.state, 'held', 'the review comes first')
      const review = ledger.createReview(project.id, number, { reviewer: id('diana') })
      assert.equal(review.message.state, 'gated', "the worker's result goes to another agent")
      assert.deepEqual(gatedIds(ledger, project), [review.message.id])
      deliver(ledger, ledger.approveMessage(review.message.id, { by: 'human' }))
      ledger.recordVerdict(project.id, review.task.number, { body: 'Fine.\n\nVERDICT: pass' })
      assert.equal(ledger.task(project.id, number).state, 'done')
      assert.equal(ledger.message(result.id).state, 'gated', 'released through the gate')
      assert.equal(ledger.nextDelivery(id('lead')), null)
      assert.equal(ledger.approveMessage(result.id, { by: 'human' }).state, 'queued')
      assert.equal(ledger.nextDelivery(id('lead')).id, result.id)

      // Declining the review brief: the work goes on unreviewed, through the gate.
      const again = working(ledger, project, id)
      const later = ledger.recordResult(project.id, again.number, { body: 'Done too' }).message
      const second = ledger.createReview(project.id, again.number, { reviewer: id('diana') })
      ledger.declineMessage(second.message.id, { by: 'human', reason: 'No review needed' })
      assert.equal(ledger.task(project.id, second.task.number).state, 'cancelled')
      const work = ledger.task(project.id, again.number)
      assert.deepEqual(
        [work.state, work.unreviewed],
        ['done', '@human declined the review: No review needed'],
      )
      assert.equal(ledger.message(later.id).state, 'gated')
      assert.equal(
        noteTo(ledger, id('lead'))[0][3],
        `@human declined the review of T-${again.number}: No review needed. The result goes on unreviewed.`,
      )
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
          message: /passed on or answered/,
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
      const declined = ledger.declineMessage(answer.id, { by: 'human', reason: 'Say red' })
      assert.equal(declined.state, 'cancelled')
      assert.equal(
        noteTo(ledger, id('lead'))[0][3],
        `@human declined your answer to m-${question.id}: Say red. Answer it again: cf answer m-${question.id} "…"`,
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
