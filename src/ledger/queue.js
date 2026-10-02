import { MAX_BODY, requireText } from './model.js'

/**
 * A message on its way, and off it. `queue` writes one by participant id:
 * queued for its recipient, read at once when the door that asked collects
 * it, or held for the human when the project's gate holds it. `send` does the
 * same by handle, and logs it. A message that no longer applies is withdrawn
 * or dropped, and never delivered.
 */

export function queue(
  store,
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
    urgent = false,
  },
) {
  // A message on its way (queued, or collected: read at once by the door
  // that asked) waits for the human instead when the project gates it.
  const gated = gateHolds(store, projectId, from, to)
  const landing = gated ? 'gated' : collected ? 'read' : 'queued'
  const { lastInsertRowid: id } = store.db
    .prepare(
      `INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, reply_to, body,
                            state, questions, choices, urgent, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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
      urgent ? 1 : 0,
      store.at(),
    )
  return id
}

/**
 * Whether the project's gate holds a message: one agent's word to another,
 * when the human approves every hand-off. What the human sends or receives,
 * what ConsensFlow itself notes, and what an agent tells itself pass.
 */
function gateHolds(store, projectId, from, to) {
  if (from === null || from === to || store.projectRow(projectId).gate !== 1) return false
  return store.participantRow(from).role !== 'human' && store.participantRow(to).role !== 'human'
}

/** A gated message the human never passed on: declined, answered, or overtaken. */
export function withdraw(store, messageId, reason) {
  store.db
    .prepare(`UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?`)
    .run(reason, messageId)
}

export function withdrawGated(store, taskId, reason) {
  store.db
    .prepare(
      `UPDATE message SET state = 'cancelled', reason = ? WHERE task_id = ? AND state = 'gated'`,
    )
    .run(reason, taskId)
}

export function send(
  store,
  projectId,
  { from, to, body, task, kind, questions = null, urgent = false },
) {
  requireText(body, 'body', MAX_BODY)
  return store.write(() => {
    const sender = from === undefined ? null : store.participantByHandle(projectId, from)
    const recipient = store.participantByHandle(projectId, to)
    const taskId = task === undefined ? null : store.taskRow(projectId, task).id
    const id = queue(store, projectId, {
      to: recipient.id,
      from: sender?.id ?? null,
      kind,
      taskId,
      body,
      questions,
      urgent,
    })
    store.log(projectId, 'message.sent', {
      message: id,
      kind,
      from: sender?.handle ?? null,
      to: recipient.handle,
    })
    return store.message(id)
  })
}

/** A cancelled or failed task's queued messages are never delivered. */
export function dropQueued(store, taskId) {
  store.db
    .prepare(
      `UPDATE message SET state = 'cancelled' WHERE task_id = ? AND state IN ('queued', 'gated')`,
    )
    .run(taskId)
}
