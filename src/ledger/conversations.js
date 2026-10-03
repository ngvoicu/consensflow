import { cut, LedgerError, requireAgentId, requireHarness, requireText } from './model.js'
import { project } from './projects.js'
import { conversationView, MESSAGE_SELECT, messageView, TASK_SELECT, taskView } from './views.js'

/**
 * Conversations: each participant's native conversations, one current at a
 * time and followed when the human switches its window to another; the copy
 * ConsensFlow keeps of each, item by item; and the chief's across a Switch
 * chief, with what a chief that takes over reads (its history, its open work).
 */

/**
 * The most of one tool's output that is copied: it can run to megabytes, and
 * the tool can be run again. Words are copied whole (the human's, an agent's,
 * ConsensFlow's): a chief switched in reads them in `cf history`.
 */
export const TRANSCRIPT_ITEM_MAX = 64_000
const TRANSCRIPT_ROLES = ['user', 'assistant', 'tool', 'custom']

/** A participant's new native conversation; the one before it ends. */
export function startConversation(store, participantId, { harness }) {
  requireHarness(harness)
  return store.write(() => {
    const participant = store.participantRow(participantId)
    const at = store.at()
    store.db
      .prepare('UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL')
      .run(at, participantId)
    const { lastInsertRowid: id } = store.db
      .prepare('INSERT INTO conversation (participant_id, harness, started_at) VALUES (?, ?, ?)')
      .run(participantId, harness, at)
    store.log(participant.project_id, 'conversation.started', {
      participant: participant.handle,
      conversation: id,
    })
    return conversationById(store, id)
  })
}

export function bindConversation(store, conversationId, nativeSession) {
  requireText(nativeSession, 'native session', 512)
  return store.write(() => {
    const conversation = conversationById(store, conversationId)
    if (conversation === null) {
      throw new LedgerError('unknown-conversation', `no conversation ${conversationId}`, 404)
    }
    const taken = store.db
      .prepare('SELECT id FROM conversation WHERE harness = ? AND native_session = ? AND id != ?')
      .get(conversation.harness, nativeSession, conversationId)
    if (taken !== undefined) {
      throw new LedgerError(
        'native-session-taken',
        `native session ${nativeSession} already belongs to conversation ${taken.id}`,
        409,
      )
    }
    store.db
      .prepare('UPDATE conversation SET native_session = ? WHERE id = ?')
      .run(nativeSession, conversationId)
    const participant = store.participantRow(conversation.participantId)
    store.log(participant.project_id, 'conversation.bound', {
      conversation: conversationId,
      nativeSession,
    })
    return conversationById(store, conversationId)
  })
}

/**
 * The copy of a window's conversation, one row per item as the harness's
 * own record has them: what is new is added, and an item still being
 * written is brought up to date. `from` is the position of the first item
 * given, so a caller may pass only the tail.
 */
export function copyTranscript(store, conversationId, items, { from = 0 } = {}) {
  if (!Array.isArray(items)) throw new LedgerError('invalid-items', 'items is a list')
  return store.write(() => {
    if (conversationById(store, conversationId) === null) {
      throw new LedgerError('unknown-conversation', `no conversation ${conversationId}`, 404)
    }
    const upsert = store.db.prepare(
      `INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (conversation_id, item_id) DO UPDATE
         SET seq = excluded.seq, text = excluded.text, complete = excluded.complete,
             at = excluded.at, copied_at = excluded.copied_at
         WHERE transcript.text != excluded.text OR transcript.complete != excluded.complete`,
    )
    const at = store.at()
    let written = 0
    for (const [index, item] of items.entries()) {
      if (typeof item?.id !== 'string' || item.id.length === 0) continue
      const text = typeof item.text === 'string' ? item.text : ''
      const role = TRANSCRIPT_ROLES.includes(item.role) ? item.role : 'custom'
      const { changes } = upsert.run(
        conversationId,
        item.id,
        from + index,
        role,
        role === 'tool' ? cut(text, TRANSCRIPT_ITEM_MAX) : text,
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
 * What the windows that had a task wrote: the copied items of each window
 * the task was given to, in the order it was (a reassigned task's first
 * window, then the one that took it), whole, the last `limit` of them (all
 * of them when no limit is given). `cf task get --transcript` reads it over
 * the local API; the page reads `latestTranscript`.
 */
export function transcript(store, projectId, number, { limit = Number.POSITIVE_INFINITY } = {}) {
  const task = store.taskRow(projectId, number)
  const windows = store.db
    .prepare(
      `SELECT recipient_id FROM message WHERE task_id = ? AND kind = 'task'
       GROUP BY recipient_id ORDER BY MIN(id)`,
    )
    .all(task.id)
    .map((row) => row.recipient_id)
  if (windows.length === 0 && task.assignee_id !== null) windows.push(task.assignee_id)
  const rows = windows.flatMap((participantId) => partOf(store, task.id, participantId))
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

/** The header every delivery opens with, naming its message. */
const HEADER = /\[ConsensFlow m-(\d+) /

/**
 * The part of a window's copy that is task `taskId`'s. A window may hold
 * more than one task (the chief's own after its other work, a follow-up
 * given with --after): each task message that arrived in it (a brief, a
 * resume, a reopen), known by its header, turns the window to that task
 * until the next one does. A copy that shows none of the task's headers is
 * the task's whole.
 */
function partOf(store, taskId, participantId) {
  const copied = store.db
    .prepare(
      `SELECT t.conversation_id, t.item_id, t.role, t.text, t.complete, t.at
       FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? ORDER BY t.conversation_id, t.seq`,
    )
    .all(participantId)
  const taskOf = new Map(
    store.db
      .prepare(
        `SELECT id, task_id FROM message
         WHERE recipient_id = ? AND kind = 'task' AND task_id IS NOT NULL`,
      )
      .all(participantId)
      .map((row) => [row.id, row.task_id]),
  )
  let current = null
  let shown = false
  const part = []
  for (const row of copied) {
    const header = row.role === 'user' ? HEADER.exec(row.text) : null
    const turned = header === null ? undefined : taskOf.get(Number(header[1]))
    if (turned !== undefined) {
      current = turned
      shown ||= turned === taskId
    }
    if (current === taskId) part.push(row)
  }
  return shown ? part : copied
}

/**
 * What the chief said and was told before its current conversation: every
 * earlier conversation of the chief, oldest first, with the harness it ran
 * on and its copied items in order. `cf history` pages it for a chief the
 * human switched in.
 */
export function chiefHistory(store, projectId) {
  const chief = store.participantByHandle(projectId, 'chief')
  const items = store.db.prepare(
    'SELECT item_id, role, text, complete, at FROM transcript WHERE conversation_id = ? ORDER BY seq',
  )
  return store.db
    .prepare(
      'SELECT * FROM conversation WHERE participant_id = ? AND ended_at IS NOT NULL ORDER BY id',
    )
    .all(chief.id)
    .map((row) => ({
      ...conversationView(row),
      items: items.all(row.id).map((item) => ({
        id: item.item_id,
        role: item.role,
        text: item.text,
        complete: item.complete === 1,
        at: item.at,
      })),
    }))
}

/**
 * What waits on the chief now, for a chief that takes over: members'
 * questions to it without an answer, results it has not decided on, and its
 * own unfinished tasks.
 */
export function chiefOpenWork(store, projectId) {
  const chief = store.participantByHandle(projectId, 'chief')
  return {
    questions: store.db
      .prepare(
        `${MESSAGE_SELECT}
         WHERE m.project_id = ? AND m.recipient_id = ? AND m.kind = 'question'
           AND m.state NOT IN ('gated', 'cancelled')
           AND NOT EXISTS (
             SELECT 1 FROM message a WHERE a.reply_to = m.id AND a.kind = 'answer'
               AND a.state NOT IN ('gated', 'cancelled')
           )
         ORDER BY m.id`,
      )
      .all(projectId, chief.id)
      .map(messageView),
    results: store.db
      .prepare(
        `${TASK_SELECT} WHERE t.project_id = ? AND t.requester_id = ? AND t.state = 'done'
         ORDER BY t.number`,
      )
      .all(projectId, chief.id)
      .map(taskView),
    own: store.db
      .prepare(
        `${TASK_SELECT} WHERE t.project_id = ? AND t.assignee_id = ?
           AND t.state IN ('queued', 'working', 'waiting', 'paused')
         ORDER BY t.number`,
      )
      .all(projectId, chief.id)
      .map(taskView),
  }
}

/**
 * The human's Switch chief: the chief runs on the saved `agent` (its model
 * and effort) on `harness` from now on, never on a harness's own default.
 * Its conversation ends here: a conversation belongs to one harness, and
 * every switch starts a fresh one that reads the history. What it was out
 * of quota for was the old harness's account, so that clears. The window is
 * the caller's to close before and open after. The chief keeps what it was
 * switched from, and `cut`, that the old chief was stopped in the middle
 * of a turn, for the handoff to say.
 */
export function switchChief(store, projectId, { harness, agent, cut = false }) {
  requireHarness(harness)
  requireAgentId(agent)
  return store.write(() => {
    const chief = store.participantByHandle(projectId, 'chief')
    store.db
      .prepare(
        `UPDATE participant SET harness = ?, agent = ?, out_until = NULL,
           switched_from_harness = ?, switched_from_agent = ?, switched_from_cut = ?
         WHERE id = ?`,
      )
      .run(harness, agent, chief.harness, chief.agent, cut === true ? 1 : 0, chief.id)
    store.db
      .prepare('UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL')
      .run(store.at(), chief.id)
    store.log(projectId, 'chief.switched', {
      from: { harness: chief.harness, agent: chief.agent },
      to: { harness, agent },
      cut: cut === true,
    })
    return project(store, projectId)
  })
}

/** The chief read its history (`cf history`): which page, or what it searched for. */
export function historyRead(store, projectId, { page, find = null, tools = false }) {
  return store.write(() => {
    store.projectRow(projectId)
    store.log(projectId, 'chief.history.read', { page, find, tools })
  })
}

/**
 * The project's latest Switch chief: what the chief was switched from
 * (its harness and agent) and whether its turn was cut; null before any.
 */
export function lastSwitch(store, projectId) {
  const chief = store.participantByHandle(projectId, 'chief')
  if (chief.switched_from_harness === null) return null
  return {
    from: { harness: chief.switched_from_harness, agent: chief.switched_from_agent },
    cut: chief.switched_from_cut === 1,
  }
}

export function endConversation(store, conversationId) {
  return store.write(() => {
    const conversation = conversationById(store, conversationId)
    if (conversation === null || conversation.endedAt !== null) return conversation
    store.db
      .prepare('UPDATE conversation SET ended_at = ? WHERE id = ?')
      .run(store.at(), conversationId)
    const participant = store.participantRow(conversation.participantId)
    store.log(participant.project_id, 'conversation.ended', { conversation: conversationId })
    return conversationById(store, conversationId)
  })
}

export function currentConversation(store, participantId) {
  return conversationView(
    store.db
      .prepare('SELECT * FROM conversation WHERE participant_id = ? AND ended_at IS NULL')
      .get(participantId),
  )
}

/**
 * The first item ConsensFlow's copy of the participant's current
 * conversation shows it was given (a user item) that contains `text`, or
 * null: a delivery's header there proves the delivery arrived.
 */
export function copiedItemWith(store, participantId, text) {
  const row = store.db
    .prepare(
      `SELECT t.item_id FROM transcript t JOIN conversation c ON c.id = t.conversation_id
       WHERE c.participant_id = ? AND c.ended_at IS NULL AND t.role = 'user'
         AND instr(t.text, ?) > 0
       ORDER BY t.seq LIMIT 1`,
    )
    .get(participantId, text)
  return row?.item_id ?? null
}

/**
 * A window the human switched to another conversation (/clear, /new,
 * /resume): the participant's conversation is the one on `nativeSession`
 * from now on, its own earlier one when it had it, else a new one bound to
 * it, and the one in progress ends. A session that another participant's
 * conversation holds stays with it: the new conversation is left unbound.
 */
export function followConversation(store, participantId, { harness, nativeSession }) {
  requireHarness(harness)
  requireText(nativeSession, 'native session', 512)
  return store.write(() => {
    const participant = store.participantRow(participantId)
    const held = store.db
      .prepare('SELECT * FROM conversation WHERE harness = ? AND native_session = ?')
      .get(harness, nativeSession)
    const own = held?.participant_id === participantId
    if (own && held.ended_at === null) return conversationView(held)
    const at = store.at()
    store.db
      .prepare('UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL')
      .run(at, participantId)
    let id = held?.id
    if (own) {
      store.db.prepare('UPDATE conversation SET ended_at = NULL WHERE id = ?').run(id)
    } else {
      id = store.db
        .prepare(
          `INSERT INTO conversation (participant_id, harness, native_session, started_at)
           VALUES (?, ?, ?, ?)`,
        )
        .run(participantId, harness, held === undefined ? nativeSession : null, at).lastInsertRowid
    }
    store.log(participant.project_id, 'conversation.followed', {
      participant: participant.handle,
      conversation: id,
      nativeSession,
    })
    return conversationById(store, id)
  })
}

function conversationById(store, id) {
  return conversationView(store.db.prepare('SELECT * FROM conversation WHERE id = ?').get(id))
}
