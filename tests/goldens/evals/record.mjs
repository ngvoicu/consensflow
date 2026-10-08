#!/usr/bin/env node
/**
 * Records the ledgers the evals' tests read (`tests/fixtures/ledgers`), while
 * Node's ledger still exists: each ledger is built here with the ledger's own
 * operations, on a clock that moves a second a reading, and written down as
 * the SQL that fills a fresh ledger of the native migrations with its rows
 * (`tests/ledger-file.mjs` loads it). Beside each is what `measure()` answered
 * on the ledger file itself while it read the ledger through Node's code, the
 * numbers the evals' measure is held to now that it reads the file with SQLite:
 * `<name>.json`. That answer is given by the module `--measure` names, which
 * must be that one (`evals/measure.mjs` as it was at 44e1ea95, saved where
 * its imports resolve): this checkout's reads with SQLite, and would hold the
 * measure to itself.
 *
 *   node tests/goldens/evals/record.mjs --measure <module> [name …]
 *   node tests/goldens/evals/record.mjs --measure <module> --file <ledger.db> <name>
 *
 * (then `npm run format`). The second records a ledger file as it is, a real
 * run's. It retires with Node's ledger. The recordings stay as they are: a
 * change to the measure that moves a number is made on purpose, in the
 * recording with it.
 */
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir, userInfo } from 'node:os'
import { join, resolve } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import { openLedger } from '../../../src/ledger/index.js'
import { busyProject, clock, deliver, names } from '../../ledger-fixtures.mjs'

const OUT = fileURLToPath(new URL('../../fixtures/ledgers/', import.meta.url))

/** Where the projects of the home recorded live; loading puts the folder the home is built in. */
const WORK = '@WORK@'

/** The recordings that are no eval's: nothing is measured on them. */
const NOT_MEASURED = new Set(['candidate-home'])

const handle = (ledger, project, name) =>
  ledger.project(project.id).participants.find((p) => p.handle === name)
const project = (ledger, harness = 'claude-code', directory = '/work/site') =>
  ledger.createProject({ directory, name: 'site', chief: { harness } })
const worker = (agent, role = 'worker') => ({
  agent,
  harness: 'claude-code',
  role,
  tier: 'standard',
})
const turn = (id, role, text, complete = true) => ({ id, role, text, complete, at: null })

/**
 * Each ledger: a build, and what else is written down of it. `answer` is
 * what is recorded besides `measure()`'s answer on the file.
 */
const BUILDS = {
  // A tell and its answer, a pause and a resume, an --after follow-up.
  controls(ledger) {
    const made = project(ledger)
    ledger.addMember(made.id, worker('zeus'))
    ledger.createTask(made.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Sleep, then write',
    })
    const given = ledger.assignTask(made.id, 1, handle(ledger, made, 'zeus').id)
    deliver(ledger, given.message)
    ledger.pauseTask(made.id, 1, { by: 'chief' })
    const told = ledger.ask(made.id, {
      from: 'chief',
      to: given.task.assignee,
      task: 1,
      body: 'Which file?',
      urgent: true,
    })
    deliver(ledger, told)
    deliver(ledger, ledger.answer(told.id, { from: told.recipientId, body: 'site/notes.md' }))
    const resumed = ledger.resumeTask(made.id, 1, { by: 'chief', body: 'Go on' })
    if (resumed.message !== null) deliver(ledger, resumed.message)
    ledger.recordResult(made.id, 1, { body: 'Written' })
    ledger.acceptTask(made.id, 1, { by: 'chief' })
    ledger.createTask(made.id, { from: 'chief', after: 1, body: 'Add a line' })
  },

  // The longest result of each kind, the owner's answers, the notes whole.
  'long-messages'(ledger) {
    const made = project(ledger)
    for (const [agent, role] of [
      ['zeus', 'worker'],
      ['athena', 'advisor'],
    ]) {
      ledger.addMember(made.id, worker(agent, role))
    }
    deliver(
      ledger,
      ledger.createTask(made.id, { from: 'chief', to: 'zeus', body: 'Report' }).message,
    )
    ledger.createTask(made.id, { from: 'chief', pool: 'advisor', tier: 'standard', body: 'Advise' })
    ledger.assignTask(made.id, 2, handle(ledger, made, 'athena').id)
    deliver(
      ledger,
      ledger.task(made.id, 2).messages.find((m) => m.kind === 'task'),
    )
    ledger.recordResult(made.id, 1, { body: `${'r'.repeat(8999)}\nCEDRU-7314` })
    ledger.recordResult(made.id, 2, { body: 'short advice' })
    const chief = handle(ledger, made, 'chief')
    ledger.copyTranscript(ledger.startConversation(chief.id, { harness: 'claude-code' }).id, [
      turn('u1', 'user', 'The report?'),
      turn('a1', 'assistant', 'Which name should it have?'),
      turn('u2', 'user', 'a'.repeat(6000)),
      turn('u3', 'user', '[ConsensFlow m-9 · note from ConsensFlow]\nnot the owner'),
    ])
    ledger.note(made.id, {
      from: 'chief',
      to: 'human',
      body: `${'n'.repeat(300)} DELTA-5530 at the end`,
    })
  },

  // Tasks, parallel work, advice, reviews, a member's question, notes, the chief's own edits.
  'chief-turns'(ledger) {
    const made = project(ledger)
    for (const [agent, role] of [
      ['zeus', 'worker'],
      ['diana', 'worker'],
      ['athena', 'advisor'],
      ['hera', 'reviewer'],
    ]) {
      ledger.addMember(made.id, worker(agent, role))
    }
    const one = ledger.createTask(made.id, { from: 'chief', to: 'zeus', body: 'Write it' })
    const two = ledger.createTask(made.id, { from: 'chief', to: 'diana', body: 'Translate it' })
    deliver(ledger, one.message)
    deliver(ledger, two.message)
    const asked = ledger.ask(made.id, { from: 'zeus', to: 'chief', body: 'Which colour?', task: 1 })
    deliver(ledger, asked)
    deliver(ledger, ledger.answer(asked.id, { from: asked.recipientId, body: 'Blue' }))
    deliver(ledger, ledger.recordResult(made.id, 1, { body: 'Written' }).message)
    ledger.recordResult(made.id, 2, { body: 'Translated' })
    ledger.acceptTask(made.id, 1, { by: 'chief' })
    ledger.createTask(made.id, {
      from: 'chief',
      pool: 'advisor',
      tier: 'standard',
      body: 'Which law applies?',
    })
    ledger.createTask(made.id, {
      from: 'chief',
      pool: 'reviewer',
      tier: 'standard',
      body: 'Review T-1',
    })
    ledger.note(made.id, {
      from: 'chief',
      to: 'human',
      body: 'The guide says 1 to 5; the site shows a colour.',
    })
    const conversation = ledger.startConversation(handle(ledger, made, 'chief').id, {
      harness: 'claude-code',
    })
    ledger.copyTranscript(conversation.id, [
      turn('u1', 'user', 'Add the page'),
      turn('a1', 'assistant', 'Looking.', false),
      turn('t1', 'tool', 'The file /work/site/index.html has been updated.'),
      turn('t0', 'tool', 'cf: ask the human here in your terminal: they read and answer you there'),
      turn('a2', 'assistant', 'Now the menu?', false),
      turn('t2', 'tool', 'File created successfully at: /work/site/legislatie.html'),
      turn('t3', 'tool', 'diff --git a/x b/x'),
      turn('a3', 'assistant', 'Done: the page is in place.'),
      turn('u2', 'user', 'Next'),
      turn('a4', 'assistant', 'Shall I translate it? Or wait for you?'),
    ])
  },

  // Only windows that had their brief ran: tasks still on the board never did.
  'parallel-unrun'(ledger) {
    const made = project(ledger)
    for (const agent of ['zeus', 'diana']) ledger.addMember(made.id, worker(agent))
    deliver(
      ledger,
      ledger.createTask(made.id, { from: 'chief', to: 'zeus', body: 'Write it' }).message,
    )
    for (const body of ['Translate it', 'Check it']) {
      ledger.createTask(made.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
    }
  },

  // A task that ended without a result ends where it ended.
  'parallel-cancelled'(ledger) {
    const made = project(ledger)
    for (const agent of ['zeus', 'diana']) ledger.addMember(made.id, worker(agent))
    deliver(
      ledger,
      ledger.createTask(made.id, { from: 'chief', to: 'zeus', body: 'Write it' }).message,
    )
    ledger.cancelTask(made.id, 1, { by: 'chief' })
    deliver(
      ledger,
      ledger.createTask(made.id, { from: 'chief', to: 'diana', body: 'Then this' }).message,
    )
  },

  // A chief switched from Claude Code to Codex, which read the history.
  'chief-switch'(ledger) {
    const made = project(ledger)
    const chief = handle(ledger, made, 'chief')
    ledger.copyTranscript(ledger.startConversation(chief.id, { harness: 'claude-code' }).id, [
      turn('u', 'user', 'The codeword is TERN-4821'),
    ])
    ledger.switchChief(made.id, { harness: 'codex', agent: 'eval-codex-worker' })
    ledger.copyTranscript(ledger.startConversation(chief.id, { harness: 'codex' }).id, [
      turn('a', 'assistant', 'The codeword is TERN-4821; it goes out on Friday.'),
    ])
    ledger.historyRead(made.id, { page: 2 })
  },

  // A Codex chief that asked while its tasks ran, before the owner typed.
  'open-question-asked'(ledger) {
    const made = project(ledger, 'codex')
    const conversation = ledger.startConversation(handle(ledger, made, 'chief').id, {
      harness: 'codex',
    })
    ledger.copyTranscript(conversation.id, OPEN_QUESTION)
  },

  // The same chief once the owner answered it.
  'open-question-answered'(ledger) {
    const made = project(ledger, 'codex')
    const conversation = ledger.startConversation(handle(ledger, made, 'chief').id, {
      harness: 'codex',
    })
    ledger.copyTranscript(conversation.id, [
      ...OPEN_QUESTION,
      turn('u3', 'user', 'DELTA-5530.'),
      turn('a4', 'assistant', 'Noted; the note is sent.'),
    ])
  },

  // A Devin chief, its conversation bound to the session its own store names.
  'devin-chief'(ledger) {
    const made = project(ledger, 'devin')
    ledger.bindConversation(
      ledger.startConversation(handle(ledger, made, 'chief').id, { harness: 'devin' }).id,
      'swift-owl',
    )
  },

  // A home of the Candidate's shape (tests/integration/home-fixture.mjs): projects open when the
  // app quit, and projects closed with the history that went with them.
  'candidate-home'(ledger) {
    const folder = (name) => `${WORK}/${name}`
    const site = ledger.createProject({
      directory: folder('site'),
      name: 'site',
      chief: { harness: 'claude-code', agent: 'lead' },
      staff: [{ agent: 'builder', harness: 'claude-code', roles: ['worker'], tier: 'standard' }],
    })
    const chiefConversation = ledger.startConversation(handle(ledger, site, 'chief').id, {
      harness: 'claude-code',
    })
    ledger.bindConversation(chiefConversation.id, 'site-chief-session')
    ledger.copyTranscript(chiefConversation.id, [
      { id: 'u1', role: 'user', text: 'Build the parser, then the changelog.' },
      { id: 'a1', role: 'assistant', text: 'T-1 goes to the builder.' },
    ])
    ledger.createTask(site.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Build the parser\n\nThe grammar is in docs/grammar.md.',
    })
    const first = ledger.assignTask(site.id, 1, handle(ledger, site, 'builder').id)
    deliver(ledger, first.message)
    const session = ledger.startConversation(first.message.recipientId, { harness: 'claude-code' })
    ledger.bindConversation(session.id, 'site-builder-session')
    ledger.copyTranscript(session.id, [
      { id: 'u1', role: 'user', text: 'Build the parser' },
      { id: 'a1', role: 'assistant', text: 'Parser done: 14 tests pass' },
    ])
    deliver(ledger, ledger.recordResult(site.id, 1, { body: 'Parser done: 14 tests pass' }).message)
    ledger.acceptTask(site.id, 1, { by: 'chief' })
    ledger.createTask(site.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Draft the changelog',
    })
    ledger.cancelTask(site.id, 2, { by: 'chief' })
    ledger.note(site.id, { from: 'chief', to: 'human', body: 'T-1 is accepted: the parser is in.' })
    const docs = ledger.createProject({
      directory: folder('docs'),
      name: 'docs',
      chief: { harness: 'claude-code', agent: 'lead' },
    })
    ledger.note(docs.id, { from: 'chief', to: 'human', body: 'The docs project is open.' })
    // Closed by the human, with all that went with them: a project with a row in every table
    // and a chief switched to Codex, and one whose member's agent has left the roster, with its
    // gate holding a brief and a question.
    const billing = busyProject(ledger, folder('billing'))
    ledger.setProjectState(billing.project[0], 'suspended')
    const legacy = ledger.createProject({
      directory: folder('legacy'),
      name: 'legacy',
      chief: { harness: 'codex', agent: 'hyperion' },
      gate: true,
    })
    ledger.addMember(legacy.id, {
      agent: 'retired-one',
      harness: 'pi',
      role: 'worker',
      tier: 'light',
    })
    ledger.createTask(legacy.id, { from: 'chief', to: 'retired-one', body: 'Tidy the changelog' })
    ledger.note(legacy.id, { from: 'retired-one', to: 'chief', body: 'Which entries are old?' })
    ledger.setProjectState(legacy.id, 'suspended')
  },

  // Tasks waiting, paused, called off, failed, on a session's lane, a member's who left, deleted.
  placed(ledger) {
    const made = project(ledger)
    for (const agent of ['zeus', 'diana']) ledger.addMember(made.id, worker(agent))
    const open = (body) =>
      ledger.createTask(made.id, { from: 'chief', pool: 'worker', tier: 'standard', body })
    const finish = (number, agent) => {
      const given = ledger.assignTask(made.id, number, handle(ledger, made, agent).id)
      deliver(ledger, given.message)
      ledger.recordResult(made.id, number, { body: 'Done' })
    }
    for (const body of ['Waits', 'Paused', 'Called off', 'Failed']) open(body)
    ledger.pauseTask(made.id, 2, { by: 'human' })
    ledger.cancelTask(made.id, 3, { by: 'chief' })
    ledger.failTask(made.id, 4, { reason: 'its launch never came up' })
    open('Parser')
    finish(5, 'zeus')
    open('Docs')
    finish(6, 'diana')
    ledger.removeMember(made.id, 'diana')
    open('Old spike')
    finish(7, 'zeus')
    ledger.acceptTask(made.id, 7, { by: 'chief' })
    ledger.deleteTasks(made.id, [7])
    const board = ledger.board(made.id)
    // How many tasks the board lists, which the eval's count of the ones it places is held to.
    return { listed: board.open.length + board.lanes.reduce((n, lane) => n + lane.tasks.length, 0) }
  },
}

/** A Codex chief asks while its tasks run; a result arrives, and its next turn asks nothing. */
const OPEN_QUESTION = [
  turn('u1', 'user', 'Put three tasks on the board, then ask me the report name.'),
  turn('a1', 'assistant', 'Reading?', false),
  turn('a2', 'assistant', 'Tasks are out. What is the final report called?'),
  turn('u2', 'user', '[ConsensFlow m-7 · T-1 · result from @worker]\nDone.'),
  turn('a3', 'assistant', 'T-1 is in; waiting for the rest.'),
]

/**
 * A recording says nothing of who made it: the home folder of the machine is
 * `/home/user` in it, and the name of the user `user` (a ledger of a real run
 * holds both, in the folder its project works in and in a listing a window
 * made). `tests/ledger-file.test.mjs` holds every recording to it.
 */
function neutral(text) {
  const home = homedir()
  const name = userInfo().username.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return text
    .replaceAll(home, '/home/user')
    .replaceAll(home.replaceAll('\\', '/'), '/home/user')
    .replace(new RegExp(`\\b${name}\\b`, 'g'), 'user')
}

/** The rows of the ledger file `file`, as SQL that fills a fresh ledger with them. */
function dump(file) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const quote = (value) => {
      if (value === null) return 'NULL'
      if (typeof value === 'number' || typeof value === 'bigint') return String(value)
      if (value.includes('\0')) throw new Error('a text holds a NUL, which SQL text cannot')
      return `'${value.replaceAll("'", "''")}'`
    }
    return db
      .prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY rowid",
      )
      .all()
      .flatMap(({ name }) => {
        const columns = db
          .prepare(`PRAGMA table_info(${name})`)
          .all()
          .map((column) => column.name)
        return db
          .prepare(`SELECT * FROM ${name} ORDER BY rowid`)
          .all()
          .map(
            (row) =>
              `INSERT INTO ${name} (${columns.join(', ')}) VALUES (${columns.map((column) => quote(row[column])).join(', ')});`,
          )
      })
      .join('\n')
  } finally {
    db.close()
  }
}

const { values, positionals: wanted } = parseArgs({
  options: { measure: { type: 'string' }, file: { type: 'string' } },
  allowPositionals: true,
})
if (values.measure === undefined) throw new Error('--measure names the module that answers')
const { chiefOpenQuestion, measure } = await import(pathToFileURL(resolve(values.measure)).href)

/** The ledgers to record, by name: the file as it is when `--file` is given, else each one built here. */
const todo = wanted.length > 0 ? wanted : Object.keys(BUILDS)
if (values.file !== undefined) {
  if (wanted.length !== 1) throw new Error('--file records one ledger: name it')
  if (!existsSync(values.file)) throw new Error(`no ledger file ${values.file}`)
} else {
  for (const name of todo) {
    if (BUILDS[name] === undefined) throw new Error(`no such ledger: ${name}`)
  }
}

mkdirSync(OUT, { recursive: true })
const dir = mkdtempSync(join(tmpdir(), 'cf-record-evals-'))
try {
  for (const name of todo) {
    let file = values.file
    let extra = {}
    if (file === undefined) {
      file = join(dir, `${name}.db`)
      const ledger = openLedger(file, { now: clock(), names: names() })
      extra = BUILDS[name](ledger) ?? {}
      ledger.close()
    }
    writeFileSync(join(OUT, `${name}.sql`), `${neutral(dump(file))}\n`)
    if (!NOT_MEASURED.has(name)) {
      const answer = { measure: measure(file), ...extra }
      if (name.startsWith('open-question')) {
        answer.chiefOpenQuestion = chiefOpenQuestion(file) ?? null
      }
      writeFileSync(join(OUT, `${name}.json`), `${neutral(JSON.stringify(answer, null, 2))}\n`)
    }
    process.stdout.write(`recorded ${name}\n`)
  }
} finally {
  rmSync(dir, { recursive: true, force: true })
}
