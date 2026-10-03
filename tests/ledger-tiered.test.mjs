import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { deliver, sessionId, withLedger } from './ledger-fixtures.mjs'

/**
 * Tasks for a tier of the staff: opened on the board, assigned by the
 * daemon, taken back from a member out of quota (src/ledger/tasks.js,
 * src/ledger/staff.js).
 */

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
        harness: 'codex',
        designer: true,
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
      const session = ledger.project(project.id).participants.find((p) => p.member === 'pygmalion')
      assert.deepEqual(
        [session.harness, session.designer],
        ['codex', true],
        'its session draws on Codex as it does',
      )
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
