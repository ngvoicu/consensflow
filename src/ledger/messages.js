import {
  HELD_TASK_STATES,
  LedgerError,
  MAX_BODY,
  MEMBER_ROLES,
  requireActive,
  requireText,
} from './model.js'
import { renderChoices, renderQuestions, requireChoices, requireQuestions } from './questions.js'
import { queue, send, withdraw } from './queue.js'
import { callOff, pauseTask } from './tasks.js'
import { MESSAGE_SELECT, messageView } from './views.js'

/**
 * Messages: notes, questions and their answers, delivered into a window one
 * at a time and oldest first, except the human's, which wait in the app
 * until read; and the human's gate, which holds one agent's word to another
 * until the human passes it on or declines it.
 */

export function note(store, projectId, { from, to, body, task }) {
  return send(store, projectId, { from, to, body, task, kind: 'note' })
}

/**
 * A question for a coordinator; the asker's task waits for the answer. The
 * human is never asked on the board: the chief asks them in its own terminal,
 * where they work with it. With `questions`, the question carries options as a harness's own
 * question tool asked them, and its text is rendered from them. An `urgent`
 * question is the chief's `cf tell` to a task's window: the task is paused
 * for it, and the question says so.
 */
export function ask(store, projectId, { from, to, body, task, questions, urgent = false }) {
  if (from === undefined) {
    throw new LedgerError('unknown-participant', 'a question names who asks it', 400)
  }
  const options = questions === undefined ? null : requireQuestions(questions)
  return store.write(() => {
    if (store.participantByHandle(projectId, to).role === 'human') {
      throw new LedgerError(
        'ask-in-your-terminal',
        "the human is asked in the chief's terminal, not on the board",
        403,
      )
    }
    // The pause first, as one step with the tell: what it withdraws from
    // the window is never the tell, and a tell refused below pauses nothing.
    if (urgent && store.taskRow(projectId, task).state !== 'paused') {
      pauseTask(store, projectId, task, { by: from })
    }
    const message = send(store, projectId, {
      from,
      to,
      body: options === null ? body : renderQuestions(options),
      task,
      kind: 'question',
      questions: options,
      urgent,
    })
    if (task !== undefined) {
      const row = store.taskRow(projectId, task)
      const asker = store.participantByHandle(projectId, from)
      if (row.assignee_id === asker.id && row.state === 'working') {
        store.moveTask(row, 'waiting')
      }
    }
    return message
  })
}

/**
 * The answer goes back to whoever asked; the one asked answers. A plain question's answer is delivered into the asker's window. A
 * question with options is answered by choice (or by text, one line per
 * question): that answer is read at once and never delivered, because the
 * harness door that asked collects it and the tool call completes with it;
 * the asker's task resumes here. A question on a cancelled task takes no
 * answer, from either side: nobody waits for it. `from` is the answerer's
 * participant id: handles repeat across projects (every chief is `chief`),
 * ids never do.
 */
export function answer(store, questionId, { from, body, choices }) {
  return store.write(() => {
    const question = store.message(questionId)
    if (question === null || question.kind !== 'question') {
      throw new LedgerError('not-a-question', `message ${questionId} is not a question`, 409)
    }
    const answerer = store.participantRow(from)
    const asker = store.db
      .prepare('SELECT sender_id, task_id FROM message WHERE id = ?')
      .get(questionId)
    // The one asked, or (a question with options) the asker itself: its
    // window may have answered first, and the board's copy takes that answer.
    const fromWindow = question.questions !== null && answerer.id === asker.sender_id
    if (answerer.id !== question.recipientId && !fromWindow) {
      throw new LedgerError(
        'not-your-question',
        `the question was put to ${question.recipient}, not ${answerer.handle}`,
        403,
      )
    }
    const task = messageTask(store, questionId)
    if (task?.state === 'cancelled') {
      throw new LedgerError(
        'task-cancelled',
        `T-${task.number} is cancelled: nobody waits for this answer`,
        409,
      )
    }
    if (answered(store, questionId)) {
      throw new LedgerError('already-answered', `m-${questionId} has its answer`, 409)
    }
    const picks =
      question.questions === null ? null : requireChoices(question.questions, { choices, body })
    const text = picks === null ? body : renderChoices(question.questions, picks)
    requireText(text, 'body', MAX_BODY)
    requireActive(answerer)
    requireActive(store.participantRow(asker.sender_id))
    const id = queue(store, question.projectId, {
      to: asker.sender_id,
      from: answerer.id,
      kind: 'answer',
      taskId: asker.task_id,
      replyTo: questionId,
      body: text,
      ...(picks === null ? {} : { collected: true, choices: picks }),
    })
    store.log(question.projectId, 'message.sent', {
      message: id,
      kind: 'answer',
      from: answerer.handle,
      to: question.sender,
    })
    // A question still gated is answered before the one asked saw it: it goes no further.
    if (question.state === 'gated') withdraw(store, questionId, `answered by @${answerer.handle}`)
    const answer = store.message(id)
    if (answer.state === 'read') resume(store, asker.task_id)
    return answer
  })
}

/** A choice answer is read by the door that asked, at once: the asker's task goes on. */
function resume(store, taskId) {
  if (taskId === null) return
  const task = store.db.prepare('SELECT * FROM task WHERE id = ?').get(taskId)
  if (task.state === 'waiting') store.moveTask(task, 'working')
}

/** Whether a question on the task still waits for its answer. */
function unanswered(store, taskId) {
  return (
    store.db
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
function answered(store, questionId) {
  return (
    store.db
      .prepare(
        `SELECT 1 FROM message WHERE reply_to = ? AND kind = 'answer' AND state != 'cancelled'`,
      )
      .get(questionId) !== undefined
  )
}

/** The answer to a question, or null while it waits: for the one asked, or for the human's approval. */
export function answerTo(store, questionId) {
  const row = store.db
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
export function nextDelivery(store, participantId) {
  const participant = store.participantRow(participantId)
  if (participant.role === 'human') return null
  const busy = store.db
    .prepare(`SELECT 1 FROM message WHERE recipient_id = ? AND state = 'delivering'`)
    .get(participantId)
  if (busy !== undefined) return null
  const serial = MEMBER_ROLES.includes(participant.role)
  const row = store.db
    .prepare(
      `SELECT id FROM message
       WHERE recipient_id = ? AND state = 'queued'
         AND (kind != 'task' OR ? = 0 OR NOT EXISTS (
           SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')
         ))
         AND (? = 0 OR kind = 'task' OR urgent = 1 OR task_id IN (
           SELECT id FROM task WHERE assignee_id = ? AND state IN (${HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')})
         ))
       ORDER BY id LIMIT 1`,
    )
    .get(participantId, serial ? 1 : 0, participantId, serial ? 1 : 0, participantId)
  return row === undefined ? null : store.message(row.id)
}

export function beginDelivery(store, messageId) {
  return store.write(() => {
    const message = requireMessage(store, messageId, 'queued')
    if (message.recipientRole === 'human') {
      throw new LedgerError('human-reads-in-app', 'the human reads messages in the app', 409)
    }
    const busy = store.db
      .prepare(`SELECT id FROM message WHERE recipient_id = ? AND state = 'delivering'`)
      .get(message.recipientId)
    const working =
      message.kind === 'task' &&
      MEMBER_ROLES.includes(message.recipientRole) &&
      store.db
        .prepare(`SELECT 1 FROM task WHERE assignee_id = ? AND state IN ('working', 'waiting')`)
        .get(message.recipientId) !== undefined
    if (busy !== undefined || working) {
      throw new LedgerError(
        'recipient-busy',
        `${message.recipient} is still receiving or working; message ${messageId} waits`,
        409,
      )
    }
    store.db
      .prepare(
        `UPDATE message SET state = 'delivering', attempts = attempts + 1, reason = NULL
         WHERE id = ?`,
      )
      .run(messageId)
    store.log(message.projectId, 'delivery.begun', {
      message: messageId,
      attempt: message.attempts + 1,
    })
    return store.message(messageId)
  })
}

/** The harness's own record proves the message arrived. */
export function confirmDelivery(store, messageId, receipt) {
  return store.write(() => {
    const message = requireMessage(store, messageId, 'delivering')
    store.db
      .prepare(`UPDATE message SET state = 'delivered', delivered_at = ?, receipt = ? WHERE id = ?`)
      .run(store.at(), JSON.stringify(receipt ?? null), messageId)
    store.log(message.projectId, 'delivery.confirmed', { message: messageId })
    const task = messageTask(store, messageId)
    if (task !== undefined) {
      // A task whose window already asked a question arrives waiting, not working.
      if (message.kind === 'task' && task.state === 'queued') {
        store.moveTask(task, unanswered(store, task.id) ? 'waiting' : 'working')
      }
      if (message.kind === 'answer' && task.state === 'waiting') store.moveTask(task, 'working')
    }
    return store.message(messageId)
  })
}

/** A message not yet delivered that no longer applies: it will not be delivered. */
export function cancelMessage(store, messageId, reason) {
  requireText(reason, 'reason', 1000)
  return store.write(() => {
    const message = store.message(messageId)
    if (message === null || !['queued', 'delivering'].includes(message.state)) {
      throw new LedgerError(
        'not-pending',
        `message ${messageId} is not waiting to be delivered`,
        409,
      )
    }
    store.db
      .prepare(`UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?`)
      .run(reason, messageId)
    store.log(message.projectId, 'delivery.cancelled', { message: messageId, reason })
    return store.message(messageId)
  })
}

/** Queued again; `refund` gives back the attempt when the window went before it could land. */
export function retryDelivery(store, messageId, reason, { refund = false } = {}) {
  requireText(reason, 'reason', 1000)
  return store.write(() => {
    const message = requireMessage(store, messageId, 'delivering')
    store.db
      .prepare(
        `UPDATE message SET state = 'queued', reason = ?, attempts = MAX(attempts - ?, 0)
         WHERE id = ?`,
      )
      .run(reason, refund ? 1 : 0, messageId)
    store.log(message.projectId, 'delivery.retried', { message: messageId, reason })
    return store.message(messageId)
  })
}

export function failDelivery(store, messageId, reason) {
  requireText(reason, 'reason', 1000)
  return store.write(() => {
    const message = requireMessage(store, messageId, 'delivering')
    store.db
      .prepare(`UPDATE message SET state = 'failed', reason = ? WHERE id = ?`)
      .run(reason, messageId)
    store.log(message.projectId, 'delivery.failed', { message: messageId, reason })
    const task = messageTask(store, messageId)
    if (message.kind === 'task' && task !== undefined && task.state === 'queued') {
      store.moveTask(task, 'failed')
    }
    return store.message(messageId)
  })
}

/**
 * Every message on its way to a window, oldest first. At start none of
 * those windows is left (they died with the previous process), so each is
 * settled before anything else is delivered.
 */
export function inFlight(store) {
  return store.db
    .prepare(`${MESSAGE_SELECT} WHERE m.state = 'delivering' ORDER BY m.id`)
    .all()
    .map(messageView)
}

/** What is on its way to a participant (queued, or being delivered), oldest first. */
export function pending(store, participantId) {
  return store.db
    .prepare(
      `${MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state IN ('queued', 'delivering')
       ORDER BY m.id`,
    )
    .all(participantId)
    .map(messageView)
}

/** The human read a message in the app. */
export function markRead(store, messageId) {
  return store.write(() => {
    const message = store.message(messageId)
    if (message === null) throw new LedgerError('unknown-message', `no message ${messageId}`, 404)
    if (message.recipientRole !== 'human') {
      throw new LedgerError('not-for-the-human', `message ${messageId} is delivered to a pane`, 409)
    }
    if (message.state === 'read') return message
    requireMessage(store, messageId, 'queued')
    store.db
      .prepare(`UPDATE message SET state = 'read', delivered_at = ? WHERE id = ?`)
      .run(store.at(), messageId)
    store.log(message.projectId, 'message.read', { message: messageId })
    return store.message(messageId)
  })
}

/** The human passes a gated message on: it goes the way it would have gone without the gate. */
export function approveMessage(store, messageId, { by }) {
  return store.write(() => {
    const message = requireMessage(store, messageId, 'gated')
    store.participantByHandle(message.projectId, by)
    const landing = message.choices === null ? 'queued' : 'read'
    store.db.prepare('UPDATE message SET state = ? WHERE id = ?').run(landing, messageId)
    store.log(message.projectId, 'message.approved', { message: messageId, by })
    if (landing === 'read') resume(store, messageTask(store, messageId)?.id ?? null)
    return store.message(messageId)
  })
}

/**
 * The human declines a gated message, and whoever sent it is told. A
 * declined task is cancelled; a declined answer leaves its question open
 * for another. A result or a question is passed on, never declined.
 */
export function declineMessage(store, messageId, { by }) {
  return store.write(() => {
    const message = requireMessage(store, messageId, 'gated')
    store.participantByHandle(message.projectId, by)
    if (message.kind !== 'task' && message.kind !== 'answer') {
      throw new LedgerError('not-declinable', `a ${message.kind} is passed on, not declined`, 409)
    }
    withdraw(store, messageId, `declined by @${by}`)
    store.log(message.projectId, 'message.declined', { message: messageId, by })
    const task = messageTask(store, messageId)
    let told = message.sender
    let word = `@${by} declined your answer to m-${message.replyTo}. Answer it again: cf answer m-${message.replyTo} "…"`
    if (message.kind === 'task') {
      told = store.participantRow(task.requester_id).handle
      callOff(store, message.projectId, task.number, by)
      word = `@${by} declined T-${task.number} (${task.title}). It is cancelled.`
    }
    if (told !== by) {
      send(store, message.projectId, {
        from: by,
        to: told,
        task: task?.number,
        body: word,
        kind: 'note',
      })
    }
    return store.message(messageId)
  })
}

/**
 * A participant's messages, newest first, whole: what reached it or is on
 * its way, never what still waits for the human. `cf inbox` lists them
 * over the local API; the page reads `latestMessages`.
 */
export function inbox(store, participantId, { limit = 100 } = {}) {
  return store.db
    .prepare(
      `${MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state != 'gated' ORDER BY m.id DESC LIMIT ?`,
    )
    .all(participantId, limit)
    .map(messageView)
}

function requireMessage(store, id, state) {
  const message = store.message(id)
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

function messageTask(store, messageId) {
  return store.db
    .prepare('SELECT t.* FROM task t JOIN message m ON m.task_id = t.id WHERE m.id = ?')
    .get(messageId)
}
