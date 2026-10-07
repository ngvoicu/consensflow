import assert from 'node:assert/strict'
import path from 'node:path'
import { describe, it } from 'node:test'
import { openLedger, PAGE_BYTES, TRANSCRIPT_ITEM_MAX } from '../src/ledger/index.js'
import { deliver, names, staff, withDir, withLedger } from './ledger-fixtures.mjs'

/** What the page reads in one frame, and the whole reads beside them (src/ledger/page-reads.js). */

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
      const ledger = openLedger(path.join(dir, 'consensflow.db'), { names: names() })
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
        ledger.ask(project.id, { from: working(2), to: 'chief', task: 2, body: long })
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
        const bay = board.gated
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

describe('the board keeps every task in view', () => {
  it('lists one no lane has among the open ones whatever its state, each by its number', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const open = (body) =>
        ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
      const byName = (to, body, needs) =>
        ledger.createTask(project.id, { from: 'chief', to, body, needs })
      /** A worker's task through to done: assigned to a new session of it, delivered, answered. */
      const done = (number, member) => {
        deliver(ledger, ledger.assignTask(project.id, number, id(member)).message)
        ledger.recordResult(project.id, number, { body: 'Done' })
      }
      // Never given to a member: waiting (T-1), paused by the human (T-2),
      // called off (T-3), failed (T-4).
      for (const body of ['Waits', 'Paused', 'Called off', 'Failed']) open(body)
      ledger.pauseTask(project.id, 2, { by: 'human' })
      ledger.cancelTask(project.id, 3, { by: 'chief' })
      ledger.failTask(project.id, 4, { reason: 'its launch never came up' })
      // Diana's, before the human removes her from the staff: queued (T-5,
      // cancelled with her), paused (T-6), done (T-7), accepted (T-8) and
      // waiting for T-1 (T-9).
      byName('diana', 'Queued')
      byName('diana', 'Held')
      ledger.pauseTask(project.id, 6, { by: 'human' })
      for (const body of ['Finished', 'Accepted']) open(body)
      done(7, 'diana')
      done(8, 'diana')
      ledger.acceptTask(project.id, 8, { by: 'chief' })
      byName('diana', 'Waits for T-1', [1])
      // Zeus's stays on his session's lane; a task the human deleted is nowhere.
      open('Parser')
      done(10, 'zeus')
      open('Deleted')
      done(11, 'zeus')
      ledger.acceptTask(project.id, 11, { by: 'chief' })
      ledger.deleteTasks(project.id, [11])
      assert.deepEqual(ledger.removeMember(project.id, 'diana').cancelled, [5])

      const board = ledger.board(project.id)
      assert.deepEqual(
        [
          ...board.open.map((task) => ['open', task.number, task.state]),
          ...board.lanes.flatMap((lane) =>
            lane.tasks.map((task) => [lane.participant.handle, task.number, task.state]),
          ),
        ],
        [
          ['open', 1, 'open'],
          ['open', 2, 'paused'],
          ['open', 3, 'cancelled'],
          ['open', 4, 'failed'],
          ['open', 5, 'cancelled'],
          ['open', 6, 'paused'],
          ['open', 7, 'done'],
          ['open', 8, 'accepted'],
          ['open', 9, 'open'],
          ['zeus-calm-brook', 10, 'done'],
        ],
      )
      assert.deepEqual(
        ledger.openTasks(project.id).map((task) => task.number),
        [1],
        'the daemon gives out only what waits for a member',
      )
    })
  })
})
