import assert from 'node:assert/strict'
import path from 'node:path'
import { describe, it } from 'node:test'
import { OVERDUE_MS, openLedger } from '../src/ledger/index.js'
import { deliver, names, staff, withDir, withLedger } from './ledger-fixtures.mjs'

/** The human's gate on one agent's word to another (src/ledger/messages.js, src/ledger/queue.js). */

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
