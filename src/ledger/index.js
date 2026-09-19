import { DatabaseSync } from 'node:sqlite'
import { MIGRATIONS, SCHEMA_VERSION } from './schema.js'

export { SCHEMA_VERSION }

/**
 * The ledger: the one durable record of every project, participant, task and
 * inbox message, in `<home>/consensflow.db`. The board and every inbox are
 * views of it.
 *
 * Rules, each with a test in `tests/ledger.test.mjs`:
 * - The daemon is the only process that opens the file. The connection runs in
 *   SQLite's exclusive locking mode and takes the write lock when it opens, so
 *   the lock IS the instance lock: a second opener, in this process or another,
 *   is refused with `ledger-locked`, and the operating system releases the lock
 *   when the holder closes or dies. This works the same on macOS and Windows.
 * - Every operation is one transaction: it validates, writes and logs, or it
 *   throws and writes nothing. A process killed mid-write leaves the last
 *   committed state.
 * - A recipient has at most one message in delivery. Its queue is delivered
 *   oldest first. A worker, advisor or reviewer does one task at a time: a
 *   task message waits while it has another in progress (answers and notes do
 *   not). Coordinators take tasks as they come; theirs end when they say so.
 * - The human never receives through a pane: their messages stay in the inbox
 *   until they are read in the app.
 * - A member who leaves the team keeps its history but gets nothing new: its
 *   open tasks are cancelled, its undelivered messages and its unread
 *   questions too, and it is refused as a recipient (`member-left`) until it
 *   rejoins. The coordinators whose windows already run are told when the
 *   team changes; a window that has not started reads the team at launch.
 * - Task states move only along the state machine below; anything else is
 *   refused with `invalid-transition`.
 *
 *   queued ──delivered──▶ working ──question──▶ waiting ──answer delivered──▶ working
 *   working, waiting ──result──▶ done ──accept──▶ accepted
 *   done, failed ──reopen──▶ queued
 *   queued, working, waiting ──cancel──▶ cancelled, ──fail──▶ failed
 *
 * This module never reads `process.env` and never logs: the file and the
 * clock are arguments, and every refusal is a `LedgerError` with a stable code.
 */

export const HARNESSES = ['claude-code', 'codex', 'opencode', 'pi', 'devin', 'kimi']
const MEMBER_ROLES = ['worker', 'advisor', 'reviewer']
/** Who hands each kind of member its work, and hears when the team changes. */
const COORDINATOR_OF = { worker: 'lead', reviewer: 'lead', advisor: 'pm' }
const MEMBER_ROLE_NAMES = { worker: 'a worker', reviewer: 'a reviewer', advisor: 'an advisor' }
const COORDINATOR_HANDLES = ['human', 'lead', 'pm']
const ACTIVE_TASK_STATES = ['working', 'waiting']
const MAX_BODY = 1_000_000
const MAX_TITLE = 120
const AGENT_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/

export class LedgerError extends Error {
  constructor(code, message, status = 400) {
    super(message)
    this.name = 'LedgerError'
    this.code = code
    this.status = status
  }
}

/** SQLite's primary result code, without the extended bits. */
const primary = (cause) => (cause?.errcode ?? 0) & 0xff
const SQLITE_BUSY = 5
const SQLITE_LOCKED = 6
const SQLITE_CORRUPT = 11
const SQLITE_NOTADB = 26

export function openLedger(file, { now = () => new Date() } = {}) {
  const db = new DatabaseSync(file, { timeout: 0, enableForeignKeyConstraints: true })
  try {
    db.exec('PRAGMA locking_mode = EXCLUSIVE')
    // In exclusive mode the first write lock is kept until the connection
    // closes; taking it here is what makes this connection the only one.
    db.exec('BEGIN EXCLUSIVE; COMMIT')
    db.exec('PRAGMA journal_mode = WAL')
    db.exec('PRAGMA synchronous = FULL')
    migrate(db)
  } catch (cause) {
    db.close()
    if ([SQLITE_BUSY, SQLITE_LOCKED].includes(primary(cause))) {
      throw new LedgerError('ledger-locked', `another ConsensFlow has ${file} open`, 409)
    }
    if ([SQLITE_CORRUPT, SQLITE_NOTADB].includes(primary(cause))) {
      throw new LedgerError(
        'ledger-unreadable',
        `${file} is not a readable ConsensFlow ledger: ${cause.message}`,
        409,
      )
    }
    throw cause
  }
  return new Ledger(db, now)
}

function migrate(db) {
  const version = db.prepare('PRAGMA user_version').get().user_version
  if (version > SCHEMA_VERSION) {
    throw new LedgerError(
      'ledger-newer',
      `this home was written by a newer ConsensFlow (schema ${version}; this build knows ${SCHEMA_VERSION})`,
      409,
    )
  }
  for (let from = version; from < SCHEMA_VERSION; from += 1) {
    db.exec('BEGIN IMMEDIATE')
    try {
      db.exec(MIGRATIONS[from])
      db.exec(`PRAGMA user_version = ${from + 1}`)
      db.exec('COMMIT')
    } catch (cause) {
      db.exec('ROLLBACK')
      throw cause
    }
  }
}

function requireText(value, field, max) {
  if (typeof value !== 'string' || value.trim().length === 0 || value.length > max) {
    throw new LedgerError(
      'invalid-text',
      `${field} must be text, not empty, at most ${max} characters`,
    )
  }
  return value
}

function requireHarness(harness) {
  if (!HARNESSES.includes(harness)) {
    throw new LedgerError('invalid-harness', `unknown harness ${JSON.stringify(harness)}`)
  }
  return harness
}

/** A participant that is still in the project; a member who left is refused. */
function requireActive(row) {
  if (row.left_at !== null) {
    throw new LedgerError('member-left', `@${row.handle} left the team`, 409)
  }
  return row
}

function requireMember({ agent, harness, role }) {
  if (!MEMBER_ROLES.includes(role)) {
    throw new LedgerError('invalid-role', `a member is a ${MEMBER_ROLES.join(', ')}, not ${role}`)
  }
  if (typeof agent !== 'string' || !AGENT_ID.test(agent) || COORDINATOR_HANDLES.includes(agent)) {
    throw new LedgerError('invalid-agent', `not an agent id: ${JSON.stringify(agent)}`)
  }
  requireHarness(harness)
}

/** A card title: the first line that says something, shortened to fit. */
function titleOf(body) {
  const line = body
    .split('\n')
    .map((text) => text.trim())
    .find(Boolean)
  return line.length <= MAX_TITLE ? line : `${line.slice(0, MAX_TITLE - 1)}…`
}

const MESSAGE_SELECT = `
  SELECT m.*, r.handle AS recipient, r.role AS recipient_role, s.handle AS sender,
         t.number AS task_number
  FROM message m
  JOIN participant r ON r.id = m.recipient_id
  LEFT JOIN participant s ON s.id = m.sender_id
  LEFT JOIN task t ON t.id = m.task_id`

const TASK_SELECT = `
  SELECT t.*, q.handle AS requester, a.handle AS assignee
  FROM task t
  JOIN participant q ON q.id = t.requester_id
  JOIN participant a ON a.id = t.assignee_id`

const participantView = (row) => ({
  id: row.id,
  projectId: row.project_id,
  handle: row.handle,
  role: row.role,
  agent: row.agent,
  harness: row.harness,
  createdAt: row.created_at,
  leftAt: row.left_at,
})

const conversationView = (row) =>
  row === undefined
    ? null
    : {
        id: row.id,
        participantId: row.participant_id,
        harness: row.harness,
        nativeSession: row.native_session,
        startedAt: row.started_at,
        endedAt: row.ended_at,
      }

const taskView = (row) => ({
  id: row.id,
  projectId: row.project_id,
  number: row.number,
  title: row.title,
  body: row.body,
  state: row.state,
  requester: row.requester,
  assignee: row.assignee,
  createdAt: row.created_at,
  updatedAt: row.updated_at,
})

const messageView = (row) => ({
  id: row.id,
  projectId: row.project_id,
  recipient: row.recipient,
  recipientId: row.recipient_id,
  recipientRole: row.recipient_role,
  sender: row.sender,
  kind: row.kind,
  taskNumber: row.task_number,
  replyTo: row.reply_to,
  body: row.body,
  state: row.state,
  attempts: row.attempts,
  reason: row.reason,
  receipt: row.receipt === null ? null : JSON.parse(row.receipt),
  createdAt: row.created_at,
  deliveredAt: row.delivered_at,
})

class Ledger {
  #db
  #now

  constructor(db, now) {
    this.#db = db
    this.#now = now
  }

  close() {
    if (this.#db.isOpen) this.#db.close()
  }

  /** SQLite's own consistency check of the whole file: 'ok' when sound. */
  integrity() {
    return this.#db.prepare('PRAGMA integrity_check').get().integrity_check
  }

  // --- projects and participants ---------------------------------------------

  /** A project with its lead and, when given, its team (the last project's, usually). */
  createProject({ directory, name, lead, team = [] }) {
    requireText(directory, 'directory', 4096)
    requireText(name, 'name', 100)
    requireHarness(lead?.harness)
    for (const member of team) requireMember(member)
    return this.#write(() => {
      const at = this.#at()
      const { lastInsertRowid: id } = this.#db
        .prepare(
          `INSERT INTO project (directory, name, state, created_at, updated_at)
           VALUES (?, ?, 'open', ?, ?)`,
        )
        .run(directory, name, at, at)
      this.#addParticipant(id, { handle: 'human', role: 'human', agent: null, harness: null })
      this.#addParticipant(id, { handle: 'lead', role: 'lead', agent: null, harness: lead.harness })
      for (const { agent, harness, role } of team) {
        this.#addParticipant(id, { handle: agent, role, agent, harness })
      }
      this.#log(id, 'project.created', { name, directory })
      return this.project(id)
    })
  }

  project(id) {
    const row = this.#db.prepare('SELECT * FROM project WHERE id = ?').get(id)
    if (row === undefined) return null
    return {
      id: row.id,
      directory: row.directory,
      name: row.name,
      state: row.state,
      resumeOnStart: row.resume_on_start === 1,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
      participants: this.#db
        .prepare('SELECT * FROM participant WHERE project_id = ? AND left_at IS NULL ORDER BY id')
        .all(id)
        .map(participantView),
    }
  }

  projects() {
    return this.#db
      .prepare('SELECT id FROM project ORDER BY id')
      .all()
      .map((row) => this.project(row.id))
  }

  /** Open or suspend a project by hand; either way it is no longer due a resume. */
  setProjectState(id, state) {
    if (state !== 'open' && state !== 'suspended') {
      throw new LedgerError('invalid-state', `a project is open or suspended, not ${state}`)
    }
    return this.#write(() => {
      const from = this.#projectRow(id).state
      this.#db
        .prepare('UPDATE project SET state = ?, resume_on_start = 0, updated_at = ? WHERE id = ?')
        .run(state, this.#at(), id)
      if (from !== state) this.#log(id, 'project.state', { from, to: state })
      return this.project(id)
    })
  }

  /**
   * At daemon start: the panes of every open project died with the previous
   * process, so each becomes suspended and is marked to come back by itself.
   */
  suspendForRestart() {
    return this.#write(() => {
      const open = this.#db.prepare(`SELECT id FROM project WHERE state = 'open' ORDER BY id`).all()
      for (const { id } of open) {
        this.#db
          .prepare(
            `UPDATE project SET state = 'suspended', resume_on_start = 1, updated_at = ?
             WHERE id = ?`,
          )
          .run(this.#at(), id)
        this.#log(id, 'project.state', { from: 'open', to: 'suspended', resumeOnStart: true })
      }
      return open.map(({ id }) => this.project(id))
    })
  }

  /** A resume on start is tried once: after it, success or not, the mark goes. */
  forgetResume(id) {
    return this.#write(() => {
      this.#projectRow(id)
      this.#db.prepare('UPDATE project SET resume_on_start = 0 WHERE id = ?').run(id)
    })
  }

  deleteProject(id) {
    return this.#write(() => {
      this.#projectRow(id)
      this.#db.prepare('DELETE FROM project WHERE id = ?').run(id)
    })
  }

  /** A member joins the team, or rejoins it in the role and harness given now. */
  addMember(projectId, { agent, harness, role }) {
    requireMember({ agent, harness, role })
    return this.#write(() => {
      const left = this.#db
        .prepare(
          'SELECT * FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NOT NULL',
        )
        .get(projectId, agent)
      let member
      if (left === undefined) {
        member = this.#addParticipant(projectId, { handle: agent, role, agent, harness })
      } else {
        this.#db
          .prepare('UPDATE participant SET role = ?, harness = ?, left_at = NULL WHERE id = ?')
          .run(role, harness, left.id)
        this.#log(projectId, 'member.added', { handle: agent, role, harness, rejoined: true })
        member = participantView(this.#participantRow(left.id))
      }
      this.#tellIfRunning(
        projectId,
        COORDINATOR_OF[role],
        `@${agent} joined the team as ${MEMBER_ROLE_NAMES[role]}. Give it work with: cf task add @${agent} "…"`,
      )
      return member
    })
  }

  /**
   * A member leaves the team. Its open tasks are cancelled, with the messages
   * still on their way to it and its unread questions; its coordinator and
   * whoever asked for those tasks are told, if their windows run.
   */
  removeMember(projectId, handle) {
    return this.#write(() => {
      const member = this.#participantByHandle(projectId, handle)
      if (!MEMBER_ROLES.includes(member.role)) {
        throw new LedgerError(
          'not-a-member',
          `${handle} is the project's ${member.role}, not a member of its team`,
          409,
        )
      }
      const open = this.#db
        .prepare(
          `SELECT t.*, q.handle AS requester FROM task t JOIN participant q ON q.id = t.requester_id
           WHERE t.assignee_id = ? AND t.state IN ('queued', 'working', 'waiting')
           ORDER BY t.number`,
        )
        .all(member.id)
      for (const task of open) {
        this.#dropQueued(task.id)
        this.#moveTask(task, 'cancelled', { reason: `@${handle} left the team` })
      }
      this.#db
        .prepare(
          `UPDATE message SET state = 'cancelled'
           WHERE (recipient_id = ? AND state IN ('queued', 'delivering'))
              OR (sender_id = ? AND kind = 'question' AND state = 'queued')`,
        )
        .run(member.id, member.id)
      this.#db.prepare('UPDATE participant SET left_at = ? WHERE id = ?').run(this.#at(), member.id)
      const cancelled = open.map((task) => task.number)
      this.#log(projectId, 'member.left', { handle, cancelled })
      const body = `@${handle} left the team; it takes no more tasks.${
        cancelled.length === 0
          ? ''
          : ` Cancelled with it: ${cancelled.map((number) => `T-${number}`).join(', ')}.`
      }`
      for (const coordinator of new Set([
        COORDINATOR_OF[member.role],
        ...open.map((task) => task.requester),
      ])) {
        if (coordinator !== 'human') this.#tellIfRunning(projectId, coordinator, body)
      }
      return { member: participantView(this.#participantRow(member.id)), cancelled }
    })
  }

  addPm(projectId, { harness }) {
    requireHarness(harness)
    return this.#write(() =>
      this.#addParticipant(projectId, { handle: 'pm', role: 'pm', agent: null, harness }),
    )
  }

  /** The members of the newest project that has any: the team a new project starts from. */
  lastTeam() {
    return this.#db
      .prepare(
        `SELECT agent, harness, role FROM participant
         WHERE project_id = (
           SELECT MAX(project_id) FROM participant
           WHERE role IN ('worker', 'advisor', 'reviewer') AND left_at IS NULL
         ) AND role IN ('worker', 'advisor', 'reviewer') AND left_at IS NULL
         ORDER BY id`,
      )
      .all()
      .map((row) => ({ agent: row.agent, harness: row.harness, role: row.role }))
  }

  // --- conversations -----------------------------------------------------------

  /** A participant's new native conversation; the one before it ends. */
  startConversation(participantId, { harness }) {
    requireHarness(harness)
    return this.#write(() => {
      const participant = this.#participantRow(participantId)
      const at = this.#at()
      this.#db
        .prepare(
          'UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL',
        )
        .run(at, participantId)
      const { lastInsertRowid: id } = this.#db
        .prepare('INSERT INTO conversation (participant_id, harness, started_at) VALUES (?, ?, ?)')
        .run(participantId, harness, at)
      this.#log(participant.project_id, 'conversation.started', {
        participant: participant.handle,
        conversation: id,
      })
      return this.#conversation(id)
    })
  }

  bindConversation(conversationId, nativeSession) {
    requireText(nativeSession, 'native session', 512)
    return this.#write(() => {
      const conversation = this.#conversation(conversationId)
      if (conversation === null) {
        throw new LedgerError('unknown-conversation', `no conversation ${conversationId}`, 404)
      }
      const taken = this.#db
        .prepare('SELECT id FROM conversation WHERE harness = ? AND native_session = ? AND id != ?')
        .get(conversation.harness, nativeSession, conversationId)
      if (taken !== undefined) {
        throw new LedgerError(
          'native-session-taken',
          `native session ${nativeSession} already belongs to conversation ${taken.id}`,
          409,
        )
      }
      this.#db
        .prepare('UPDATE conversation SET native_session = ? WHERE id = ?')
        .run(nativeSession, conversationId)
      const participant = this.#participantRow(conversation.participantId)
      this.#log(participant.project_id, 'conversation.bound', {
        conversation: conversationId,
        nativeSession,
      })
      return this.#conversation(conversationId)
    })
  }

  endConversation(conversationId) {
    return this.#write(() => {
      const conversation = this.#conversation(conversationId)
      if (conversation === null || conversation.endedAt !== null) return conversation
      this.#db
        .prepare('UPDATE conversation SET ended_at = ? WHERE id = ?')
        .run(this.#at(), conversationId)
      const participant = this.#participantRow(conversation.participantId)
      this.#log(participant.project_id, 'conversation.ended', { conversation: conversationId })
      return this.#conversation(conversationId)
    })
  }

  currentConversation(participantId) {
    return conversationView(
      this.#db
        .prepare('SELECT * FROM conversation WHERE participant_id = ? AND ended_at IS NULL')
        .get(participantId),
    )
  }

  // --- tasks and messages ------------------------------------------------------

  /** Creates the card and queues it for its assignee, in one step. */
  createTask(projectId, { from, to, body, title }) {
    requireText(body, 'body', MAX_BODY)
    if (title !== undefined) requireText(title, 'title', MAX_TITLE)
    return this.#write(() => {
      const requester = this.#participantByHandle(projectId, from)
      const assignee = this.#participantByHandle(projectId, to)
      const { next } = this.#db
        .prepare('SELECT COALESCE(MAX(number), 0) + 1 AS next FROM task WHERE project_id = ?')
        .get(projectId)
      const at = this.#at()
      const { lastInsertRowid: taskId } = this.#db
        .prepare(
          `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state,
                             created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, 'queued', ?, ?)`,
        )
        .run(projectId, next, title ?? titleOf(body), body, requester.id, assignee.id, at, at)
      const messageId = this.#queue(projectId, {
        to: assignee.id,
        from: requester.id,
        kind: 'task',
        taskId,
        body,
      })
      this.#log(projectId, 'task.created', {
        task: next,
        from: requester.handle,
        to: assignee.handle,
        message: messageId,
      })
      return { task: this.#task(taskId), message: this.#message(messageId) }
    })
  }

  note(projectId, { from, to, body, task }) {
    return this.#send(projectId, { from, to, body, task, kind: 'note' })
  }

  /** A question for a coordinator or the human; the asker's task waits for the answer. */
  ask(projectId, { from, to, body, task }) {
    if (from === undefined) {
      throw new LedgerError('unknown-participant', 'a question names who asks it', 400)
    }
    return this.#write(() => {
      const message = this.#send(projectId, { from, to, body, task, kind: 'question' })
      if (task !== undefined) {
        const row = this.#taskRow(projectId, task)
        const asker = this.#participantByHandle(projectId, from)
        if (row.assignee_id === asker.id && row.state === 'working') {
          this.#moveTask(row, 'waiting')
        }
      }
      return message
    })
  }

  /** The answer goes back to whoever asked; only the one asked may answer. */
  answer(questionId, { from, body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const question = this.#message(questionId)
      if (question === null || question.kind !== 'question') {
        throw new LedgerError('not-a-question', `message ${questionId} is not a question`, 409)
      }
      if (question.recipient !== from) {
        throw new LedgerError(
          'not-your-question',
          `the question was put to ${question.recipient}, not ${from}`,
          403,
        )
      }
      const answerer = this.#participantByHandle(question.projectId, from)
      const asker = this.#db
        .prepare('SELECT sender_id, task_id FROM message WHERE id = ?')
        .get(questionId)
      requireActive(this.#participantRow(asker.sender_id))
      const id = this.#queue(question.projectId, {
        to: asker.sender_id,
        from: answerer.id,
        kind: 'answer',
        taskId: asker.task_id,
        replyTo: questionId,
        body,
      })
      this.#log(question.projectId, 'message.sent', {
        message: id,
        kind: 'answer',
        from,
        to: question.sender,
      })
      return this.#message(id)
    })
  }

  /** The head of a participant's queue that may go now, or null. */
  nextDelivery(participantId) {
    const participant = this.#participantRow(participantId)
    if (participant.role === 'human') return null
    const busy = this.#db
      .prepare(`SELECT 1 FROM message WHERE recipient_id = ? AND state = 'delivering'`)
      .get(participantId)
    if (busy !== undefined) return null
    const serial = MEMBER_ROLES.includes(participant.role)
    const row = this.#db
      .prepare(
        `SELECT id FROM message
         WHERE recipient_id = ? AND state = 'queued'
           AND (kind != 'task' OR ? = 0 OR NOT EXISTS (
             SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')
           ))
         ORDER BY id LIMIT 1`,
      )
      .get(participantId, serial ? 1 : 0, participantId)
    return row === undefined ? null : this.#message(row.id)
  }

  beginDelivery(messageId) {
    return this.#write(() => {
      const message = this.#requireMessage(messageId, 'queued')
      if (message.recipientRole === 'human') {
        throw new LedgerError('human-reads-in-app', 'the human reads messages in the app', 409)
      }
      const busy = this.#db
        .prepare(`SELECT id FROM message WHERE recipient_id = ? AND state = 'delivering'`)
        .get(message.recipientId)
      const working =
        message.kind === 'task' &&
        MEMBER_ROLES.includes(message.recipientRole) &&
        this.#db
          .prepare(`SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')`)
          .get(message.recipientId) !== undefined
      if (busy !== undefined || working) {
        throw new LedgerError(
          'recipient-busy',
          `${message.recipient} is still receiving or working; message ${messageId} waits`,
          409,
        )
      }
      this.#db
        .prepare(
          `UPDATE message SET state = 'delivering', attempts = attempts + 1, reason = NULL
           WHERE id = ?`,
        )
        .run(messageId)
      this.#log(message.projectId, 'delivery.begun', {
        message: messageId,
        attempt: message.attempts + 1,
      })
      return this.#message(messageId)
    })
  }

  /** The harness's own record proves the message arrived. */
  confirmDelivery(messageId, receipt) {
    return this.#write(() => {
      const message = this.#requireMessage(messageId, 'delivering')
      this.#db
        .prepare(
          `UPDATE message SET state = 'delivered', delivered_at = ?, receipt = ? WHERE id = ?`,
        )
        .run(this.#at(), JSON.stringify(receipt ?? null), messageId)
      this.#log(message.projectId, 'delivery.confirmed', { message: messageId })
      const task = this.#messageTask(messageId)
      if (task !== undefined) {
        if (message.kind === 'task' && task.state === 'queued') this.#moveTask(task, 'working')
        if (message.kind === 'answer' && task.state === 'waiting') this.#moveTask(task, 'working')
      }
      return this.#message(messageId)
    })
  }

  retryDelivery(messageId, reason) {
    requireText(reason, 'reason', 1000)
    return this.#write(() => {
      const message = this.#requireMessage(messageId, 'delivering')
      this.#db
        .prepare(`UPDATE message SET state = 'queued', reason = ? WHERE id = ?`)
        .run(reason, messageId)
      this.#log(message.projectId, 'delivery.retried', { message: messageId, reason })
      return this.#message(messageId)
    })
  }

  failDelivery(messageId, reason) {
    requireText(reason, 'reason', 1000)
    return this.#write(() => {
      const message = this.#requireMessage(messageId, 'delivering')
      this.#db
        .prepare(`UPDATE message SET state = 'failed', reason = ? WHERE id = ?`)
        .run(reason, messageId)
      this.#log(message.projectId, 'delivery.failed', { message: messageId, reason })
      const task = this.#messageTask(messageId)
      if (message.kind === 'task' && task !== undefined && task.state === 'queued') {
        this.#moveTask(task, 'failed')
      }
      return this.#message(messageId)
    })
  }

  /** The human opened a message in the app; reading a task takes it on. */
  markRead(messageId) {
    return this.#write(() => {
      const message = this.#message(messageId)
      if (message === null) throw new LedgerError('unknown-message', `no message ${messageId}`, 404)
      if (message.recipientRole !== 'human') {
        throw new LedgerError(
          'not-for-the-human',
          `message ${messageId} is delivered to a pane`,
          409,
        )
      }
      if (message.state === 'read') return message
      this.#requireMessage(messageId, 'queued')
      this.#db
        .prepare(`UPDATE message SET state = 'read', delivered_at = ? WHERE id = ?`)
        .run(this.#at(), messageId)
      this.#log(message.projectId, 'message.read', { message: messageId })
      const task = this.#messageTask(messageId)
      if (message.kind === 'task' && task?.state === 'queued') this.#moveTask(task, 'working')
      return this.#message(messageId)
    })
  }

  /** The assignee's answer finishes the task and is queued for whoever asked for it. */
  recordResult(projectId, number, { body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ACTIVE_TASK_STATES, 'record a result for')
      const messageId = this.#queue(projectId, {
        to: task.requester_id,
        from: task.assignee_id,
        kind: 'result',
        taskId: task.id,
        body,
      })
      this.#moveTask(task, 'done', { result: messageId })
      return { task: this.#task(task.id), message: this.#message(messageId) }
    })
  }

  acceptTask(projectId, number, { by }) {
    return this.#write(() => {
      this.#participantByHandle(projectId, by)
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['done'], 'accept')
      this.#moveTask(task, 'accepted', { by })
      return this.#task(task.id)
    })
  }

  /** A follow-up on a finished or failed task: it goes back to its assignee's queue. */
  reopenTask(projectId, number, { by, body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const author = this.#participantByHandle(projectId, by)
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['done', 'failed'], 'reopen')
      requireActive(this.#participantRow(task.assignee_id))
      const messageId = this.#queue(projectId, {
        to: task.assignee_id,
        from: author.id,
        kind: 'task',
        taskId: task.id,
        body,
      })
      this.#moveTask(task, 'queued', { by, message: messageId })
      return { task: this.#task(task.id), message: this.#message(messageId) }
    })
  }

  cancelTask(projectId, number, { by }) {
    return this.#write(() => {
      this.#participantByHandle(projectId, by)
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['queued', ...ACTIVE_TASK_STATES], 'cancel')
      this.#dropQueued(task.id)
      this.#moveTask(task, 'cancelled', { by })
      return this.#task(task.id)
    })
  }

  /** The daemon gives up on a task: its pane died, or its launch never came up. */
  failTask(projectId, number, { reason }) {
    requireText(reason, 'reason', 1000)
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['queued', ...ACTIVE_TASK_STATES], 'fail')
      this.#dropQueued(task.id)
      this.#moveTask(task, 'failed', { reason })
      return this.#task(task.id)
    })
  }

  // --- views -------------------------------------------------------------------

  board(projectId) {
    const project = this.project(projectId)
    if (project === null) throw new LedgerError('unknown-project', `no project ${projectId}`, 404)
    const tasks = this.#db
      .prepare(`${TASK_SELECT} WHERE t.project_id = ? ORDER BY t.number`)
      .all(projectId)
      .map(taskView)
    return {
      project,
      lanes: project.participants.map((participant) => ({
        participant,
        tasks: tasks.filter((task) => task.assignee === participant.handle),
      })),
    }
  }

  /** A task and its whole thread, oldest first; null when there is no such task. */
  task(projectId, number) {
    const row = this.#db
      .prepare(`${TASK_SELECT} WHERE t.project_id = ? AND t.number = ?`)
      .get(projectId, number)
    if (row === undefined) return null
    return {
      ...taskView(row),
      messages: this.#db
        .prepare(`${MESSAGE_SELECT} WHERE m.task_id = ? ORDER BY m.id`)
        .all(row.id)
        .map(messageView),
    }
  }

  /** One message, or null. */
  message(id) {
    return this.#message(id)
  }

  /** The task a participant has in progress (working or waiting on an answer), or null. */
  activeTask(participantId) {
    const row = this.#db
      .prepare(
        `SELECT project_id, number FROM task
         WHERE assignee_id = ? AND state IN ('working', 'waiting') ORDER BY id LIMIT 1`,
      )
      .get(participantId)
    return row === undefined ? null : this.task(row.project_id, row.number)
  }

  inbox(participantId, { limit = 100 } = {}) {
    return this.#db
      .prepare(`${MESSAGE_SELECT} WHERE m.recipient_id = ? ORDER BY m.id DESC LIMIT ?`)
      .all(participantId, limit)
      .map(messageView)
  }

  events(projectId, { after = 0, limit = 500 } = {}) {
    return this.#db
      .prepare('SELECT * FROM event WHERE project_id = ? AND id > ? ORDER BY id LIMIT ?')
      .all(projectId, after, limit)
      .map((row) => ({
        id: row.id,
        projectId: row.project_id,
        at: row.at,
        kind: row.kind,
        data: JSON.parse(row.data),
      }))
  }

  // --- internals ---------------------------------------------------------------

  /** One transaction around `work`; an operation called inside another joins it. */
  #write(work) {
    if (this.#db.isTransaction) return work()
    this.#db.exec('BEGIN IMMEDIATE')
    try {
      const result = work()
      this.#db.exec('COMMIT')
      return result
    } catch (cause) {
      this.#db.exec('ROLLBACK')
      throw cause
    }
  }

  #at() {
    return this.#now().toISOString()
  }

  #log(projectId, kind, data) {
    this.#db
      .prepare('INSERT INTO event (project_id, at, kind, data) VALUES (?, ?, ?, ?)')
      .run(projectId, this.#at(), kind, JSON.stringify(data))
  }

  #projectRow(id) {
    const row = this.#db.prepare('SELECT * FROM project WHERE id = ?').get(id)
    if (row === undefined) throw new LedgerError('unknown-project', `no project ${id}`, 404)
    return row
  }

  #participantRow(id) {
    const row = this.#db.prepare('SELECT * FROM participant WHERE id = ?').get(id)
    if (row === undefined) throw new LedgerError('unknown-participant', `no participant ${id}`, 404)
    return row
  }

  #participantByHandle(projectId, handle) {
    this.#projectRow(projectId)
    const row = this.#db
      .prepare('SELECT * FROM participant WHERE project_id = ? AND handle = ?')
      .get(projectId, handle)
    if (row === undefined) {
      throw new LedgerError(
        'unknown-participant',
        `${JSON.stringify(handle)} is not in project ${projectId}`,
        404,
      )
    }
    return requireActive(row)
  }

  #addParticipant(projectId, { handle, role, agent, harness }) {
    this.#projectRow(projectId)
    const taken = this.#db
      .prepare('SELECT 1 FROM participant WHERE project_id = ? AND handle = ?')
      .get(projectId, handle)
    if (taken !== undefined) {
      throw new LedgerError('member-exists', `${handle} is already in project ${projectId}`, 409)
    }
    const { lastInsertRowid: id } = this.#db
      .prepare(
        `INSERT INTO participant (project_id, handle, role, agent, harness, created_at)
         VALUES (?, ?, ?, ?, ?, ?)`,
      )
      .run(projectId, handle, role, agent, harness, this.#at())
    if (role !== 'human' && role !== 'lead') {
      this.#log(projectId, 'member.added', { handle, role, harness })
    }
    return participantView(this.#participantRow(id))
  }

  #conversation(id) {
    return conversationView(this.#db.prepare('SELECT * FROM conversation WHERE id = ?').get(id))
  }

  /** A note from ConsensFlow, for a participant whose window has already started. */
  #tellIfRunning(projectId, handle, body) {
    const row = this.#db
      .prepare('SELECT id FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NULL')
      .get(projectId, handle)
    if (row === undefined || this.currentConversation(row.id) === null) return
    this.#send(projectId, { to: handle, body, kind: 'note' })
  }

  #queue(projectId, { to, from, kind, taskId = null, replyTo = null, body }) {
    const { lastInsertRowid: id } = this.#db
      .prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, reply_to, body,
                              state, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'queued', ?)`,
      )
      .run(projectId, to, from, kind, taskId, replyTo, body, this.#at())
    return id
  }

  #send(projectId, { from, to, body, task, kind }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const sender = from === undefined ? null : this.#participantByHandle(projectId, from)
      const recipient = this.#participantByHandle(projectId, to)
      const taskId = task === undefined ? null : this.#taskRow(projectId, task).id
      const id = this.#queue(projectId, {
        to: recipient.id,
        from: sender?.id ?? null,
        kind,
        taskId,
        body,
      })
      this.#log(projectId, 'message.sent', {
        message: id,
        kind,
        from: sender?.handle ?? null,
        to: recipient.handle,
      })
      return this.#message(id)
    })
  }

  #message(id) {
    const row = this.#db.prepare(`${MESSAGE_SELECT} WHERE m.id = ?`).get(id)
    return row === undefined ? null : messageView(row)
  }

  #requireMessage(id, state) {
    const message = this.#message(id)
    if (message === null) throw new LedgerError('unknown-message', `no message ${id}`, 404)
    if (message.state !== state) {
      throw new LedgerError(
        'invalid-transition',
        `message ${id} is ${message.state}, not ${state}`,
        409,
      )
    }
    return message
  }

  #messageTask(messageId) {
    return this.#db
      .prepare('SELECT t.* FROM task t JOIN message m ON m.task_id = t.id WHERE m.id = ?')
      .get(messageId)
  }

  #task(id) {
    return taskView(this.#db.prepare(`${TASK_SELECT} WHERE t.id = ?`).get(id))
  }

  #taskRow(projectId, number) {
    const row = this.#db
      .prepare('SELECT * FROM task WHERE project_id = ? AND number = ?')
      .get(projectId, number)
    if (row === undefined) {
      throw new LedgerError('unknown-task', `no task T-${number} in project ${projectId}`, 404)
    }
    return row
  }

  #requireTaskState(task, states, action) {
    if (!states.includes(task.state)) {
      throw new LedgerError(
        'invalid-transition',
        `cannot ${action} T-${task.number}: it is ${task.state}`,
        409,
      )
    }
  }

  #moveTask(task, to, detail = {}) {
    this.#db
      .prepare('UPDATE task SET state = ?, updated_at = ? WHERE id = ?')
      .run(to, this.#at(), task.id)
    this.#log(task.project_id, 'task.state', { task: task.number, from: task.state, to, ...detail })
  }

  /** A cancelled or failed task's queued messages are never delivered. */
  #dropQueued(taskId) {
    this.#db
      .prepare(`UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state = 'queued'`)
      .run(taskId)
  }
}
