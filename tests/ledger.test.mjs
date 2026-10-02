import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import {
  OVERDUE_MS,
  openLedger,
  PAGE_BYTES,
  RESUME_WORDS,
  SCHEMA_VERSION,
  TRANSCRIPT_ITEM_MAX,
} from '../src/ledger/index.js'
import { MIGRATIONS } from '../src/ledger/schema.js'

/**
 * The ledger (TEST-BDC-01): one SQLite file in the home that holds every
 * project, participant, task and inbox message. Each test gets a throwaway
 * directory and a clock that moves one second per reading.
 */
const LEDGER = new URL('../src/ledger/index.js', import.meta.url).href

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

/** A project with a chief and two workers, the shape most tests start from. */
function staff(ledger) {
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    chief: { harness: 'claude-code' },
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
        chief: { harness: 'opencode' },
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

  it('tells a trace every event as it is logged, with the time the ledger gave it', async () => {
    await withDir(async (dir) => {
      const entries = []
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: clock(),
        trace: (entry) => entries.push(entry),
      })
      try {
        const project = ledger.createProject({
          directory: '/work/app',
          name: 'app',
          chief: { harness: 'pi' },
        })
        const logged = ledger.events(project.id)
        assert.deepEqual(entries[0], {
          at: logged[0].at,
          project: project.id,
          kind: 'project.created',
          data: { name: 'app', directory: '/work/app' },
        })
        assert.deepEqual(
          entries.map((entry) => [entry.at, entry.kind]),
          logged.map((event) => [event.at, event.kind]),
          'the trace is the event log, as it happens',
        )
      } finally {
        ledger.close()
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
          `import { openLedger, RESUME_WORDS } from ${JSON.stringify(LEDGER)}
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
         const project = ledger.createProject({ directory: '/w', name: 'w', chief: { harness: 'pi' } })
         ledger.addMember(project.id, { agent: 'zeus', harness: 'pi', role: 'worker', tier: 'standard' })
         console.log('ready')
         for (let n = 0; ; n++) ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'task ' + n })`,
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
      const { project, id } = staff(ledger)
      const { message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'Which?' })
      const other = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        chief: { harness: 'pi' },
      })
      const leadId = id('chief')
      assert.throws(() => ledger.deleteProject(project.id), { code: 'project-open' })
      ledger.setProjectState(project.id, 'suspended')
      const gone = ledger.deleteProject(project.id)
      assert.deepEqual(
        [
          gone.id,
          gone.name,
          gone.directory,
          gone.members,
          gone.sessions,
          gone.tasks,
          gone.messages,
        ],
        [project.id, 'app', '/work/app', 2, 0, 1, 2],
        'what went, for the trace',
      )
      assert.match(gone.createdAt, /^\d{4}-/)
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
  it('turns a ledger written before the Chief of Staff into one that says chief, ids kept', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-ledger-chief-'))
    try {
      const file = path.join(dir, 'consensflow.db')
      // A version-4 ledger as the 2026-09-24 build wrote it: `lead` in its rows and its check.
      const old = new DatabaseSync(file)
      old.exec(MIGRATIONS[0].replaceAll("'chief'", "'lead'"))
      for (const migration of MIGRATIONS.slice(1, 4)) old.exec(migration)
      old.exec('PRAGMA user_version = 4')
      const at = '2026-09-24T10:00:00.000Z'
      old.exec(
        `INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES (1, '/work/app', 'app', 'open', 0, '${at}', '${at}')`,
      )
      old.exec(
        `INSERT INTO participant (id, project_id, handle, role, created_at) VALUES (1, 1, 'human', 'human', '${at}'), (2, 1, 'lead', 'lead', '${at}'), (3, 1, 'zeus', 'worker', '${at}')`,
      )
      old.exec(
        `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, pool, created_at, updated_at) VALUES (1, 1, 'Parser', 'Parser', 2, 3, 'working', 'worker', '${at}', '${at}')`,
      )
      old.close()
      const ledger = openLedger(file)
      try {
        const chief = ledger.project(1).participants.find((p) => p.id === 2)
        assert.deepEqual([chief.handle, chief.role], ['chief', 'chief'])
        assert.deepEqual(
          [ledger.task(1, 1).requester, ledger.task(1, 1).assignee],
          ['chief', 'zeus'],
          'references follow the ids',
        )
        assert.equal(ledger.project(1).participants.length, 3)
      } finally {
        ledger.close()
      }
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })

  it('keeps a migration that leaves a reference dangling from counting: the next start checks again', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const old = new DatabaseSync(file, { enableForeignKeyConstraints: false })
      for (const migration of MIGRATIONS.slice(0, SCHEMA_VERSION - 1)) old.exec(migration)
      old.exec(`PRAGMA user_version = ${SCHEMA_VERSION - 1}`)
      const at = '2026-09-24T10:00:00.000Z'
      old.exec(
        `INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES (1, '/work/app', 'app', 'open', 0, '${at}', '${at}')`,
      )
      // A task asked for by a participant the file does not have.
      old.exec(
        `INSERT INTO task (project_id, number, title, body, requester_id, state, pool, created_at, updated_at) VALUES (1, 1, 'Parser', 'Parser', 99, 'open', 'worker', '${at}', '${at}')`,
      )
      old.close()
      for (const start of ['first', 'next']) {
        assert.throws(() => openLedger(file), { code: 'ledger-broken' }, `the ${start} start`)
      }
      const raw = new DatabaseSync(file, { readOnly: true })
      assert.equal(
        raw.prepare('PRAGMA user_version').get().user_version,
        SCHEMA_VERSION - 1,
        'the version stays where it was',
      )
      raw.close()
    })
  })

  it('refuses what the model never holds, and keeps every reference whole', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedger(file, { now: clock(), names: names() })
      const { project, id } = staff(ledger)
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      const chief = id('chief')
      const zeus = id('zeus')
      ledger.close()
      const raw = new DatabaseSync(file)
      raw.exec('PRAGMA foreign_keys = ON')
      const at = '2026-09-21T10:00:00.000Z'
      assert.throws(() => raw.prepare('UPDATE project SET gate = 2').run(), /CHECK/)
      assert.throws(() => raw.prepare("UPDATE task SET pool = 'judge'").run(), /CHECK/)
      assert.throws(() => raw.prepare("UPDATE task SET state = 'review'").run(), /CHECK/)
      assert.throws(
        () => raw.prepare('UPDATE task SET taken_from_id = 99').run(),
        /FOREIGN KEY/,
        'a task is taken back only from a participant that exists',
      )
      assert.throws(() => raw.prepare('INSERT INTO task_need VALUES (1, 1)').run(), /CHECK/)
      assert.throws(() => raw.prepare('INSERT INTO task_need VALUES (1, 99)').run(), /FOREIGN KEY/)
      const insert = raw.prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
         VALUES (?, ?, ?, 'note', ?, ?, ?)`,
      )
      assert.throws(() => insert.run(project.id, chief, zeus, 'Held', 'held', at), /CHECK/)
      insert.run(project.id, chief, zeus, 'One', 'delivering', at)
      assert.throws(
        () => insert.run(project.id, chief, zeus, 'Two', 'delivering', at),
        /UNIQUE constraint failed: message.recipient_id/,
        'one delivery at a time per recipient',
      )
      assert.equal(raw.prepare('PRAGMA foreign_key_check').all().length, 0, 'nothing dangles')
      raw.close()
    })
  })
})

describe('projects and participants', () => {
  it('starts a project with the human and its chief, each with a lane', async () => {
    await withLedger((ledger) => {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code' },
      })
      assert.equal(project.state, 'open')
      assert.equal(project.resumeOnStart, false)
      assert.deepEqual(
        project.participants.map((p) => [p.handle, p.role, p.harness]),
        [
          ['human', 'human', null],
          ['chief', 'chief', 'claude-code'],
        ],
      )
      // A chief runs where a Switch lead could take it: a harness with a terminal.
      for (const harness of ['kimi', 'image', 'nope', undefined]) {
        assert.throws(
          () => ledger.createProject({ directory: '/work/site', name: 'site', chief: { harness } }),
          { code: 'invalid-harness' },
          String(harness),
        )
      }
      assert.equal(ledger.projects().length, 1)
    })
  })

  it('lets a member hold several roles, and asks for members by any of them', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
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
      assert.throws(() => ledger.setRoles(project.id, 'hera', ['chief']), { code: 'invalid-role' })
      assert.throws(() => ledger.setRoles(project.id, 'chief', ['worker']), {
        code: 'not-a-member',
      })
      assert.deepEqual(ledger.lastStaff().find((m) => m.agent === 'hera').roles, ['reviewer'])
    })
  })

  it("refreshes each member's tier from its saved agent, sessions included, and leaves an unknown agent alone", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body: 'P' })
      ledger.assignTask(project.id, 1, id('zeus'))
      const changed = ledger.refreshMemberTiers((agent) => ({ zeus: 'complex' })[agent] ?? null)
      assert.deepEqual(changed, [
        { project: project.id, handle: 'zeus', from: 'standard', to: 'complex' },
      ])
      const tiers = Object.fromEntries(
        ledger.project(project.id).participants.map((p) => [p.handle, p.tier]),
      )
      assert.deepEqual(
        [tiers.zeus, tiers['zeus-amber-pine'], tiers.diana],
        ['complex', 'complex', 'standard'],
        "the session follows its member; diana's agent is unknown to the roster here",
      )
      assert.deepEqual(ledger.events(project.id).findLast((e) => e.kind === 'member.tier').data, {
        handle: 'zeus',
        from: 'standard',
        to: 'complex',
      })
      assert.deepEqual(
        ledger.refreshMemberTiers(() => 'complex'),
        [{ project: project.id, handle: 'diana', from: 'standard', to: 'complex' }],
      )
    })
  })

  it('tells no coordinator about a member joining, only a requester about work cancelled by one leaving', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const chief =
        ledger.currentConversation(id('chief')) ??
        ledger.startConversation(id('chief'), { harness: 'claude-code' })
      assert.ok(chief)
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      assert.equal(ledger.inbox(id('chief')).length, 0, 'no joining note')
      ledger.removeMember(project.id, 'hera')
      assert.equal(ledger.inbox(id('chief')).length, 0, 'nothing cancelled, nothing to say')
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      const given = ledger.createTask(project.id, { from: 'chief', to: 'hera', body: 'Lexer' })
      deliver(ledger, given.message)
      ledger.removeMember(project.id, 'hera')
      const notes = ledger.inbox(id('chief')).filter((m) => m.kind === 'note')
      assert.equal(notes.length, 1)
      assert.match(
        notes[0].body,
        /^@hera left the staff; it takes no more tasks\. Cancelled with it: T-1\.$/,
      )
    })
  })

  it('adds members once each', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
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
        () => ledger.addMember(project.id, { agent: 'hera', harness: 'pi', role: 'chief' }),
        { code: 'invalid-role' },
      )
      // Kimi left ConsensFlow (2026-10): no member runs on it.
      for (const harness of ['emacs', 'kimi']) {
        assert.throws(
          () =>
            ledger.addMember(project.id, {
              agent: 'hera',
              harness,
              role: 'worker',
              tier: 'standard',
            }),
          { code: 'invalid-harness' },
          harness,
        )
      }
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'opencode',
        role: 'advisor',
        tier: 'standard',
      })
      assert.deepEqual(
        ledger.project(project.id).participants.map((p) => p.handle),
        ['human', 'chief', 'zeus', 'diana', 'athena'],
      )
    })
  })

  it('reuses the previous project staff for the next project', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const iris = { agent: 'iris', harness: 'image', role: 'designer', tier: 'standard' }
      ledger.addMember(project.id, iris)
      assert.deepEqual(ledger.lastStaff(), [
        { agent: 'zeus', harness: 'claude-code', role: 'worker', roles: ['worker'] },
        { agent: 'diana', harness: 'codex', role: 'worker', roles: ['worker'] },
        { agent: 'iris', harness: 'image', role: 'designer', roles: ['designer'] },
      ])
      // A project whose staff is its image designer alone is the last staff too.
      ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
        staff: [iris],
      })
      assert.deepEqual(
        ledger.lastStaff().map((member) => member.agent),
        ['iris'],
      )
    })
  })

  it('marks the projects that were open for resume after a restart, once', async () => {
    await withLedger((ledger) => {
      const open = staff(ledger).project
      const suspended = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        chief: { harness: 'pi' },
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

describe('the project staff', () => {
  const notes = (ledger, participantId) =>
    ledger
      .inbox(participantId)
      .filter((message) => message.kind === 'note')
      .map((message) => [message.sender, message.body])
      .reverse()

  it('starts a project with the staff it is given, or not at all', async () => {
    await withLedger((ledger) => {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'pi' },
        staff: [
          { agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' },
          { agent: 'athena', harness: 'opencode', role: 'advisor', tier: 'standard' },
        ],
      })
      assert.deepEqual(
        project.participants.map((p) => [p.handle, p.role, p.harness]),
        [
          ['human', 'human', null],
          ['chief', 'chief', 'pi'],
          ['zeus', 'worker', 'claude-code'],
          ['athena', 'advisor', 'opencode'],
        ],
      )
      assert.throws(
        () =>
          ledger.createProject({
            directory: '/work/other',
            name: 'other',
            chief: { harness: 'pi' },
            staff: [{ agent: 'hera', harness: 'pi', role: 'chief' }],
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
      const { project, id } = staff(ledger)
      const zeus = id('zeus')
      const first = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      deliver(ledger, first.message)
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Lexer' })
      ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Docs' })
      const note = ledger.note(project.id, { from: 'chief', to: 'zeus', body: 'Mind the tests' })
      ledger.beginDelivery(note.id)
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
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
          ['chief', []],
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
        () => ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'More' }),
        { code: 'member-left' },
      )
      assert.throws(() => ledger.note(project.id, { from: 'chief', to: 'zeus', body: 'Hi' }), {
        code: 'member-left',
      })
      assert.deepEqual(ledger.lastStaff(), [
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
      const { project } = staff(ledger)
      const task = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      deliver(ledger, task.message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      const question = ledger.ask(project.id, { from: 'zeus', to: 'chief', body: 'More?' })
      deliver(ledger, question)
      ledger.removeMember(project.id, 'zeus')

      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Again' }), {
        code: 'member-left',
      })
      assert.throws(() => ledger.answer(question.id, { from: question.recipientId, body: 'No' }), {
        code: 'member-left',
      })
      assert.throws(() => ledger.removeMember(project.id, 'zeus'), { code: 'member-left' })
    })
  })

  it('only staff members leave', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      for (const handle of ['human', 'chief']) {
        assert.throws(() => ledger.removeMember(project.id, handle), { code: 'not-a-member' })
      }
      assert.throws(() => ledger.removeMember(project.id, 'nobody'), {
        code: 'unknown-participant',
      })
    })
  })

  it('takes a member back in the role and harness it rejoins with', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
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
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Review' }).task.assignee,
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

  it('tells a running chief who joined or left, and whose tasks went with them', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
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
      assert.deepEqual(notes(ledger, id('chief')), [], 'no window yet: its launch reads the staff')

      ledger.startConversation(id('chief'), { harness: 'claude-code' })
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
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Estimate' })
      ledger.createTask(project.id, { from: 'human', to: 'zeus', body: 'Logo' })
      ledger.removeMember(project.id, 'zeus')

      assert.deepEqual(notes(ledger, id('chief')), [
        [null, '@zeus left the staff; it takes no more tasks. Cancelled with it: T-1, T-2, T-3.'],
      ])
      assert.deepEqual(notes(ledger, id('human')), [])
    })
  })
})

describe('conversations', () => {
  it('gives a participant one current conversation, and a native session to one conversation', async () => {
    await withLedger((ledger) => {
      const { id } = staff(ledger)
      const first = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      const second = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      assert.notEqual(first.id, second.id)
      assert.equal(ledger.currentConversation(id('zeus')).id, second.id)

      ledger.bindConversation(second.id, 'native-1')
      assert.equal(ledger.currentConversation(id('zeus')).nativeSession, 'native-1')
      // Unique within a harness: two harnesses may mint the same string.
      const chief = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      assert.throws(() => ledger.bindConversation(chief.id, 'native-1'), {
        code: 'native-session-taken',
      })
      const codex = ledger.startConversation(id('diana'), { harness: 'codex' })
      assert.equal(ledger.bindConversation(codex.id, 'native-1').nativeSession, 'native-1')
      ledger.endConversation(second.id)
      assert.equal(ledger.currentConversation(id('zeus')), null)
    })
  })
})

describe('switching the lead', () => {
  it('moves the chief to another harness and agent: its conversation ends, its quota clears, the switch is logged', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const first = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      ledger.bindConversation(first.id, 'claude-session')
      ledger.markOut(id('chief'), { until: '2026-10-02T00:00:00.000Z', reason: 'out' })
      const before = ledger.project(project.id).participants.find((p) => p.handle === 'chief')

      ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      const chief = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      assert.deepEqual(
        [chief.harness, chief.agent, chief.outUntil, chief.outSince],
        ['codex', 'astraeus', null, before.outSince],
        "the old harness's quota is not the new one's; when it was marked stays",
      )
      assert.equal(ledger.currentConversation(id('chief')), null, 'every switch starts fresh')
      const switched = ledger.events(project.id).filter((e) => e.kind === 'chief.switched')
      assert.deepEqual(switched.at(-1).data, {
        from: { harness: 'claude-code', agent: null },
        to: { harness: 'codex', agent: 'astraeus' },
        cut: false,
      })
      assert.deepEqual(ledger.lastSwitch(project.id), switched.at(-1).data)

      ledger.switchChief(project.id, { harness: 'pi', cut: true })
      assert.equal(ledger.lastSwitch(project.id).cut, true, 'the old lead was cut mid-turn')
      const back = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      assert.deepEqual([back.harness, back.agent], ['pi', null], "the harness's own default")
      for (const harness of ['kimi', 'image', 'nope']) {
        assert.throws(() => ledger.switchChief(project.id, { harness }), {
          code: 'invalid-harness',
        })
      }
      assert.throws(() => ledger.switchChief(project.id, { harness: 'pi', agent: 'no such!' }), {
        code: 'invalid-agent',
      })
    })
  })

  it("keeps the lead's earlier conversations as its history: oldest first, each with its harness and items", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const claude = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      ledger.copyTranscript(claude.id, [
        { id: 'c1', role: 'user', text: 'the codeword is tern' },
        { id: 'c2', role: 'assistant', text: 'noted' },
      ])
      ledger.switchChief(project.id, { harness: 'codex' })
      const codex = ledger.startConversation(id('chief'), { harness: 'codex' })
      ledger.copyTranscript(codex.id, [{ id: 'x1', role: 'user', text: 'and the next step?' }])
      ledger.switchChief(project.id, { harness: 'pi' })
      ledger.startConversation(id('chief'), { harness: 'pi' })

      const history = ledger.leadHistory(project.id)
      assert.deepEqual(
        history.map((c) => [c.harness, c.items.map((i) => [i.role, i.text])]),
        [
          [
            'claude-code',
            [
              ['user', 'the codeword is tern'],
              ['assistant', 'noted'],
            ],
          ],
          ['codex', [['user', 'and the next step?']]],
        ],
        'the current conversation is not history',
      )
      assert.ok(history.every((c) => c.endedAt !== null))
    })
  })

  it('lists what waits on the lead: questions to it, results it has not decided, its own unfinished tasks', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const parser = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      deliver(ledger, parser.message)
      const lexer = ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Lexer' })
      deliver(ledger, lexer.message)
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Which grammar?',
      })
      ledger.recordResult(project.id, 2, { body: 'Lexer done' })
      const own = ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Plan' })
      deliver(ledger, own.message)

      const open = ledger.leadOpenWork(project.id)
      assert.deepEqual(
        open.questions.map((m) => [m.id, m.sender, m.taskNumber]),
        [[question.id, 'zeus', 1]],
      )
      assert.deepEqual(
        open.results.map((t) => [t.number, t.assignee]),
        [[2, 'diana']],
      )
      assert.deepEqual(
        open.own.map((t) => [t.number, t.state]),
        [[3, 'working']],
      )

      deliver(ledger, ledger.answer(question.id, { from: question.recipientId, body: 'LL(1)' }))
      ledger.acceptTask(project.id, 2, { by: 'chief' })
      const after = ledger.leadOpenWork(project.id)
      assert.deepEqual([after.questions, after.results], [[], []])
    })
  })

  it('gives back the attempt of a delivery a switch cut off before it could land', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const note = ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Ready' })
      ledger.beginDelivery(note.id)
      ledger.retryDelivery(note.id, 'the lead was switched', { refund: true })
      assert.deepEqual(
        [ledger.message(note.id).state, ledger.message(note.id).attempts],
        ['queued', 0],
      )
      ledger.beginDelivery(note.id)
      ledger.retryDelivery(note.id, 'the window closed')
      assert.equal(ledger.message(note.id).attempts, 1, 'an ordinary retry keeps the count')
    })
  })

  it('does not count a chief with a saved agent as staff', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.deepEqual(
        ledger.refreshMemberTiers(() => 'critical').map((change) => change.handle),
        ['zeus', 'diana'],
        "the chief's tier is not a member's",
      )
      ledger.setProjectState(project.id, 'suspended')
      assert.equal(ledger.deleteProject(project.id).members, 2)
    })
  })
})

describe('tasks and the inbox queue', () => {
  it('creates a task and queues it for its assignee in one step', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const { task, message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Write the parser\nwith tests',
      })
      assert.deepEqual(
        [task.number, task.title, task.state, task.requester, task.assignee],
        [1, 'Write the parser', 'queued', 'chief', 'zeus'],
      )
      assert.deepEqual(
        [message.kind, message.state, message.recipient, message.sender, message.taskNumber],
        ['task', 'queued', 'zeus', 'chief', 1],
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Docs' }).task.number,
        2,
      )
    })
  })

  it("marks the chief's tell urgent, and a paused window still takes it", async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const { task, message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Parser',
      })
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, { evidence: 'native' })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      ledger.pauseTask(project.id, task.number, { by: 'chief' })
      const plain = ledger.ask(project.id, { from: 'chief', to: 'zeus', task: 1, body: 'Later' })
      assert.equal(plain.urgent, false)
      assert.equal(ledger.nextDelivery(zeus.id), null, 'a paused task takes no ordinary message')
      const told = ledger.ask(project.id, {
        from: 'chief',
        to: 'zeus',
        task: 1,
        body: 'Use the new grammar',
        urgent: true,
      })
      assert.deepEqual([told.kind, told.urgent, told.taskNumber], ['question', true, 1])
      assert.equal(ledger.nextDelivery(zeus.id)?.id, told.id, 'the tell goes to the paused window')
    })
  })

  it("pauses the task for the chief's tell in the same step, and a refused tell pauses nothing", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const tell = (body) =>
        ledger.ask(project.id, { from: 'chief', to: 'zeus', task: 1, body, urgent: true })
      assert.throws(() => tell('  '), { code: 'invalid-text' })
      assert.equal(ledger.task(project.id, 1).state, 'working', 'nothing was paused')
      const told = tell('Use the new grammar')
      assert.equal(ledger.task(project.id, 1).state, 'paused')
      assert.equal(ledger.nextDelivery(id('zeus'))?.id, told.id, 'the pause withdrew nothing of it')
    })
  })

  it('refuses a task for someone outside the project and writes nothing', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const before = ledger.events(project.id).length
      assert.throws(
        () => ledger.createTask(project.id, { from: 'chief', to: 'ghost', body: 'Boo' }),
        { code: 'unknown-participant' },
      )
      assert.throws(
        () => ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: '  ' }),
        {
          code: 'invalid-text',
        },
      )
      assert.deepEqual(
        ledger.board(project.id).lanes.flatMap((lane) => lane.tasks),
        [],
      )
      assert.equal(ledger.events(project.id).length, before)
    })
  })

  it('delivers one message at a time per recipient, oldest first', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const first = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'One',
      }).message
      const second = ledger.createTask(project.id, {
        from: 'chief',
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
      const { project, id } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Ship v2' }).message,
      )
      const second = ledger.createTask(project.id, {
        from: 'human',
        to: 'chief',
        body: 'Also fix the docs',
      })
      assert.equal(ledger.nextDelivery(id('chief')).id, second.message.id)
      deliver(ledger, second.message)
      assert.equal(ledger.task(project.id, 2).state, 'working')
    })
  })

  it('still delivers answers and notes about its task to a worker busy with it, and nothing about no task', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'One' }).message,
      )
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Two' })
      const stray = ledger.note(project.id, { from: 'chief', to: 'zeus', body: 'Hello there' })
      assert.equal(ledger.nextDelivery(id('zeus')), null, "a member's session is its task's")
      const note = ledger.note(project.id, { from: 'chief', to: 'zeus', task: 1, body: 'Use JSON' })
      assert.equal(ledger.nextDelivery(id('zeus')).id, note.id)
      assert.equal(ledger.message(stray.id).state, 'queued')
    })
  })

  it('retries a delivery, then fails it and its task', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const { message } = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'One' })
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
      const { project, id } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const { task, message } = ledger.recordResult(project.id, 1, { body: 'Parser done' })
      assert.equal(task.state, 'done')
      assert.deepEqual(
        [message.kind, message.recipient, message.sender, message.body, message.taskNumber],
        ['result', 'chief', 'zeus', 'Parser done', 1],
      )
      assert.equal(ledger.nextDelivery(id('chief')).id, message.id)
      assert.throws(() => ledger.recordResult(project.id, 1, { body: 'again' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('makes a task wait for a question and resumes it when the answer is delivered', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Which format?',
      })
      assert.deepEqual([question.kind, question.recipient], ['question', 'chief'])
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      deliver(ledger, question)
      const answer = ledger.answer(question.id, { from: question.recipientId, body: 'JSON' })
      assert.deepEqual(
        [answer.kind, answer.recipient, answer.replyTo, answer.taskNumber],
        ['answer', 'zeus', question.id, 1],
      )
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      deliver(ledger, answer)
      assert.equal(ledger.task(project.id, 1).state, 'working')
      assert.throws(() => ledger.answer(answer.id, { from: answer.recipientId, body: 'thanks' }), {
        code: 'not-a-question',
      })
      assert.throws(() => ledger.ask(project.id, { to: 'chief', body: 'Who asks?' }), {
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

  /** Zeus on T-1, asking the chief a question with options. */
  function asked(ledger, questions = OPTIONS) {
    const { project, id } = staff(ledger)
    deliver(
      ledger,
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
    )
    const question = ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, questions })
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
          () => ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, questions: bad }),
          { code: 'bad-questions' },
        )
      }
      // A question's text may run as long as any message (a reviewer's 6249
      // characters, 2026-09-29); its header and labels stay short.
      const long = `${'Întrebarea, pe larg. '.repeat(300)}Codul: PLOP-6142`
      const option = { label: 'Doar româna' }
      const longAsked = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        questions: [{ question: long, header: 'Limba', options: [option] }],
      })
      assert.ok(longAsked.body.endsWith('PLOP-6142\n- Doar româna'))
      for (const bad of [
        [{ question: 'x', header: 'h'.repeat(1201), options: [option] }],
        [{ question: 'x', header: 'h', options: [{ label: 'l'.repeat(1201) }] }],
      ]) {
        assert.throws(
          () => ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, questions: bad }),
          { code: 'bad-questions' },
        )
      }
    })
  })

  it('answers a question with options by choice: the answer is read at once, never delivered, and the task resumes', async () => {
    await withLedger((ledger) => {
      const { project, id, question } = asked(ledger)
      assert.equal(ledger.answerTo(question.id), null)
      const answer = ledger.answer(question.id, {
        from: question.recipientId,
        choices: [['blue'], ['yes']],
      })
      assert.deepEqual(
        [answer.kind, answer.recipient, answer.replyTo, answer.state, answer.choices, answer.body],
        ['answer', 'zeus', question.id, 'read', [['blue'], ['yes']], 'Colour: blue\nShip: yes'],
      )
      assert.equal(ledger.task(project.id, 1).state, 'working', 'the door resumes the task')
      assert.equal(ledger.nextDelivery(id('zeus')), null, 'nothing is pasted into the window')
      assert.deepEqual(ledger.answerTo(question.id).choices, [['blue'], ['yes']])
      assert.throws(
        () =>
          ledger.answer(question.id, { from: question.recipientId, choices: [['red'], ['no']] }),
        {
          code: 'already-answered',
        },
      )
    })
  })

  it('keeps a question asked before the task arrived on that task, which then arrives waiting', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const { message } = ledger.createTask(project.id, {
        from: 'chief',
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
        to: 'chief',
        task: 1,
        questions: OPTIONS,
      })
      assert.equal(ledger.task(project.id, 1).state, 'queued')
      deliver(ledger, message)
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      ledger.answer(question.id, { from: question.recipientId, choices: [['red'], ['no']] })
      assert.equal(ledger.task(project.id, 1).state, 'working')
    })
  })

  it('lets the asker answer its own question with options, when its window answered first', async () => {
    await withLedger((ledger) => {
      const { project, id, question } = asked(ledger)
      const answer = ledger.answer(question.id, { from: id('zeus'), choices: [['red'], ['no']] })
      assert.deepEqual([answer.sender, answer.recipient, answer.state], ['zeus', 'zeus', 'read'])
      assert.equal(ledger.task(project.id, 1).state, 'working')
      const plain = ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'Plain?' })
      assert.throws(() => ledger.answer(plain.id, { from: id('zeus'), body: 'Me' }), {
        code: 'not-your-question',
      })
    })
  })

  it('maps a text answer onto the options, one line per question, and keeps free text', async () => {
    await withLedger((ledger) => {
      const { question } = asked(ledger)
      assert.throws(
        () => ledger.answer(question.id, { from: question.recipientId, body: 'blue' }),
        {
          code: 'bad-choices',
        },
      )
      const answer = ledger.answer(question.id, {
        from: question.recipientId,
        body: 'BLUE\nmaybe later',
      })
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
        () => ledger.answer(question.id, { from: question.recipientId, choices: [['a'], ['b']] }),
        { code: 'bad-choices' },
        'one array of labels per question',
      )
      const answer = ledger.answer(question.id, { from: question.recipientId, body: 'a, C' })
      assert.deepEqual([answer.choices, answer.body], [[['a', 'c']], 'Which: a, c'])
    })
  })

  it('takes a long answer in the human’s own words to a question with options', async () => {
    await withLedger((ledger) => {
      const { question } = asked(ledger, [
        { question: 'Name?', header: 'Name', options: [{ label: 'a' }, { label: 'b' }] },
      ])
      // An empty pick is empty; one past the body limit is too long, and says so.
      assert.throws(
        () => ledger.answer(question.id, { from: question.recipientId, choices: [['  ']] }),
        /empty pick/,
      )
      assert.throws(
        () =>
          ledger.answer(question.id, {
            from: question.recipientId,
            choices: [['x'.repeat(1_000_001)]],
          }),
        /too long/,
      )
      // "Something else" at the length of a real answer (6000 characters in the evals) goes through.
      const long = `${'Why this name. '.repeat(400)}DELTA-5530`
      const answer = ledger.answer(question.id, { from: question.recipientId, choices: [[long]] })
      assert.deepEqual(answer.choices, [[long]])
    })
  })

  it("lets only the one asked answer, and shows a coordinator's unanswered question as overdue until then", async () => {
    await withDir((dir) => {
      let at = Date.parse('2026-09-19T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: () => new Date(at) })
      try {
        const { project, id } = staff(ledger)
        deliver(
          ledger,
          ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
        )
        const question = ledger.ask(project.id, {
          from: 'zeus',
          to: 'chief',
          task: 1,
          body: 'Which format?',
        })
        at += OVERDUE_MS - 1000
        assert.deepEqual(ledger.board(project.id).overdue, [])
        at += 2000
        assert.deepEqual(
          ledger.board(project.id).overdue.map((m) => [m.id, m.recipient]),
          [[question.id, 'chief']],
        )
        assert.throws(() => ledger.answer(question.id, { from: id('human'), body: 'JSON' }), {
          code: 'not-your-question',
        })
        const answer = ledger.answer(question.id, { from: question.recipientId, body: 'JSON' })
        assert.deepEqual(
          [answer.recipient, answer.state, answer.choices],
          ['zeus', 'queued', null],
          'a plain question is answered in text, delivered as before',
        )
        assert.deepEqual(ledger.board(project.id).overdue, [])
        assert.throws(() => ledger.answer(question.id, { from: id('diana'), body: 'CSV' }), {
          code: 'not-your-question',
        })
      } finally {
        ledger.close()
      }
    })
  })

  it('shows as overdue only a question still waiting: none withdrawn, none whose task or asker went', async () => {
    await withDir((dir) => {
      let at = Date.parse('2026-09-19T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: () => new Date(at),
        names: names(),
      })
      try {
        const { project, id } = staff(ledger)
        const asks = (body, question) => {
          const { task } = ledger.createTask(project.id, {
            from: 'chief',
            pool: 'worker',
            tier: 'standard',
            body,
          })
          const { message } = ledger.assignTask(project.id, task.number, id('zeus'))
          deliver(ledger, message)
          const from = message.recipient
          return ledger.ask(project.id, { from, to: 'chief', task: task.number, body: question })
        }
        // T-1 is cancelled with its question in the chief's window; T-2's
        // question is withdrawn before it went; diana asks about no task and
        // leaves the staff; T-3's question waits.
        deliver(ledger, asks('Parser', 'Which grammar?'))
        const withdrawn = asks('Lexer', 'Which tokens?')
        deliver(ledger, asks('Tests', 'Which runner?'))
        deliver(ledger, ledger.ask(project.id, { from: 'diana', to: 'chief', body: 'Any work?' }))
        ledger.cancelTask(project.id, 1, { by: 'chief' })
        ledger.cancelMessage(withdrawn.id, 'no longer asked')
        ledger.removeMember(project.id, 'diana')
        at += OVERDUE_MS
        assert.deepEqual(
          ledger.board(project.id).overdue.map((m) => [m.taskNumber, m.body]),
          [[3, 'Which runner?']],
        )
      } finally {
        ledger.close()
      }
    })
  })

  it("knows the one asked by participant, not by handle: another project's chief is not it", async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const other = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'codex' },
      })
      const theirs = ledger.project(other.id).participants.find((p) => p.handle === 'chief')
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const question = ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'JSON?' })
      assert.throws(() => ledger.answer(question.id, { from: theirs.id, body: 'Yes' }), {
        code: 'not-your-question',
      })
      assert.equal(ledger.answerTo(question.id), null)
      const answer = ledger.answer(question.id, { from: question.recipientId, body: 'Yes' })
      assert.deepEqual([answer.sender, answer.recipient], ['chief', 'zeus'])
    })
  })

  it('keeps notes for the human in the human inbox until they are read, and never asks the human there', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const note = ledger.note(project.id, { from: 'chief', to: 'human', body: 'T-1 is done.' })
      assert.equal(ledger.nextDelivery(id('human')), null, 'the human reads in the app')
      assert.throws(() => ledger.beginDelivery(note.id), { code: 'human-reads-in-app' })
      assert.deepEqual(
        ledger.inbox(id('human')).map((m) => [m.id, m.state]),
        [[note.id, 'queued']],
      )
      assert.equal(ledger.markRead(note.id).state, 'read')
      // The chief asks the human in its own terminal; nobody asks on the board.
      for (const from of ['chief', 'zeus'])
        assert.throws(() => ledger.ask(project.id, { from, to: 'human', body: 'Deploy now?' }), {
          code: 'ask-in-your-terminal',
        })
      assert.deepEqual(
        ledger.inbox(id('human')).map((m) => m.id),
        [note.id],
      )
    })
  })

  it('withdraws what is still on its way to the member when its task is accepted', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      // The worker asks, goes on without waiting, and finishes; the answer comes after the result.
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Blue or green?',
      })
      deliver(ledger, question)
      ledger.recordResult(project.id, 1, { body: 'done, in blue' })
      const answer = ledger.answer(question.id, { from: question.recipientId, body: 'Green' })
      assert.equal(answer.state, 'queued')
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      const after = ledger.message(answer.id)
      assert.equal(after.state, 'cancelled', 'no window will ever take it')
      assert.match(after.reason, /T-1 was accepted before it reached @zeus/)
    })
  })

  it('follows the task state machine for accept, reopen, cancel and fail', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      ledger.recordResult(project.id, 1, { body: 'done' })
      const reopened = ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Add tests' })
      assert.equal(reopened.task.state, 'queued')
      assert.deepEqual([reopened.message.kind, reopened.message.recipient], ['task', 'zeus'])
      deliver(ledger, reopened.message)
      ledger.recordResult(project.id, 1, { body: 'tests added' })
      assert.equal(ledger.acceptTask(project.id, 1, { by: 'chief' }).state, 'accepted')
      assert.throws(() => ledger.acceptTask(project.id, 1, { by: 'chief' }), {
        code: 'invalid-transition',
      })
      assert.throws(() => ledger.cancelTask(project.id, 1, { by: 'chief' }), {
        code: 'invalid-transition',
      })

      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Lexer' })
      assert.equal(ledger.cancelTask(project.id, 2, { by: 'chief' }).state, 'cancelled')
      assert.equal(ledger.nextDelivery(id('zeus')), null, 'a cancelled task is never delivered')

      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Docs' }).message,
      )
      assert.equal(ledger.failTask(project.id, 3, { reason: 'pane exited' }).state, 'failed')
      assert.equal(
        ledger.reopenTask(project.id, 3, { by: 'chief', body: 'Retry' }).task.state,
        'queued',
      )
      // A paused task waits with its window: it is cancelled, never failed.
      ledger.pauseTask(project.id, 3, { by: 'chief' })
      assert.throws(() => ledger.failTask(project.id, 3, { reason: 'pane exited' }), {
        code: 'invalid-transition',
      })
      assert.equal(ledger.cancelTask(project.id, 3, { by: 'chief' }).state, 'cancelled')
    })
  })

  it('writes every change into the project log, in order', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
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
      const { project } = staff(ledger)
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Ship v2' })
      const board = ledger.board(project.id)
      assert.equal(board.project.id, project.id)
      assert.deepEqual(
        board.lanes.map((lane) => [lane.participant.handle, lane.tasks.map((t) => t.number)]),
        [
          ['human', []],
          ['chief', [2]],
          ['zeus', [1]],
          ['diana', []],
        ],
      )
    })
  })

  it('stays small however long the briefs and results run: no brief on a card, a part in the bay', async () => {
    await withDir((dir) => {
      let at = Date.parse('2026-09-19T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: () => new Date(at),
        names: names(),
      })
      try {
        const { project, id } = staff(ledger)
        ledger.setGate(project.id, true)
        // More than the page's 1 MiB frame of briefs, a one-line result of
        // 900,000 characters, and a question as long, both in the bay.
        const brief = `Fix the parser.\n${'Context, with "quotes" and a path, /src/x.js.\n'.repeat(80)}`
        for (let n = 0; n < 300; n += 1) {
          ledger.createTask(project.id, {
            from: 'chief',
            pool: 'worker',
            tier: 'standard',
            body: brief,
          })
        }
        const working = (number) => {
          const { message } = ledger.assignTask(project.id, number, id('zeus'))
          deliver(ledger, ledger.approveMessage(message.id, { by: 'human' }))
          return message.recipient
        }
        const long = 'A finding, with the evidence for it. '.repeat(24_000).trim()
        working(1)
        ledger.recordResult(project.id, 1, { body: long })
        const question = ledger.ask(project.id, {
          from: working(2),
          to: 'chief',
          task: 2,
          body: long,
        })
        ledger.approveMessage(question.id, { by: 'human' })
        at += OVERDUE_MS
        const board = ledger.board(project.id)
        const bytes = Buffer.byteLength(JSON.stringify(board))
        assert.ok(300 * brief.length > 1024 * 1024, 'more than a frame of briefs')
        assert.ok(bytes < 256 * 1024, `the board takes ${bytes} bytes`)
        const cards = [...board.open, ...board.lanes.flatMap((lane) => lane.tasks)]
        assert.equal(cards.length, 300)
        assert.equal(
          cards.some((task) => 'body' in task),
          false,
          'the drawer reads the brief with the task',
        )
        assert.equal(cards.find((task) => task.number === 1).result, `${long.slice(0, 119)}…`)
        const bay = [...board.gated, ...board.overdue]
        assert.deepEqual(
          bay.map((m) => [m.kind, m.taskNumber]),
          [
            ['result', 1],
            ['question', 2],
          ],
        )
        for (const message of bay) {
          assert.ok(message.body.startsWith('A finding, with the evidence for it.'))
          assert.ok(message.body.endsWith(`\n… (${long.length} characters; cut here)`))
          assert.ok(message.body.length < 2100, `${message.body.length} characters`)
        }
        assert.equal(
          ledger.task(project.id, 1).messages.find((m) => m.kind === 'result').body,
          long,
          'the whole stays with its task',
        )
      } finally {
        ledger.close()
      }
    })
  })

  it("reads the human's unread notes in a frame however long they run: the newest that fit, one too long cut, and how many", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const human = id('human')
      // Neither a note already read nor the result of a task the human gave is For you's.
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Ship it' }).message,
      )
      ledger.recordResult(project.id, 1, { body: 'Shipped' })
      ledger.markRead(ledger.note(project.id, { from: 'chief', to: 'human', body: 'Seen.' }).id)
      // More than the page's 1 MiB frame of notes, the newest longer than half of it alone.
      const report = 'A finding, with the evidence for it. '.repeat(8_000)
      const older = [1, 2, 3, 4].map((n) =>
        ledger.note(project.id, { from: 'chief', to: 'human', body: `${n}. ${report}` }),
      )
      const long = 'A log line the chief pasted whole. '.repeat(25_000)
      const newest = ledger.note(project.id, { from: 'chief', to: 'human', body: long })
      assert.ok(Buffer.byteLength(JSON.stringify(ledger.inbox(human))) > 1024 * 1024)

      const read = ledger.latestMessages(human, { unread: true })
      const bytes = Buffer.byteLength(JSON.stringify(read.messages))
      assert.ok(bytes <= PAGE_BYTES, `the notes take ${bytes} bytes`)
      assert.deepEqual([read.total, read.shown], [5, 2])
      assert.deepEqual(
        read.messages.map((m) => [m.id, m.kind, m.state]),
        [
          [newest.id, 'note', 'queued'],
          [older[3].id, 'note', 'queued'],
        ],
      )
      assert.equal(
        read.messages[0].body,
        `${long.slice(0, TRANSCRIPT_ITEM_MAX)}\n… (${long.length} characters; cut here)`,
      )
      assert.equal(read.messages[1].body, older[3].body, 'one that fits is whole')
      assert.equal(ledger.message(newest.id).body, long, 'the whole stays with the message')
      // Without `unread`, every message the human has, read or not, the same way.
      const everything = ledger.latestMessages(human)
      assert.deepEqual([everything.total, everything.shown], [7, 2])
      assert.ok(Buffer.byteLength(JSON.stringify(everything.messages)) <= PAGE_BYTES)
    })
  })

  it('shows a task with its whole thread, oldest first', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Format?',
      })
      ledger.answer(question.id, { from: question.recipientId, body: 'JSON' })
      assert.deepEqual(
        ledger.task(project.id, 1).messages.map((m) => [m.kind, m.sender, m.recipient]),
        [
          ['task', 'chief', 'zeus'],
          ['question', 'zeus', 'chief'],
          ['answer', 'chief', 'zeus'],
        ],
      )
      assert.equal(ledger.task(project.id, 99), null)
    })
  })

  it('reads a task in a frame however long its brief and thread: the brief whole first, then the newest messages, a body that does not fit cut and marked', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const cutAt = (text, max) => `${text.slice(0, max)}\n… (${text.length} characters; cut here)`
      /** A task for zeus with this brief, a question and its answer, then this result. */
      const worked = (brief, result) => {
        const { task, message } = ledger.createTask(project.id, {
          from: 'chief',
          to: 'zeus',
          body: brief,
        })
        deliver(ledger, message)
        const question = ledger.ask(project.id, {
          from: 'zeus',
          to: 'chief',
          task: task.number,
          body: 'JSON or YAML?',
        })
        ledger.answer(question.id, { from: question.recipientId, body: 'JSON.' })
        ledger.recordResult(project.id, task.number, { body: result })
        return task.number
      }
      // A brief and a result that pass the page's 1 MiB frame together, each
      // longer than PAGE_BYTES alone.
      const brief = `Fix the parser.\n${'Context, with "quotes" and a path, /src/x.js.\n'.repeat(13_000)}`
      const result = 'A finding, with the evidence for it. '.repeat(16_000)
      const long = worked(brief, result)
      assert.ok(Buffer.byteLength(JSON.stringify(ledger.task(project.id, long))) > 1024 * 1024)

      const read = ledger.taskThatFits(project.id, long)
      const bytes = Buffer.byteLength(JSON.stringify(read))
      assert.ok(bytes <= PAGE_BYTES, `the task takes ${bytes} bytes`)
      assert.deepEqual(
        [read.body, read.bodyCut],
        [cutAt(brief, TRANSCRIPT_ITEM_MAX), true],
        'its brief, cut and marked',
      )
      assert.deepEqual(
        read.messages.map((m) => [m.kind, m.body, m.bodyCut === true]),
        [
          ['task', cutAt(brief, TRANSCRIPT_ITEM_MAX), true],
          ['question', 'JSON or YAML?', false],
          ['answer', 'JSON.', false],
          ['result', cutAt(result, TRANSCRIPT_ITEM_MAX), true],
        ],
        'every message stays, a long body cut and marked',
      )
      assert.equal(ledger.task(project.id, long).body, brief, 'the whole stays with the task')

      // A brief that fits stays whole, first in line, though the result is cut.
      const shorter = brief.slice(0, 300_000)
      const read2 = ledger.taskThatFits(project.id, worked(shorter, result))
      assert.ok(Buffer.byteLength(JSON.stringify(read2)) <= PAGE_BYTES)
      assert.deepEqual([read2.body, 'bodyCut' in read2], [shorter, false])
      assert.deepEqual(read2.messages.at(-1).body, cutAt(result, TRANSCRIPT_ITEM_MAX))
      // A task that fits is read as it is.
      assert.deepEqual(
        ledger.taskThatFits(project.id, worked('Lexer', 'Lexer done')),
        ledger.task(project.id, 3),
      )
      assert.equal(ledger.taskThatFits(project.id, 99), null)
    })
  })

  it('leaves out the earliest messages of a thread too long for the frame, never a task message, and says how many', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const brief = 'Keep the parser going. '.repeat(30_000)
      const { message } = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: brief })
      deliver(ledger, message)
      // Twelve hundred notes on the task: more than a frame even cut to their lines.
      const notes = Array.from({ length: 1_200 }, (_, n) =>
        ledger.note(project.id, {
          from: 'zeus',
          to: 'chief',
          task: 1,
          body: `Progress ${n}: ${'step '.repeat(100)}`,
        }),
      )
      const read = ledger.taskThatFits(project.id, 1)
      const bytes = Buffer.byteLength(JSON.stringify(read))
      assert.ok(bytes <= PAGE_BYTES, `the task takes ${bytes} bytes`)
      assert.equal(
        read.body,
        `${brief.slice(0, TRANSCRIPT_ITEM_MAX)}\n… (${brief.length} characters; cut here)`,
      )
      assert.ok(read.messagesLeftOut > 0)
      assert.equal(read.messages.length + read.messagesLeftOut, 1_201)
      // The brief as delivered stays, cut to its line; the newest notes are whole.
      assert.deepEqual(
        [read.messages[0].id, read.messages[0].body, read.messages[0].bodyCut],
        [message.id, `… (${brief.length} characters; cut here)`, true],
      )
      assert.deepEqual(
        read.messages.slice(-2).map((m) => [m.id, m.body]),
        notes.slice(-2).map((m) => [m.id, m.body]),
      )
      assert.ok(
        read.messages
          .slice(1)
          .every((m, at) => m.id === notes[notes.length - read.messages.length + 1 + at].id),
        'the notes kept are the newest, with none between them left out',
      )
    })
  })

  it('names the task a participant is on, with its thread', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      assert.equal(ledger.activeTask(id('zeus')), null)
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'One' })
      assert.equal(ledger.activeTask(id('zeus')), null, 'a queued task is not started')
      assert.equal(ledger.activeTask(id('zeus'), { queued: true }).state, 'queued')
      deliver(ledger, ledger.task(project.id, 1).messages[0])
      assert.deepEqual(
        [ledger.activeTask(id('zeus')).number, ledger.activeTask(id('zeus')).messages.length],
        [1, 1],
      )
      ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'Format?' })
      assert.equal(ledger.activeTask(id('zeus')).state, 'waiting')
    })
  })

  it('lists an inbox newest first', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const one = ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'one' })
      const two = ledger.note(project.id, { from: 'diana', to: 'chief', body: 'two' })
      assert.deepEqual(
        ledger.inbox(id('chief')).map((m) => m.id),
        [two.id, one.id],
      )
    })
  })
})

describe('tiered dispatch: open tasks the daemon assigns', () => {
  /** A chief with two standard workers, a light worker, a complex advisor and a reviewer. */
  function tiered(ledger) {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      chief: { harness: 'claude-code' },
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
      from: 'chief',
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
          ['chief', null],
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
        chief: { harness: 'pi' },
        staff: [{ agent: 'zeus', harness: 'pi', role: 'reviewer', tier: 'critical' }],
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
        ['open', null, 'worker', 'standard', 'chief', null],
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
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
      })
    })
  })

  it('refuses a bad tier or pool and critical work without its purpose; a tier nobody holds goes to the nearest', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      const refuses = (extra, code) =>
        assert.throws(() => openTask(ledger, project, extra), { code })
      refuses({ tier: 'max' }, 'invalid-tier')
      refuses({ pool: 'chief' }, 'invalid-pool')
      // Workers are standard and light: critical and complex work goes to the
      // nearest tier held, the next one up first, and says what was asked.
      const up = openTask(ledger, project, { tier: 'critical', purpose: 'architecture' })
      assert.deepEqual([up.task.tier, up.asked], ['standard', 'critical'])
      const near = openTask(ledger, project, { pool: 'worker', tier: 'complex' })
      assert.deepEqual([near.task.tier, near.asked], ['standard', 'complex'])
      // Light advice with only a complex advisor goes up to it.
      const advice = openTask(ledger, project, { pool: 'advisor', tier: 'light' })
      assert.deepEqual([advice.task.tier, advice.asked], ['complex', 'light'])
      // Held tiers stay as asked.
      assert.equal(openTask(ledger, project, { tier: 'light' }).asked, undefined)
      for (const number of [1, 2, 3, 4]) ledger.cancelTask(project.id, number, { by: 'chief' })
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
        () => ledger.createTask(project.id, { from: 'chief', pool: 'designer', body: 'A logo' }),
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
        from: 'chief',
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
        ['done', 'chief'],
        'the drawing goes to the chief',
      )
    })
  })

  it('counts a member for every role it holds when a tier is checked, not only its first', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      const before = openTask(ledger, project, { pool: 'advisor', tier: 'light' })
      assert.equal(before.task.tier, 'complex', 'no light advisor yet: the nearest')
      ledger.cancelTask(project.id, before.task.number, { by: 'chief' })
      ledger.setRoles(project.id, 'hera', ['worker', 'advisor'])
      const { task } = openTask(ledger, project, { pool: 'advisor', tier: 'light' })
      assert.deepEqual([task.pool, task.tier, task.state], ['advisor', 'light', 'open'])
    })
  })

  it('lets only the chief and the human create tasks', async () => {
    await withLedger((ledger) => {
      const { project } = tiered(ledger)
      assert.throws(() => openTask(ledger, project, { from: 'zeus' }), {
        code: 'not-a-coordinator',
      })
      assert.throws(
        () => ledger.createTask(project.id, { from: 'zeus', to: 'chief', body: 'Do it' }),
        { code: 'not-a-coordinator' },
      )
      assert.throws(
        () => ledger.createTask(project.id, { from: 'athena', to: 'chief', body: 'Plan' }),
        { code: 'not-a-coordinator' },
      )
      assert.equal(
        ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'My own' }).task.assignee,
        'chief',
      )
      assert.throws(
        () =>
          ledger.createTask(project.id, {
            from: 'human',
            pool: 'advisor',
            tier: 'complex',
            body: 'Look',
          }),
        { code: 'advice-for-the-chief' },
        'advice is for the chief alone to ask',
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
      assert.equal(
        openTask(ledger, project, { pool: 'reviewer', tier: 'complex' }).task.tier,
        'standard',
        'no complex reviewer: the nearest tier held',
      )
      deliver(ledger, ledger.assignTask(project.id, 2, id('nemesis')).message)
      const done = ledger.recordResult(project.id, 2, { body: 'No test for empty input.' })
      assert.deepEqual(
        [done.task.state, done.message.recipient, done.message.body],
        ['done', 'chief', 'No test for empty input.'],
      )
      assert.equal(ledger.task(project.id, 1).state, 'done', 'the chief decides the work')
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
        from: 'chief',
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
      const told = ledger.inbox(id('chief'))[0]
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
        'Write the parser\n\nReassigned from @zeus-amber-pine (ran out of quota after starting); check the working tree for partial changes.',
      )
      deliver(ledger, second.message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      assert.throws(() => ledger.releaseTask(project.id, 1, { because: 'x' }), {
        code: 'invalid-transition',
      })
      const direct = ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Ship' })
      assert.throws(() => ledger.releaseTask(project.id, direct.task.number, { because: 'x' }), {
        code: 'invalid-transition',
      })
    })
  })

  it('gives a paused task back to the board too, and withdraws a brief still held for the human', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      ledger.pauseTask(project.id, 1, { by: 'chief' })
      const paused = ledger.releaseTask(project.id, 1, { because: 'by @human' }).task
      assert.deepEqual([paused.state, paused.assignee], ['open', null])
      assert.deepEqual(
        ledger.candidates(project.id, 1).map((c) => [c.handle, c.hadIt]),
        [
          ['zeus', true],
          ['diana', false],
        ],
        'the member it was taken from is marked, for the daemon to pass over',
      )
      assert.match(
        paused.body,
        /Reassigned from @zeus-amber-pine \(by @human\); check the working tree/,
      )
      assert.equal(
        ledger.inbox(id('chief'))[0].body,
        'T-1 was taken back from @zeus-amber-pine (by @human) and waits for another standard worker.',
      )

      // With human approval required, a brief waits gated; it goes with the task.
      ledger.setGate(project.id, true)
      openTask(ledger, project, { body: 'Write the lexer' })
      const { message } = ledger.assignTask(project.id, 2, id('diana'))
      assert.equal(ledger.message(message.id).state, 'gated')
      ledger.releaseTask(project.id, 2, { because: 'by @human' })
      assert.equal(ledger.message(message.id).state, 'cancelled', 'never reaches the old window')

      // A task paused before anyone took it has nobody to take it from.
      openTask(ledger, project, { body: 'Write the docs' })
      ledger.pauseTask(project.id, 3, { by: 'chief' })
      const unassigned = ledger.releaseTask(project.id, 3, { because: 'by @human' }).task
      assert.deepEqual([unassigned.state, unassigned.body], ['open', 'Write the docs'])
    })
  })

  it('cancels an open task; a task queued by handle still follows the old path', async () => {
    await withLedger((ledger) => {
      const { project, id } = tiered(ledger)
      openTask(ledger, project)
      assert.equal(ledger.cancelTask(project.id, 1, { by: 'chief' }).state, 'cancelled')
      assert.equal(ledger.task(project.id, 1).messages.length, 0)
      const direct = ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Ship it' })
      assert.deepEqual(
        [direct.task.state, direct.task.pool, direct.task.tier],
        ['queued', null, null],
      )
      assert.equal(ledger.nextDelivery(id('chief')).id, direct.message.id)
    })
  })
})

describe("sessions: a member's named windows", () => {
  /** A staff with a tiered task open for a standard worker. */
  function opened(ledger, body = 'Write the parser') {
    const { project, id } = staff(ledger)
    ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
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
        ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
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
        from: 'chief',
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

  it('never runs out of session names: when every name it draws is taken, the name takes a number', async () => {
    await withDir((dir) => {
      // Names are never reused, so a member a thousand sessions in draws only taken ones.
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: clock(),
        names: () => 'amber-pine',
      })
      try {
        const { project, id } = opened(ledger)
        for (const body of ['Lexer', 'Docs', 'Tests']) {
          ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
        }
        assert.deepEqual(
          [1, 2, 3].map(
            (number) => ledger.assignTask(project.id, number, id('zeus')).task.assignee,
          ),
          ['zeus-amber-pine', 'zeus-amber-pine-2', 'zeus-amber-pine-3'],
        )
        assert.equal(
          ledger.assignTask(project.id, 4, id('diana')).task.assignee,
          'diana-amber-pine',
        )
      } finally {
        ledger.close()
      }
    })
  })

  it('continues a session with --after: the follow-up goes to the same window, alive and free', async () => {
    await withLedger((ledger) => {
      const { project, id } = opened(ledger)
      deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
      assert.throws(
        () => ledger.createTask(project.id, { from: 'chief', after: 1, body: 'Also the lexer' }),
        { code: 'session-busy' },
      )
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const { task, message } = ledger.createTask(project.id, {
        from: 'chief',
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
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      assert.ok(
        sessionOf(ledger, project.id, 'zeus-amber-pine'),
        'the session stays while T-2 is unaccepted',
      )
      ledger.acceptTask(project.id, 2, { by: 'chief' })
      assert.ok(
        sessionOf(ledger, project.id, 'zeus-amber-pine'),
        'accepted work keeps the session: only the human ends it',
      )
      const more = ledger.createTask(project.id, { from: 'chief', after: 1, body: 'More' })
      assert.equal(more.task.assignee, 'zeus-amber-pine', 'and a follow-up still finds it')
      ledger.cancelTask(project.id, more.task.number, { by: 'chief' })
      ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
      assert.equal(sessionOf(ledger, project.id, 'zeus-amber-pine'), undefined)
      assert.throws(
        () => ledger.createTask(project.id, { from: 'chief', after: 1, body: 'More' }),
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
          ['chief', null, []],
          ['zeus', null, []],
          ['diana', null, []],
          ['zeus-amber-pine', 'zeus', [1]],
        ],
      )
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
      board = ledger.board(project.id)
      assert.deepEqual(
        board.lanes.map((l) => [
          l.participant.handle,
          l.tasks.map((t) => [t.number, t.state, t.session]),
        ]),
        [
          ['human', []],
          ['chief', []],
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
      const { task } = ledger.reopenTask(project.id, 1, {
        by: 'chief',
        body: 'Handle comments too',
      })
      assert.deepEqual([task.assignee, task.state], ['zeus-amber-pine', 'queued'])
      ledger.cancelTask(project.id, 1, { by: 'chief' })
      assert.ok(sessionOf(ledger, project.id, 'zeus-amber-pine'), 'the session stays')
      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Again' }), {
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
      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Again' }), {
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
        from: 'chief',
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
  /** A chief, a standard worker and a reviewer on another model, with the gate on. */
  function gated(ledger) {
    const project = ledger.createProject({
      directory: '/work/app',
      name: 'app',
      chief: { harness: 'claude-code' },
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
  /** The chief's task for a worker, assigned by the daemon: the task and its brief. */
  function briefed(ledger, project, id) {
    ledger.createTask(project.id, {
      from: 'chief',
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
      const { project } = staff(ledger)
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
            chief: { harness: 'pi' },
            gate: 1,
          }),
        { code: 'invalid-gate' },
      )
      assert.equal(ledger.projects().length, 1, 'nothing was created')
    })
  })

  it("holds the chief's brief for the human, who passes it on: nothing reaches the worker before", async () => {
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

  it('declines a brief: the task is cancelled, its session kept, and the chief told why', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, message } = briefed(ledger, project, id)
      const declined = ledger.declineMessage(message.id, { by: 'human' })
      assert.deepEqual([declined.state, declined.reason], ['cancelled', 'declined by @human'])
      assert.equal(ledger.task(project.id, number).state, 'cancelled')
      assert.deepEqual(noteTo(ledger, id('chief')), [
        ['human', number, 'queued', '@human declined T-1 (Parser). It is cancelled.'],
      ])
      assert.equal(ledger.nextDelivery(id('chief')).kind, 'note', 'the note goes without the gate')
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
        noteTo(ledger, id('chief'))[0][3],
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
      assert.equal(ledger.nextDelivery(id('chief')), null, 'the chief waits')
      assert.throws(() => ledger.declineMessage(message.id, { by: 'human' }), {
        code: 'not-declinable',
        message: /a result is passed on, not declined/,
      })
      assert.equal(ledger.approveMessage(message.id, { by: 'human' }).state, 'queued')
      assert.equal(ledger.nextDelivery(id('chief')).id, message.id)

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

  it("holds a worker's question for the human, who passes it to the chief; one its window answered first goes no further", async () => {
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
          to: 'chief',
          task: number,
          body: 'Which colour?',
        })
        assert.equal(question.state, 'gated')
        assert.equal(ledger.task(project.id, number).state, 'waiting')
        assert.equal(ledger.nextDelivery(id('chief')), null)
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
        assert.equal(ledger.nextDelivery(id('chief')).id, question.id)
        assert.deepEqual(
          ledger.board(project.id).overdue.map((m) => m.id),
          [question.id],
          'and overdue once on its way to the chief',
        )

        const other = ledger.ask(project.id, {
          from: session,
          to: 'chief',
          task: number,
          questions: [
            {
              question: 'Which size?',
              header: 'Size',
              options: [{ label: 'Large' }, { label: 'Small' }],
            },
          ],
        })
        assert.throws(() => ledger.answer(other.id, { from: id('human'), body: 'Large' }), {
          code: 'not-your-question',
        })
        const answer = ledger.answer(other.id, { from: id(session), choices: [['Large']] })
        assert.deepEqual([answer.state, answer.recipient], ['read', session])
        assert.deepEqual(
          [ledger.message(other.id).state, ledger.message(other.id).reason],
          ['cancelled', `answered by @${session}`],
          'the chief never gets a question its window answered first',
        )
        assert.equal(ledger.answerTo(other.id).id, answer.id)
        assert.deepEqual(gatedIds(ledger, project), [])
      } finally {
        ledger.close()
      }
    })
  })

  it("holds the chief's answer for the human, who passes it on or declines it for another", async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, session } = working(ledger, project, id)
      const question = ledger.ask(project.id, {
        from: session,
        to: 'chief',
        task: number,
        body: 'Which colour?',
      })
      deliver(ledger, ledger.approveMessage(question.id, { by: 'human' }))
      const answer = ledger.answer(question.id, { from: question.recipientId, body: 'Blue' })
      assert.equal(answer.state, 'gated')
      assert.equal(ledger.answerTo(question.id), null, 'the door keeps waiting')
      assert.throws(() => ledger.answer(question.id, { from: question.recipientId, body: 'Red' }), {
        code: 'already-answered',
      })
      const declined = ledger.declineMessage(answer.id, { by: 'human' })
      assert.equal(declined.state, 'cancelled')
      assert.equal(
        noteTo(ledger, id('chief'))[0][3],
        `@human declined your answer to m-${question.id}. Answer it again: cf answer m-${question.id} "…"`,
      )
      const again = ledger.answer(question.id, { from: question.recipientId, body: 'Red' })
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

  it('keeps a question overdue while its answer waits for the human, and again once that is declined', async () => {
    await withDir((dir) => {
      let at = Date.parse('2026-09-20T10:00:00.000Z')
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: () => new Date(at),
        names: names(),
      })
      try {
        const { project, id } = gated(ledger)
        const { number, session } = working(ledger, project, id)
        const question = ledger.ask(project.id, {
          from: session,
          to: 'chief',
          task: number,
          body: 'Which colour?',
        })
        deliver(ledger, ledger.approveMessage(question.id, { by: 'human' }))
        at += OVERDUE_MS
        const overdue = () => ledger.board(project.id).overdue.map((m) => m.id)
        assert.deepEqual(overdue(), [question.id])
        const answer = ledger.answer(question.id, { from: question.recipientId, body: 'Blue' })
        assert.deepEqual(overdue(), [question.id], 'the window still waits for an answer')
        ledger.declineMessage(answer.id, { by: 'human' })
        assert.deepEqual(overdue(), [question.id], 'declined, it is open again')
        const again = ledger.answer(question.id, { from: question.recipientId, body: 'Red' })
        ledger.approveMessage(again.id, { by: 'human' })
        assert.deepEqual(overdue(), [], 'answered at last')
      } finally {
        ledger.close()
      }
    })
  })

  it('holds a choice answer too, and lands it read for the door once approved', async () => {
    await withLedger((ledger) => {
      const { project, id } = gated(ledger)
      const { number, session } = working(ledger, project, id)
      const question = ledger.ask(project.id, {
        from: session,
        to: 'chief',
        task: number,
        questions: [{ question: 'Colour?', header: 'Colour', options: [{ label: 'red' }] }],
      })
      deliver(ledger, ledger.approveMessage(question.id, { by: 'human' }))
      const answer = ledger.answer(question.id, { from: question.recipientId, choices: [['red']] })
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
      const own = ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Plan' })
      assert.equal(own.message.state, 'queued', "the chief's own work")
      assert.equal(
        ledger.note(project.id, { from: 'chief', to: 'human', body: 'T-1 shipped.' }).state,
        'queued',
        'a note for the human',
      )
      assert.equal(
        ledger.note(project.id, { to: 'chief', body: 'T-9 waits for a free worker.' }).state,
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
      ledger.cancelTask(project.id, first.number, { by: 'chief' })
      assert.equal(ledger.message(first.message.id).state, 'cancelled')
      const second = briefed(ledger, project, id)
      ledger.removeMember(project.id, 'zeus')
      assert.equal(ledger.message(second.message.id).state, 'cancelled')
      assert.deepEqual(gatedIds(ledger, project), [])
    })
  })
})

describe('a plan on the board: needs', () => {
  /** A chief and two standard workers, with T-1 open for a worker. */
  function planned(ledger) {
    const { project, id } = staff(ledger)
    ledger.createTask(project.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Lexer',
    })
    return { project, id }
  }
  const open = (ledger, project, body, extra = {}) =>
    ledger.createTask(project.id, {
      from: 'chief',
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

  it('a task given by name waits on the board for what it needs, then goes to its window', async () => {
    await withLedger((ledger) => {
      const { project, id } = planned(ledger)
      const own = ledger.createTask(project.id, {
        from: 'chief',
        to: 'chief',
        body: 'Write it up',
        needs: [1],
      })
      assert.equal(own.message, null, 'nothing goes to the window yet')
      assert.deepEqual(
        [own.task.state, own.task.assignee, own.task.blockedBy],
        ['open', 'chief', [1]],
      )
      assert.deepEqual(
        ledger.events(project.id).find((e) => e.kind === 'task.opened' && e.data.task === 2).data,
        { task: 2, from: 'chief', to: 'chief', needs: [1] },
      )
      finish(ledger, project, id, 1)
      assert.equal(ledger.task(project.id, 2).state, 'open', 'done is not accepted')
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      const freed = ledger.task(project.id, 2)
      assert.deepEqual([freed.state, freed.blockedBy], ['queued', []])
      const queued = ledger
        .inbox(id('chief'))
        .find((message) => message.kind === 'task' && message.taskNumber === 2)
      assert.deepEqual([queued.state, queued.recipient], ['queued', 'chief'])
      assert.deepEqual(
        ledger.events(project.id).find((e) => e.kind === 'task.state' && e.data.task === 2).data,
        { task: 2, from: 'open', to: 'queued', message: queued.id },
      )
    })
  })

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
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      const freed = ledger.task(project.id, 2)
      assert.deepEqual([freed.needs, freed.blockedBy], [[{ number: 1, state: 'accepted' }], []])
      assert.deepEqual(
        ledger.events(project.id).find((e) => e.kind === 'task.opened' && e.data.task === 2).data,
        { task: 2, from: 'chief', pool: 'worker', tier: 'standard', needs: [1] },
      )
      // A need already accepted blocks nothing; one named twice counts once.
      const cli = open(ledger, project, 'CLI', { needs: [1, 2, 2] })
      assert.deepEqual(cli.blockedBy, [2])
      assert.equal(id('chief') > 0, true)
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
      ledger.acceptTask(project.id, 3, { by: 'chief' })
      assert.deepEqual(ledger.task(project.id, 1).blockedBy, [])
      ledger.assignTask(project.id, 1, id('zeus'))
      assert.throws(() => open(ledger, project, 'Too late', { before: [1] }), {
        code: 'not-on-the-board',
        message: /T-1 is queued/,
      })
      assert.equal(ledger.task(project.id, 4), null, 'nothing of the refused task is left')
    })
  })

  it('refuses a need that does not exist or is cancelled, for a task given by name too', async () => {
    await withLedger((ledger) => {
      const { project } = planned(ledger)
      assert.throws(() => open(ledger, project, 'Parser', { needs: [9] }), { code: 'unknown-task' })
      assert.throws(() => open(ledger, project, 'Parser', { needs: ['T-1'] }), {
        code: 'invalid-needs',
      })
      ledger.cancelTask(project.id, 1, { by: 'chief' })
      assert.throws(() => open(ledger, project, 'Parser', { needs: [1] }), {
        code: 'need-cancelled',
      })
      for (const address of [{ to: 'chief' }, { to: 'zeus' }]) {
        assert.throws(
          () =>
            ledger.createTask(project.id, { from: 'chief', ...address, body: 'Plan', needs: [1] }),
          { code: 'need-cancelled' },
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
      ledger.cancelTask(project.id, 1, { by: 'chief' })
      const parser = ledger.task(project.id, 2)
      assert.deepEqual([parser.needs, parser.blockedBy], [[{ number: 1, state: 'cancelled' }], [1]])
      ledger.cancelTask(project.id, 2, { by: 'chief' })
      assert.equal(ledger.task(project.id, 2).state, 'cancelled', 'the chief decides')
    })
  })
})

describe('the transcript copy', () => {
  const item = (id, role, text, extra = {}) => ({ id, role, text, complete: true, ...extra })
  /** A worker's task assigned to a session with a conversation of its own. */
  function windowed(ledger) {
    const { project, id } = staff(ledger)
    ledger.createTask(project.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Parser',
    })
    const { message } = ledger.assignTask(project.id, 1, id('zeus'))
    const conversation = ledger.startConversation(message.recipientId, { harness: 'claude-code' })
    return { project, id, session: message.recipientId, conversation }
  }

  it("a task's part of a window's copy starts at its brief: the chief's own, after its other work", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const own = ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Write it up' })
      const conversation = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      ledger.copyTranscript(conversation.id, [
        item('u0', 'user', 'Earlier, from the human'),
        item('a0', 'assistant', 'Looking into it'),
        item(
          'u1',
          'user',
          `[ConsensFlow m-${own.message.id} · T-1 · task from @chief]\nWrite it up`,
        ),
        item('a1', 'assistant', 'Writing'),
      ])
      const { items, total } = ledger.transcript(project.id, 1)
      assert.equal(total, 2)
      assert.deepEqual(
        items.map((i) => i.id),
        ['u1', 'a1'],
      )
    })
  })

  it("keeps all of a task's part across a pause, a resume and a reopen: it starts at the first brief", async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      const marker = (message) => `[ConsensFlow m-${message.id} · T-1 · task from @chief]`
      const brief = ledger.task(project.id, 1).messages[0]
      deliver(ledger, brief)
      ledger.copyTranscript(conversation.id, [
        item('u1', 'user', `${marker(brief)}\nParser`),
        item('a1', 'assistant', 'Wrote src/parser.js'),
      ])
      ledger.pauseTask(project.id, 1, { by: 'chief' })
      const resumed = ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Use v2' }).message
      deliver(ledger, resumed)
      ledger.copyTranscript(
        conversation.id,
        [
          item('u2', 'user', `${marker(resumed)}\nResumed: Use v2`),
          item('a2', 'assistant', 'On v2'),
        ],
        { from: 2 },
      )
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const again = ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Add tests' }).message
      deliver(ledger, again)
      ledger.copyTranscript(
        conversation.id,
        [item('u3', 'user', `${marker(again)}\nAdd tests`), item('a3', 'assistant', 'Tests added')],
        { from: 4 },
      )
      const { items, total } = ledger.transcript(project.id, 1)
      assert.equal(total, 6)
      assert.deepEqual(
        items.map((i) => i.id),
        ['u1', 'a1', 'u2', 'a2', 'u3', 'a3'],
      )
    })
  })

  it('copies what is new, brings an item still being written up to date, and reads it by task', async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      assert.deepEqual(ledger.transcript(project.id, 1), { items: [], total: 0 })
      const first = [
        item('u1', 'user', '[ConsensFlow m-1 · T-1 · task from @chief]\nParser'),
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
          ['u1', 'user', '[ConsensFlow m-1 · T-1 · task from @chief]\nParser', true, null],
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

  it("cuts only a tool's output longer than it keeps, files an unknown role as custom, and skips what has no id", async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      const long = 'x'.repeat(TRANSCRIPT_ITEM_MAX + 5)
      ledger.copyTranscript(conversation.id, [
        item('big', 'tool', long),
        item('said', 'assistant', long),
        item('asked', 'user', long),
        item('odd', 'system', 'hm'),
        { role: 'user', text: 'no id' },
        item('none', 'assistant', undefined),
      ])
      const { items } = ledger.transcript(project.id, 1)
      assert.deepEqual(
        items.map((i) => [i.id, i.role, i.text.length]),
        [
          ['big', 'tool', TRANSCRIPT_ITEM_MAX + `\n… (${long.length} characters; cut here)`.length],
          // Words are kept whole: a lead switched in reads them.
          ['said', 'assistant', long.length],
          ['asked', 'user', long.length],
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

  it('reads the last of a long copy in a frame: the newest items that fit, one too long cut, and how many', async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      // More than the page's 1 MiB frame: twenty tool outputs at the most the
      // copy keeps of one, then the agent's words, longer than half of it alone.
      const output = 'PASS src/parser.test.js '.repeat(3_000)
      const words = 'The parser is done, and here is why. '.repeat(16_000)
      ledger.copyTranscript(conversation.id, [
        item('u1', 'user', '[ConsensFlow m-1 · T-1 · task from @chief]\nParser'),
        ...Array.from({ length: 20 }, (_, n) => item(`t${n + 1}`, 'tool', output)),
        item('a1', 'assistant', words),
      ])
      const whole = ledger.transcript(project.id, 1)
      assert.ok(Buffer.byteLength(JSON.stringify(whole)) > 1024 * 1024)

      const read = ledger.latestTranscript(project.id, 1)
      const bytes = Buffer.byteLength(JSON.stringify(read.items))
      assert.ok(bytes <= PAGE_BYTES, `the items take ${bytes} bytes`)
      assert.deepEqual([read.total, read.shown], [22, 8])
      assert.deepEqual(
        read.items.map((i) => i.id),
        ['t14', 't15', 't16', 't17', 't18', 't19', 't20', 'a1'],
      )
      assert.equal(
        read.items.at(-1).text,
        `${words.slice(0, TRANSCRIPT_ITEM_MAX)}\n… (${words.length} characters; cut here)`,
      )
      // A tool's output was cut once, when it was copied.
      assert.equal(read.items[0].text, whole.items[14].text)
      assert.match(read.items[0].text, /^PASS[^…]*… \(72000 characters; cut here\)$/)
      assert.equal(whole.items.at(-1).text, words, 'the copy keeps the words whole')
      assert.deepEqual(ledger.latestTranscript(project.id, 1, { limit: 2 }), {
        items: read.items.slice(-2),
        total: 22,
        shown: 2,
      })
      assert.deepEqual(ledger.latestTranscript(project.id, 1, { limit: 0 }), {
        items: [],
        total: 22,
        shown: 0,
      })
    })
  })

  it("follows a task's window across a continued conversation, and shows nothing for a task on the board", async () => {
    await withLedger((ledger) => {
      const { project, id, session, conversation } = windowed(ledger)
      ledger.copyTranscript(conversation.id, [item('a1', 'assistant', 'Parser done')])
      deliver(ledger, ledger.task(project.id, 1).messages[0])
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const again = ledger.createTask(project.id, {
        from: 'chief',
        after: 1,
        body: 'Now the lexer',
      })
      assert.equal(again.task.assignee, ledger.task(project.id, 1).assignee)
      ledger.copyTranscript(conversation.id, [item('a2', 'assistant', 'Lexer done')], { from: 1 })
      assert.deepEqual(
        ledger.transcript(project.id, 2).items.map((i) => i.text),
        ['Parser done', 'Lexer done'],
        'the follow-up shows the same window',
      )
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Tests',
      })
      assert.deepEqual(ledger.transcript(project.id, 3), { items: [], total: 0 })
      assert.throws(() => ledger.transcript(project.id, 9), { code: 'unknown-task' })
      assert.equal(session > 0 && id('chief') > 0, true)
    })
  })
})

describe('pause and resume', () => {
  /** A worker's tiered task delivered into its session window. */
  function running(ledger) {
    const { project, id } = staff(ledger)
    ledger.createTask(project.id, {
      from: 'chief',
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
        to: 'chief',
        task: 1,
        body: 'Which?',
      })
      const paused = ledger.pauseTask(project.id, 1, { by: 'chief' })
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
        by: 'chief',
      })
      ledger.createTask(project.id, {
        from: 'chief',
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
      assert.throws(() => ledger.pauseTask(project.id, 2, { by: 'chief' }), {
        code: 'invalid-transition',
      })
      assert.equal(id('chief') > 0, true)
    })
  })

  it("refuses to pause the chief's own work or finished work", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const own = ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Plan' })
      assert.throws(() => ledger.pauseTask(project.id, own.task.number, { by: 'chief' }), {
        code: 'own-work',
      })
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      deliver(ledger, ledger.assignTask(project.id, 2, id('zeus')).message)
      ledger.recordResult(project.id, 2, { body: 'Done' })
      assert.throws(() => ledger.pauseTask(project.id, 2, { by: 'chief' }), {
        code: 'invalid-transition',
      })
      assert.throws(() => ledger.pauseTask(project.id, 9, { by: 'chief' }), {
        code: 'unknown-task',
      })
    })
  })

  it('resumes into the same window with the words, with the brief first when it never arrived', async () => {
    await withLedger((ledger) => {
      const { project, session } = running(ledger)
      ledger.pauseTask(project.id, 1, { by: 'chief' })
      const { task, message } = ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      assert.deepEqual(
        [task.state, task.assignee, message.recipient, message.kind, message.state],
        ['queued', session, session, 'task', 'queued'],
      )
      assert.equal(message.body, 'Resumed: Go on')
      assert.throws(() => ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Again' }), {
        code: 'invalid-transition',
      })
      assert.throws(() => ledger.resumeTask(project.id, 1, { by: 'chief', body: '' }), {
        code: 'invalid-text',
      })
      // Paused before its brief was delivered: the brief goes in with the words.
      ledger.createTask(project.id, {
        from: 'chief',
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
        const { project, id } = staff(ledger)
        ledger.createTask(project.id, {
          from: 'chief',
          pool: 'worker',
          tier: 'standard',
          body: 'Parser',
        })
        ledger.pauseTask(project.id, 1, { by: 'chief' })
        const opened = ledger.resumeTask(project.id, 1, { by: 'chief', body: 'When you can' })
        assert.deepEqual(
          [opened.task.state, opened.task.assignee, opened.message],
          ['open', null, null],
        )
        assert.equal(opened.task.body, 'Parser\n\nResumed: When you can')

        deliver(ledger, ledger.assignTask(project.id, 1, id('zeus')).message)
        ledger.pauseTask(project.id, 1, { by: 'chief' })
        ledger.endSession(project.id, 'zeus-amber-pine', { by: 'human' })
        const fresh = ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Try again' })
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

describe('a task held with its window while its member is out of quota', () => {
  it('is paused until its time, goes on by itself then, and forgets the hold on any move', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Lexer',
      })
      const { message } = ledger.assignTask(project.id, 1, id('zeus'))
      ledger.beginDelivery(message.id)
      ledger.confirmDelivery(message.id, { item: 'x' })
      assert.throws(() => ledger.holdTask(project.id, 1, { until: 'soon', because: 'quota' }), {
        code: 'invalid-until',
      })
      const until = '2026-09-24T14:58:35.479Z'
      const held = ledger.holdTask(project.id, 1, { until, because: 'out of quota' })
      assert.deepEqual(
        [held.state, held.heldUntil, held.assignee],
        ['paused', until, message.recipient],
      )
      assert.deepEqual(
        ledger.events(project.id).find((e) => e.kind === 'task.state' && e.data.to === 'paused')
          .data,
        { task: 1, from: 'working', to: 'paused', by: null, because: 'out of quota', until },
      )
      assert.deepEqual(ledger.heldTasksDue('2026-09-24T14:58:35.478Z'), [], 'not yet')
      assert.deepEqual(ledger.heldTasksDue(until), [
        { projectId: project.id, number: 1, assigneeId: id(message.recipient) },
      ])
      // The daemon resumes in its own name: the same window, the same words as the human's Resume.
      const resumed = ledger.resumeTask(project.id, 1, { body: RESUME_WORDS })
      assert.deepEqual(
        [
          resumed.task.state,
          resumed.task.heldUntil,
          resumed.message.recipient,
          resumed.message.sender,
        ],
        ['queued', null, message.recipient, null],
      )
      assert.match(resumed.message.body, /^Resumed: Go on where you stopped\.$/)
      assert.deepEqual(ledger.heldTasksDue(until), [])
    })
  })
})
