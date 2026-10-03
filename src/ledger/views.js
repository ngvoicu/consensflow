/**
 * How the ledger's rows read: the SELECTs that join a row to the handles it
 * refers to, and the objects every operation hands back.
 */

export const MESSAGE_SELECT = `
  SELECT m.*, r.handle AS recipient, r.role AS recipient_role, s.handle AS sender,
         t.number AS task_number
  FROM message m
  JOIN participant r ON r.id = m.recipient_id
  LEFT JOIN participant s ON s.id = m.sender_id
  LEFT JOIN task t ON t.id = m.task_id`

export const TASK_SELECT = `
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
export const PARTICIPANT_SELECT = `
  SELECT p.*, m.handle AS member_handle
  FROM participant p
  LEFT JOIN participant m ON m.id = p.member_id`

export const participantView = (row) => ({
  id: row.id,
  projectId: row.project_id,
  handle: row.handle,
  role: row.role,
  agent: row.agent,
  harness: row.harness,
  designer: row.designer === 1,
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

export const conversationView = (row) =>
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

export const taskView = (row) => ({
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
  heldUntil: row.held_until ?? null,
  pausedAt: row.paused_at ?? null,
  deletedAt: row.deleted_at,
  createdAt: row.created_at,
  updatedAt: row.updated_at,
})

export const messageView = (row) => ({
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
  urgent: row.urgent === 1,
  createdAt: row.created_at,
  deliveredAt: row.delivered_at,
})
