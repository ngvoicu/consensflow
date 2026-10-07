import { createHash } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative, sep } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { devinFolders } from '../src/harnesses.js'
import { openLedger } from '../src/ledger/index.js'

/**
 * What a chief did, read from the ledger of a finished eval run: the tasks it
 * put on the board and how many ran side by side, what it asked and told the
 * human, the advice and reviews it asked for, the edits it made with its own
 * hands (the transcript copy's Edit and Write results, a Claude Code count;
 * null for another chief), its last words, and which files of the fixture
 * the run changed or added (`workspace` against `fixture`, any harness).
 * The daemon must have closed the ledger first: the ledger holds its file.
 */
export function measure(file, { fixture = null, workspace = null, pickers = 0 } = {}) {
  // Every task, from the task table itself: the board is one of the things a
  // run checks, so it cannot be what the tasks are counted from. Read before
  // the ledger opens, which holds the file to itself.
  const numbers = taskNumbers(file)
  const ledger = openLedger(file)
  let metrics
  try {
    const [project] = ledger.projects()
    if (project === undefined) throw new Error('the eval ledger holds no project')
    const full = ledger.project(project.id)
    const human = full.participants.find((p) => p.role === 'human')
    const chief = full.participants.find((p) => p.role === 'chief')
    const tasks = numbers
      .filter((row) => row.projectId === project.id)
      .map((row) => ledger.task(project.id, row.number))
    // The inbox reads newest first; a report reads in order.
    const toHuman = ledger
      .inbox(human.id, { limit: 500 })
      .filter((message) => message.sender === chief.handle)
      .sort((a, b) => a.id - b.id)
    const spans = tasks
      .filter((task) => task.requester === chief.handle)
      .map((task) => {
        const brief = task.messages.find((m) => m.kind === 'task')
        const result = task.messages.find((m) => m.kind === 'result')
        // A window at work: from its brief's delivery to its result, or to
        // the task's end when it ended without one. A task whose brief never
        // reached a window (open, queued, withdrawn) never ran.
        return {
          number: task.number,
          from: brief?.deliveredAt ?? null,
          to: result?.createdAt ?? (ENDED.has(task.state) ? task.updatedAt : null),
        }
      })
    metrics = {
      project: { id: project.id, name: project.name },
      chief: chief.handle,
      tasks: tasks
        .filter((task) => task.requester === chief.handle)
        .map((task) => ({
          number: task.number,
          title: task.title,
          pool: task.pool,
          tier: task.tier,
          state: task.state,
          assignee: task.assignee,
        })),
      parallel: mostAtOnce(spans),
      advice: tasks.filter((t) => t.requester === chief.handle && t.pool === 'advisor').length,
      reviews: tasks.filter((t) => t.requester === chief.handle && t.pool === 'reviewer').length,
      // The human is asked in the chief's terminal: the ledger refuses a
      // question for them, so this stays 0, and a run says so.
      questionsOnBoard: toHuman.filter((m) => m.kind === 'question').length,
      notesToHuman: toHuman
        .filter((m) => m.kind === 'note')
        .map((m) => ({ id: m.id, body: m.body.slice(0, 200) })),
      // The whole text of the chief's notes: a scenario may look for words at the end.
      notesText: toHuman
        .filter((m) => m.kind === 'note')
        .map((m) => m.body)
        .join('\n'),
      // The longest result each kind of member sent the chief, in characters
      // (a worker's task may name no pool: one given to a window by name).
      longestResult: Object.fromEntries(
        ['worker', 'advisor', 'reviewer'].map((pool) => [
          pool,
          Math.max(
            0,
            ...tasks
              .filter(
                (t) =>
                  t.requester === chief.handle &&
                  (pool === 'worker'
                    ? t.pool !== 'advisor' && t.pool !== 'reviewer'
                    : t.pool === pool),
              )
              .flatMap((t) => t.messages.filter((m) => m.kind === 'result'))
              .map((m) => m.body.length),
          ),
        ]),
      ),
      taskCount: tasks.length,
      // A Switch chief: the harnesses the chief ran on, in order (each switch
      // starts a conversation), how many switches, and the history the chief
      // read with cf history (each read is in the ledger, whatever the harness).
      chiefs: [...ledger.chiefHistory(project.id).map((c) => c.harness), chief.harness],
      switches: eventsOf(ledger, project.id, 'chief.switched').length,
      historyReads: eventsOf(ledger, project.id, 'chief.history.read').map((e) => e.data),
      chiefId: chief.id,
      chiefHarness: chief.harness,
      humanId: human.id,
    }
  } finally {
    ledger.close()
  }
  const { turnEnds, ...seen } = chiefWindow(file, metrics.chiefId, metrics.chiefHarness)
  return {
    ...metrics,
    ...seen,
    ownerQuestions: ownerQuestions(turnEnds, { pickers }),
    plumbing: plumbing(file, metrics.chiefId, metrics.humanId),
    chiefWordsNow: chiefWordsNow(file),
    memberQuestionsBy: memberQuestionsBy(file, metrics.chiefId, metrics.humanId),
    filesChanged: fixture === null || workspace === null ? [] : changed(fixture, workspace),
  }
}

/**
 * The board's plumbing, counted from the ledger whatever the chief decided:
 * briefs delivered to members, results delivered back to the chief, questions
 * members put to the chief and the answers delivered back, tasks accepted.
 */
/**
 * The questions members put to the chief, by the asker's role and harness:
 * how many, how many the chief answered, how many answers reached the
 * member's window, and the longest question in characters.
 */
function memberQuestionsBy(file, chiefId, humanId) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    return db
      .prepare(
        `SELECT p.role, p.harness, COUNT(*) AS asked,
           SUM(EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')) AS answered,
           SUM(EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state IN ('delivered', 'read'))) AS delivered,
           MAX(LENGTH(q.body)) AS longest
         FROM message q JOIN participant p ON p.id = q.sender_id
         WHERE q.kind = 'question' AND q.recipient_id = ? AND q.sender_id NOT IN (?, ?)
         GROUP BY p.role, p.harness ORDER BY p.role, p.harness`,
      )
      .all(chiefId, chiefId, humanId)
      .map((row) => ({ ...row }))
  } finally {
    db.close()
  }
}

function plumbing(file, chiefId, humanId) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const count = (sql, ...args) => db.prepare(sql).get(...args).n
    const fromMember = 'sender_id IS NOT NULL AND sender_id != ? AND sender_id != ?'
    return {
      // A brief withdrawn with its cancelled task was rightly never delivered.
      briefs: count(
        "SELECT COUNT(*) AS n FROM message WHERE kind = 'task' AND state != 'cancelled'",
      ),
      briefsDelivered: count(
        "SELECT COUNT(*) AS n FROM message WHERE kind = 'task' AND state IN ('delivered', 'read')",
      ),
      results: count(
        "SELECT COUNT(*) AS n FROM message WHERE kind = 'result' AND recipient_id = ?",
        chiefId,
      ),
      resultsDelivered: count(
        "SELECT COUNT(*) AS n FROM message WHERE kind = 'result' AND recipient_id = ? AND state IN ('delivered', 'read')",
        chiefId,
      ),
      memberQuestions: count(
        `SELECT COUNT(*) AS n FROM message WHERE kind = 'question' AND recipient_id = ? AND ${fromMember}`,
        chiefId,
        chiefId,
        humanId,
      ),
      memberQuestionsAnswered: count(
        `SELECT COUNT(*) AS n FROM message q WHERE q.kind = 'question' AND q.recipient_id = ? AND ${fromMember.replaceAll('sender_id', 'q.sender_id')}
           AND EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')`,
        chiefId,
        chiefId,
        humanId,
      ),
      answersDelivered: count(
        `SELECT COUNT(*) AS n FROM message a JOIN message q ON q.id = a.reply_to
           WHERE a.kind = 'answer' AND a.state IN ('delivered', 'read') AND q.kind = 'question' AND q.recipient_id = ? AND ${fromMember.replaceAll('sender_id', 'q.sender_id')}`,
        chiefId,
        chiefId,
        humanId,
      ),
      accepted: count("SELECT COUNT(*) AS n FROM task WHERE state = 'accepted'"),
      // A tell the chief's own cancel took back is not one owed an answer; an answer a
      // cancel took back was still given (both daemons withdraw what is queued for a
      // cancelled task alike: crates/cf-ledger/tests/cancelled.rs).
      tells: count(
        `SELECT COUNT(*) AS n FROM message WHERE kind = 'question' AND urgent = 1 AND sender_id = ? AND state != 'cancelled'`,
        chiefId,
      ),
      tellsAnswered: count(
        `SELECT COUNT(*) AS n FROM message q WHERE q.kind = 'question' AND q.urgent = 1 AND q.sender_id = ? AND q.state != 'cancelled'
           AND EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'failed')`,
        chiefId,
      ),
      // The tasks the board places: every one the human has not deleted. A lane has the
      // task of its participant (or of its member, once the session ended); every other,
      // whatever its state (waiting for a member, paused or called off before any had
      // it, a removed member's), is among the open ones, on both daemons alike
      // (tests/integration/core-board.test.mjs).
      placed: count('SELECT COUNT(*) AS n FROM task WHERE deleted_at IS NULL'),
      pauses: count(
        "SELECT COUNT(*) AS n FROM event WHERE kind = 'task.state' AND json_extract(data, '$.to') = 'paused'",
      ),
      resumes: count(
        "SELECT COUNT(*) AS n FROM event WHERE kind = 'task.state' AND json_extract(data, '$.from') = 'paused' AND json_extract(data, '$.to') IN ('queued', 'working')",
      ),
      // A task given straight to a session's window: only `--after` does that.
      continuations: count(
        `SELECT COUNT(*) AS n FROM event e JOIN participant p
           ON p.project_id = e.project_id AND p.handle = json_extract(e.data, '$.to')
         WHERE e.kind = 'task.created' AND p.member_id IS NOT NULL`,
      ),
    }
  } finally {
    db.close()
  }
}

/**
 * Did the board's plumbing hold, whatever the chief decided: every brief
 * delivered, every result back to the chief, every member's question
 * answered and the answer delivered, every task shown on the board
 * (`boardTasks`: how many the board listed when the run ended).
 */
export function mechanics(metrics, boardTasks) {
  const p = metrics.plumbing
  const held = (name, got, of) => ({ name: `${name} (${got}/${of})`, ok: got === of })
  return [
    held('every task brief was delivered', p.briefsDelivered, p.briefs),
    held('every result reached the chief', p.resultsDelivered, p.results),
    held(
      'every question a member asked the chief was answered',
      p.memberQuestionsAnswered,
      p.memberQuestions,
    ),
    held('every answer reached the member', p.answersDelivered, p.memberQuestionsAnswered),
    held('every tell the chief sent was answered', p.tellsAnswered, p.tells),
    held('the board showed every task', boardTasks, p.placed),
  ]
}

/**
 * Files a harness keeps in the project for itself, which no agent wrote:
 * Claude Code's lock while a wakeup it scheduled waits (a Claude chief on
 * Windows scheduled one to check on its worker, 2026-10-04).
 */
const HARNESS_OWN = new Set(['.claude/scheduled_tasks.lock'])

/** Files of the fixture that differ in the workspace, and files the run added, relative. */
export function changed(fixture, workspace) {
  const digest = (path) => createHash('sha256').update(readFileSync(path)).digest('hex')
  // Reports name files with forward slashes on every platform.
  const list = (root, dir = root) =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name)
      if (name === '.git' || name === 'node_modules') return []
      if (statSync(path).isDirectory()) return list(root, path)
      const file = relative(root, path).split(sep).join('/')
      return HARNESS_OWN.has(file) ? [] : [file]
    })
  const before = new Map(list(fixture).map((path) => [path, digest(join(fixture, path))]))
  return list(workspace)
    .filter((path) => before.get(path) !== digest(join(workspace, path)))
    .sort()
}

/** The chief's own turns and edits, from the daemon's copy of its conversation. */
function chiefWindow(file, chiefId, harness) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const items = db
      .prepare(
        `SELECT t.role, t.text, t.complete FROM transcript t
         JOIN conversation c ON c.id = t.conversation_id
         WHERE c.participant_id = ? ORDER BY t.conversation_id, t.seq`,
      )
      .all(chiefId)
    const tools = items.filter((i) => i.role === 'tool')
    const assistant = items.filter((i) => i.role === 'assistant')
    return {
      chiefTurns: assistant.length,
      // Claude's Edit and Write results say so in words; other harnesses' do not.
      chiefEdits:
        harness === 'claude-code'
          ? tools.filter((i) => /has been updated|File created successfully/.test(i.text)).length
          : null,
      chiefLastWords: assistant.at(-1)?.text.slice(0, 1500) ?? '',
      // The messages that ended its turns: what it left the owner to read.
      turnEnds: assistant.filter((i) => i.complete === 1).map((i) => i.text),
      // What the owner typed into its window (ConsensFlow's own carry its header), in characters.
      ownerMessages: items
        .filter((i) => i.role === 'user' && !i.text.startsWith('[ConsensFlow m-'))
        .map((i) => i.text.length),
      // A `cf ask` it tried: refused, it is told to ask in its terminal.
      askRefused: tools.filter((i) => i.text.includes('ask the human here in your terminal'))
        .length,
    }
  } finally {
    db.close()
  }
}

/** States a task ends in without a result. */
const ENDED = new Set(['cancelled', 'failed'])

/** Every task's project and number, read from the task table itself. */
function taskNumbers(file) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    return db
      .prepare('SELECT project_id AS projectId, number FROM task ORDER BY number')
      .all()
      .map((row) => ({ projectId: row.projectId, number: row.number }))
  } finally {
    db.close()
  }
}

/** The most tasks whose windows had the brief and no result yet at one moment. */
function mostAtOnce(spans) {
  const points = spans
    .filter((span) => span.from !== null)
    .flatMap((span) => [
      { at: span.from, delta: 1 },
      { at: span.to ?? '9999', delta: -1 },
    ])
    .sort((a, b) => (a.at < b.at ? -1 : a.at > b.at ? 1 : a.delta - b.delta))
  let open = 0
  let most = 0
  for (const point of points) {
    open += point.delta
    most = Math.max(most, open)
  }
  return most
}

/** Each of a scenario's expectations against the metrics: what held, what did not. */
export function verdict(scenario, metrics) {
  return scenario.expectations.map(({ name, holds }) => ({ name, ok: Boolean(holds(metrics)) }))
}

/**
 * The question sentences in a text put to the owner: a sentence that ends in
 * a question mark, not one in fenced or inline code or in a link. A
 * rhetorical question counts too; the report keeps the texts for a person
 * to check.
 */
export function questionSentences(text) {
  const prose = String(text ?? '')
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/`[^`\n]*`/g, ' ')
    .replace(/\b[a-z][a-z0-9+.-]*:\/\/\S+/gi, ' ')
    // Emphasis marks around a question (**Which one?**) are not its end.
    .replace(/[*_]+/g, '')
  return prose
    .split('\n')
    .flatMap((line) => line.split(/(?<=[.!?])\s+/))
    .map((sentence) => sentence.trim().replace(/["'”»)\]]+$/, ''))
    .filter((sentence) => sentence.endsWith('?'))
}

export function countQuestions(text) {
  return questionSentences(text).length
}

/** How many questions the chief put to the owner in its terminal, in words or in a picker. */
export const askedInTerminal = (m) => m.ownerQuestions.questions + m.ownerQuestions.pickers

/**
 * What a chief asked the owner, all of it in its terminal: the question
 * sentences of each message that ended a turn, and the pickers (its harness's
 * own question dialog) the owner answered there. The texts stay for a person
 * to check.
 */
export function ownerQuestions(turnEnds, { pickers = 0 } = {}) {
  const asking = turnEnds.filter((text) => countQuestions(text) > 0)
  return {
    questions: asking.reduce((sum, text) => sum + countQuestions(text), 0),
    turnsAsking: asking.length,
    pickers,
    texts: asking,
  }
}

/**
 * The chief's newest message that ended a turn, during a run. The daemon's
 * ledger holds its file exclusively (PRAGMA locking_mode = EXCLUSIVE), so
 * another connection finds it locked: this reads a copy of the file and its
 * write-ahead log.
 */
/** Every event of one kind in a project, oldest first. */
function eventsOf(ledger, projectId, kind) {
  const found = []
  let after = 0
  for (;;) {
    const page = ledger.events(projectId, { after, limit: 500 })
    found.push(...page.filter((event) => event.kind === kind))
    if (page.length < 500) return found
    after = page.at(-1).id
  }
}

/** What the chief wrote in its current conversation: the one the last switch started. */
export function chiefWordsNow(file) {
  return onCopy(file, (db) =>
    db
      .prepare(
        `SELECT t.text FROM transcript t
         JOIN conversation c ON c.id = t.conversation_id
         JOIN participant p ON p.id = c.participant_id
         WHERE p.role = 'chief' AND c.ended_at IS NULL AND t.role = 'assistant'
         ORDER BY t.seq`,
      )
      .all()
      .map((row) => row.text)
      .join('\n'),
  )
}

/**
 * The chief's newest words that ended a turn with a question since the owner
 * last typed into its window (ConsensFlow's own messages carry their header),
 * or undefined. A question asked while its tasks ran stays open through
 * later turns that ask nothing: a Codex chief asked the owner one, then
 * waited for the answer to write its note (Windows, 2026-10-03).
 */
export function chiefOpenQuestion(file) {
  return onCopy(file, (db) => {
    let open
    for (const item of db
      .prepare(
        `SELECT t.item_id AS id, t.role, t.text, t.complete FROM transcript t
         JOIN conversation c ON c.id = t.conversation_id
         JOIN participant p ON p.id = c.participant_id
         WHERE p.role = 'chief' ORDER BY t.conversation_id, t.seq`,
      )
      .all()) {
      if (item.role === 'user' && !item.text.startsWith('[ConsensFlow m-')) open = undefined
      else if (item.role === 'assistant' && item.complete === 1 && countQuestions(item.text) > 0)
        open = { id: item.id, text: item.text }
    }
    return open
  })
}

/**
 * The questions a Devin chief's own dialog holds open: its newest call to
 * its question tool that no tool message has answered, read from a copy of
 * Devin's store, each question with its options; null when none is open.
 * The chief's session is its newest conversation in the ledger `file`.
 */
export function devinChiefQuestions(file, env) {
  const session = onCopy(
    file,
    (db) =>
      db
        .prepare(
          `SELECT c.native_session AS id FROM conversation c
           JOIN participant p ON p.id = c.participant_id
           WHERE p.role = 'chief' ORDER BY c.id DESC LIMIT 1`,
        )
        .get()?.id,
  )
  const store = join(devinFolders(env).data, 'cli', 'sessions.db')
  if (!session || !existsSync(store)) return null
  return onCopy(store, (db) => {
    const answered = new Set()
    for (const row of db
      .prepare('SELECT chat_message FROM message_nodes WHERE session_id = ? ORDER BY row_id DESC')
      .all(session)) {
      const message = JSON.parse(row.chat_message)
      if (message.role === 'tool') answered.add(message.tool_call_id)
      const call =
        message.role === 'assistant'
          ? message.tool_calls?.find((c) => c.name === 'ask_user_question')
          : undefined
      if (call) return answered.has(call.id) ? null : (call.arguments?.questions ?? null)
    }
    return null
  })
}

/** `read(db)` on a copy of the SQLite database `file` and its log: the one in use stays shut. */
function onCopy(file, read) {
  const dir = mkdtempSync(join(tmpdir(), 'cf-eval-db-'))
  try {
    const copy = join(dir, 'copy.db')
    copyFileSync(file, copy)
    if (existsSync(`${file}-wal`)) copyFileSync(`${file}-wal`, `${copy}-wal`)
    const db = new DatabaseSync(copy)
    try {
      return read(db)
    } finally {
      db.close()
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}
