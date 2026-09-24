import { DatabaseSync } from 'node:sqlite'
import { sessionName } from './names.js'
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
 * - A task for a tier of member, not for a member, starts open with no
 *   assignee: the daemon assigns it to a candidate (an active member of that
 *   pool and tier) and only then is it queued. A member that runs out of quota
 *   loses its task back to open, with a warning for the next member and a note
 *   to the requester. Only coordinators and the human create tasks.
 * - A member who leaves the team keeps its history but gets nothing new: its
 *   open tasks are cancelled, its undelivered messages and its unread
 *   questions too, and it is refused as a recipient (`member-left`) until it
 *   rejoins. The coordinators whose windows already run are told when the
 *   team changes; a window that has not started reads the team at launch.
 * - Task states move only along the state machine below; anything else is
 *   refused with `invalid-transition`.
 *
 *   open ──assigned──▶ queued ──delivered──▶ working ──question──▶ waiting ──answer delivered──▶ working
 *   queued, working, waiting ──released──▶ open
 *   open, queued, working, waiting ──pause──▶ paused ──resume──▶ open, queued
 *   working, waiting ──result──▶ done ──accept──▶ accepted
 *   done, failed ──reopen──▶ queued
 *   open, queued, working, waiting, paused ──cancel──▶ cancelled, ──fail──▶ failed
 *
 * - A review is a task like any other: the lead puts it on the board for a
 *   reviewer of a tier, and the reviewer's findings come back as its result.
 *
 * This module never reads `process.env` and never logs: the file and the
 * clock are arguments, and every refusal is a `LedgerError` with a stable code.
 */

export const HARNESSES = ['claude-code', 'codex', 'opencode', 'pi', 'devin', 'kimi', 'image']
const MEMBER_ROLES = ['worker', 'advisor', 'reviewer', 'designer']
/** Who hands out work and hears when the team changes: the human and the lead. */
const COORDINATOR_HANDLES = ['human', 'lead']
const COORDINATOR_ROLES = ['human', 'lead']
export const TIERS = ['critical', 'complex', 'standard', 'light']
/** Who takes a task on the board: a worker, an advisor (advice), a reviewer, or an image designer (no tier). */
const POOLS = ['worker', 'advisor', 'reviewer', 'designer']
export const PURPOSES = ['critical-review', 'architecture', 'hard-problem', 'important-question']
/** "standard worker", "image designer": who an open task waits for; `aPool` adds the article. */
const poolName = (pool, tier) => (pool === 'designer' ? 'image designer' : `${tier} ${pool}`)
const aPool = (pool, tier) => `${pool === 'designer' ? 'an' : 'a'} ${poolName(pool, tier)}`
const CRITICAL_RULE =
  'No coding or implementation edits. Do not write or revise specifications. Return analysis, evidence and recommendations to your coordinator.'
const ACTIVE_TASK_STATES = ['working', 'waiting']
/** A task on a member's hands: from assignment until its result. */
const HELD_TASK_STATES = ['queued', 'working', 'waiting']
const HOLDS_WORK = `SELECT 1 FROM task WHERE assignee_id = ? AND state IN (${HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')})`
const MAX_BODY = 1_000_000
/** How long a coordinator may leave a question before the human sees it too. */
export const OVERDUE_MS = 10 * 60_000
/** The most of one transcript item that is copied: a tool's output can run to megabytes. */
export const TRANSCRIPT_ITEM_MAX = 64_000
/** How many items of a transcript the board reads at once, from the end. */
export const TRANSCRIPT_PAGE = 300
const TRANSCRIPT_ROLES = ['user', 'assistant', 'tool', 'custom']
const MAX_QUESTIONS = 4
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

export function openLedger(
  file,
  { now = () => new Date(), names = sessionName, trace = () => {} } = {},
) {
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
  return new Ledger(db, now, names, trace)
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

/** Task numbers a task needs or comes before: a list of distinct positive integers. */
const requireNumbers = (value, field) => {
  if (!Array.isArray(value) || value.some((number) => !Number.isInteger(number) || number <= 0)) {
    throw new LedgerError('invalid-needs', `${field} is a list of task numbers (T-3, T-4)`)
  }
  return [...new Set(value)]
}

const requireGate = (gate) => {
  if (typeof gate !== 'boolean') {
    throw new LedgerError('invalid-gate', 'human approval is required (true) or not (false)')
  }
}

function requireTier(tier) {
  if (!TIERS.includes(tier)) {
    throw new LedgerError(
      'invalid-tier',
      `a tier is ${TIERS.join(', ')}, not ${JSON.stringify(tier)}`,
    )
  }
  return tier
}

/** A member's roles, one or more of worker, advisor, reviewer and designer; the first one leads. */
function requireRoles(roles) {
  if (
    !Array.isArray(roles) ||
    roles.length === 0 ||
    !roles.every((r) => MEMBER_ROLES.includes(r))
  ) {
    throw new LedgerError(
      'invalid-role',
      `a member is one or more of ${MEMBER_ROLES.join(', ')}, not ${JSON.stringify(roles)}`,
    )
  }
  return [...new Set(roles)]
}

/** Validates a member and returns its roles, normalized. */
function requireMember({ agent, harness, role, roles, tier }) {
  const set = requireRoles(roles ?? (role === undefined ? [] : [role]))
  if (typeof agent !== 'string' || !AGENT_ID.test(agent) || COORDINATOR_HANDLES.includes(agent)) {
    throw new LedgerError('invalid-agent', `not an agent id: ${JSON.stringify(agent)}`)
  }
  requireHarness(harness)
  requireTier(tier)
  return set
}

/** How a task reads when it is handed over: critical work leads with its purpose. */
const deliveryBody = (task) =>
  task.purpose === null
    ? task.body
    : `Critical work: ${task.purpose}. ${CRITICAL_RULE}\n\n${task.body}`

/** A result as its card shows it: the first line that says something, or null. */
const firstLine = (body) =>
  body === undefined
    ? null
    : (body
        .split('\n')
        .map((line) => line.trim())
        .find(Boolean) ?? null)

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
  SELECT t.*, q.handle AS requester, a.handle AS assignee,
         am.handle AS assignee_member, a.left_at AS assignee_left_at,
         (SELECT json_group_array(json_object('number', d.number, 'state', d.state))
            FROM (SELECT d.number, d.state FROM task_need n JOIN task d ON d.id = n.needs_id
                  WHERE n.task_id = t.id ORDER BY d.number) d) AS needs
  FROM task t
  JOIN participant q ON q.id = t.requester_id
  LEFT JOIN participant a ON a.id = t.assignee_id
  LEFT JOIN participant am ON am.id = a.member_id`

/** A participant with its member's handle, when it is a member's session. */
const PARTICIPANT_SELECT = `
  SELECT p.*, m.handle AS member_handle
  FROM participant p
  LEFT JOIN participant m ON m.id = p.member_id`

const participantView = (row) => ({
  id: row.id,
  projectId: row.project_id,
  handle: row.handle,
  role: row.role,
  agent: row.agent,
  harness: row.harness,
  createdAt: row.created_at,
  leftAt: row.left_at,
  tier: row.tier,
  roles: JSON.parse(row.roles),
  outUntil: row.out_until,
  outSince: row.out_since,
  memberId: row.member_id ?? null,
  member: row.member_handle ?? null,
  session: row.member_handle ? row.handle.slice(row.member_handle.length + 1) : null,
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

/** The tasks a task needs, each with its state, and the ones not yet accepted: what blocks it. */
const needsView = (json) => {
  const needs = JSON.parse(json)
  return {
    needs,
    blockedBy: needs.filter((need) => need.state !== 'accepted').map((need) => need.number),
  }
}

const taskView = (row) => ({
  id: row.id,
  projectId: row.project_id,
  number: row.number,
  title: row.title,
  body: row.body,
  state: row.state,
  requester: row.requester,
  assignee: row.assignee ?? null,
  pool: row.pool,
  tier: row.tier,
  purpose: row.purpose,
  session: row.assignee_member ? row.assignee : null,
  ...needsView(row.needs),
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
  questions: row.questions === null ? null : JSON.parse(row.questions),
  choices: row.choices === null ? null : JSON.parse(row.choices),
  createdAt: row.created_at,
  deliveredAt: row.delivered_at,
})

const badQuestions = (why) => new LedgerError('bad-questions', `questions: ${why}`, 400)
const shortText = (value, field) => {
  if (typeof value !== 'string' || value.trim().length === 0 || value.length > MAX_TITLE * 10) {
    throw badQuestions(`${field} is a short text`)
  }
  return value.trim()
}

/**
 * Questions with options as a harness's question tool asks them: one to four,
 * each with its text, a short header, its options (a label, maybe a
 * description) and whether several may be picked.
 */
function requireQuestions(questions) {
  if (!Array.isArray(questions) || questions.length === 0 || questions.length > MAX_QUESTIONS) {
    throw badQuestions(`one to ${MAX_QUESTIONS} questions`)
  }
  return questions.map((question) => {
    if (question === null || typeof question !== 'object' || !Array.isArray(question.options)) {
      throw badQuestions('each question is an object with an options array')
    }
    return {
      question: shortText(question.question, 'question'),
      header: shortText(question.header, 'header'),
      options: question.options.map((option) => ({
        label: shortText(option?.label, 'an option label'),
        description:
          typeof option.description === 'string' && option.description.trim().length > 0
            ? option.description.trim()
            : null,
      })),
      multiple: question.multiple === true,
    }
  })
}

/** A question with options as text: what an inbox or a window shows. */
const renderQuestions = (questions) =>
  questions
    .map((q) =>
      [
        `${q.header}: ${q.question}`,
        ...q.options.map(
          (o) => `- ${o.label}${o.description === null ? '' : `: ${o.description}`}`,
        ),
      ].join('\n'),
    )
    .join('\n\n')

const badChoices = (why) => new LedgerError('bad-choices', `answer: ${why}`, 400)

/**
 * The choices for a question with options: one array of picks per question,
 * from explicit `choices` or from text, one line per question, the labels
 * matched regardless of case and free text kept as it is.
 */
function requireChoices(questions, { choices, body }) {
  const picks =
    choices !== undefined
      ? choices
      : String(body ?? '')
          .split('\n')
          .map((line) => line.trim())
          .filter(Boolean)
          .map((line, at) => (questions[at]?.multiple ? line.split(',') : [line]))
  if (!Array.isArray(picks) || picks.length !== questions.length) {
    throw badChoices(`one answer per question (${questions.length})`)
  }
  return picks.map((pick, at) => {
    const question = questions[at]
    if (!Array.isArray(pick) || pick.length === 0 || (!question.multiple && pick.length > 1)) {
      throw badChoices(
        `${question.header}: ${question.multiple ? 'one or more picks' : 'one pick'}`,
      )
    }
    return pick.map((text) => {
      const wanted = String(text).trim()
      if (wanted.length === 0 || wanted.length > MAX_TITLE * 10) throw badChoices('empty pick')
      const label = question.options.find((o) => o.label.toLowerCase() === wanted.toLowerCase())
      return label === undefined ? wanted : label.label
    })
  })
}

const renderChoices = (questions, choices) =>
  questions.map((q, at) => `${q.header}: ${choices[at].join(', ')}`).join('\n')

class Ledger {
  #db
  #now
  #names
  /** Told every event as it is logged: `{at, project, kind, data}`. */
  #trace

  constructor(db, now, names, trace) {
    this.#db = db
    this.#now = now
    this.#names = names
    this.#trace = trace
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
  createProject({ directory, name, lead, team = [], gate = false }) {
    requireText(directory, 'directory', 4096)
    requireText(name, 'name', 100)
    requireHarness(lead?.harness)
    requireGate(gate)
    const members = team.map((member) => ({ ...member, roles: requireMember(member) }))
    return this.#write(() => {
      const at = this.#at()
      const { lastInsertRowid: id } = this.#db
        .prepare(
          `INSERT INTO project (directory, name, state, gate, created_at, updated_at)
           VALUES (?, ?, 'open', ?, ?, ?)`,
        )
        .run(directory, name, gate ? 1 : 0, at, at)
      this.#addParticipant(id, { handle: 'human', role: 'human', agent: null, harness: null })
      this.#addParticipant(id, { handle: 'lead', role: 'lead', agent: null, harness: lead.harness })
      for (const { agent, harness, roles, tier } of members) {
        this.#addParticipant(id, { handle: agent, roles, agent, harness, tier })
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
      gate: row.gate === 1,
      resumeOnStart: row.resume_on_start === 1,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
      participants: this.#db
        .prepare(`${PARTICIPANT_SELECT} WHERE p.project_id = ? AND p.left_at IS NULL ORDER BY p.id`)
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
   * A closed project goes for good: its participants, conversations, tasks,
   * messages and events with it (the schema cascades). An open one is
   * refused: close it first, so nothing runs while its record disappears.
   */
  deleteProject(id) {
    return this.#write(() => {
      const row = this.#projectRow(id)
      if (row.state !== 'suspended') {
        throw new LedgerError('project-open', `${row.name} is open: close it first`, 409)
      }
      const count = (sql) => this.#db.prepare(sql).get(id).n
      // What goes, for the one line the trace keeps.
      const gone = {
        id: row.id,
        name: row.name,
        directory: row.directory,
        createdAt: row.created_at,
        members: count(
          `SELECT COUNT(*) AS n FROM participant
           WHERE project_id = ? AND agent IS NOT NULL AND member_id IS NULL AND left_at IS NULL`,
        ),
        sessions: count(
          'SELECT COUNT(*) AS n FROM participant WHERE project_id = ? AND member_id IS NOT NULL',
        ),
        tasks: count('SELECT COUNT(*) AS n FROM task WHERE project_id = ?'),
        messages: count('SELECT COUNT(*) AS n FROM message WHERE project_id = ?'),
      }
      this.#db.prepare('DELETE FROM project WHERE id = ?').run(id)
      return gone
    })
  }

  /**
   * Human approval required: with the gate on, every message between two
   * agents waits for the human, who passes it on or declines it. What is
   * already gated stays so when the gate goes off; the human decides it.
   */
  setGate(id, gate) {
    requireGate(gate)
    return this.#write(() => {
      const from = this.#projectRow(id).gate === 1
      this.#db
        .prepare('UPDATE project SET gate = ?, updated_at = ? WHERE id = ?')
        .run(gate ? 1 : 0, this.#at(), id)
      if (from !== gate) this.#log(id, 'project.gate', { from, to: gate })
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

  /** A member joins the team, or rejoins it in the roles, harness and tier given now. */
  addMember(projectId, { agent, harness, role, roles, tier }) {
    roles = requireMember({ agent, harness, role, roles, tier })
    return this.#write(() => {
      const left = this.#db
        .prepare(
          'SELECT * FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NOT NULL',
        )
        .get(projectId, agent)
      if (left === undefined) {
        return this.#addParticipant(projectId, { handle: agent, roles, agent, harness, tier })
      }
      this.#db
        .prepare(
          `UPDATE participant SET role = ?, roles = ?, harness = ?, tier = ?, left_at = NULL
           WHERE id = ?`,
        )
        .run(roles[0], JSON.stringify(roles), harness, tier, left.id)
      this.#log(projectId, 'member.added', { handle: agent, roles, harness, rejoined: true })
      return participantView(this.#participantRow(left.id))
    })
  }

  /**
   * Members follow the roster: each active member's tier (and its sessions')
   * becomes what its saved agent has now, since the app's catalog may have
   * moved the model. Says which members changed; an agent the roster no
   * longer has leaves its member as it is.
   */
  refreshMemberTiers(tierOf) {
    return this.#write(() => {
      const members = this.#db
        .prepare(
          `SELECT id, project_id, handle, agent, tier FROM participant
           WHERE agent IS NOT NULL AND member_id IS NULL AND left_at IS NULL ORDER BY id`,
        )
        .all()
      const changed = []
      for (const member of members) {
        const tier = tierOf(member.agent) ?? null
        if (tier === null || tier === member.tier) continue
        this.#db
          .prepare('UPDATE participant SET tier = ? WHERE id = ? OR member_id = ?')
          .run(tier, member.id, member.id)
        this.#log(member.project_id, 'member.tier', {
          handle: member.handle,
          from: member.tier,
          to: tier,
        })
        changed.push({
          project: member.project_id,
          handle: member.handle,
          from: member.tier,
          to: tier,
        })
      }
      return changed
    })
  }

  /** A member's roles change in place. */
  setRoles(projectId, handle, roles) {
    roles = requireRoles(roles)
    return this.#write(() => {
      const member = this.#participantByHandle(projectId, handle)
      this.#requireMemberRow(member.id, 'changes roles')
      if (!MEMBER_ROLES.includes(member.role)) {
        throw new LedgerError(
          'not-a-member',
          `${handle} is the project's ${member.role}, not a member of its team`,
          409,
        )
      }
      this.#db
        .prepare('UPDATE participant SET role = ?, roles = ? WHERE id = ?')
        .run(roles[0], JSON.stringify(roles), member.id)
      this.#log(projectId, 'member.roles', { handle, roles })
      return participantView(this.#participantRow(member.id))
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
      this.#requireMemberRow(member.id, 'leaves the team')
      if (!MEMBER_ROLES.includes(member.role)) {
        throw new LedgerError(
          'not-a-member',
          `${handle} is the project's ${member.role}, not a member of its team`,
          409,
        )
      }
      const sessions = this.#db
        .prepare(`${PARTICIPANT_SELECT} WHERE p.member_id = ? AND p.left_at IS NULL ORDER BY p.id`)
        .all(member.id)
      const windows = [member, ...sessions]
      const open = this.#db
        .prepare(
          `SELECT t.*, q.handle AS requester FROM task t JOIN participant q ON q.id = t.requester_id
           WHERE t.assignee_id IN (${windows.map(() => '?').join(', ')})
             AND t.state IN ('queued', 'working', 'waiting')
           ORDER BY t.number`,
        )
        .all(...windows.map((row) => row.id))
      for (const task of open) {
        this.#dropQueued(task.id)
        this.#moveTask(task, 'cancelled', { reason: `@${handle} left the team` })
      }
      for (const row of windows) {
        this.#db
          .prepare(
            `UPDATE message SET state = 'cancelled'
             WHERE (recipient_id = ? AND state IN ('queued', 'delivering', 'gated'))
                OR (sender_id = ? AND kind = 'question' AND state IN ('queued', 'gated'))`,
          )
          .run(row.id, row.id)
      }
      for (const session of sessions) this.#endSession(session, `@${handle} left the team`)
      this.#db.prepare('UPDATE participant SET left_at = ? WHERE id = ?').run(this.#at(), member.id)
      const cancelled = open.map((task) => task.number)
      this.#log(projectId, 'member.left', { handle, cancelled })
      // Only work that went with it is worth a word, and only to whoever asked for it.
      if (cancelled.length > 0) {
        const body = `@${handle} left the team; it takes no more tasks. Cancelled with it: ${cancelled.map((number) => `T-${number}`).join(', ')}.`
        for (const requester of new Set(open.map((task) => task.requester))) {
          if (requester !== 'human') this.#tellIfRunning(projectId, requester, body)
        }
      }
      return { member: participantView(this.#participantRow(member.id)), cancelled }
    })
  }

  /** The members of the newest project that has any: the team a new project starts from. */
  lastTeam() {
    return this.#db
      .prepare(
        `SELECT agent, harness, role, roles FROM participant
         WHERE project_id = (
           SELECT MAX(project_id) FROM participant
           WHERE role IN ('worker', 'advisor', 'reviewer') AND left_at IS NULL AND member_id IS NULL
         ) AND role IN ('worker', 'advisor', 'reviewer') AND left_at IS NULL AND member_id IS NULL
         ORDER BY id`,
      )
      .all()
      .map((row) => ({
        agent: row.agent,
        harness: row.harness,
        role: row.role,
        roles: JSON.parse(row.roles),
      }))
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

  /**
   * The copy of a window's conversation, one row per item as the harness's
   * own record has them: what is new is added, and an item still being
   * written is brought up to date. `from` is the position of the first item
   * given, so a caller may pass only the tail.
   */
  copyTranscript(conversationId, items, { from = 0 } = {}) {
    if (!Array.isArray(items)) throw new LedgerError('invalid-items', 'items is a list')
    return this.#write(() => {
      if (this.#conversation(conversationId) === null) {
        throw new LedgerError('unknown-conversation', `no conversation ${conversationId}`, 404)
      }
      const upsert = this.#db.prepare(
        `INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (conversation_id, item_id) DO UPDATE
           SET seq = excluded.seq, text = excluded.text, complete = excluded.complete,
               at = excluded.at, copied_at = excluded.copied_at
           WHERE transcript.text != excluded.text OR transcript.complete != excluded.complete`,
      )
      const at = this.#at()
      let written = 0
      for (const [index, item] of items.entries()) {
        if (typeof item?.id !== 'string' || item.id.length === 0) continue
        const text = typeof item.text === 'string' ? item.text : ''
        const { changes } = upsert.run(
          conversationId,
          item.id,
          from + index,
          TRANSCRIPT_ROLES.includes(item.role) ? item.role : 'custom',
          text.length > TRANSCRIPT_ITEM_MAX
            ? `${text.slice(0, TRANSCRIPT_ITEM_MAX)}\n… (${text.length} characters; cut here)`
            : text,
          item.complete === false ? 0 : 1,
          typeof item.at === 'string' && !Number.isNaN(Date.parse(item.at)) ? item.at : null,
          at,
        )
        written += changes
      }
      return written
    })
  }

  /**
   * What the window that has a task wrote, for the board: the copied items
   * of its assignee's conversations in order, the last `limit` of them.
   */
  transcript(projectId, number, { limit = TRANSCRIPT_PAGE } = {}) {
    const task = this.#taskRow(projectId, number)
    if (task.assignee_id === null) return { items: [], total: 0 }
    const copied = this.#db
      .prepare(
        `SELECT t.conversation_id, t.item_id, t.role, t.text, t.complete, t.at
         FROM transcript t JOIN conversation c ON c.id = t.conversation_id
         WHERE c.participant_id = ? ORDER BY t.conversation_id, t.seq`,
      )
      .all(task.assignee_id)
    // A window's copy may hold more than this task (the lead's own, after
    // its other work): the task's part starts where its brief arrived.
    const brief = this.#db
      .prepare(
        `SELECT id FROM message WHERE task_id = ? AND kind = 'task' AND recipient_id = ?
         ORDER BY id DESC LIMIT 1`,
      )
      .get(task.id, task.assignee_id)
    const start =
      brief === undefined
        ? -1
        : copied.findIndex((row) => row.text.includes(`[ConsensFlow m-${brief.id} ·`))
    const rows = start > 0 ? copied.slice(start) : copied
    return {
      total: rows.length,
      items: rows.slice(Math.max(0, rows.length - limit)).map((row) => ({
        id: row.item_id,
        conversation: row.conversation_id,
        role: row.role,
        text: row.text,
        complete: row.complete === 1,
        at: row.at,
      })),
    }
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

  /**
   * A task, from the lead or the human. Given `to`, it is queued for that
   * participant at once (the lead, the human, or the requester itself). Given
   * a `pool` and `tier` instead, it opens for the daemon to assign to a member
   * of that pool and tier (work for a worker, advice from an advisor; an image
   * from a designer, which has no tier), and critical work names its `purpose`.
   * A task on the board may `needs` other tasks: it waits until each is
   * accepted. With `before`, tasks still on the board wait for this one.
   */
  createTask(
    projectId,
    { from, to, after, pool, tier, purpose, body, title, needs = [], before = [] },
  ) {
    requireText(body, 'body', MAX_BODY)
    if (title !== undefined) requireText(title, 'title', MAX_TITLE)
    const needed = requireNumbers(needs, 'needs')
    const blocking = requireNumbers(before, 'before')
    if (to === undefined && after === undefined) {
      if (!POOLS.includes(pool)) {
        throw new LedgerError('invalid-pool', `a pool is ${POOLS.join(', ')}, not ${pool}`)
      }
      if (pool === 'designer') tier = null
      else requireTier(tier)
      if (tier === 'critical' && !PURPOSES.includes(purpose)) {
        throw new LedgerError(
          'purpose-required',
          `critical work names its purpose: ${PURPOSES.join(', ')}`,
        )
      }
    }
    return this.#write(() => {
      const requester = this.#participantByHandle(projectId, from)
      if (!COORDINATOR_ROLES.includes(requester.role)) {
        throw new LedgerError(
          'not-a-coordinator',
          `@${requester.handle} is a ${requester.role}: members do not hand out tasks`,
          403,
        )
      }
      // Advice is the lead's alone to ask: the human gives the lead work, not its advisors.
      if (
        pool === 'advisor' &&
        to === undefined &&
        after === undefined &&
        requester.role !== 'lead'
      ) {
        throw new LedgerError('advice-for-the-lead', 'only the lead asks an advisor', 403)
      }
      // A follow-up on a finished task goes to the session that did it, while
      // it is still there and free: the one case a coordinator names a window.
      const assignee =
        after !== undefined
          ? this.#continuableSession(projectId, after)
          : to === undefined
            ? null
            : this.#participantByHandle(projectId, to)
      if (assignee === null && this.#members(projectId, pool, tier).length === 0) {
        throw new LedgerError(
          'no-member-of-tier',
          `no ${poolName(pool, tier)} is on the team: ask the human for one with cf ask --human "…"`,
          409,
        )
      }
      // A task given by name waits on the board too while what it needs is
      // not yet accepted; it goes to its window then.
      const blocked =
        assignee !== null &&
        needed.some((number) => this.#taskRow(projectId, number).state !== 'accepted')
      const { next } = this.#db
        .prepare('SELECT COALESCE(MAX(number), 0) + 1 AS next FROM task WHERE project_id = ?')
        .get(projectId)
      const at = this.#at()
      const { lastInsertRowid: taskId } = this.#db
        .prepare(
          `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state,
                             pool, tier, purpose, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          projectId,
          next,
          title ?? titleOf(body),
          body,
          requester.id,
          assignee?.id ?? null,
          assignee === null || blocked ? 'open' : 'queued',
          assignee === null ? pool : null,
          assignee === null ? tier : null,
          purpose ?? null,
          at,
          at,
        )
      for (const number of needed) {
        const need = this.#taskRow(projectId, number)
        if (need.state === 'cancelled') {
          throw new LedgerError(
            'need-cancelled',
            `T-${number} is cancelled: nothing waits for it`,
            409,
          )
        }
        this.#db
          .prepare('INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)')
          .run(taskId, need.id)
      }
      for (const number of blocking) {
        const waits = this.#taskRow(projectId, number)
        if (waits.state !== 'open') {
          throw new LedgerError(
            'not-on-the-board',
            `T-${number} is ${waits.state}: only a task still on the board can wait for a new one`,
            409,
          )
        }
        // A plan has no circles: what the new task waits for, near or far, cannot wait for it.
        if (this.#upstream(taskId, waits.id)) {
          throw new LedgerError(
            'circular-needs',
            `T-${number} is already what T-${next} waits for: a plan has no circles`,
            409,
          )
        }
        this.#db
          .prepare('INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)')
          .run(waits.id, taskId)
      }
      if (assignee === null || blocked) {
        this.#log(projectId, 'task.opened', {
          task: next,
          from: requester.handle,
          ...(assignee === null ? { pool, tier } : { to: assignee.handle }),
          ...(needed.length === 0 ? {} : { needs: needed }),
          ...(blocking.length === 0 ? {} : { before: blocking }),
        })
        return { task: this.#task(taskId), message: null }
      }
      const messageId = this.#queue(projectId, {
        to: assignee.id,
        from: requester.id,
        kind: 'task',
        taskId,
        body: deliveryBody(this.#taskById(taskId)),
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

  /** Whether `taskId` needs `otherId`, directly or through the tasks it needs. */
  #upstream(taskId, otherId) {
    return (
      this.#db
        .prepare(
          `WITH RECURSIVE upstream (id) AS (
             SELECT needs_id FROM task_need WHERE task_id = ?
             UNION
             SELECT n.needs_id FROM task_need n JOIN upstream u ON n.task_id = u.id
           )
           SELECT 1 FROM upstream WHERE id = ? LIMIT 1`,
        )
        .get(taskId, otherId) !== undefined
    )
  }

  /** The active members an open task may go to, with what the daemon ranks them by. */
  /**
   * Whether a member has a task on its hands: one task per member session
   * ends when this is false. Paused work counts: its window stays for the
   * resumption.
   */
  holdsWork(participantId) {
    return (
      this.#db
        .prepare(
          `SELECT 1 FROM task WHERE assignee_id = ? AND state IN (${[...HELD_TASK_STATES, 'paused'].map((state) => `'${state}'`).join(', ')})`,
        )
        .get(participantId) !== undefined
    )
  }

  candidates(projectId, number) {
    const task = this.#taskRow(projectId, number)
    return this.members(projectId, task.pool)
      .filter((member) => task.tier === null || member.tier === task.tier)
      .map((member) => ({ ...member, hadIt: member.id === task.taken_from_id }))
  }

  /**
   * The active members of one role, in join order, with what the daemon ranks
   * them by: their tier, how many tasks they have taken, whether one is on
   * their hands now, and until when they are out of quota.
   */
  members(projectId, role) {
    const held = HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')
    return this.#db
      .prepare(
        `SELECT p.*,
                (SELECT COUNT(*) FROM task t JOIN participant s ON s.id = t.assignee_id
                  WHERE s.id = p.id OR s.member_id = p.id) AS taken,
                (SELECT COUNT(*) FROM participant s
                  WHERE (s.id = p.id OR s.member_id = p.id) AND s.left_at IS NULL
                    AND EXISTS (SELECT 1 FROM task WHERE assignee_id = s.id AND state IN (${held}))
                ) AS sessions
         FROM participant p
         WHERE p.project_id = ? AND p.left_at IS NULL AND p.member_id IS NULL
           AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
         ORDER BY p.id`,
      )
      .all(projectId, role)
      .map((row) => ({
        id: row.id,
        handle: row.handle,
        agent: row.agent,
        tier: row.tier,
        roles: JSON.parse(row.roles),
        taken: row.taken,
        sessions: row.sessions,
        outUntil: row.out_until,
      }))
  }

  /**
   * A member's new session: its own participant, named after the member,
   * with the member's agent, harness and tier and the role its task needs.
   * It starts from nothing and ends with its work (CORE-19).
   */
  #startSession(projectId, member, role) {
    for (let attempt = 0; attempt < 16; attempt += 1) {
      const handle = `${member.handle}-${this.#names()}`
      const taken = this.#db
        .prepare('SELECT 1 FROM participant WHERE project_id = ? AND handle = ?')
        .get(projectId, handle)
      if (taken !== undefined) continue
      const { lastInsertRowid: id } = this.#db
        .prepare(
          `INSERT INTO participant (project_id, handle, role, roles, agent, harness, tier, member_id, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          projectId,
          handle,
          role,
          member.roles,
          member.agent,
          member.harness,
          member.tier,
          member.id,
          this.#at(),
        )
      this.#log(projectId, 'session.started', { handle, member: member.handle, role })
      return this.#participantRow(id)
    }
    throw new LedgerError('no-session-name', `no free session name for @${member.handle}`, 409)
  }

  /** A session ends: it leaves the project and its conversation closes. */
  #endSession(session, reason) {
    const at = this.#at()
    this.#db.prepare('UPDATE participant SET left_at = ? WHERE id = ?').run(at, session.id)
    this.#db
      .prepare('UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL')
      .run(at, session.id)
    this.#log(session.project_id, 'session.ended', {
      handle: session.handle,
      member: session.member_handle,
      reason,
    })
  }

  /**
   * A session is the human's to end: its lane folds into its member's, its
   * conversation closes, and nothing of it can be resumed. One still holding
   * work (queued, working or waiting) is refused; paused work goes
   * back on the board when resumed.
   */
  endSession(projectId, handle, { by }) {
    return this.#write(() => {
      this.#participantByHandle(projectId, by)
      const session = this.#participantByHandle(projectId, handle)
      if (session.member_id === null) {
        throw new LedgerError('not-a-session', `@${handle} is not a session`, 409)
      }
      if (this.#db.prepare(HOLDS_WORK).get(session.id) !== undefined) {
        throw new LedgerError(
          'session-busy',
          `@${handle} still holds work: accept, cancel or pause it first`,
          409,
        )
      }
      this.#endSession(session, `ended by @${by}`)
      return this.project(projectId)
    })
  }

  /**
   * The daemon's choice for an open task: a new session of that member,
   * which the task is queued for from here on.
   */
  assignTask(projectId, number, participantId) {
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['open'], 'assign')
      const member = this.#requireMemberRow(participantId, 'takes a task')
      const candidate =
        member.left_at === null &&
        member.project_id === projectId &&
        JSON.parse(member.roles).includes(task.pool) &&
        (task.tier === null || member.tier === task.tier)
      if (!candidate) {
        throw new LedgerError(
          'not-a-candidate',
          `@${member.handle} is not ${aPool(task.pool, task.tier)} on this team`,
          409,
        )
      }
      const session = this.#startSession(projectId, member, task.pool)
      this.#db
        .prepare('UPDATE task SET assignee_id = ?, updated_at = ? WHERE id = ?')
        .run(session.id, this.#at(), task.id)
      const messageId = this.#queue(projectId, {
        to: session.id,
        from: task.requester_id,
        kind: 'task',
        taskId: task.id,
        body: deliveryBody(this.#taskById(task.id)),
      })
      this.#moveTask(
        task,
        'queued',
        { assignee: session.handle, member: member.handle, message: messageId },
        'task.assigned',
      )
      return { task: this.#task(task.id), message: this.#message(messageId) }
    })
  }

  /** The session that did T-`after`, still there and with nothing on its hands. */
  #continuableSession(projectId, after) {
    const previous = this.#taskRow(projectId, after)
    const session =
      previous.assignee_id === null ? null : this.#participantRow(previous.assignee_id)
    if (session === null || session.member_id === null || session.left_at !== null) {
      throw new LedgerError(
        'session-ended',
        `the session that did T-${after} has ended: open the task for its tier instead`,
        409,
      )
    }
    if (this.holdsWork(session.id)) {
      throw new LedgerError(
        'session-busy',
        `@${session.handle} is still on its work: wait for its result, or open the task for its tier`,
        409,
      )
    }
    return session
  }

  /** A member of the team, never one of its sessions. */
  #requireMemberRow(participantId, does) {
    const row = this.#participantRow(participantId)
    if (row.member_id !== null) {
      throw new LedgerError(
        'not-a-member',
        `@${row.handle} is a session of @${row.member_handle}: a member ${does}`,
        409,
      )
    }
    return row
  }

  /** A member that ran out of quota takes no work until then (an ISO time); `outSince` says when it was marked. */
  markOut(participantId, { until, reason }) {
    if (Number.isNaN(Date.parse(until))) {
      throw new LedgerError('invalid-time', `not a time: ${JSON.stringify(until)}`)
    }
    requireText(reason, 'reason', 1000)
    return this.#write(() => {
      const member = this.#participantRow(participantId)
      this.#db
        .prepare('UPDATE participant SET out_until = ?, out_since = ? WHERE id = ?')
        .run(until, this.#at(), member.id)
      this.#log(member.project_id, 'member.out', { handle: member.handle, until, reason })
      return participantView(this.#participantRow(member.id))
    })
  }

  /**
   * A task given by tier goes back to the board for another member of that
   * tier: taken from a member that ran out of quota (the daemon), or
   * reassigned by the human, working or paused. The task opens again with a
   * warning for the next member, whatever was still on its way to the old
   * one (a brief held for the human too) is withdrawn, and the requester is
   * told.
   */
  releaseTask(projectId, number, { because }) {
    requireText(because, 'because', 1000)
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['queued', ...ACTIVE_TASK_STATES, 'paused'], 'release')
      if (task.pool === null) {
        throw new LedgerError(
          'invalid-transition',
          `cannot release T-${number}: it was given by name, not by tier`,
          409,
        )
      }
      // A task paused before anyone took it has nobody to take it from.
      const member = task.assignee_id === null ? null : this.#participantRow(task.assignee_id)
      if (member !== null) {
        this.#db
          .prepare(
            `UPDATE message SET state = 'cancelled'
             WHERE task_id = ? AND recipient_id = ? AND state IN ('queued', 'delivering', 'gated')`,
          )
          .run(task.id, member.id)
      }
      // One statement: the row is never without an assignee in a working
      // state. It remembers the member it was taken from (a session's member).
      this.#db
        .prepare(
          `UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?,
             taken_from_id = COALESCE(?, taken_from_id)
           WHERE id = ?`,
        )
        .run(
          member === null
            ? task.body
            : `${task.body}\n\nReassigned from @${member.handle} (${because}); check the working tree for partial changes.`,
          this.#at(),
          member === null ? null : (member.member_id ?? member.id),
          task.id,
        )
      this.#log(projectId, 'task.released', {
        task: number,
        from: task.state,
        to: 'open',
        member: member?.handle ?? null,
        because,
      })
      const requester = this.#participantRow(task.requester_id)
      this.#send(projectId, {
        to: requester.handle,
        task: number,
        kind: 'note',
        body:
          member === null
            ? `T-${number} is back on the board (${because}) and waits for ${aPool(task.pool, task.tier)}.`
            : `T-${number} was taken back from @${member.handle} (${because}) and waits for another ${poolName(task.pool, task.tier)}.`,
      })
      return { task: this.#task(task.id) }
    })
  }

  note(projectId, { from, to, body, task }) {
    return this.#send(projectId, { from, to, body, task, kind: 'note' })
  }

  /**
   * A question for a coordinator or the human; the asker's task waits for the
   * answer. With `questions`, the question carries options as a harness's own
   * question tool asked them, and its text is rendered from them.
   */
  ask(projectId, { from, to, body, task, questions }) {
    if (from === undefined) {
      throw new LedgerError('unknown-participant', 'a question names who asks it', 400)
    }
    const options = questions === undefined ? null : requireQuestions(questions)
    return this.#write(() => {
      const message = this.#send(projectId, {
        from,
        to,
        body: options === null ? body : renderQuestions(options),
        task,
        kind: 'question',
        questions: options,
      })
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

  /**
   * The answer goes back to whoever asked; the one asked or the human may
   * answer. A plain question's answer is delivered into the asker's window. A
   * question with options is answered by choice (or by text, one line per
   * question): that answer is read at once and never delivered, because the
   * harness door that asked collects it and the tool call completes with it;
   * the asker's task resumes here.
   */
  answer(questionId, { from, body, choices }) {
    return this.#write(() => {
      const question = this.#message(questionId)
      if (question === null || question.kind !== 'question') {
        throw new LedgerError('not-a-question', `message ${questionId} is not a question`, 409)
      }
      // The one asked, the human, or (a question with options) the asker itself:
      // its window may have answered first, and the board's copy takes that answer.
      const fromWindow = question.questions !== null && from === question.sender
      if (question.recipient !== from && from !== 'human' && !fromWindow) {
        throw new LedgerError(
          'not-your-question',
          `the question was put to ${question.recipient}, not ${from}`,
          403,
        )
      }
      if (this.#answered(questionId)) {
        throw new LedgerError('already-answered', `m-${questionId} has its answer`, 409)
      }
      const picks =
        question.questions === null ? null : requireChoices(question.questions, { choices, body })
      const text = picks === null ? body : renderChoices(question.questions, picks)
      requireText(text, 'body', MAX_BODY)
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
        body: text,
        ...(picks === null ? {} : { collected: true, choices: picks }),
      })
      this.#log(question.projectId, 'message.sent', {
        message: id,
        kind: 'answer',
        from,
        to: question.sender,
      })
      // A question still gated is answered before the one asked saw it: it goes no further.
      if (question.state === 'gated') this.#withdraw(questionId, `answered by @${from}`)
      const answer = this.#message(id)
      if (answer.state === 'read') this.#resume(asker.task_id)
      return answer
    })
  }

  /** A choice answer is read by the door that asked, at once: the asker's task goes on. */
  #resume(taskId) {
    if (taskId === null) return
    const task = this.#db.prepare('SELECT * FROM task WHERE id = ?').get(taskId)
    if (task.state === 'waiting') this.#moveTask(task, 'working')
  }

  /** Whether a question on the task still waits for its answer. */
  #unanswered(taskId) {
    return (
      this.#db
        .prepare(
          `SELECT 1 FROM message q WHERE q.task_id = ? AND q.kind = 'question'
             AND NOT EXISTS (
               SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer'
                 AND a.state NOT IN ('gated', 'cancelled')
             )
           LIMIT 1`,
        )
        .get(taskId) !== undefined
    )
  }

  /** Whether a question has an answer, on its way or still gated; a declined one never counts. */
  #answered(questionId) {
    return (
      this.#db
        .prepare(
          `SELECT 1 FROM message WHERE reply_to = ? AND kind = 'answer' AND state != 'cancelled'`,
        )
        .get(questionId) !== undefined
    )
  }

  /** The answer to a question, or null while it waits: for the one asked, or for the human's approval. */
  answerTo(questionId) {
    const row = this.#db
      .prepare(
        `${MESSAGE_SELECT} WHERE m.reply_to = ? AND m.kind = 'answer'
           AND m.state NOT IN ('gated', 'cancelled')
         ORDER BY m.id LIMIT 1`,
      )
      .get(questionId)
    return row === undefined ? null : messageView(row)
  }

  /**
   * The head of a participant's queue that may go now, or null. A member's
   * session is its task's: a task message waits while it holds one, and a
   * message about no task of its own (a stray note) never opens a window.
   */
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
           AND (? = 0 OR kind = 'task' OR task_id IN (
             SELECT id FROM task WHERE assignee_id = ? AND state IN (${HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')})
           ))
         ORDER BY id LIMIT 1`,
      )
      .get(participantId, serial ? 1 : 0, participantId, serial ? 1 : 0, participantId)
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
        // A task whose window already asked a question arrives waiting, not working.
        if (message.kind === 'task' && task.state === 'queued') {
          this.#moveTask(task, this.#unanswered(task.id) ? 'waiting' : 'working')
        }
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

  /** The human read a message in the app. */
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
      return this.#message(messageId)
    })
  }

  /** The human passes a gated message on: it goes the way it would have gone without the gate. */
  approveMessage(messageId, { by }) {
    return this.#write(() => {
      const message = this.#requireMessage(messageId, 'gated')
      this.#participantByHandle(message.projectId, by)
      const landing = message.choices === null ? 'queued' : 'read'
      this.#db.prepare('UPDATE message SET state = ? WHERE id = ?').run(landing, messageId)
      this.#log(message.projectId, 'message.approved', { message: messageId, by })
      if (landing === 'read') this.#resume(this.#messageTask(messageId)?.id ?? null)
      return this.#message(messageId)
    })
  }

  /**
   * The human declines a gated message, and whoever sent it is told. A
   * declined task is cancelled; a declined answer leaves its question open
   * for another. A result or a question is passed on, never declined.
   */
  declineMessage(messageId, { by }) {
    return this.#write(() => {
      const message = this.#requireMessage(messageId, 'gated')
      this.#participantByHandle(message.projectId, by)
      if (message.kind !== 'task' && message.kind !== 'answer') {
        throw new LedgerError('not-declinable', `a ${message.kind} is passed on, not declined`, 409)
      }
      this.#withdraw(messageId, `declined by @${by}`)
      this.#log(message.projectId, 'message.declined', { message: messageId, by })
      const task = this.#messageTask(messageId)
      let told = message.sender
      let word = `@${by} declined your answer to m-${message.replyTo}. Answer it again: cf answer m-${message.replyTo} "…"`
      if (message.kind === 'task') {
        told = this.#participantRow(task.requester_id).handle
        this.cancelTask(message.projectId, task.number, { by })
        word = `@${by} declined T-${task.number} (${task.title}). It is cancelled.`
      }
      if (told !== by) {
        this.#send(message.projectId, {
          from: by,
          to: told,
          task: task?.number,
          body: word,
          kind: 'note',
        })
      }
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
      this.#withdrawGated(task.id, `accepted by @${by}`)
      this.#moveTask(task, 'accepted', { by })
      this.#releaseWaiting(projectId, task.id)
      return this.#task(task.id)
    })
  }

  /**
   * The lead (or the human) stops a worker's task without ending it: its
   * window closes on the daemon's next look, whatever was on its way to it is
   * withdrawn, and the task keeps its member, its conversation and its place
   * until it is resumed or cancelled. The lead's own work is not paused.
   */
  pauseTask(projectId, number, { by, because } = {}) {
    if (because !== undefined) requireText(because, 'because', 1000)
    return this.#write(() => {
      if (by !== undefined) this.#participantByHandle(projectId, by)
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES], 'pause')
      if (task.assignee_id !== null && this.#participantRow(task.assignee_id).role === 'lead') {
        throw new LedgerError('own-work', `T-${number} is the lead's own: finish or cancel it`, 409)
      }
      this.#dropQueued(task.id)
      this.#moveTask(task, 'paused', {
        by: by ?? null,
        ...(because === undefined ? {} : { because }),
      })
      return this.#task(task.id)
    })
  }

  /** The paused task a participant still holds, or null. */
  pausedTask(participantId) {
    const row = this.#db
      .prepare(
        `SELECT project_id, number FROM task WHERE assignee_id = ? AND state = 'paused'
         ORDER BY id LIMIT 1`,
      )
      .get(participantId)
    return row === undefined ? null : this.task(row.project_id, row.number)
  }

  /**
   * A paused task goes on with the words that resume it: into the same
   * window when its session is still there (a brief never delivered goes in
   * first), or back on the board for its tier when the session has ended.
   */
  resumeTask(projectId, number, { by, body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const author = this.#participantByHandle(projectId, by)
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['paused'], 'resume')
      const assignee = task.assignee_id === null ? null : this.#participantRow(task.assignee_id)
      if (assignee === null) {
        this.#db
          .prepare('UPDATE task SET body = ?, updated_at = ? WHERE id = ?')
          .run(`${task.body}\n\nResumed: ${body}`, this.#at(), task.id)
        this.#moveTask(task, 'open', { by })
        return { task: this.#task(task.id), message: null }
      }
      if (assignee.left_at !== null) {
        if (task.pool === null) {
          throw new LedgerError(
            'session-ended',
            `the window that had T-${number} has ended: cancel it and open the work for its tier`,
            409,
          )
        }
        this.#db
          .prepare(
            `UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?
             WHERE id = ?`,
          )
          .run(
            `${task.body}\n\nResumed after a pause, in a fresh window (the one that had it ended; check the working tree for partial changes): ${body}`,
            this.#at(),
            task.id,
          )
        this.#log(projectId, 'task.state', { task: number, from: 'paused', to: 'open', by })
        return { task: this.#task(task.id), message: null }
      }
      const delivered = this.#db
        .prepare(
          `SELECT 1 FROM message WHERE task_id = ? AND kind = 'task' AND recipient_id = ?
             AND state IN ('delivered', 'read')`,
        )
        .get(task.id, assignee.id)
      const messageId = this.#queue(projectId, {
        to: assignee.id,
        from: author.id,
        kind: 'task',
        taskId: task.id,
        body:
          delivered === undefined
            ? `${deliveryBody(task)}\n\nResumed: ${body}`
            : `Resumed: ${body}`,
      })
      this.#moveTask(task, 'queued', { by, message: messageId })
      return { task: this.#task(task.id), message: this.#message(messageId) }
    })
  }

  /** A follow-up on a finished or failed task: it goes back to its assignee's queue. */
  reopenTask(projectId, number, { by, body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const author = this.#participantByHandle(projectId, by)
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['done', 'failed'], 'reopen')
      const assignee = this.#participantRow(task.assignee_id)
      if (assignee.member_id !== null && assignee.left_at !== null) {
        throw new LedgerError(
          'session-ended',
          `@${assignee.handle} has ended: open the task for its tier instead`,
          409,
        )
      }
      requireActive(assignee)
      this.#withdrawGated(task.id, `sent back by @${by}`)
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
      this.#requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES, 'paused'], 'cancel')
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
      this.#requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES], 'fail')
      this.#dropQueued(task.id)
      this.#moveTask(task, 'failed', { reason })
      return this.#task(task.id)
    })
  }

  // --- views -------------------------------------------------------------------

  board(projectId) {
    const project = this.project(projectId)
    if (project === null) throw new LedgerError('unknown-project', `no project ${projectId}`, 404)
    const results = new Map(
      this.#db
        .prepare(
          `SELECT task_id, body FROM message WHERE project_id = ? AND kind = 'result' ORDER BY id`,
        )
        .all(projectId)
        .map((row) => [row.task_id, row.body]),
    )
    // Each task with the first line of its latest result: what its card shows.
    // A task of a session that has ended sits on its member's lane.
    const rows = this.#db
      .prepare(`${TASK_SELECT} WHERE t.project_id = ? ORDER BY t.number`)
      .all(projectId)
    const laneOf = new Map(
      rows.map((row) => [
        row.id,
        row.assignee_left_at !== null && row.assignee_member !== null
          ? row.assignee_member
          : row.assignee,
      ]),
    )
    const tasks = rows.map((row) => ({ ...taskView(row), result: firstLine(results.get(row.id)) }))
    return {
      project,
      // On the board for a member; one given by name waits in its own lane.
      open: tasks.filter((task) => task.state === 'open' && task.assignee === null),
      lanes: project.participants.map((participant) => ({
        participant,
        tasks: tasks.filter((task) => laneOf.get(task.id) === participant.handle),
      })),
      overdue: this.#overdueQuestions(projectId),
      gated: this.#gatedMessages(projectId),
    }
  }

  /** What waits for the human's approval, oldest first. */
  #gatedMessages(projectId) {
    return this.#db
      .prepare(`${MESSAGE_SELECT} WHERE m.project_id = ? AND m.state = 'gated' ORDER BY m.id`)
      .all(projectId)
      .map(messageView)
  }

  /** Questions a coordinator has left unanswered for OVERDUE_MS: the human sees them too. */
  #overdueQuestions(projectId) {
    const before = new Date(this.#now().getTime() - OVERDUE_MS).toISOString()
    return this.#db
      .prepare(
        `${MESSAGE_SELECT}
         WHERE m.project_id = ? AND m.kind = 'question' AND r.role = 'lead'
           AND m.state != 'gated' AND m.created_at <= ?
           AND NOT EXISTS (SELECT 1 FROM message a WHERE a.reply_to = m.id AND a.kind = 'answer')
         ORDER BY m.id`,
      )
      .all(projectId, before)
      .map(messageView)
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

  /**
   * The task a participant has in progress (working or waiting on an answer),
   * or null. With `queued`, one whose delivery is still being confirmed
   * counts too: a window may ask its first question before the record that
   * confirms the task's arrival is read.
   */
  activeTask(participantId, { queued = false } = {}) {
    const row = this.#db
      .prepare(
        `SELECT project_id, number FROM task
         WHERE assignee_id = ? AND state IN (${queued ? "'queued', " : ''}'working', 'waiting')
         ORDER BY id LIMIT 1`,
      )
      .get(participantId)
    return row === undefined ? null : this.task(row.project_id, row.number)
  }

  /** A participant's messages, newest first: what reached it or is on its way, never what still waits for the human. */
  inbox(participantId, { limit = 100 } = {}) {
    return this.#db
      .prepare(
        `${MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state != 'gated' ORDER BY m.id DESC LIMIT ?`,
      )
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
    const at = this.#at()
    this.#db
      .prepare('INSERT INTO event (project_id, at, kind, data) VALUES (?, ?, ?, ?)')
      .run(projectId, at, kind, JSON.stringify(data))
    this.#trace({ at, project: projectId, kind, data })
  }

  #projectRow(id) {
    const row = this.#db.prepare('SELECT * FROM project WHERE id = ?').get(id)
    if (row === undefined) throw new LedgerError('unknown-project', `no project ${id}`, 404)
    return row
  }

  #participantRow(id) {
    const row = this.#db.prepare(`${PARTICIPANT_SELECT} WHERE p.id = ?`).get(id)
    if (row === undefined) throw new LedgerError('unknown-participant', `no participant ${id}`, 404)
    return row
  }

  #participantByHandle(projectId, handle) {
    this.#projectRow(projectId)
    const row = this.#db
      .prepare(`${PARTICIPANT_SELECT} WHERE p.project_id = ? AND p.handle = ?`)
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

  #addParticipant(projectId, { handle, role, roles = [], agent, harness, tier = null }) {
    role ??= roles[0]
    this.#projectRow(projectId)
    const taken = this.#db
      .prepare('SELECT 1 FROM participant WHERE project_id = ? AND handle = ?')
      .get(projectId, handle)
    if (taken !== undefined) {
      throw new LedgerError('member-exists', `${handle} is already in project ${projectId}`, 409)
    }
    const { lastInsertRowid: id } = this.#db
      .prepare(
        `INSERT INTO participant (project_id, handle, role, roles, agent, harness, tier, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(projectId, handle, role, JSON.stringify(roles), agent, harness, tier, this.#at())
    if (role !== 'human' && role !== 'lead') {
      this.#log(projectId, 'member.added', { handle, role, roles, harness })
    }
    return participantView(this.#participantRow(id))
  }

  /** The active members of one pool and tier, in join order. */
  /** The team's members of one role and tier (any tier when null), whatever role they were saved with first. */
  #members(projectId, pool, tier) {
    return this.#db
      .prepare(
        `SELECT * FROM participant p
         WHERE p.project_id = ? AND (? IS NULL OR p.tier = ?) AND p.left_at IS NULL AND p.member_id IS NULL
           AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
         ORDER BY p.id`,
      )
      .all(projectId, tier, tier, pool)
  }

  #taskById(id) {
    return this.#db.prepare('SELECT * FROM task WHERE id = ?').get(id)
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

  #queue(
    projectId,
    {
      to,
      from,
      kind,
      taskId = null,
      replyTo = null,
      body,
      collected = false,
      questions = null,
      choices = null,
    },
  ) {
    // A message on its way (queued, or collected: read at once by the door
    // that asked) waits for the human instead when the project gates it.
    const gated = this.#gateHolds(projectId, from, to)
    const landing = gated ? 'gated' : collected ? 'read' : 'queued'
    const { lastInsertRowid: id } = this.#db
      .prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, reply_to, body,
                              state, questions, choices, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        projectId,
        to,
        from,
        kind,
        taskId,
        replyTo,
        body,
        landing,
        questions === null ? null : JSON.stringify(questions),
        choices === null ? null : JSON.stringify(choices),
        this.#at(),
      )
    return id
  }

  /**
   * Whether the project's gate holds a message: one agent's word to another,
   * when the human approves every hand-off. What the human sends or receives,
   * what ConsensFlow itself notes, and what an agent tells itself pass.
   */
  #gateHolds(projectId, from, to) {
    if (from === null || from === to || this.#projectRow(projectId).gate !== 1) return false
    return this.#participantRow(from).role !== 'human' && this.#participantRow(to).role !== 'human'
  }

  /** A gated message the human never passed on: declined, answered, or overtaken. */
  #withdraw(messageId, reason) {
    this.#db
      .prepare(`UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?`)
      .run(reason, messageId)
  }

  #withdrawGated(taskId, reason) {
    this.#db
      .prepare(
        `UPDATE message SET state = 'cancelled', reason = ? WHERE task_id = ? AND state = 'gated'`,
      )
      .run(reason, taskId)
  }

  #send(projectId, { from, to, body, task, kind, questions = null }) {
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
        questions,
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

  /**
   * The tasks given by name that waited on the board for the one just
   * accepted: each with nothing left to wait for goes to its window now.
   */
  #releaseWaiting(projectId, acceptedId) {
    const waiting = this.#db
      .prepare(
        `SELECT t.* FROM task t JOIN task_need n ON n.task_id = t.id
         WHERE n.needs_id = ? AND t.state = 'open' AND t.assignee_id IS NOT NULL ORDER BY t.id`,
      )
      .all(acceptedId)
    for (const task of waiting) {
      const still = this.#db
        .prepare(
          `SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
           WHERE n.task_id = ? AND d.state != 'accepted'`,
        )
        .get(task.id)
      if (still !== undefined) continue
      const messageId = this.#queue(projectId, {
        to: task.assignee_id,
        from: task.requester_id,
        kind: 'task',
        taskId: task.id,
        body: deliveryBody(task),
      })
      this.#moveTask(task, 'queued', { message: messageId })
    }
  }

  #moveTask(task, to, detail = {}, kind = 'task.state') {
    this.#db
      .prepare('UPDATE task SET state = ?, updated_at = ? WHERE id = ?')
      .run(to, this.#at(), task.id)
    this.#log(task.project_id, kind, { task: task.number, from: task.state, to, ...detail })
  }

  /** A cancelled or failed task's queued messages are never delivered. */
  #dropQueued(taskId) {
    this.#db
      .prepare(
        `UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state IN ('queued', 'gated')`,
      )
      .run(taskId)
  }
}
