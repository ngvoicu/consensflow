import assert from 'node:assert/strict'
import path from 'node:path'
import { describe, it } from 'node:test'
import { OVERDUE_MS, openLedger } from '../src/ledger/index.js'
import { deliver, names, staff, withDir, withLedger } from './ledger-fixtures.mjs'

/** Messages: delivery one at a time, questions and their answers (src/ledger/messages.js). */

describe('the inbox queue: delivery, questions and answers', () => {
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

  it('takes no answer on a cancelled task, either way: nobody waits for it', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const asked = ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'Which?' })
      deliver(ledger, asked)
      const told = ledger.ask(project.id, {
        from: 'chief',
        to: 'zeus',
        task: 1,
        body: 'Where are you?',
        urgent: true,
      })
      deliver(ledger, told)
      ledger.cancelTask(project.id, 1, { by: 'human' })
      for (const [question, from] of [
        [told, id('zeus')],
        [asked, id('chief')],
      ]) {
        assert.throws(() => ledger.answer(question.id, { from, body: 'Here' }), {
          code: 'task-cancelled',
          message: 'T-1 is cancelled: nobody waits for this answer',
        })
      }
      assert.deepEqual(
        ledger.task(project.id, 1).messages.filter((m) => m.kind === 'answer'),
        [],
      )
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

  it('lists what is on its way: to one participant, and into any window, oldest first', async () => {
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
      const note = ledger.note(project.id, { from: 'human', to: 'chief', body: 'Hello' })
      const states = (participant) => ledger.pending(participant).map((m) => [m.id, m.state])
      assert.deepEqual(states(id('zeus')), [
        [first.id, 'queued'],
        [second.id, 'queued'],
      ])
      assert.deepEqual(ledger.inFlight(), [])
      ledger.beginDelivery(first.id)
      ledger.beginDelivery(note.id)
      assert.deepEqual(
        ledger.inFlight().map((m) => [m.id, m.recipient]),
        [
          [first.id, 'zeus'],
          [note.id, 'chief'],
        ],
      )
      assert.deepEqual(states(id('zeus')), [
        [first.id, 'delivering'],
        [second.id, 'queued'],
      ])
      ledger.confirmDelivery(first.id, { evidence: 'native-1' })
      assert.deepEqual(
        ledger.inFlight().map((m) => m.id),
        [note.id],
      )
      assert.deepEqual(states(id('zeus')), [[second.id, 'queued']])
    })
  })

  it('cancels a message still on its way, never one delivered, cancelled or unknown', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const { message } = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'One' })
      const delivered = deliver(
        ledger,
        ledger.note(project.id, { from: 'human', to: 'chief', body: 'Hi' }),
      )
      const cancelled = ledger.cancelMessage(message.id, 'no longer wanted')
      assert.deepEqual([cancelled.state, cancelled.reason], ['cancelled', 'no longer wanted'])
      for (const id of [message.id, delivered.id, 99]) {
        assert.throws(() => ledger.cancelMessage(id, 'gone'), { code: 'not-pending' })
      }
      assert.throws(() => ledger.cancelMessage(message.id, ' '), { code: 'invalid-text' })
    })
  })

  it('reads in the app only a message the human has, once', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const { message } = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'One' })
      assert.throws(() => ledger.markRead(99), { code: 'unknown-message' })
      assert.throws(() => ledger.markRead(message.id), { code: 'not-for-the-human' })
      const note = ledger.note(project.id, { from: 'chief', to: 'human', body: 'T-1 is done.' })
      const read = ledger.markRead(note.id)
      assert.equal(read.state, 'read')
      assert.deepEqual(ledger.markRead(note.id), read, 'read already: as it was')
    })
  })

  it('refuses a question with options that has no text, and an answer with more picks than a question takes', async () => {
    await withLedger((ledger) => {
      const { project, question } = asked(ledger)
      assert.throws(
        () =>
          ledger.ask(project.id, {
            from: 'zeus',
            to: 'chief',
            task: 1,
            questions: [{ question: '  ', header: 'Colour', options: [] }],
          }),
        { code: 'bad-questions', message: 'questions: each question has its text' },
      )
      for (const choices of [
        [['red', 'blue'], ['yes']],
        [[], ['yes']],
      ]) {
        assert.throws(() => ledger.answer(question.id, { from: question.recipientId, choices }), {
          code: 'bad-choices',
          message: 'answer: Colour: one pick',
        })
      }
      assert.equal(ledger.answerTo(question.id), null, 'it still waits for its answer')
    })
  })
})
