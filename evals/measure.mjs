import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
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
export function measure(file, { fixture = null, workspace = null } = {}) {
  const ledger = openLedger(file)
  let metrics
  try {
    const [project] = ledger.projects()
    if (project === undefined) throw new Error('the eval ledger holds no project')
    const full = ledger.project(project.id)
    const human = full.participants.find((p) => p.role === 'human')
    const chief = full.participants.find((p) => p.role === 'chief')
    const tasks = ledger
      .board(project.id)
      .lanes.flatMap((lane) => lane.tasks)
      .concat(ledger.board(project.id).open)
      .map((task) => ledger.task(project.id, task.number))
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
        return {
          number: task.number,
          from: brief?.deliveredAt ?? brief?.createdAt ?? task.createdAt,
          to: result?.createdAt ?? null,
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
      questionsToHuman: toHuman
        .filter((m) => m.kind === 'question')
        .map((m) => ({ id: m.id, options: m.questions !== null, body: m.body.slice(0, 200) })),
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
      // The human's answers to the chief, in characters.
      answersToChief: ledger
        .inbox(chief.id, { limit: 500 })
        .filter((m) => m.kind === 'answer' && m.sender === human.handle)
        .map((m) => m.body.length),
      taskCount: tasks.length,
      chiefId: chief.id,
      chiefHarness: chief.harness,
      humanId: human.id,
    }
  } finally {
    ledger.close()
  }
  return {
    ...metrics,
    ...chiefWindow(file, metrics.chiefId, metrics.chiefHarness),
    plumbing: plumbing(file, metrics.chiefId, metrics.humanId),
    filesChanged: fixture === null || workspace === null ? [] : changed(fixture, workspace),
  }
}

/**
 * The board's plumbing, counted from the ledger whatever the chief decided:
 * briefs delivered to members, results delivered back to the chief, questions
 * members put to the chief and the answers delivered back, tasks accepted.
 */
function plumbing(file, chiefId, humanId) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const count = (sql, ...args) => db.prepare(sql).get(...args).n
    const fromMember = 'sender_id IS NOT NULL AND sender_id != ? AND sender_id != ?'
    return {
      briefs: count("SELECT COUNT(*) AS n FROM message WHERE kind = 'task'"),
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
      tells: count(
        `SELECT COUNT(*) AS n FROM message WHERE kind = 'question' AND urgent = 1 AND sender_id = ?`,
        chiefId,
      ),
      tellsAnswered: count(
        `SELECT COUNT(*) AS n FROM message q WHERE q.kind = 'question' AND q.urgent = 1 AND q.sender_id = ?
           AND EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')`,
        chiefId,
      ),
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
      ownerQuestions: count(
        "SELECT COUNT(*) AS n FROM message WHERE kind = 'question' AND recipient_id = ? AND sender_id = ?",
        humanId,
        chiefId,
      ),
      ownerQuestionsAnswered: count(
        `SELECT COUNT(*) AS n FROM message q WHERE q.kind = 'question' AND q.recipient_id = ? AND q.sender_id = ?
           AND EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')`,
        humanId,
        chiefId,
      ),
    }
  } finally {
    db.close()
  }
}

/**
 * Did the board's plumbing hold, whatever the chief decided: every brief
 * delivered, every result back to the chief, every member's question
 * answered and the answer delivered, every question the chief put to the
 * owner answered (a refused answer leaves the chief waiting), every task
 * shown on the board
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
    held(
      "every question the chief put to the owner got the owner's answer",
      p.ownerQuestionsAnswered,
      p.ownerQuestions,
    ),
    held('the board showed every task', boardTasks, metrics.taskCount),
  ]
}

/** Files of the fixture that differ in the workspace, and files the run added, relative. */
export function changed(fixture, workspace) {
  const digest = (path) => createHash('sha256').update(readFileSync(path)).digest('hex')
  // Reports name files with forward slashes on every platform.
  const list = (root, dir = root) =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name)
      if (name === '.git' || name === 'node_modules') return []
      return statSync(path).isDirectory()
        ? list(root, path)
        : [relative(root, path).split(sep).join('/')]
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
        `SELECT t.role, t.text FROM transcript t
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
    }
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
