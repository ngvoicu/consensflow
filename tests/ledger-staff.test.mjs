import assert from 'node:assert/strict'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { openLedger } from '../src/ledger/index.js'
import { sessionName } from '../src/ledger/names.js'
import { clock, deliver, staff, withDir, withLedger } from './ledger-fixtures.mjs'

/** A project's staff: members who join and leave, and their sessions (src/ledger/staff.js). */

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

  it('takes a member back in the role, harness and designer flag it rejoins with', async () => {
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
      // Its agent became an image agent meanwhile: it rejoins as one, and only as one.
      ledger.removeMember(project.id, 'zeus')
      const drawing = ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'codex',
        designer: true,
        role: 'designer',
        tier: 'light',
      })
      assert.deepEqual(
        [drawing.id, drawing.roles, drawing.harness, drawing.designer],
        [before, ['designer'], 'codex', true],
      )
      assert.throws(() => ledger.setRoles(project.id, 'zeus', ['designer', 'reviewer']), {
        code: 'invalid-role',
      })
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

  it('refuses a member that is not an agent id, or is the human or the chief, and a quota mark without a time', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      for (const agent of ['human', 'chief', 'no such!', 42]) {
        assert.throws(
          () =>
            ledger.addMember(project.id, { agent, harness: 'pi', role: 'worker', tier: 'light' }),
          { code: 'invalid-agent' },
        )
      }
      assert.throws(() => ledger.markOut(id('zeus'), { until: 'soon', reason: 'quota' }), {
        code: 'invalid-time',
      })
      const out = ledger.markOut(id('zeus'), { until: '2026-09-20T00:00:00.000Z', reason: 'quota' })
      assert.equal(out.outUntil, '2026-09-20T00:00:00.000Z')
    })
  })

  it('keeps a member out until the later reset, from when it was first marked, and takes it back early with what was held for it', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const first = ledger.markOut(id('zeus'), {
        until: '2026-09-20T00:00:00.000Z',
        reason: 'out of quota',
      })
      // Another of its windows runs into a shorter limit: nothing changes.
      const shorter = ledger.markOut(id('zeus'), {
        until: '2026-09-19T18:00:00.000Z',
        reason: 'out of quota',
      })
      assert.deepEqual([shorter.outUntil, shorter.outSince], [first.outUntil, first.outSince])
      // A longer one: out until then, still from when it was first marked.
      const longer = ledger.markOut(id('zeus'), {
        until: '2026-09-21T00:00:00.000Z',
        reason: 'out of quota',
      })
      assert.deepEqual(
        [longer.outUntil, longer.outSince],
        ['2026-09-21T00:00:00.000Z', first.outSince],
      )
      ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      ledger.holdTask(project.id, 1, { until: '2026-09-21T00:00:00.000Z', because: 'out of quota' })
      const back = ledger.markBack(id('zeus'), { because: 'by @human' })
      assert.equal(back.outUntil, null)
      assert.ok(
        Date.parse(back.outSince) > Date.parse(first.outSince),
        'what came before is history',
      )
      assert.equal(ledger.task(project.id, 1).heldUntil, back.outSince, 'the held task goes on now')
      assert.deepEqual(
        ledger.heldTasksDue(back.outSince).map((task) => task.number),
        [1],
      )
      // Back already: nothing more happens, and nothing more is logged.
      ledger.markBack(id('zeus'), { because: 'by @human' })
      const logged = ledger
        .events(project.id)
        .filter((event) => ['member.out', 'member.back'].includes(event.kind))
      assert.deepEqual(
        logged.map((event) => [event.kind, event.data.until ?? event.data.because]),
        [
          ['member.out', '2026-09-20T00:00:00.000Z'],
          ['member.out', '2026-09-21T00:00:00.000Z'],
          ['member.back', 'by @human'],
        ],
      )
    })
  })

  it('makes an image designer of an image agent alone, and an image agent nothing else', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      // An image agent is a Codex agent with the designer flag: Codex alone is no designer.
      const notImage = {
        code: 'invalid-role',
        message: 'only an image agent can be an image designer, and hera is not one',
      }
      const image = {
        code: 'invalid-role',
        message: 'pygmalion is an image agent, which can only be an image designer',
      }
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'hera',
            harness: 'claude-code',
            roles: ['worker', 'designer'],
            tier: 'standard',
          }),
        notImage,
      )
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'diana-2',
            harness: 'codex',
            roles: ['designer'],
            tier: 'light',
          }),
        {
          code: 'invalid-role',
          message: 'only an image agent can be an image designer, and diana-2 is not one',
        },
      )
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'pygmalion',
            harness: 'codex',
            designer: true,
            roles: ['designer', 'reviewer'],
            tier: 'light',
          }),
        image,
      )
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'pygmalion',
            harness: 'codex',
            designer: 'yes',
            roles: ['designer'],
            tier: 'light',
          }),
        { code: 'invalid-designer' },
      )
      // Nor does a new project's staff hold one.
      assert.throws(
        () =>
          ledger.createProject({
            directory: '/work/site',
            name: 'site',
            chief: { harness: 'pi', agent: 'leto' },
            staff: [{ agent: 'hera', harness: 'pi', role: 'designer', tier: 'standard' }],
          }),
        notImage,
      )
      const pygmalion = ledger.addMember(project.id, {
        agent: 'pygmalion',
        harness: 'codex',
        designer: true,
        role: 'designer',
        tier: 'light',
      })
      assert.deepEqual(
        [pygmalion.roles, pygmalion.harness, pygmalion.designer],
        [['designer'], 'codex', true],
      )
      // A member's roles change only to roles that fit its agent.
      assert.throws(() => ledger.setRoles(project.id, 'pygmalion', ['designer', 'worker']), image)
      assert.throws(() => ledger.setRoles(project.id, 'zeus', ['worker', 'designer']), {
        code: 'invalid-role',
      })
      assert.deepEqual(
        ledger
          .project(project.id)
          .participants.filter((p) => ['zeus', 'pygmalion'].includes(p.handle))
          .map((p) => [p.handle, p.roles]),
        [
          ['zeus', ['worker']],
          ['pygmalion', ['designer']],
        ],
      )
      assert.equal(ledger.projects().length, 1)
    })
  })

  it('keeps a role a member held before roles had to fit its agent: it works on, and goes when dropped', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      let ledger = openLedger(file, { now: clock() })
      const { project } = staff(ledger)
      ledger.addMember(project.id, {
        agent: 'pygmalion',
        harness: 'codex',
        designer: true,
        role: 'designer',
        tier: 'light',
      })
      ledger.close()
      // As an older ledger has them: zeus (Claude Code) draws, pygmalion works.
      const raw = new DatabaseSync(file)
      const roles = raw.prepare('UPDATE participant SET role = ?, roles = ? WHERE handle = ?')
      roles.run('worker', JSON.stringify(['worker', 'designer']), 'zeus')
      roles.run('worker', JSON.stringify(['worker']), 'pygmalion')
      raw.close()
      ledger = openLedger(file, { now: clock() })
      try {
        assert.deepEqual(
          ledger.members(project.id, 'designer').map((m) => m.handle),
          ['zeus'],
          'it still takes image tasks',
        )
        // A role that fits is added beside the one held; the one held may go.
        ledger.setRoles(project.id, 'zeus', ['worker', 'designer', 'reviewer'])
        ledger.setRoles(project.id, 'zeus', ['reviewer'])
        ledger.setRoles(project.id, 'pygmalion', ['worker', 'designer'])
        ledger.setRoles(project.id, 'pygmalion', ['designer'])
        assert.throws(() => ledger.setRoles(project.id, 'zeus', ['reviewer', 'designer']), {
          code: 'invalid-role',
        })
        assert.deepEqual(
          ledger
            .project(project.id)
            .participants.filter((p) => ['zeus', 'pygmalion'].includes(p.handle))
            .map((p) => [p.handle, p.roles]),
          [
            ['zeus', ['reviewer']],
            ['pygmalion', ['designer']],
          ],
        )
      } finally {
        ledger.close()
      }
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

  it('names a session with two plain words, drawn by the random it is given', () => {
    const drawn = (at) => sessionName(() => at)
    // A random that reaches 1 still picks the last word of each list.
    assert.deepEqual([drawn(0), drawn(1)], ['amber-anchor', 'zesty-yarrow'])
    assert.match(sessionName(), /^[a-z]+-[a-z]+$/)
  })
})
