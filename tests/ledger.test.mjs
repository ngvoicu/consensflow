import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { openLedger, SCHEMA_VERSION } from '../src/ledger/index.js'

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

/** A project with a lead and two workers, the shape most tests start from. */
function team(ledger) {
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    lead: { harness: 'claude-code' },
  })
  ledger.addMember(project.id, { agent: 'zeus', harness: 'claude-code', role: 'worker' })
  ledger.addMember(project.id, { agent: 'diana', harness: 'codex', role: 'worker' })
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
         ledger.addMember(project.id, { agent: 'zeus', harness: 'pi', role: 'worker' })
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

  it('adds members once each, and a PM as the second coordinator', async () => {
    await withLedger((ledger) => {
      const { project } = team(ledger)
      assert.throws(
        () => ledger.addMember(project.id, { agent: 'zeus', harness: 'pi', role: 'worker' }),
        { code: 'member-exists' },
      )
      assert.throws(
        () => ledger.addMember(project.id, { agent: 'hera', harness: 'pi', role: 'lead' }),
        { code: 'invalid-role' },
      )
      assert.throws(
        () => ledger.addMember(project.id, { agent: 'hera', harness: 'emacs', role: 'worker' }),
        { code: 'invalid-harness' },
      )
      const pm = ledger.addPm(project.id, { harness: 'codex' })
      assert.deepEqual([pm.handle, pm.role, pm.harness], ['pm', 'pm', 'codex'])
      assert.throws(() => ledger.addPm(project.id, { harness: 'pi' }), { code: 'member-exists' })
      ledger.addMember(project.id, { agent: 'athena', harness: 'opencode', role: 'advisor' })
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
        { agent: 'zeus', harness: 'claude-code', role: 'worker' },
        { agent: 'diana', harness: 'codex', role: 'worker' },
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
          { agent: 'zeus', harness: 'claude-code', role: 'worker' },
          { agent: 'athena', harness: 'opencode', role: 'advisor' },
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
      assert.deepEqual(ledger.lastTeam(), [{ agent: 'diana', harness: 'codex', role: 'worker' }])
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
      const back = ledger.addMember(project.id, { agent: 'zeus', harness: 'pi', role: 'reviewer' })

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
      ledger.addMember(project.id, { agent: 'hera', harness: 'pi', role: 'worker' })
      ledger.addMember(project.id, { agent: 'athena', harness: 'pi', role: 'advisor' })
      assert.deepEqual(notes(ledger, id('lead')), [], 'no window yet: its launch reads the team')
      assert.deepEqual(notes(ledger, pm.id), [])

      ledger.startConversation(id('lead'), { harness: 'claude-code' })
      ledger.startConversation(pm.id, { harness: 'pi' })
      ledger.addMember(project.id, { agent: 'apollo', harness: 'codex', role: 'worker' })
      ledger.addMember(project.id, { agent: 'metis', harness: 'codex', role: 'advisor' })
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      ledger.createTask(project.id, { from: 'pm', to: 'zeus', body: 'Estimate' })
      ledger.createTask(project.id, { from: 'human', to: 'zeus', body: 'Logo' })
      ledger.removeMember(project.id, 'zeus')

      assert.deepEqual(notes(ledger, id('lead')), [
        [null, '@apollo joined the team as a worker. Give it work with: cf task add @apollo "…"'],
        [null, '@zeus left the team; it takes no more tasks. Cancelled with it: T-1, T-2, T-3.'],
      ])
      assert.deepEqual(notes(ledger, pm.id), [
        [null, '@metis joined the team as an advisor. Give it work with: cf task add @metis "…"'],
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

  it('still delivers answers and notes to a worker busy with a task', async () => {
    await withLedger((ledger) => {
      const { project, id } = team(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'One' }).message,
      )
      ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Two' })
      const note = ledger.note(project.id, { from: 'lead', to: 'zeus', body: 'Use JSON' })
      assert.equal(ledger.nextDelivery(id('zeus')).id, note.id)
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
