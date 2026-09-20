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
 *   working, waiting ──result──▶ done, or ──result, review due──▶ review
 *   review ──pass, no reviewer, two rounds──▶ done, ──changes──▶ queued (back to its author)
 *   done ──review asked──▶ review, ──accept──▶ accepted
 *   done, failed ──reopen──▶ queued
 *   open, queued, working, waiting, review ──cancel──▶ cancelled, ──fail──▶ failed
 *
 * - The review gate: under the project's policy (none, members, all) a finished
 *   task waits in review with its result held; the daemon gives a reviewer a
 *   review task, and the verdict on its last line decides: pass releases the
 *   result and the review to the requester; changes sends the work back to its
 *   author with the findings, once; a second changes releases everything to the
 *   requester with a note to decide. A review task is never itself reviewed.
 *
 * This module never reads `process.env` and never logs: the file and the
 * clock are arguments, and every refusal is a `LedgerError` with a stable code.
 */

export const HARNESSES = ['claude-code', 'codex', 'opencode', 'pi', 'devin', 'kimi']
const MEMBER_ROLES = ['worker', 'advisor', 'reviewer']
/** Who hands each kind of member its work, and hears when the team changes. */
const COORDINATOR_HANDLES = ['human', 'lead', 'pm']
const COORDINATOR_ROLES = ['human', 'lead', 'pm']
export const TIERS = ['critical', 'complex', 'standard', 'light']
export const POOLS = ['worker', 'advisor']
export const PURPOSES = ['critical-review', 'architecture', 'hard-problem', 'important-question']
const CRITICAL_RULE =
  'No coding or implementation edits. Do not write or revise specifications. Return analysis, evidence and recommendations to your coordinator.'
export const REVIEW_POLICIES = ['none', 'members', 'all']
const REVIEW_ROUNDS = 2
/** The reviewer's last word, with whatever emphasis its harness wrapped it in: `**VERDICT: pass**`, `Verdict: **changes**`. */
const VERDICT = /^[\s*_`#>-]*VERDICT[\s*_`]*[:\-–—][\s*_`]*(pass|changes)\b/i
const REVIEW_STATES = ['open', 'queued', 'working', 'waiting']
const TAG = /^[a-z0-9][a-z0-9-]{0,31}$/
const MAX_TAGS = 20
const ACTIVE_TASK_STATES = ['working', 'waiting']
/** A task on a member's hands: from assignment until its review is over. */
const HELD_TASK_STATES = ['queued', 'working', 'waiting', 'review']
const MAX_BODY = 1_000_000
/** How long a coordinator may leave a question before the human sees it too. */
export const OVERDUE_MS = 10 * 60_000
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

const noReviewer = () =>
  new LedgerError(
    'no-reviewer',
    'a review policy needs a reviewer on the team: add one first, or keep the policy at none',
    409,
  )

function requireReview(policy) {
  if (!REVIEW_POLICIES.includes(policy)) {
    throw new LedgerError(
      'invalid-review',
      `a review policy is ${REVIEW_POLICIES.join(', ')}, not ${JSON.stringify(policy)}`,
    )
  }
  return policy
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

/** Tags are short lowercase words; the same tag twice counts once. */
function requireTags(tags) {
  if (
    !Array.isArray(tags) ||
    tags.length > MAX_TAGS ||
    !tags.every((tag) => typeof tag === 'string' && TAG.test(tag))
  ) {
    throw new LedgerError(
      'invalid-tags',
      `tags are up to ${MAX_TAGS} short lowercase words (letters, digits, dashes)`,
    )
  }
  return [...new Set(tags)]
}

/** A member's roles, one or more of worker, advisor and reviewer; the first one leads. */
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

/** Validates a member and returns its tags and roles, normalized. */
function requireMember({ agent, harness, role, roles, tier, tags = [] }) {
  const set = requireRoles(roles ?? (role === undefined ? [] : [role]))
  if (typeof agent !== 'string' || !AGENT_ID.test(agent) || COORDINATOR_HANDLES.includes(agent)) {
    throw new LedgerError('invalid-agent', `not an agent id: ${JSON.stringify(agent)}`)
  }
  requireHarness(harness)
  requireTier(tier)
  return { tags: requireTags(tags), roles: set }
}

/** The verdict on a review's last line that says something, or null. */
export function verdictOf(body) {
  const line = body
    .split('\n')
    .map((text) => text.trim())
    .filter(Boolean)
    .at(-1)
  const match = line === undefined ? null : VERDICT.exec(line)
  return match === null ? null : match[1].toLowerCase()
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
  SELECT t.*, q.handle AS requester, a.handle AS assignee, o.number AS review_of_number
  FROM task t
  JOIN participant q ON q.id = t.requester_id
  LEFT JOIN participant a ON a.id = t.assignee_id
  LEFT JOIN task o ON o.id = t.review_of`

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
  tags: JSON.parse(row.tags),
  roles: JSON.parse(row.roles),
  outUntil: row.out_until,
  outSince: row.out_since,
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
  assignee: row.assignee ?? null,
  pool: row.pool,
  tier: row.tier,
  tags: JSON.parse(row.tags),
  purpose: row.purpose,
  kind: row.kind,
  reviewOf: row.review_of_number ?? null,
  round: row.round,
  verdict: row.verdict,
  unreviewed: row.unreviewed ?? null,
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
  createProject({ directory, name, lead, team = [], review }) {
    requireText(directory, 'directory', 4096)
    requireText(name, 'name', 100)
    requireHarness(lead?.harness)
    const members = team.map((member) => ({ ...member, ...requireMember(member) }))
    const reviewer = members.some((member) => member.roles.includes('reviewer'))
    // A review policy needs someone to review: without a reviewer on the
    // team nothing is reviewed, and asking for it is refused.
    review ??= reviewer ? 'members' : 'none'
    requireReview(review)
    if (review !== 'none' && !reviewer) throw noReviewer()
    return this.#write(() => {
      const at = this.#at()
      const { lastInsertRowid: id } = this.#db
        .prepare(
          `INSERT INTO project (directory, name, state, review, created_at, updated_at)
           VALUES (?, ?, 'open', ?, ?, ?)`,
        )
        .run(directory, name, review, at, at)
      this.#addParticipant(id, { handle: 'human', role: 'human', agent: null, harness: null })
      this.#addParticipant(id, { handle: 'lead', role: 'lead', agent: null, harness: lead.harness })
      for (const { agent, harness, roles, tier, tags } of members) {
        this.#addParticipant(id, { handle: agent, roles, agent, harness, tier, tags })
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
      review: row.review,
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

  /** Which finished work gets a second review from now on: none, members' or all. */
  setReview(id, policy) {
    requireReview(policy)
    return this.#write(() => {
      if (policy !== 'none' && !this.#reviewerOnTeam(id)) throw noReviewer()
      const from = this.#projectRow(id).review
      this.#db
        .prepare('UPDATE project SET review = ?, updated_at = ? WHERE id = ?')
        .run(policy, this.#at(), id)
      if (from !== policy) this.#log(id, 'project.review', { from, to: policy })
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

  /** A member joins the team, or rejoins it in the role, harness, tier and tags given now. */
  addMember(projectId, { agent, harness, role, roles, tier, tags = [] }) {
    const member = requireMember({ agent, harness, role, roles, tier, tags })
    tags = member.tags
    roles = member.roles
    return this.#write(() => {
      const left = this.#db
        .prepare(
          'SELECT * FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NOT NULL',
        )
        .get(projectId, agent)
      if (left === undefined) {
        return this.#addParticipant(projectId, { handle: agent, roles, agent, harness, tier, tags })
      }
      this.#db
        .prepare(
          `UPDATE participant SET role = ?, roles = ?, harness = ?, tier = ?, tags = ?, left_at = NULL
           WHERE id = ?`,
        )
        .run(roles[0], JSON.stringify(roles), harness, tier, JSON.stringify(tags), left.id)
      this.#log(projectId, 'member.added', { handle: agent, roles, harness, rejoined: true })
      return participantView(this.#participantRow(left.id))
    })
  }

  /** A member's roles change in place; the last reviewer stays while the policy needs one. */
  setRoles(projectId, handle, roles) {
    roles = requireRoles(roles)
    return this.#write(() => {
      const member = this.#participantByHandle(projectId, handle)
      if (!MEMBER_ROLES.includes(member.role)) {
        throw new LedgerError(
          'not-a-member',
          `${handle} is the project's ${member.role}, not a member of its team`,
          409,
        )
      }
      if (!roles.includes('reviewer')) this.#requireAnotherReviewer(projectId, member.id)
      this.#db
        .prepare('UPDATE participant SET role = ?, roles = ? WHERE id = ?')
        .run(roles[0], JSON.stringify(roles), member.id)
      this.#log(projectId, 'member.roles', { handle, roles })
      return participantView(this.#participantRow(member.id))
    })
  }

  /** Whether an active member can review. */
  #reviewerOnTeam(projectId, except = null) {
    return (
      this.#db
        .prepare(
          `SELECT 1 FROM participant p, json_each(p.roles) r
           WHERE p.project_id = ? AND p.left_at IS NULL AND r.value = 'reviewer' AND p.id != ?`,
        )
        .get(projectId, except ?? -1) !== undefined
    )
  }

  /** Under a review policy, the last reviewer cannot go. */
  #requireAnotherReviewer(projectId, memberId) {
    if (this.#projectRow(projectId).review === 'none') return
    if (this.#reviewerOnTeam(projectId, memberId)) return
    throw new LedgerError(
      'last-reviewer',
      'the review policy needs a reviewer on the team: add another one, or set the policy to none first',
      409,
    )
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
      if (JSON.parse(member.roles).includes('reviewer')) {
        this.#requireAnotherReviewer(projectId, member.id)
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
        `SELECT agent, harness, role, roles FROM participant
         WHERE project_id = (
           SELECT MAX(project_id) FROM participant
           WHERE role IN ('worker', 'advisor', 'reviewer') AND left_at IS NULL
         ) AND role IN ('worker', 'advisor', 'reviewer') AND left_at IS NULL
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
   * A task, from a coordinator or the human. Given `to`, it is queued for that
   * participant at once (a coordinator, the human, or the requester itself).
   * Given a `pool` and `tier` instead, it opens for the daemon to assign to a
   * member of that pool and tier; `tags` say which member it prefers, and
   * critical work names its `purpose`.
   */
  createTask(projectId, { from, to, pool, tier, tags = [], purpose, body, title }) {
    requireText(body, 'body', MAX_BODY)
    if (title !== undefined) requireText(title, 'title', MAX_TITLE)
    tags = requireTags(tags)
    if (to === undefined) {
      requireTier(tier)
      if (!POOLS.includes(pool)) {
        throw new LedgerError('invalid-pool', `a pool is ${POOLS.join(' or ')}, not ${pool}`)
      }
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
      const assignee = to === undefined ? null : this.#participantByHandle(projectId, to)
      if (assignee === null && this.#members(projectId, pool, tier).length === 0) {
        throw new LedgerError('no-member-of-tier', `no ${tier} ${pool} is on the team`, 409)
      }
      const { next } = this.#db
        .prepare('SELECT COALESCE(MAX(number), 0) + 1 AS next FROM task WHERE project_id = ?')
        .get(projectId)
      const at = this.#at()
      const { lastInsertRowid: taskId } = this.#db
        .prepare(
          `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state,
                             pool, tier, tags, purpose, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          projectId,
          next,
          title ?? titleOf(body),
          body,
          requester.id,
          assignee?.id ?? null,
          assignee === null ? 'open' : 'queued',
          assignee === null ? pool : null,
          assignee === null ? tier : null,
          JSON.stringify(tags),
          purpose ?? null,
          at,
          at,
        )
      if (assignee === null) {
        this.#log(projectId, 'task.opened', {
          task: next,
          from: requester.handle,
          pool,
          tier,
          tags,
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

  /** The active members an open task may go to, with what the daemon ranks them by. */
  /** Whether a member has a task on its hands: one task per member session ends when this is false. */
  holdsWork(participantId) {
    return (
      this.#db
        .prepare(
          `SELECT 1 FROM task WHERE assignee_id = ? AND state IN (${HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')})`,
        )
        .get(participantId) !== undefined
    )
  }

  candidates(projectId, number) {
    const task = this.#taskRow(projectId, number)
    return this.members(projectId, task.pool).filter((member) => member.tier === task.tier)
  }

  /**
   * The active members of one role, in join order, with what the daemon ranks
   * them by: their tier and tags, how many tasks they have taken, whether one
   * is on their hands now, and until when they are out of quota.
   */
  members(projectId, role) {
    return this.#db
      .prepare(
        `SELECT p.*,
                (SELECT COUNT(*) FROM task WHERE assignee_id = p.id) AS taken,
                EXISTS (
                  SELECT 1 FROM task WHERE assignee_id = p.id AND state IN (${HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')})
                ) AS busy
         FROM participant p
         WHERE p.project_id = ? AND p.left_at IS NULL
           AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
         ORDER BY p.id`,
      )
      .all(projectId, role)
      .map((row) => ({
        id: row.id,
        handle: row.handle,
        tier: row.tier,
        tags: JSON.parse(row.tags),
        roles: JSON.parse(row.roles),
        taken: row.taken,
        busy: row.busy === 1,
        outUntil: row.out_until,
      }))
  }

  /** The daemon's choice for an open task: it is queued for that member from here on. */
  assignTask(projectId, number, participantId) {
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['open'], 'assign')
      const member = this.#participantRow(participantId)
      const candidate =
        member.left_at === null &&
        member.project_id === projectId &&
        member.role === task.pool &&
        member.tier === task.tier
      if (!candidate) {
        throw new LedgerError(
          'not-a-candidate',
          `@${member.handle} is not a ${task.tier} ${task.pool} on this team`,
          409,
        )
      }
      this.#db
        .prepare('UPDATE task SET assignee_id = ?, updated_at = ? WHERE id = ?')
        .run(member.id, this.#at(), task.id)
      const messageId = this.#queue(projectId, {
        to: member.id,
        from: task.requester_id,
        kind: 'task',
        taskId: task.id,
        body: deliveryBody(this.#taskById(task.id)),
      })
      this.#moveTask(
        task,
        'queued',
        { assignee: member.handle, message: messageId },
        'task.assigned',
      )
      return { task: this.#task(task.id), message: this.#message(messageId) }
    })
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
   * The daemon takes a task back from its member (it ran out of quota): the
   * task opens again with a warning for the next member, whatever was still
   * on its way to the member is cancelled, and the requester is told.
   */
  releaseTask(projectId, number, { because }) {
    requireText(because, 'because', 1000)
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['queued', ...ACTIVE_TASK_STATES], 'release')
      if (task.pool === null) {
        throw new LedgerError(
          'invalid-transition',
          `cannot release T-${number}: it was given by name, not by tier`,
          409,
        )
      }
      const member = this.#participantRow(task.assignee_id)
      this.#db
        .prepare(
          `UPDATE message SET state = 'cancelled'
           WHERE task_id = ? AND recipient_id = ? AND state IN ('queued', 'delivering')`,
        )
        .run(task.id, member.id)
      // One statement: the row is never without an assignee in a working state.
      this.#db
        .prepare(
          `UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?
           WHERE id = ?`,
        )
        .run(
          `${task.body}\n\nReassigned from @${member.handle}, which ${because}; check the working tree for partial changes.`,
          this.#at(),
          task.id,
        )
      this.#log(projectId, 'task.released', {
        task: number,
        from: task.state,
        to: 'open',
        member: member.handle,
        because,
      })
      const requester = this.#participantRow(task.requester_id)
      this.#send(projectId, {
        to: requester.handle,
        task: number,
        kind: 'note',
        body: `T-${number} was taken back from @${member.handle} (${because}) and waits for another ${task.tier} ${task.pool}.`,
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
      if (this.answerTo(questionId) !== null) {
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
        ...(picks === null ? {} : { state: 'read', choices: picks }),
      })
      this.#log(question.projectId, 'message.sent', {
        message: id,
        kind: 'answer',
        from,
        to: question.sender,
      })
      if (picks !== null && asker.task_id !== null) {
        const task = this.#db.prepare('SELECT * FROM task WHERE id = ?').get(asker.task_id)
        if (task.state === 'waiting') this.#moveTask(task, 'working')
      }
      return this.#message(id)
    })
  }

  /** Whether a question on the task still waits for its answer. */
  #unanswered(taskId) {
    return (
      this.#db
        .prepare(
          `SELECT 1 FROM message q WHERE q.task_id = ? AND q.kind = 'question'
             AND NOT EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer')
           LIMIT 1`,
        )
        .get(taskId) !== undefined
    )
  }

  /** The answer to a question, or null while it waits. */
  answerTo(questionId) {
    const row = this.#db
      .prepare(`${MESSAGE_SELECT} WHERE m.reply_to = ? AND m.kind = 'answer' ORDER BY m.id LIMIT 1`)
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

  /**
   * The assignee's answer finishes the task and is queued for whoever asked
   * for it, unless the project's policy puts the work in review first: then
   * the result is held until the verdict. A review task ends with
   * `recordVerdict`, not here.
   */
  recordResult(projectId, number, { body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ACTIVE_TASK_STATES, 'record a result for')
      if (task.kind === 'review') {
        throw new LedgerError('not-work', `T-${number} is a review: it ends with a verdict`, 409)
      }
      const review = this.#reviewDue(projectId, task)
      const messageId = this.#queue(projectId, {
        to: task.requester_id,
        from: task.assignee_id,
        kind: 'result',
        taskId: task.id,
        body,
        state: review ? 'held' : 'queued',
      })
      this.#moveTask(task, review ? 'review' : 'done', { result: messageId })
      return { task: this.#task(task.id), message: this.#message(messageId) }
    })
  }

  /** The tasks in review that no reviewer is on yet: the daemon finds each one a reviewer. */
  reviewsPending(projectId) {
    return this.#db
      .prepare(
        `${TASK_SELECT} WHERE t.project_id = ? AND t.state = 'review' AND NOT EXISTS (
           SELECT 1 FROM task r WHERE r.review_of = t.id AND r.state IN (${REVIEW_STATES.map(() => '?').join(', ')})
         ) ORDER BY t.number`,
      )
      .all(projectId, ...REVIEW_STATES)
      .map(taskView)
  }

  /** The daemon's reviewer for a task in review: a review task, queued for that reviewer. */
  createReview(projectId, number, { reviewer }) {
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['review'], 'review')
      if (!this.reviewsPending(projectId).some((pending) => pending.id === task.id)) {
        throw new LedgerError('invalid-transition', `T-${number} already has its reviewer`, 409)
      }
      const member = this.#participantRow(reviewer)
      if (
        !JSON.parse(member.roles).includes('reviewer') ||
        member.left_at !== null ||
        member.project_id !== projectId
      ) {
        throw new LedgerError(
          'not-a-reviewer',
          `@${member.handle} is not a reviewer on this team`,
          409,
        )
      }
      const author = this.#participantRow(task.assignee_id)
      const result = this.#db
        .prepare(
          `SELECT body FROM message WHERE task_id = ? AND kind = 'result' ORDER BY id DESC LIMIT 1`,
        )
        .get(task.id)
      const round = task.round + 1
      const body = [
        `Review T-${number} (round ${round}) by @${author.handle}.`,
        `The task:\n${task.body}`,
        `The result:\n${result?.body ?? '(no written result)'}`,
        'Report errors, omissions and actionable findings with evidence; suggested fixes belong in your findings. Change no file. End with one line: VERDICT: pass or VERDICT: changes.',
      ].join('\n\n')
      const { next } = this.#db
        .prepare('SELECT COALESCE(MAX(number), 0) + 1 AS next FROM task WHERE project_id = ?')
        .get(projectId)
      const at = this.#at()
      const { lastInsertRowid: reviewId } = this.#db
        .prepare(
          `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state,
                             kind, review_of, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, 'queued', 'review', ?, ?, ?)`,
        )
        .run(
          projectId,
          next,
          `Review T-${number}`,
          body,
          task.requester_id,
          member.id,
          task.id,
          at,
          at,
        )
      const messageId = this.#queue(projectId, {
        to: member.id,
        from: task.requester_id,
        kind: 'task',
        taskId: reviewId,
        body,
      })
      this.#log(projectId, 'review.created', {
        task: number,
        review: next,
        reviewer: member.handle,
        round,
        message: messageId,
      })
      return { task: this.#task(reviewId), message: this.#message(messageId) }
    })
  }

  /**
   * The reviewer's answer ends its review task; the verdict on its last line
   * decides what happens to the work it reviewed.
   */
  recordVerdict(projectId, number, { body }) {
    requireText(body, 'body', MAX_BODY)
    return this.#write(() => {
      const review = this.#taskRow(projectId, number)
      this.#requireTaskState(review, ACTIVE_TASK_STATES, 'record a verdict for')
      if (review.kind !== 'review') {
        throw new LedgerError('not-a-review', `T-${number} is not a review`, 409)
      }
      const task = this.#taskById(review.review_of)
      const reviewer = this.#participantRow(review.assignee_id)
      const verdict = verdictOf(body)
      const round = task.round + 1
      this.#db
        .prepare('UPDATE task SET round = ?, updated_at = ? WHERE id = ?')
        .run(round, this.#at(), task.id)
      this.#db.prepare('UPDATE task SET verdict = ? WHERE id = ?').run(verdict, review.id)
      this.#log(projectId, 'review.verdict', { task: task.number, review: number, verdict, round })
      if (verdict === 'changes' && round < REVIEW_ROUNDS) {
        // Back to its author with the findings; the held result is superseded.
        this.#db
          .prepare(`UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state = 'held'`)
          .run(task.id)
        const messageId = this.#queue(projectId, {
          to: task.assignee_id,
          from: reviewer.id,
          kind: 'task',
          taskId: task.id,
          body: `Review round ${round} by @${reviewer.handle} asks for changes:\n\n${body}`,
        })
        this.#moveTask(review, 'done', {
          result: this.#findings(projectId, review, reviewer, body),
        })
        this.#moveTask({ ...task, round }, 'queued', { review: number, message: messageId })
      } else {
        // One delivery: the result goes on its way and the dispatcher writes
        // the verdicts under it. The findings stay on the review task, where
        // the board shows them under the work; nobody gets a second message.
        this.#releaseHeld(task.id)
        this.#moveTask(review, 'done', {
          result: this.#findings(
            projectId,
            review,
            reviewer,
            verdict === null ? `No VERDICT line; read as pass.\n\n${body}` : body,
          ),
        })
        this.#moveTask({ ...task, round }, 'done', { review: number, verdict })
      }
      return {
        verdict,
        task: this.#task(task.id),
        review: this.#task(review.id),
      }
    })
  }

  /** The reviewer's findings, kept on the review task for the board: read there, never delivered on their own. */
  #findings(projectId, review, reviewer, body) {
    return this.#queue(projectId, {
      to: review.requester_id,
      from: reviewer.id,
      kind: 'result',
      taskId: review.id,
      body,
      state: 'read',
    })
  }

  /** A task's reviews in order, each with its state, verdict and the findings the reviewer wrote. */
  reviewsOf(projectId, number) {
    const task = this.#taskRow(projectId, number)
    return this.#db
      .prepare(
        `SELECT t.number, t.state, t.verdict, p.handle AS reviewer,
                (SELECT body FROM message WHERE task_id = t.id AND kind = 'result' ORDER BY id DESC LIMIT 1) AS findings
         FROM task t JOIN participant p ON p.id = t.assignee_id
         WHERE t.review_of = ? ORDER BY t.number`,
      )
      .all(task.id)
      .map((row, index) => ({
        number: row.number,
        round: index + 1,
        reviewer: row.reviewer,
        state: row.state,
        verdict: row.verdict,
        findings: row.findings ?? null,
      }))
  }

  /** A review its reviewer cannot finish (its window closed, its quota ran out): the work waits for another. */
  withdrawReview(projectId, number, { reason }) {
    requireText(reason, 'reason', 1000)
    return this.#write(() => {
      const review = this.#taskRow(projectId, number)
      if (review.kind !== 'review') {
        throw new LedgerError('not-a-review', `T-${number} is not a review`, 409)
      }
      this.#requireTaskState(review, ['queued', ...ACTIVE_TASK_STATES], 'withdraw')
      this.#dropQueued(review.id)
      this.#moveTask(review, 'cancelled', { reason })
      return this.#task(review.id)
    })
  }

  /** A coordinator asks for a review of finished work by hand. */
  requestReview(projectId, number, { by }) {
    return this.#write(() => {
      const asker = this.#participantByHandle(projectId, by)
      if (!COORDINATOR_ROLES.includes(asker.role)) {
        throw new LedgerError(
          'not-a-coordinator',
          `@${by} is a ${asker.role}: only coordinators ask for reviews`,
          403,
        )
      }
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['done'], 'ask a review of')
      this.#moveTask(task, 'review', { by })
      return this.#task(task.id)
    })
  }

  /** No reviewer can take this one: the work is done as it is, and the requester is told. */
  skipReview(projectId, number, { reason }) {
    requireText(reason, 'reason', 1000)
    return this.#write(() => {
      const task = this.#taskRow(projectId, number)
      this.#requireTaskState(task, ['review'], 'skip the review of')
      if (!this.reviewsPending(projectId).some((pending) => pending.id === task.id)) {
        throw new LedgerError('invalid-transition', `T-${number} is being reviewed`, 409)
      }
      this.#releaseHeld(task.id)
      // The result says it as it goes (the dispatcher writes the reason under it).
      this.#db.prepare('UPDATE task SET unreviewed = ? WHERE id = ?').run(reason, task.id)
      this.#moveTask(task, 'done', { unreviewed: reason })
      return this.#task(task.id)
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
      this.#requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES, 'review'], 'cancel')
      this.#dropQueued(task.id)
      for (const review of this.#openReviews(task.id)) {
        this.#dropQueued(review.id)
        this.#moveTask(review, 'cancelled', { by, with: task.number })
      }
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
    const tasks = this.#db
      .prepare(`${TASK_SELECT} WHERE t.project_id = ? ORDER BY t.number`)
      .all(projectId)
      .map((row) => ({ ...taskView(row), result: firstLine(results.get(row.id)) }))
    return {
      project,
      open: tasks.filter((task) => task.state === 'open'),
      lanes: project.participants.map((participant) => ({
        participant,
        tasks: tasks.filter((task) => task.assignee === participant.handle),
      })),
      overdue: this.#overdueQuestions(projectId),
    }
  }

  /** Questions a coordinator has left unanswered for OVERDUE_MS: the human sees them too. */
  #overdueQuestions(projectId) {
    const before = new Date(this.#now().getTime() - OVERDUE_MS).toISOString()
    return this.#db
      .prepare(
        `${MESSAGE_SELECT}
         WHERE m.project_id = ? AND m.kind = 'question' AND r.role IN ('lead', 'pm')
           AND m.created_at <= ?
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
      reviews: row.kind === 'work' ? this.reviewsOf(projectId, number) : [],
    }
  }

  /** One message, or null. */
  message(id) {
    return this.#message(id)
  }

  /**
   * The task a participant is on: working, waiting on an answer, or queued
   * while its delivery is still being confirmed (a window may ask its first
   * question before the record that confirms the task's arrival is read).
   */
  activeTask(participantId) {
    const row = this.#db
      .prepare(
        `SELECT project_id, number FROM task
         WHERE assignee_id = ? AND state IN ('queued', 'working', 'waiting') ORDER BY id LIMIT 1`,
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

  #addParticipant(projectId, { handle, role, roles = [], agent, harness, tier = null, tags = [] }) {
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
        `INSERT INTO participant (project_id, handle, role, roles, agent, harness, tier, tags, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        projectId,
        handle,
        role,
        JSON.stringify(roles),
        agent,
        harness,
        tier,
        JSON.stringify(tags),
        this.#at(),
      )
    if (role !== 'human' && role !== 'lead') {
      this.#log(projectId, 'member.added', { handle, role, roles, harness })
    }
    return participantView(this.#participantRow(id))
  }

  /** The active members of one pool and tier, in join order. */
  #members(projectId, pool, tier) {
    return this.#db
      .prepare(
        `SELECT * FROM participant
         WHERE project_id = ? AND role = ? AND tier = ? AND left_at IS NULL ORDER BY id`,
      )
      .all(projectId, pool, tier)
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
      state = 'queued',
      questions = null,
      choices = null,
    },
  ) {
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
        state,
        questions === null ? null : JSON.stringify(questions),
        choices === null ? null : JSON.stringify(choices),
        this.#at(),
      )
    return id
  }

  /** A held result goes on its way once its review is over. */
  #releaseHeld(taskId) {
    this.#db
      .prepare(`UPDATE message SET state = 'queued' WHERE task_id = ? AND state = 'held'`)
      .run(taskId)
  }

  /** Whether a finished task waits for a review under the project's policy. */
  #reviewDue(projectId, task) {
    const policy = this.#projectRow(projectId).review
    if (policy === 'none') return false
    if (policy === 'all') return true
    return MEMBER_ROLES.includes(this.#participantRow(task.assignee_id).role)
  }

  #openReviews(taskId) {
    return this.#db
      .prepare(
        `SELECT * FROM task WHERE review_of = ? AND state IN (${REVIEW_STATES.map(() => '?').join(', ')})`,
      )
      .all(taskId, ...REVIEW_STATES)
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
        `UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state IN ('queued', 'held')`,
      )
      .run(taskId)
  }
}
