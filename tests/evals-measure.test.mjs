import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import {
  changed,
  chiefOpenQuestion,
  countQuestions,
  devinChiefQuestions,
  measure,
  mechanics,
  ownerQuestions,
  verdict,
} from '../evals/measure.mjs'
import chiefSwitch, { CODEWORD } from '../evals/scenarios/chief-switch.mjs'
import sixDecisions from '../evals/scenarios/six-decisions.mjs'
import { openLedger } from '../src/ledger/index.js'

/** The eval's numbers come from the ledger; a ledger built with the real API proves each one. */
describe('what a run changed on disk', () => {
  it('lists the fixture files that differ in the workspace and the files the run added', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-diff-'))
    try {
      for (const [file, text] of [
        ['fixture/site/index.html', 'home'],
        ['fixture/site/evaluare.html', 'scor 0 și 100'],
        ['fixture/docs/guide.md', 'guide'],
        ['workspace/site/index.html', 'home'],
        ['workspace/site/evaluare.html', 'scor 0 și 10'],
        ['workspace/docs/guide.md', 'guide'],
        ['workspace/site/legislatie.html', 'new page'],
        ['workspace/.claude/scheduled_tasks.lock', '{"pid":1}'],
      ]) {
        await mkdir(path.dirname(path.join(dir, file)), { recursive: true })
        await writeFile(path.join(dir, file), text)
      }
      assert.deepEqual(changed(path.join(dir, 'fixture'), path.join(dir, 'workspace')), [
        'site/evaluare.html',
        'site/legislatie.html',
      ])
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe('measuring a chief from the ledger', () => {
  it('counts the chief’s controls: a tell and its answer, a pause and a resume, an --after follow-up', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-controls-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedger(file)
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
      })
      ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      const deliver = (message) => {
        ledger.beginDelivery(message.id)
        ledger.confirmDelivery(message.id, { evidence: 'native' })
      }
      // As the dispatcher does it: an open task, given to a member, opens its session.
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Sleep, then write',
      })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const given = ledger.assignTask(project.id, 1, zeus.id)
      deliver(given.message)
      const session = given.task.assignee
      // cf tell: the task paused, an urgent question to its window, answered.
      ledger.pauseTask(project.id, 1, { by: 'chief' })
      const told = ledger.ask(project.id, {
        from: 'chief',
        to: session,
        task: 1,
        body: 'Which file?',
        urgent: true,
      })
      deliver(told)
      deliver(ledger.answer(told.id, { from: told.recipientId, body: 'site/notes.md' }))
      const resumed = ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      if (resumed.message !== null) deliver(resumed.message)
      ledger.recordResult(project.id, 1, { body: 'Written' })
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      const next = ledger.createTask(project.id, { from: 'chief', after: 1, body: 'Add a line' })
      assert.equal(next.task.assignee, session, 'the follow-up goes to the same window')
      ledger.close()

      const metrics = measure(file)
      const p = metrics.plumbing
      assert.deepEqual(
        [p.tells, p.tellsAnswered, p.pauses, p.resumes, p.continuations],
        [1, 1, 1, 1, 1],
      )
      assert.deepEqual(
        mechanics(metrics, 2)
          .filter((c) => c.name.startsWith('every tell'))
          .map((c) => [c.name, c.ok]),
        [['every tell the chief sent was answered (1/1)', true]],
      )
      const trip = (await import('../evals/scenarios/control-trip.mjs')).default
      assert.deepEqual(
        verdict(trip, { ...metrics, filesChanged: ['site/notes.md'] }).map((c) => [c.name, c.ok]),
        [
          ['the chief stops the task with cf tell', true],
          ['the worker answers the tell', true],
          ['the task is paused and resumed', true],
          ['a follow-up goes to the same window (--after)', true],
          ['both tasks are accepted', false],
          ['only site/notes.md is new', true],
          ['the owner is not asked anything', true],
        ],
        'the follow-up is not yet accepted here',
      )
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })

  it('does not count a result the chief decided on before it was given it as one that never reached the chief', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-withdrawn-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedger(file)
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
      })
      for (const agent of ['zeus', 'diana']) {
        ledger.addMember(project.id, {
          agent,
          harness: 'claude-code',
          role: 'worker',
          tier: 'standard',
        })
      }
      const deliver = (message) => {
        ledger.beginDelivery(message.id)
        ledger.confirmDelivery(message.id, { evidence: 'native' })
      }
      const one = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Write it' })
      const two = ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Check it' })
      deliver(one.message)
      deliver(two.message)
      // T-1's result waited behind the chief's turn, which accepted the task: the native
      // daemon withdraws it with the decision (Node's leaves it queued, so it is done by hand).
      const waiting = ledger.recordResult(project.id, 1, { body: 'Written' }).message
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      ledger.cancelMessage(waiting.id, 'T-1 was accepted')
      // T-2's reached the chief.
      deliver(ledger.recordResult(project.id, 2, { body: 'Checked' }).message)
      ledger.close()

      const metrics = measure(file)
      assert.deepEqual(
        [metrics.plumbing.results, metrics.plumbing.resultsDelivered],
        [1, 1],
        'the one withdrawn was not owed',
      )
      assert.deepEqual(
        mechanics(metrics, 2)
          .filter((c) => c.name.startsWith('every result'))
          .map((c) => [c.name, c.ok]),
        [['every result reached the chief (1/1)', true]],
      )
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })

  it('measures long messages: the longest result of each kind, the owner’s answers, the notes whole', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-long-'))
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
        ['athena', 'advisor'],
      ]) {
        ledger.addMember(project.id, { agent, harness: 'claude-code', role, tier: 'standard' })
      }
      const deliver = (message) => {
        ledger.beginDelivery(message.id)
        ledger.confirmDelivery(message.id, { evidence: 'native' })
      }
      deliver(ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Report' }).message)
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'advisor',
        tier: 'standard',
        body: 'Advise',
      })
      const athena = ledger.project(project.id).participants.find((p) => p.handle === 'athena')
      ledger.assignTask(project.id, 2, athena.id)
      deliver(ledger.task(project.id, 2).messages.find((m) => m.kind === 'task'))
      ledger.recordResult(project.id, 1, { body: `${'r'.repeat(8999)}\nCEDRU-7314` })
      ledger.recordResult(project.id, 2, { body: 'short advice' })
      // The owner answers the chief in its window: typed there, no ConsensFlow header.
      const chief = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      ledger.copyTranscript(ledger.startConversation(chief.id, { harness: 'claude-code' }).id, [
        { id: 'u1', role: 'user', text: 'The report?', complete: true, at: null },
        {
          id: 'a1',
          role: 'assistant',
          text: 'Which name should it have?',
          complete: true,
          at: null,
        },
        { id: 'u2', role: 'user', text: 'a'.repeat(6000), complete: true, at: null },
        {
          id: 'u3',
          role: 'user',
          text: '[ConsensFlow m-9 · note from ConsensFlow]\nnot the owner',
          complete: true,
          at: null,
        },
      ])
      ledger.note(project.id, {
        from: 'chief',
        to: 'human',
        body: `${'n'.repeat(300)} DELTA-5530 at the end`,
      })
      ledger.close()

      const metrics = measure(file)
      assert.deepEqual(metrics.longestResult, { worker: 9010, advisor: 12, reviewer: 0 })
      assert.deepEqual(metrics.ownerMessages, [11, 6000])
      assert.match(metrics.notesText, /DELTA-5530 at the end$/)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })

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
      // The plumbing: zeus asks the chief, the chief answers, both delivered;
      // both results reach the chief; one task is accepted.
      const asked = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        body: 'Which colour?',
        task: 1,
      })
      deliver(asked)
      deliver(ledger.answer(asked.id, { from: asked.recipientId, body: 'Blue' }))
      deliver(ledger.recordResult(project.id, 1, { body: 'Written' }).message)
      ledger.recordResult(project.id, 2, { body: 'Translated' })
      ledger.acceptTask(project.id, 1, { by: 'chief' })
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
      // To the human on the board: one note; the chief asks in its terminal.
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
        // As the adapters report a turn: only the message that ends it is complete.
        { id: 'a1', role: 'assistant', text: 'Looking.', complete: false, at: null },
        {
          id: 't1',
          role: 'tool',
          text: 'The file /work/site/index.html has been updated.',
          complete: true,
          at: null,
        },
        {
          id: 't0',
          role: 'tool',
          text: 'cf: ask the human here in your terminal: they read and answer you there',
          complete: true,
          at: null,
        },
        { id: 'a2', role: 'assistant', text: 'Now the menu?', complete: false, at: null },
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
        { id: 'u2', role: 'user', text: 'Next', complete: true, at: null },
        {
          id: 'a4',
          role: 'assistant',
          text: 'Shall I translate it? Or wait for you?',
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
      assert.equal(metrics.questionsOnBoard, 0)
      assert.equal(
        metrics.askRefused,
        1,
        'the chief tried cf ask once and was sent to its terminal',
      )
      assert.equal(metrics.notesToHuman.length, 1)
      assert.deepEqual([metrics.chiefTurns, metrics.chiefEdits], [4, 2])
      assert.equal(metrics.chiefLastWords, 'Shall I translate it? Or wait for you?')
      // What the owner was asked in the terminal: two questions, in one turn's
      // end; a question mid-turn ("Now the menu?") is not put to anyone.
      const owner = metrics.ownerQuestions
      assert.deepEqual([owner.questions, owner.turnsAsking, owner.pickers], [2, 1, 0])
      assert.deepEqual(metrics.filesChanged, [], 'no fixture given: nothing compared')
      // Who asked the chief, by role and harness: every run tells it.
      assert.deepEqual(metrics.memberQuestionsBy, [
        {
          role: 'worker',
          harness: 'claude-code',
          asked: 1,
          answered: 1,
          delivered: 1,
          longest: 13,
        },
      ])
      // A task given by pool has no brief until a member takes it: two briefs.
      assert.deepEqual(metrics.plumbing, {
        briefs: 2,
        briefsDelivered: 2,
        results: 2,
        resultsDelivered: 1,
        memberQuestions: 1,
        memberQuestionsAnswered: 1,
        answersDelivered: 1,
        accepted: 1,
        tells: 0,
        tellsAnswered: 0,
        placed: 4,
        pauses: 0,
        resumes: 0,
        continuations: 0,
      })
      assert.equal(metrics.taskCount, 4)
      assert.deepEqual(
        mechanics(metrics, 4).map((c) => [c.name, c.ok]),
        [
          ['every task brief was delivered (2/2)', true],
          ['every result reached the chief (1/2)', false],
          ['every question a member asked the chief was answered (1/1)', true],
          ['every answer reached the member (1/1)', true],
          ['every tell the chief sent was answered (0/0)', true],
          ['the board showed every task (4/4)', true],
        ],
      )
      assert.equal(mechanics(metrics, 3).at(-1).ok, false, 'a task the board did not list')

      const checks = verdict(sixDecisions, metrics)
      assert.deepEqual(
        checks.map((c) => [c.name, c.ok]),
        [
          ['the owner is asked in the terminal, at least three questions', false],
          ['nothing is asked on the board', true],
          ['the chief never tries cf ask', false],
          ['a finding reaches the owner as a note', true],
          ['at least two tasks go on the board', true],
          ['two tasks run side by side at some point', true],
          ['finished work goes to a review', true],
          ["the chief's own edits stay under ten (counted for a Claude chief only)", true],
        ],
      )
      const trip = (await import('../evals/scenarios/round-trip.mjs')).default
      assert.deepEqual(
        verdict(trip, {
          ...metrics,
          filesChanged: ['site/notes.md'],
          ownerQuestions: { questions: 0, turnsAsking: 0, pickers: 0, texts: [] },
        }).map((c) => c.ok),
        [true, true, true, true, true, true, true],
        'the round trip holds on this ledger once only notes.md is new and the owner was not asked',
      )
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe('measuring parallel work', () => {
  const fixture = async (build) => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-parallel-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedger(file)
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
      })
      for (const agent of ['zeus', 'diana']) {
        ledger.addMember(project.id, {
          agent,
          harness: 'claude-code',
          role: 'worker',
          tier: 'standard',
        })
      }
      const deliver = (message) => {
        ledger.beginDelivery(message.id)
        ledger.confirmDelivery(message.id, { evidence: 'native' })
      }
      build(ledger, project, deliver)
      ledger.close()
      return measure(file)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  }

  it('counts only windows that had their brief: tasks still on the board never ran', async () => {
    const metrics = await fixture((ledger, project, deliver) => {
      deliver(
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Write it' }).message,
      )
      for (const body of ['Translate it', 'Check it']) {
        ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
      }
    })
    assert.equal(metrics.taskCount, 3)
    assert.equal(metrics.parallel, 1)
  })

  it('ends a task that ended without a result where it ended', async () => {
    const metrics = await fixture((ledger, project, deliver) => {
      deliver(
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Write it' }).message,
      )
      ledger.cancelTask(project.id, 1, { by: 'chief' })
      deliver(
        ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Then this' }).message,
      )
    })
    assert.equal(metrics.parallel, 1)
  })
})

describe('measuring a Switch chief', () => {
  it('reads the chiefs in order, the switches, the history the new chief read and its words, and judges the scenario', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-switch-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedger(file)
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
      })
      const chief = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      ledger.copyTranscript(ledger.startConversation(chief.id, { harness: 'claude-code' }).id, [
        { id: 'u', role: 'user', text: `The codeword is ${CODEWORD}`, complete: true, at: null },
      ])
      ledger.switchChief(project.id, { harness: 'codex', agent: 'eval-codex-worker' })
      ledger.copyTranscript(ledger.startConversation(chief.id, { harness: 'codex' }).id, [
        {
          id: 'a',
          role: 'assistant',
          text: `The codeword is ${CODEWORD}; it goes out on Friday.`,
          complete: true,
          at: null,
        },
      ])
      ledger.historyRead(project.id, { page: 2 })
      ledger.close()
      const metrics = measure(file)
      assert.deepEqual(
        [metrics.chiefs, metrics.switches, metrics.historyReads],
        [['claude-code', 'codex'], 1, [{ page: 2, find: null, tools: false }]],
      )
      assert.equal(metrics.chiefWordsNow, `The codeword is ${CODEWORD}; it goes out on Friday.`)
      assert.ok(verdict(chiefSwitch, metrics).every((check) => check.ok))
      assert.match(CODEWORD, /^(TERN|LARK|WREN|KITE|ROOK|SWIFT)-\d{4}$/)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe('counting what a chief asks the owner', () => {
  it('counts question sentences, not a question mark in code or a link', () => {
    const terminal = [
      'Am citit fișierele. Păstrăm vechiul document din docs/? Sau îl ștergem?',
      '',
      '```sh',
      'grep -n "ce?" site/*.html',
      '```',
      'Vezi `a ? b : c` și https://example.com/page?lang=en pentru detalii.',
      '- Publicăm acum sau după ce vezi pagina?',
    ].join('\n')
    assert.equal(countQuestions(terminal), 3)
    // Markdown around a question still makes it one (an Opus chief, 2026-09-29).
    const bold = [
      'Nu am pornit nimic pe board încă — două întrebări:',
      '',
      '1. **Cum se numește fișierul paginii?** (spune-mi exact numele, inclusiv extensia)',
      '2. **În ce limbă o facem întâi — română sau engleză?**',
      '',
      '_Pornesc imediat?_',
    ].join('\n')
    assert.equal(countQuestions(bold), 3)
    assert.equal(countQuestions('Gata. Pagina e făcută.'), 0)
    assert.equal(countQuestions(''), 0)
  })

  it("counts the questions of each turn's end, and the pickers the owner answered", () => {
    const counted = ownerQuestions(
      ['Two pages are done.', 'Should I also translate the FAQ? And the footer?'],
      { pickers: 2 },
    )
    assert.deepEqual([counted.questions, counted.turnsAsking, counted.pickers], [2, 1, 2])
    // What was counted, for a human to check.
    assert.deepEqual(counted.texts, ['Should I also translate the FAQ? And the footer?'])
  })
})

describe('the question trip', () => {
  const asked = (role, longest = 60) => ({
    role,
    harness: 'pi',
    asked: 1,
    answered: 1,
    delivered: 1,
    longest,
  })
  const good = {
    tasks: [{ pool: null }, { pool: 'advisor' }, { pool: 'reviewer' }],
    memberQuestionsBy: [asked('advisor'), asked('reviewer', 6250), asked('worker')],
    notesText: 'The reviewer asked about the English version; its code: PLOP-6142.',
    plumbing: { accepted: 3 },
    ownerQuestions: { questions: 0, turnsAsking: 0, pickers: 0, texts: [] },
  }

  it('holds when every kind of member asked, got its answer, and the long question was read whole', async () => {
    for (const id of ['question-trip', 'question-trip-native']) {
      const trip = (await import(`../evals/scenarios/${id}.mjs`)).default
      assert.equal(trip.id, id)
      assert.deepEqual(
        verdict(trip, good)
          .filter((c) => !c.ok)
          .map((c) => c.name),
        [],
      )
    }
  })

  it('names what is missing: a role that never asked, an answer not delivered, a short question, no code', async () => {
    const trip = (await import('../evals/scenarios/question-trip.mjs')).default
    const failing = (metrics) =>
      verdict(trip, metrics)
        .filter((c) => !c.ok)
        .map((c) => c.name)
    assert.deepEqual(failing({ ...good, memberQuestionsBy: [asked('advisor'), asked('worker')] }), [
      'the reviewer asks the chief, and the answer reaches its window',
      'the long question reaches the chief whole (over 4000 characters)',
    ])
    assert.deepEqual(
      failing({
        ...good,
        memberQuestionsBy: [
          asked('advisor'),
          { ...asked('reviewer', 6250), delivered: 0 },
          asked('worker'),
        ],
      }),
      ['the reviewer asks the chief, and the answer reaches its window'],
    )
    assert.deepEqual(failing({ ...good, notesText: 'Done.' }), [
      "the chief's note holds the long question's code",
    ])
    const inTerminal = { questions: 1, turnsAsking: 1, pickers: 0, texts: ['Which one?'] }
    assert.deepEqual(failing({ ...good, ownerQuestions: inTerminal }), [
      'the owner is not asked anything',
    ])
  })
})

describe("the chief's open question, read while the daemon runs", () => {
  it('is its newest asking turn end since the owner typed, read from a copy of the ledger', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-turn-'))
    const file = path.join(dir, 'consensflow.db')
    const ledger = openLedger(file)
    try {
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'codex' },
      })
      const chief = ledger.project(project.id).participants.find((p) => p.role === 'chief')
      assert.equal(chiefOpenQuestion(file), undefined, 'no turn yet')
      const conversation = ledger.startConversation(chief.id, { harness: 'codex' })
      const said = (id, role, text, complete = true) => ({ id, role, text, complete, at: null })
      // The chief asks while its tasks run; a result arrives, and its next
      // turn asks nothing: the question is still open.
      const record = [
        said('u1', 'user', 'Put three tasks on the board, then ask me the report name.'),
        said('a1', 'assistant', 'Reading?', false),
        said('a2', 'assistant', 'Tasks are out. What is the final report called?'),
        said('u2', 'user', '[ConsensFlow m-7 · T-1 · result from @worker]\nDone.'),
        said('a3', 'assistant', 'T-1 is in; waiting for the rest.'),
      ]
      ledger.copyTranscript(conversation.id, record)
      // The ledger is still open, as the daemon's is during a run.
      assert.deepEqual(
        { ...chiefOpenQuestion(file) },
        { id: 'a2', text: 'Tasks are out. What is the final report called?' },
      )
      // The daemon copies the whole record each time, and keeps what is new.
      record.push(
        said('u3', 'user', 'DELTA-5530.'),
        said('a4', 'assistant', 'Noted; the note is sent.'),
      )
      ledger.copyTranscript(conversation.id, record)
      assert.equal(chiefOpenQuestion(file), undefined, 'the owner answered')
    } finally {
      ledger.close()
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe("a Devin chief's own question dialog", () => {
  it('is its newest question call no answer followed, read from a copy of its store', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-devin-dialog-'))
    const file = path.join(dir, 'consensflow.db')
    // Devin's data folder, on Windows (APPDATA) or elsewhere: dir/data/devin.
    const env = {
      HOME: dir,
      XDG_DATA_HOME: path.join(dir, 'data'),
      APPDATA: path.join(dir, 'data'),
    }
    const ledger = openLedger(file)
    try {
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'devin' },
      })
      const chief = ledger.project(project.id).participants.find((p) => p.role === 'chief')
      ledger.bindConversation(
        ledger.startConversation(chief.id, { harness: 'devin' }).id,
        'swift-owl',
      )
      const folder = path.join(dir, 'data', 'devin', 'cli')
      await mkdir(folder, { recursive: true })
      const store = new DatabaseSync(path.join(folder, 'sessions.db'))
      store.exec(
        'CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, chat_message TEXT)',
      )
      const add = (message) =>
        store
          .prepare('INSERT INTO message_nodes (session_id, chat_message) VALUES (?, ?)')
          .run('swift-owl', JSON.stringify(message))
      const ask = (id, question) => ({
        role: 'assistant',
        tool_calls: [
          {
            id,
            name: 'ask_user_question',
            arguments: {
              questions: [{ question, header: 'File', options: [{ label: 'a.html' }] }],
            },
          },
        ],
      })
      assert.equal(devinChiefQuestions(file, env), null, 'nothing asked yet')
      add(ask('q1', 'Which file?'))
      add({ role: 'tool', tool_call_id: 'q1', content: 'User answered your questions' })
      assert.equal(devinChiefQuestions(file, env), null, 'answered')
      add(ask('q2', 'Which language?'))
      assert.deepEqual(devinChiefQuestions(file, env), [
        { question: 'Which language?', header: 'File', options: [{ label: 'a.html' }] },
      ])
      store.close()
    } finally {
      ledger.close()
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe('the terminal questions eval', () => {
  const good = {
    ownerQuestions: {
      questions: 1,
      turnsAsking: 1,
      pickers: 1,
      texts: ['Cum se numește fișierul?'],
    },
    questionsOnBoard: 0,
    askRefused: 0,
    tasks: [{ number: 1 }],
    filesChanged: ['site/cariere.html'],
  }

  it('holds when the chief asked in its terminal and the answer reached the work', async () => {
    const trip = (await import('../evals/scenarios/terminal-questions.mjs')).default
    assert.deepEqual(
      verdict(trip, good)
        .filter((c) => !c.ok)
        .map((c) => c.name),
      [],
    )
  })

  it('fails a chief that asked nothing, tried cf ask, or whose answer never reached the work', async () => {
    const trip = (await import('../evals/scenarios/terminal-questions.mjs')).default
    const failing = (metrics) =>
      verdict(trip, { ...good, ...metrics })
        .filter((c) => !c.ok)
        .map((c) => c.name)
    assert.deepEqual(
      failing({
        ownerQuestions: { questions: 0, turnsAsking: 0, pickers: 0, texts: [] },
        askRefused: 1,
      }),
      ['the owner is asked in the terminal', 'the chief never tries cf ask'],
    )
    assert.deepEqual(failing({ filesChanged: ['site/careers.html'] }), [
      "the owner's answer reaches the work: site/cariere.html exists",
    ])
  })
})

describe('the tasks the board places', () => {
  it('are every one it lists: one no lane has, whatever its state, and not one the human deleted', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-placed-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedger(file)
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
      })
      for (const agent of ['zeus', 'diana']) {
        ledger.addMember(project.id, {
          agent,
          harness: 'claude-code',
          role: 'worker',
          tier: 'standard',
        })
      }
      const open = (body) =>
        ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
      const finish = (number, agent) => {
        const given = ledger.assignTask(
          project.id,
          number,
          ledger.project(project.id).participants.find((p) => p.handle === agent).id,
        )
        ledger.beginDelivery(given.message.id)
        ledger.confirmDelivery(given.message.id, { evidence: 'native' })
        ledger.recordResult(project.id, number, { body: 'Done' })
      }
      // Waiting (T-1), paused (T-2), called off (T-3), failed (T-4): no member had them.
      for (const body of ['Waits', 'Paused', 'Called off', 'Failed']) open(body)
      ledger.pauseTask(project.id, 2, { by: 'human' })
      ledger.cancelTask(project.id, 3, { by: 'chief' })
      ledger.failTask(project.id, 4, { reason: 'its launch never came up' })
      // Zeus's, on his session's lane (T-5); diana's, who then left the staff (T-6);
      // and one the human deleted (T-7).
      open('Parser')
      finish(5, 'zeus')
      open('Docs')
      finish(6, 'diana')
      ledger.removeMember(project.id, 'diana')
      open('Old spike')
      finish(7, 'zeus')
      ledger.acceptTask(project.id, 7, { by: 'chief' })
      ledger.deleteTasks(project.id, [7])
      const board = ledger.board(project.id)
      const listed = board.open.length + board.lanes.reduce((n, lane) => n + lane.tasks.length, 0)
      ledger.close()

      assert.equal(listed, 6, 'every task but the deleted one is on the board')
      assert.equal(measure(file).plumbing.placed, listed)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
