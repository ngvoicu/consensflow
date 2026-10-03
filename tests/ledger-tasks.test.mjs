import assert from 'node:assert/strict'
import path from 'node:path'
import { describe, it } from 'node:test'
import { openLedger, RESUME_WORDS } from '../src/ledger/index.js'
import { deliver, names, staff, withDir, withLedger } from './ledger-fixtures.mjs'

/** Tasks along their state machine, and the plan their needs make (src/ledger/tasks.js). */

describe('tasks and their state machine', () => {
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

describe('a cancelled task', () => {
  const open = (ledger, project, body) =>
    ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })

  it('tells whoever asked for it, in the same step, unless it cancelled the task itself', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      ledger.setGate(project.id, true)
      const notes = () =>
        ledger
          .inbox(id('chief'))
          .filter((m) => m.kind === 'note')
          .map((m) => [m.sender, m.taskNumber, m.state, m.body])
      // Still on the board, or given out but not yet in a window: nobody began on it.
      open(ledger, project, 'Write the parser\nwith tests')
      ledger.cancelTask(project.id, 1, { by: 'human' })
      open(ledger, project, 'Write the lexer')
      ledger.assignTask(project.id, 2, id('zeus'))
      ledger.cancelTask(project.id, 2, { by: 'human' })
      assert.deepEqual(notes().reverse(), [
        [null, 1, 'queued', '@human cancelled T-1 (Write the parser).'],
        [null, 2, 'queued', '@human cancelled T-2 (Write the lexer).'],
      ])
      // In a member's window, or on its way in: that window is named.
      for (const [number, body, delivered] of [
        [3, 'Write the docs', true],
        [4, 'Write the CLI', false],
      ]) {
        open(ledger, project, body)
        const { message } = ledger.assignTask(project.id, number, id('diana'))
        ledger.approveMessage(message.id, { by: 'human' })
        if (delivered) deliver(ledger, message)
        else ledger.beginDelivery(message.id)
        ledger.cancelTask(project.id, number, { by: 'human' })
        assert.deepEqual(notes()[0], [
          null,
          number,
          'queued',
          `@human cancelled T-${number} (${body}): @${message.recipient}'s window was stopped.`,
        ])
      }
      // The chief's own work stops no window of a member.
      const own = ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Plan it' })
      deliver(ledger, own.message)
      ledger.cancelTask(project.id, 5, { by: 'human' })
      assert.equal(notes()[0][3], '@human cancelled T-5 (Plan it).')
      // The chief cancels what it asked for: it knows. A refused cancel tells nobody.
      open(ledger, project, 'Write the tests')
      ledger.cancelTask(project.id, 6, { by: 'chief' })
      assert.throws(() => ledger.cancelTask(project.id, 6, { by: 'human' }), {
        code: 'invalid-transition',
      })
      assert.equal(notes().length, 5)
    })
  })

  it('withdraws whatever of it is still on its way, a delivery in progress too', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      open(ledger, project, 'Parser')
      const { message } = ledger.assignTask(project.id, 1, id('zeus'))
      ledger.beginDelivery(message.id)
      ledger.cancelTask(project.id, 1, { by: 'chief' })
      const brief = ledger.message(message.id)
      assert.deepEqual([brief.state, brief.reason], ['cancelled', 'cancelled by @chief'])
      assert.deepEqual(ledger.inFlight(), [], 'a restart finds nothing of it to settle')
      assert.throws(() => ledger.retryDelivery(message.id, 'its window closed'), {
        code: 'invalid-transition',
      })
      assert.equal(ledger.nextDelivery(brief.recipientId), null, 'nothing of it is tried again')
    })
  })

  it('is still the task its window was last given a message about, until another comes', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      assert.equal(ledger.lastTask(id('zeus')), null, 'nothing given yet')
      open(ledger, project, 'Parser')
      const { message } = ledger.assignTask(project.id, 1, id('zeus'))
      deliver(ledger, message)
      ledger.cancelTask(project.id, 1, { by: 'human' })
      const last = () => ledger.lastTask(message.recipientId)
      assert.deepEqual([last().number, last().state], [1, 'cancelled'])
      // A follow-up for the same session, once the human lets it go.
      ledger.setGate(project.id, true)
      const after = ledger.createTask(project.id, { from: 'chief', after: 1, body: 'The lexer' })
      assert.equal(last().number, 1, 'not while it waits for the human')
      ledger.approveMessage(after.message.id, { by: 'human' })
      assert.deepEqual([last().number, last().state], [2, 'queued'])
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

  it("knows whether the chief's tell reached the window since the task was last paused", async () => {
    await withLedger((ledger) => {
      const { project, session, sessionId } = running(ledger)
      const task = ledger.task(project.id, 1)
      const told = () => ledger.toldSincePaused(sessionId, task.id)
      const tell = ledger.ask(project.id, {
        from: 'chief',
        to: session,
        task: 1,
        body: 'Use the new grammar',
        urgent: true,
      })
      assert.equal(ledger.task(project.id, 1).state, 'paused')
      assert.equal(told(), false, 'not yet in the window')
      ledger.beginDelivery(tell.id)
      assert.equal(told(), true, 'on its way in counts')
      ledger.confirmDelivery(tell.id, { evidence: 'native' })
      assert.equal(told(), true)
      ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      ledger.pauseTask(project.id, 1, { by: 'chief' })
      assert.equal(told(), false, 'a tell from before the latest pause does not count')
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

describe('a finished task the human deletes from the board', () => {
  const open = (ledger, project, body, extra = {}) =>
    ledger.createTask(project.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body,
      ...extra,
    }).task
  /** A worker's task in its session's window: the session's message. */
  const start = (ledger, project, id, number) => {
    const { message } = ledger.assignTask(project.id, number, id('zeus'))
    deliver(ledger, message)
    return message
  }
  /** Every task on the board, by number: what waits for a member and every lane. */
  const onBoard = (ledger, project) => {
    const board = ledger.board(project.id)
    return [...board.open, ...board.lanes.flatMap((lane) => lane.tasks)]
      .map((task) => task.number)
      .sort((a, b) => a - b)
  }

  it('leaves the board and the list for good, keeps its thread, tells nobody and gives its number to no other task', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      open(ledger, project, 'Lexer')
      open(ledger, project, 'Parser')
      start(ledger, project, id, 2)
      ledger.recordResult(project.id, 2, { body: 'Parser done' })
      ledger.acceptTask(project.id, 2, { by: 'chief' })
      const told = ['chief', 'human'].map((handle) => ledger.inbox(id(handle)).length)

      const [deleted] = ledger.deleteTasks(project.id, [2])
      assert.deepEqual([deleted.number, deleted.state], [2, 'accepted'])
      assert.match(deleted.deletedAt, /^2026-09-19T10:\d\d:\d\d\.000Z$/)
      assert.deepEqual(onBoard(ledger, project), [1])
      const kept = ledger.task(project.id, 2)
      assert.deepEqual(
        [kept.state, kept.deletedAt, kept.messages.map((message) => message.kind)],
        ['accepted', deleted.deletedAt, ['task', 'result']],
        'the ledger still reads it, thread and all',
      )
      const { kind, data } = ledger.events(project.id).at(-1)
      assert.deepEqual([kind, data], ['task.deleted', { task: 2, state: 'accepted' }])
      assert.deepEqual(
        ['chief', 'human'].map((handle) => ledger.inbox(id(handle)).length),
        told,
        'the human tidies the board: nobody is told',
      )
      assert.equal(open(ledger, project, 'Docs').number, 3, 'T-2 is never given again')
    })
  })

  it('deletes only a finished task, and says what to do with one that is not', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      for (const body of ['Open', 'Working', 'Paused', 'Done', 'Cancelled', 'Failed']) {
        open(ledger, project, body)
      }
      start(ledger, project, id, 2)
      start(ledger, project, id, 3)
      ledger.pauseTask(project.id, 3, { by: 'chief' })
      start(ledger, project, id, 4)
      ledger.recordResult(project.id, 4, { body: 'Done' })
      ledger.cancelTask(project.id, 5, { by: 'chief' })
      start(ledger, project, id, 6)
      ledger.failTask(project.id, 6, { reason: 'its window closed' })
      const before = ledger.events(project.id).length
      for (const [number, state] of [
        [1, 'open'],
        [2, 'working'],
        [3, 'paused'],
      ]) {
        assert.throws(() => ledger.deleteTasks(project.id, [number]), {
          code: 'not-finished',
          message: `T-${number} is ${state}: only a finished task leaves the board; cancel it first`,
        })
      }
      assert.throws(() => ledger.deleteTasks(project.id, [4]), {
        code: 'not-finished',
        message:
          'T-4 is done: only a finished task leaves the board; the chief accepts it or sends it back first',
      })
      // One refused, none goes: the human confirmed them all.
      assert.throws(() => ledger.deleteTasks(project.id, [5, 1]), { code: 'not-finished' })
      assert.equal(ledger.task(project.id, 5).deletedAt, null)
      assert.equal(ledger.events(project.id).length, before, 'nothing written')
      assert.throws(() => ledger.deleteTasks(project.id, [9]), { code: 'unknown-task' })
      assert.deepEqual(
        ledger.deleteTasks(project.id, [5, 6]).map((task) => [task.number, task.state]),
        [
          [5, 'cancelled'],
          [6, 'failed'],
        ],
      )
      assert.deepEqual(onBoard(ledger, project), [1, 2, 3, 4])
    })
  })

  it('keeps a finished task that a task not yet finished still needs, and names that task', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      open(ledger, project, 'Lexer')
      open(ledger, project, 'Parser', { needs: [1] })
      open(ledger, project, 'Docs', { needs: [1] })
      ledger.cancelTask(project.id, 1, { by: 'chief' })
      assert.throws(() => ledger.deleteTasks(project.id, [1]), {
        code: 'task-needed',
        message: 'T-2, T-3 still need T-1: it stays on the board until they are finished',
      })
      ledger.cancelTask(project.id, 2, { by: 'chief' })
      assert.throws(() => ledger.deleteTasks(project.id, [1]), {
        code: 'task-needed',
        message: 'T-3 still needs T-1: it stays on the board until T-3 is finished',
      })
      ledger.cancelTask(project.id, 3, { by: 'chief' })
      ledger.deleteTasks(project.id, [1, 2, 3])
      assert.deepEqual(onBoard(ledger, project), [])
    })
  })

  it('moves no more once deleted, and nothing new waits for it; a window still on it is still stopped', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      open(ledger, project, 'Parser')
      open(ledger, project, 'Lexer')
      const parser = start(ledger, project, id, 1)
      ledger.failTask(project.id, 1, { reason: 'its window closed' })
      const lexer = start(ledger, project, id, 2)
      ledger.cancelTask(project.id, 2, { by: 'human' })
      ledger.deleteTasks(project.id, [1, 2])
      assert.throws(() => ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Again' }), {
        code: 'task-deleted',
        message: 'cannot reopen T-1: it was deleted from the board',
      })
      assert.throws(() => ledger.deleteTasks(project.id, [1]), {
        code: 'task-deleted',
        message: 'cannot delete T-1: it was deleted from the board',
      })
      assert.throws(() => open(ledger, project, 'Docs', { needs: [1] }), {
        code: 'task-deleted',
        message: 'cannot wait for T-1: it was deleted from the board',
      })
      assert.equal(ledger.task(project.id, 3), null, 'nothing of the refused task is left')
      // The cancelled task its window may still be at work on is the one it is stopped for.
      assert.deepEqual(
        [ledger.lastTask(lexer.recipientId).number, ledger.lastTask(lexer.recipientId).state],
        [2, 'cancelled'],
      )
      // A follow-up in the window that did it is new work, on the board.
      const after = ledger.createTask(project.id, { from: 'chief', after: 1, body: 'Retry' })
      assert.deepEqual([after.task.number, after.task.assignee], [3, parser.recipient])
      assert.deepEqual(onBoard(ledger, project), [3])
    })
  })
})

describe('what a pass reads', () => {
  it('gives out the open tasks alone, each saying what it still waits for', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const open = (body, needs = []) =>
        ledger.createTask(project.id, {
          from: 'chief',
          pool: 'worker',
          tier: 'standard',
          body,
          needs,
        })
      open('Parser')
      open('Docs', [1])
      open('Tests')
      ledger.assignTask(project.id, 3, id('zeus'))
      assert.deepEqual(
        ledger.openTasks(project.id).map((task) => [task.number, task.blockedBy]),
        [
          [1, []],
          [2, [1]],
        ],
      )
    })
  })

  it('names who has work: a message on its way, or a task in hand', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      const { message } = ledger.assignTask(project.id, 1, id('zeus'))
      const session = message.recipientId
      assert.deepEqual([...ledger.withWork(project.id)], [session], 'its brief is on its way')
      deliver(ledger, message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      assert.ok(!ledger.withWork(project.id).has(session), 'done: nothing in hand')
      assert.ok(
        ledger.withWork(project.id).has(id('chief')),
        'the result is on its way to the lead',
      )
    })
  })
})
