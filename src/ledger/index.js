import { DatabaseSync } from 'node:sqlite'
import * as conversations from './conversations.js'
import * as messages from './messages.js'
import { LedgerError } from './model.js'
import { sessionName } from './names.js'
import * as pageReads from './page-reads.js'
import * as projects from './projects.js'
import { migrate } from './schema.js'
import * as staff from './staff.js'
import { Store } from './store.js'
import * as tasks from './tasks.js'

export { TRANSCRIPT_ITEM_MAX } from './conversations.js'
export { CHIEF_HARNESSES, HARNESSES, LedgerError, TIERS } from './model.js'
export { OVERDUE_MS, PAGE_BYTES } from './page-reads.js'
export { SCHEMA_VERSION } from './schema.js'
export { RESUME_WORDS } from './tasks.js'

/**
 * The ledger: the one durable record of every project, participant, task and
 * inbox message, in `<home>/consensflow.db`. The board and every inbox are
 * views of it.
 *
 * Rules, each with a test in `tests/ledger*.test.mjs`:
 * - The daemon is the only process that opens the file. The connection runs in
 *   SQLite's exclusive locking mode and takes the write lock when it opens, so
 *   the lock IS the instance lock: a second opener, in this process or another,
 *   is refused with `ledger-locked`, and the operating system releases the lock
 *   when the holder closes or dies. This works the same on macOS and Windows.
 * - Every operation is one transaction: it validates, writes and logs, or it
 *   throws and writes nothing. A process killed mid-write leaves the last
 *   committed state.
 * - No id is given twice: a deleted project's ids (its own, its
 *   participants', conversations', tasks', messages' and events') go with
 *   it, and whatever still holds one finds nothing by it.
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
 * - A member who leaves the staff keeps its history but gets nothing new: its
 *   open tasks are cancelled, its undelivered messages and its unread
 *   questions too, and it is refused as a recipient (`member-left`) until it
 *   rejoins. The coordinators whose windows already run are told when the
 *   staff changes; a window that has not started reads the staff at launch.
 * - Task states move only along the state machine below; anything else is
 *   refused with `invalid-transition`.
 *
 *   open ──assigned──▶ queued ──delivered──▶ working ──question──▶ waiting ──answer delivered──▶ working
 *   queued, working, waiting, paused ──released──▶ open
 *   open, queued, working, waiting ──pause──▶ paused ──resume──▶ open, queued
 *   working, waiting ──result──▶ done ──accept──▶ accepted
 *   done, failed ──reopen──▶ queued
 *   open, queued, working, waiting, paused ──cancel──▶ cancelled
 *   open, queued, working, waiting ──fail──▶ failed
 *
 * - Nothing of a cancelled task is delivered or tried again: what of it is
 *   still on its way, a delivery into a window included, is withdrawn. Its
 *   requester hears of the cancel in the same step, unless it made it.
 * - A review is a task like any other: the chief puts it on the board for a
 *   reviewer of a tier, and the reviewer's findings come back as its result.
 *
 * This module never reads `process.env` and never logs: the file and the
 * clock are arguments, and every refusal is a `LedgerError` with a stable code.
 */

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

/**
 * The ledger as its callers hold it. Each operation is its concern's, in
 * projects.js, staff.js, conversations.js, tasks.js, messages.js or
 * page-reads.js, and runs on this ledger's store, which no caller reaches.
 */
class Ledger {
  #store

  constructor(db, now, names, trace) {
    this.#store = new Store(db, now, names, trace)
  }

  close() {
    this.#store.close()
  }

  integrity() {
    return this.#store.integrity()
  }

  // --- projects and participants ---------------------------------------------

  createProject(request) {
    return projects.createProject(this.#store, request)
  }
  project(id) {
    return projects.project(this.#store, id)
  }
  projects() {
    return projects.projects(this.#store)
  }
  setProjectState(id, state) {
    return projects.setProjectState(this.#store, id, state)
  }
  deleteProject(id) {
    return projects.deleteProject(this.#store, id)
  }
  setGate(id, gate) {
    return projects.setGate(this.#store, id, gate)
  }
  suspendForRestart() {
    return projects.suspendForRestart(this.#store)
  }
  forgetResume(id) {
    return projects.forgetResume(this.#store, id)
  }
  addMember(projectId, request) {
    return staff.addMember(this.#store, projectId, request)
  }
  refreshMemberTiers(tierOf) {
    return staff.refreshMemberTiers(this.#store, tierOf)
  }
  setRoles(projectId, handle, roles) {
    return staff.setRoles(this.#store, projectId, handle, roles)
  }
  removeMember(projectId, handle) {
    return staff.removeMember(this.#store, projectId, handle)
  }
  lastStaff() {
    return staff.lastStaff(this.#store)
  }

  // --- conversations -----------------------------------------------------------

  startConversation(participantId, request) {
    return conversations.startConversation(this.#store, participantId, request)
  }
  bindConversation(conversationId, nativeSession) {
    return conversations.bindConversation(this.#store, conversationId, nativeSession)
  }
  copyTranscript(conversationId, items, options = {}) {
    return conversations.copyTranscript(this.#store, conversationId, items, options)
  }
  transcript(projectId, number, options = {}) {
    return conversations.transcript(this.#store, projectId, number, options)
  }
  latestTranscript(projectId, number, options = {}) {
    return pageReads.latestTranscript(this.#store, projectId, number, options)
  }
  leadHistory(projectId) {
    return conversations.leadHistory(this.#store, projectId)
  }
  leadOpenWork(projectId) {
    return conversations.leadOpenWork(this.#store, projectId)
  }
  switchChief(projectId, request) {
    return conversations.switchChief(this.#store, projectId, request)
  }
  historyRead(projectId, request) {
    return conversations.historyRead(this.#store, projectId, request)
  }
  lastSwitch(projectId) {
    return conversations.lastSwitch(this.#store, projectId)
  }
  endConversation(conversationId) {
    return conversations.endConversation(this.#store, conversationId)
  }
  currentConversation(participantId) {
    return conversations.currentConversation(this.#store, participantId)
  }

  // --- tasks and messages ------------------------------------------------------

  createTask(projectId, request) {
    return tasks.createTask(this.#store, projectId, request)
  }
  holdsWork(participantId) {
    return staff.holdsWork(this.#store, participantId)
  }
  candidates(projectId, number) {
    return staff.candidates(this.#store, projectId, number)
  }
  members(projectId, role) {
    return staff.members(this.#store, projectId, role)
  }
  endSession(projectId, handle, request) {
    return staff.endSession(this.#store, projectId, handle, request)
  }
  assignTask(projectId, number, participantId) {
    return tasks.assignTask(this.#store, projectId, number, participantId)
  }
  markOut(participantId, request) {
    return staff.markOut(this.#store, participantId, request)
  }
  releaseTask(projectId, number, request) {
    return tasks.releaseTask(this.#store, projectId, number, request)
  }
  note(projectId, request) {
    return messages.note(this.#store, projectId, request)
  }
  ask(projectId, request) {
    return messages.ask(this.#store, projectId, request)
  }
  answer(questionId, request) {
    return messages.answer(this.#store, questionId, request)
  }
  answerTo(questionId) {
    return messages.answerTo(this.#store, questionId)
  }
  nextDelivery(participantId) {
    return messages.nextDelivery(this.#store, participantId)
  }
  beginDelivery(messageId) {
    return messages.beginDelivery(this.#store, messageId)
  }
  confirmDelivery(messageId, receipt) {
    return messages.confirmDelivery(this.#store, messageId, receipt)
  }
  cancelMessage(messageId, reason) {
    return messages.cancelMessage(this.#store, messageId, reason)
  }
  retryDelivery(messageId, reason, options = {}) {
    return messages.retryDelivery(this.#store, messageId, reason, options)
  }
  failDelivery(messageId, reason) {
    return messages.failDelivery(this.#store, messageId, reason)
  }
  inFlight() {
    return messages.inFlight(this.#store)
  }
  copiedItemWith(participantId, text) {
    return conversations.copiedItemWith(this.#store, participantId, text)
  }
  pending(participantId) {
    return messages.pending(this.#store, participantId)
  }
  followConversation(participantId, request) {
    return conversations.followConversation(this.#store, participantId, request)
  }
  markRead(messageId) {
    return messages.markRead(this.#store, messageId)
  }
  approveMessage(messageId, request) {
    return messages.approveMessage(this.#store, messageId, request)
  }
  declineMessage(messageId, request) {
    return messages.declineMessage(this.#store, messageId, request)
  }
  recordResult(projectId, number, request) {
    return tasks.recordResult(this.#store, projectId, number, request)
  }
  acceptTask(projectId, number, request) {
    return tasks.acceptTask(this.#store, projectId, number, request)
  }
  pauseTask(projectId, number, options = {}) {
    return tasks.pauseTask(this.#store, projectId, number, options)
  }
  holdTask(projectId, number, request) {
    return tasks.holdTask(this.#store, projectId, number, request)
  }
  heldTasksDue(nowIso) {
    return tasks.heldTasksDue(this.#store, nowIso)
  }
  pausedTask(participantId) {
    return tasks.pausedTask(this.#store, participantId)
  }
  toldSincePaused(participantId, taskId) {
    return tasks.toldSincePaused(this.#store, participantId, taskId)
  }
  resumeTask(projectId, number, request) {
    return tasks.resumeTask(this.#store, projectId, number, request)
  }
  reopenTask(projectId, number, request) {
    return tasks.reopenTask(this.#store, projectId, number, request)
  }
  cancelTask(projectId, number, request) {
    return tasks.cancelTask(this.#store, projectId, number, request)
  }
  failTask(projectId, number, request) {
    return tasks.failTask(this.#store, projectId, number, request)
  }

  // --- views -------------------------------------------------------------------

  board(projectId) {
    return pageReads.board(this.#store, projectId)
  }
  task(projectId, number) {
    return tasks.task(this.#store, projectId, number)
  }
  taskThatFits(projectId, number) {
    return pageReads.taskThatFits(this.#store, projectId, number)
  }
  message(id) {
    return this.#store.message(id)
  }
  activeTask(participantId, options = {}) {
    return tasks.activeTask(this.#store, participantId, options)
  }
  lastTask(participantId) {
    return tasks.lastTask(this.#store, participantId)
  }
  inbox(participantId, options = {}) {
    return messages.inbox(this.#store, participantId, options)
  }
  latestMessages(participantId, options = {}) {
    return pageReads.latestMessages(this.#store, participantId, options)
  }
  events(projectId, options = {}) {
    return projects.events(this.#store, projectId, options)
  }
}
