import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { measure, verdict } from '../evals/measure.mjs'
import sixDecisions from '../evals/scenarios/six-decisions.mjs'
import { openLedger } from '../src/ledger/index.js'

/** The eval's numbers come from the ledger; a ledger built with the real API proves each one. */
describe('measuring a chief from the ledger', () => {
  it('counts tasks, parallel work, advice, reviews, questions, notes and the chief’s own edits', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedger(file)
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
      })
      for (const [agent, role] of [
        ['zeus', 'worker'],
        ['diana', 'worker'],
        ['athena', 'advisor'],
        ['hera', 'reviewer'],
      ]) {
        ledger.addMember(project.id, { agent, harness: 'claude-code', role, tier: 'standard' })
      }
      const participant = (handle) =>
        ledger.project(project.id).participants.find((p) => p.handle === handle)
      const deliver = (message) => {
        ledger.beginDelivery(message.id)
        ledger.confirmDelivery(message.id, { evidence: 'native' })
      }
      // Two tasks side by side, one after them; advice; a review.
      const one = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Write it' })
      const two = ledger.createTask(project.id, {
        from: 'chief',
        to: 'diana',
        body: 'Translate it',
      })
      deliver(one.message)
      deliver(two.message)
      ledger.recordResult(project.id, 1, { body: 'Written' })
      ledger.recordResult(project.id, 2, { body: 'Translated' })
      const advice = ledger.createTask(project.id, {
        from: 'chief',
        pool: 'advisor',
        tier: 'standard',
        body: 'Which law applies?',
      })
      const review = ledger.createTask(project.id, {
        from: 'chief',
        pool: 'reviewer',
        tier: 'standard',
        body: 'Review T-1',
      })
      assert.deepEqual([advice.task.pool, review.task.pool], ['advisor', 'reviewer'])
      // To the human: two questions (one with options), one note.
      ledger.ask(project.id, { from: 'chief', to: 'human', body: 'Keep the old document?' })
      ledger.ask(project.id, {
        from: 'chief',
        to: 'human',
        body: 'Publish now?',
        questions: [
          {
            question: 'Publish now?',
            header: 'Publish',
            options: [{ label: 'Yes' }, { label: 'No' }],
          },
        ],
      })
      ledger.note(project.id, {
        from: 'chief',
        to: 'human',
        body: 'The guide says 1 to 5; the site shows a colour.',
      })
      // The chief's own window: three turns, two edits.
      const conversation = ledger.startConversation(participant('chief').id, {
        harness: 'claude-code',
      })
      ledger.copyTranscript(conversation.id, [
        { id: 'u1', role: 'user', text: 'Add the page', complete: true, at: null },
        { id: 'a1', role: 'assistant', text: 'Looking.', complete: true, at: null },
        {
          id: 't1',
          role: 'tool',
          text: 'The file /work/site/index.html has been updated.',
          complete: true,
          at: null,
        },
        { id: 'a2', role: 'assistant', text: 'Now the menu.', complete: true, at: null },
        {
          id: 't2',
          role: 'tool',
          text: 'File created successfully at: /work/site/legislatie.html',
          complete: true,
          at: null,
        },
        { id: 't3', role: 'tool', text: 'diff --git a/x b/x', complete: true, at: null },
        {
          id: 'a3',
          role: 'assistant',
          text: 'Done: the page is in place.',
          complete: true,
          at: null,
        },
      ])
      ledger.close()

      const metrics = measure(file)
      assert.equal(metrics.chief, 'chief')
      assert.deepEqual(
        metrics.tasks.map((t) => [t.number, t.pool]),
        [
          [1, null],
          [2, null],
          [3, 'advisor'],
          [4, 'reviewer'],
        ]
          .map(([n, p]) => [n, p])
          .sort((a, b) => a[0] - b[0]),
      )
      assert.deepEqual(
        [metrics.parallel, metrics.advice, metrics.reviews],
        [2, 1, 1],
        'two briefs delivered before either result',
      )
      assert.deepEqual(
        [metrics.questionsToHuman.length, metrics.questionsToHuman.map((q) => q.options)],
        [2, [false, true]],
      )
      assert.equal(metrics.notesToHuman.length, 1)
      assert.deepEqual([metrics.chiefTurns, metrics.chiefEdits], [3, 2])
      assert.equal(metrics.chiefLastWords, 'Done: the page is in place.')

      const checks = verdict(sixDecisions, metrics)
      assert.deepEqual(
        checks.map((c) => [c.name, c.ok]),
        [
          ['the owner is asked on the board, at least three questions', false],
          ['at least one question offers options', true],
          ['a finding reaches the owner as a note', true],
          ['at least two tasks go on the board', true],
          ['two tasks run side by side at some point', true],
          ['finished work goes to a review', true],
          ["the chief's own edits stay under ten", true],
        ],
      )
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
