import assert from 'node:assert/strict'
import { readdirSync } from 'node:fs'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
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
import { addProject, fixtureJson, loadLedger, openLedgerFile } from './ledger-file.mjs'

/**
 * The eval's numbers come from the ledger file, read with SQLite. Each test
 * reads a ledger recorded while Node's ledger could still build one
 * (tests/fixtures/ledgers, fixed since Node went), where each number is the one
 * the sequence of operations behind it leaves.
 */
const FIXTURES = fileURLToPath(new URL('./fixtures/ledgers/', import.meta.url))

/** `run(file)` on the frozen ledger `name`, loaded into a file of a folder of its own. */
async function onLedger(name, run, options = {}) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-'))
  try {
    const file = path.join(dir, 'consensflow.db')
    loadLedger(file, name, options).close()
    return await run(file)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

/** What `measure()` says of the frozen ledger `name`, read from a file closed as a finished run leaves it. */
const measured = (name, options) => onLedger(name, (file) => measure(file), options)

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

/**
 * The numbers `measure()` gave while it read the ledger through Node's code,
 * on the file each recording was made from: what reading it with SQLite is
 * held to, every field of the answer and not only those the tests below look
 * at. A recording of a real run is among them.
 */
describe('the numbers of a ledger, as they were when Node’s ledger read it', () => {
  const recorded = readdirSync(FIXTURES)
    .filter((file) => file.endsWith('.json'))
    .map((file) => file.slice(0, -'.json'.length))

  it('has a recording of each shape of ledger the evals read', () => {
    assert.ok(recorded.length >= 10, `${recorded.length} recordings`)
  })

  for (const name of recorded) {
    it(`are the same on ${name}`, async () => {
      const said = await measured(name)
      assert.deepEqual(JSON.parse(JSON.stringify(said)), fixtureJson(name).measure)
    })
  }
})

describe('measuring a chief from the ledger', () => {
  it('counts the chief’s controls: a tell and its answer, a pause and a resume, an --after follow-up', async () => {
    const metrics = await measured('controls')
    const p = metrics.plumbing
    assert.deepEqual(
      [p.tells, p.tellsAnswered, p.pauses, p.resumes, p.continuations],
      [1, 1, 1, 1, 1],
    )
    assert.equal(
      metrics.tasks[1].assignee,
      metrics.tasks[0].assignee,
      'the follow-up goes to the same window',
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
  })

  it('does not count a result the chief decided on before it was given it as one that never reached the chief', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-withdrawn-'))
    const file = path.join(dir, 'consensflow.db')
    try {
      const ledger = openLedgerFile(file)
      const worker = (agent) => ({
        agent,
        harness: 'claude-code',
        roles: ['worker'],
        tier: 'standard',
      })
      const project = addProject(ledger, {
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code', agent: null },
        staff: [worker('zeus'), worker('diana')],
      })
      // The participants are the human (1), the chief (2), zeus (3) and diana (4).
      const task = ledger.prepare(
        `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
         VALUES (?, ?, ?, ?, 2, ?, ?, '2026-10-08T10:00:00.000Z', '2026-10-08T10:20:00.000Z')`,
      )
      const message = ledger.prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at, delivered_at)
         VALUES (?, ?, ?, ?, ?, 'x', ?, '2026-10-08T10:00:00.000Z', ?)`,
      )
      // T-1's result waited behind the chief's turn, which accepted the task: the
      // decision withdrew it, never given.
      const one = Number(
        task.run(project, 1, 'Write it', 'Write it', 3, 'accepted').lastInsertRowid,
      )
      message.run(project, 3, 2, 'task', one, 'delivered', '2026-10-08T10:01:00.000Z')
      message.run(project, 2, 3, 'result', one, 'cancelled', null)
      // T-2's reached the chief.
      const two = Number(task.run(project, 2, 'Check it', 'Check it', 4, 'done').lastInsertRowid)
      message.run(project, 4, 2, 'task', two, 'delivered', '2026-10-08T10:02:00.000Z')
      message.run(project, 2, 4, 'result', two, 'delivered', '2026-10-08T10:05:00.000Z')
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
    const metrics = await measured('long-messages')
    assert.deepEqual(metrics.longestResult, { worker: 9010, advisor: 12, reviewer: 0 })
    assert.deepEqual(metrics.ownerMessages, [11, 6000])
    assert.match(metrics.notesText, /DELTA-5530 at the end$/)
  })

  it('counts tasks, parallel work, advice, reviews, questions, notes and the chief’s own edits', async () => {
    const metrics = await measured('chief-turns')
    assert.equal(metrics.chief, 'chief')
    assert.deepEqual(
      metrics.tasks.map((t) => [t.number, t.pool]),
      [
        [1, null],
        [2, null],
        [3, 'advisor'],
        [4, 'reviewer'],
      ],
    )
    assert.deepEqual(
      [metrics.parallel, metrics.advice, metrics.reviews],
      [2, 1, 1],
      'two briefs delivered before either result',
    )
    assert.equal(metrics.questionsOnBoard, 0)
    assert.equal(metrics.askRefused, 1, 'the chief tried cf ask once and was sent to its terminal')
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
  })
})

describe('what the human’s inbox holds', () => {
  it('is its newest 500 messages, which a report reads in order', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-inbox-'))
    try {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedgerFile(file)
      const project = addProject(ledger, {
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code', agent: null },
        staff: [],
      })
      // The human is the first participant and the chief the second: 600 notes from one to the other.
      const note = ledger.prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
         VALUES (?, 1, 2, 'note', ?, 'delivered', '2026-10-08T10:00:00.000Z')`,
      )
      for (let n = 1; n <= 600; n += 1) note.run(project, `note ${n}`)
      ledger.close()
      const { notesToHuman } = measure(file)
      assert.equal(notesToHuman.length, 500)
      assert.deepEqual([notesToHuman[0].id, notesToHuman.at(-1).id], [101, 600])
      assert.equal(notesToHuman[0].body, 'note 101')
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe('the ledger of a real run', () => {
  it('is judged as the run judged it: the round trip held, and so did the board’s plumbing', async () => {
    // `npm run eval -- --scenario round-trip --chief claude --staff claude`, on the native daemon.
    const metrics = await measured('eval-round-trip')
    const trip = (await import('../evals/scenarios/round-trip.mjs')).default
    // The files the run changed are the workspace's, which the ledger does not hold.
    const checks = verdict(trip, { ...metrics, filesChanged: ['site/notes.md'] })
    assert.deepEqual(
      checks.filter((check) => !check.ok),
      [],
    )
    assert.equal(checks.length, 7)
    assert.deepEqual(
      mechanics(metrics, 2).filter((check) => !check.ok),
      [],
    )
    assert.deepEqual([metrics.taskCount, metrics.parallel, metrics.reviews], [2, 1, 1])
    assert.equal(metrics.memberQuestionsBy[0].harness, 'claude-code')
  })
})

describe('measuring parallel work', () => {
  it('counts only windows that had their brief: tasks still on the board never ran', async () => {
    const metrics = await measured('parallel-unrun')
    assert.equal(metrics.taskCount, 3)
    assert.equal(metrics.parallel, 1)
  })

  it('ends a task that ended without a result where it ended', async () => {
    const metrics = await measured('parallel-cancelled')
    assert.equal(metrics.parallel, 1)
  })

  it('counts a window at work from the delivery of its brief, not from the writing of it', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-delivery-'))
    try {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedgerFile(file)
      const worker = (agent) => ({
        agent,
        harness: 'claude-code',
        roles: ['worker'],
        tier: 'standard',
      })
      const project = addProject(ledger, {
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code', agent: null },
        staff: [worker('zeus'), worker('diana')],
      })
      // The participants are the human (1), the chief (2), zeus (3) and diana (4).
      const task = ledger.prepare(
        `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
         VALUES (?, ?, ?, ?, 2, ?, 'done', '2026-10-08T10:00:00.000Z', '2026-10-08T10:20:00.000Z')`,
      )
      const message = ledger.prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at, delivered_at)
         VALUES (?, ?, ?, ?, ?, 'x', 'delivered', ?, ?)`,
      )
      const at = (time) => `2026-10-08T${time}.000Z`
      // T-1 is written at 10:00 and given to its window at 10:10, its result at 10:20;
      // T-2 is written at 10:01, given at 10:02, done at 10:05: before T-1 began.
      const one = Number(task.run(project, 1, 'one', 'one', 3).lastInsertRowid)
      message.run(project, 3, 2, 'task', one, at('10:00:00'), at('10:10:00'))
      message.run(project, 2, 3, 'result', one, at('10:20:00'), null)
      const two = Number(task.run(project, 2, 'two', 'two', 4).lastInsertRowid)
      message.run(project, 4, 2, 'task', two, at('10:01:00'), at('10:02:00'))
      message.run(project, 2, 4, 'result', two, at('10:05:00'), null)
      ledger.close()
      assert.equal(measure(file).parallel, 1)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})

describe('measuring a Switch chief', () => {
  it('reads the chiefs in order, the switches, the history the new chief read and its words, and judges the scenario', async () => {
    // The run's codeword is random; the recording holds the one it was made with.
    const metrics = await measured('chief-switch', { replacing: { 'TERN-4821': CODEWORD } })
    assert.deepEqual(
      [metrics.chiefs, metrics.switches, metrics.historyReads],
      [['claude-code', 'codex'], 1, [{ page: 2, find: null, tools: false }]],
    )
    assert.equal(metrics.chiefWordsNow, `The codeword is ${CODEWORD}; it goes out on Friday.`)
    assert.ok(verdict(chiefSwitch, metrics).every((check) => check.ok))
    assert.match(CODEWORD, /^(TERN|LARK|WREN|KITE|ROOK|SWIFT)-\d{4}$/)
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
  /** A ledger in write-ahead mode and still open, as the daemon's is during a run. */
  async function whileOpen(name, read) {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-turn-'))
    const file = path.join(dir, 'consensflow.db')
    const ledger = loadLedger(file, name, { wal: true })
    try {
      return await read(file)
    } finally {
      ledger.close()
      await rm(dir, { recursive: true, force: true })
    }
  }

  it('is its newest asking turn end since the owner typed, read from a copy of the ledger', async () => {
    // The chief asks while its tasks run; a result arrives, and its next turn
    // asks nothing: the question is still open.
    await whileOpen('open-question-asked', (file) => {
      assert.deepEqual(
        { ...chiefOpenQuestion(file) },
        { id: 'a2', text: 'Tasks are out. What is the final report called?' },
      )
    })
  })

  it('is none before the chief has said anything, and none once the owner answered', async () => {
    await whileOpen('parallel-unrun', (file) => {
      assert.equal(chiefOpenQuestion(file), undefined, 'no turn yet')
    })
    // The same record with the owner’s answer and the chief’s next turn after it.
    await whileOpen('open-question-answered', (file) => {
      assert.equal(chiefOpenQuestion(file), undefined, 'the owner answered')
    })
  })
})

describe("a Devin chief's own question dialog", () => {
  it('is its newest question call no answer followed, read from a copy of its store', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-devin-dialog-'))
    // Devin's data folder, on Windows (APPDATA) or elsewhere: dir/data/devin.
    const env = {
      HOME: dir,
      XDG_DATA_HOME: path.join(dir, 'data'),
      APPDATA: path.join(dir, 'data'),
    }
    try {
      // The chief's conversation is bound to the session 'swift-owl', which Devin's store names.
      const file = path.join(dir, 'consensflow.db')
      loadLedger(file, 'devin-chief').close()
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
    // Waiting (T-1), paused (T-2), called off (T-3), failed (T-4): no member had them.
    // Zeus's, on his session's lane (T-5); diana's, who then left the staff (T-6);
    // and one the human deleted (T-7). `listed` is how many the ledger's own board
    // listed when the ledger was recorded: the lanes' tasks and the open ones.
    const { listed } = fixtureJson('placed')
    assert.equal(listed, 6, 'every task but the deleted one is on the board')
    assert.equal((await measured('placed')).plumbing.placed, listed)
  })
})
