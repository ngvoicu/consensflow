import {
  ACTIVE_TASK_STATES,
  COORDINATOR_ROLES,
  LedgerError,
  MAX_BODY,
  POOLS,
  PURPOSES,
  requireActive,
  requireNumbers,
  requireText,
  requireTier,
  titleOf,
} from './model.js'
import { dropQueued, queue, send, withdrawGated } from './queue.js'
import {
  continuableSession,
  membersOfTier,
  nearestTier,
  requireMemberRow,
  startSession,
} from './staff.js'
import { MESSAGE_SELECT, messageView, TASK_SELECT, taskView } from './views.js'

/**
 * Tasks, along the state machine in index.js: given by a coordinator to a
 * participant by name, or opened for a pool and tier of the staff and
 * assigned by the daemon; finished with a result and accepted, or paused,
 * held, resumed, reopened, released, cancelled or failed; and the plan their
 * needs make on the board.
 */

/** When a task's tier moved to one the staff holds, the tier that was asked. */
const moved = (asked, tier) => (asked !== undefined && asked !== tier ? { asked } : {})
/** "standard worker", "image designer": who an open task waits for; `aPool` adds the article. */
const poolName = (pool, tier) => (pool === 'designer' ? 'image designer' : `${tier} ${pool}`)
const aPool = (pool, tier) => `${pool === 'designer' ? 'an' : 'a'} ${poolName(pool, tier)}`
const CRITICAL_RULE =
  'No coding or implementation edits. Do not write or revise specifications. Return analysis, evidence and recommendations to your coordinator.'
/** What a paused task's window is told when it goes on: the human's Resume and the daemon's alike. */
export const RESUME_WORDS = 'Go on where you stopped.'

/** How a task reads when it is handed over: critical work leads with its purpose. */
const deliveryBody = (task) =>
  task.purpose === null
    ? task.body
    : `Critical work: ${task.purpose}. ${CRITICAL_RULE}\n\n${task.body}`

/**
 * A task, from the chief or the human. Given `to`, it is queued for that
 * participant at once (the chief, the human, or the requester itself). Given
 * a `pool` and `tier` instead, it opens for the daemon to assign to a member
 * of that pool and tier (work for a worker, advice from an advisor; an image
 * from a designer, which has no tier), and critical work names its `purpose`.
 * A task on the board may `needs` other tasks: it waits until each is
 * accepted. With `before`, tasks still on the board wait for this one.
 */
export function createTask(
  store,
  projectId,
  { from, to, after, pool, tier, purpose, body, needs = [], before = [] },
) {
  requireText(body, 'body', MAX_BODY)
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
  return store.write(() => {
    const requester = store.participantByHandle(projectId, from)
    if (!COORDINATOR_ROLES.includes(requester.role)) {
      throw new LedgerError(
        'not-a-coordinator',
        `@${requester.handle} is a ${requester.role}: members do not hand out tasks`,
        403,
      )
    }
    // Advice is the chief's alone to ask: the human gives the chief work, not its advisors.
    if (
      pool === 'advisor' &&
      to === undefined &&
      after === undefined &&
      requester.role !== 'chief'
    ) {
      throw new LedgerError('advice-for-the-chief', 'only the chief asks an advisor', 403)
    }
    // A follow-up on a finished task goes to the session that did it, while
    // it is still there and free: the one case a coordinator names a window.
    const assignee =
      after !== undefined
        ? continuableSession(store, projectId, after)
        : to === undefined
          ? null
          : store.participantByHandle(projectId, to)
    // A tier nobody on the staff holds goes to the nearest one somebody does,
    // the next one up first: light work with only critical members still goes.
    const asked = tier
    if (assignee === null) tier = nearestTier(store, projectId, pool, tier)
    if (assignee === null && membersOfTier(store, projectId, pool, tier).length === 0) {
      throw new LedgerError(
        'no-member-of-tier',
        `no ${pool === 'designer' ? 'image designer' : pool} is on the staff: ask the human for one, in your terminal`,
        409,
      )
    }
    // A task given by name waits on the board too while what it needs is
    // not yet accepted; it goes to its window then.
    const blocked =
      assignee !== null &&
      needed.some((number) => store.taskRow(projectId, number).state !== 'accepted')
    const { next } = store.db
      .prepare('SELECT COALESCE(MAX(number), 0) + 1 AS next FROM task WHERE project_id = ?')
      .get(projectId)
    const at = store.at()
    const { lastInsertRowid: taskId } = store.db
      .prepare(
        `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state,
                           pool, tier, purpose, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        projectId,
        next,
        titleOf(body),
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
      const need = store.taskRow(projectId, number)
      if (need.state === 'cancelled') {
        throw new LedgerError(
          'need-cancelled',
          `T-${number} is cancelled: nothing waits for it`,
          409,
        )
      }
      store.db
        .prepare('INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)')
        .run(taskId, need.id)
    }
    for (const number of blocking) {
      const waits = store.taskRow(projectId, number)
      if (waits.state !== 'open') {
        throw new LedgerError(
          'not-on-the-board',
          `T-${number} is ${waits.state}: only a task still on the board can wait for a new one`,
          409,
        )
      }
      // A plan has no circles: what the new task waits for, near or far, cannot wait for it.
      if (upstream(store, taskId, waits.id)) {
        throw new LedgerError(
          'circular-needs',
          `T-${number} is already what T-${next} waits for: a plan has no circles`,
          409,
        )
      }
      store.db
        .prepare('INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)')
        .run(waits.id, taskId)
    }
    if (assignee === null || blocked) {
      store.log(projectId, 'task.opened', {
        task: next,
        from: requester.handle,
        ...(assignee === null ? { pool, tier } : { to: assignee.handle }),
        ...(needed.length === 0 ? {} : { needs: needed }),
        ...(blocking.length === 0 ? {} : { before: blocking }),
      })
      return { task: taskById(store, taskId), message: null, ...moved(asked, tier) }
    }
    const messageId = queue(store, projectId, {
      to: assignee.id,
      from: requester.id,
      kind: 'task',
      taskId,
      body: deliveryBody(taskRowById(store, taskId)),
    })
    store.log(projectId, 'task.created', {
      task: next,
      from: requester.handle,
      to: assignee.handle,
      message: messageId,
    })
    return {
      task: taskById(store, taskId),
      message: store.message(messageId),
      ...moved(asked, tier),
    }
  })
}

/** Whether `taskId` needs `otherId`, directly or through the tasks it needs. */
function upstream(store, taskId, otherId) {
  return (
    store.db
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

/**
 * The daemon's choice for an open task: a new session of that member,
 * which the task is queued for from here on.
 */
export function assignTask(store, projectId, number, participantId) {
  return store.write(() => {
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['open'], 'assign')
    const member = requireMemberRow(store, participantId, 'takes a task')
    const candidate =
      member.left_at === null &&
      member.project_id === projectId &&
      JSON.parse(member.roles).includes(task.pool) &&
      (task.tier === null || member.tier === task.tier)
    if (!candidate) {
      throw new LedgerError(
        'not-a-candidate',
        `@${member.handle} is not ${aPool(task.pool, task.tier)} on this staff`,
        409,
      )
    }
    const session = startSession(store, projectId, member, task.pool)
    store.db
      .prepare('UPDATE task SET assignee_id = ?, updated_at = ? WHERE id = ?')
      .run(session.id, store.at(), task.id)
    const messageId = queue(store, projectId, {
      to: session.id,
      from: task.requester_id,
      kind: 'task',
      taskId: task.id,
      body: deliveryBody(taskRowById(store, task.id)),
    })
    store.moveTask(
      task,
      'queued',
      { assignee: session.handle, member: member.handle, message: messageId },
      'task.assigned',
    )
    return { task: taskById(store, task.id), message: store.message(messageId) }
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
export function releaseTask(store, projectId, number, { because }) {
  requireText(because, 'because', 1000)
  return store.write(() => {
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['queued', ...ACTIVE_TASK_STATES, 'paused'], 'release')
    if (task.pool === null) {
      throw new LedgerError(
        'invalid-transition',
        `cannot release T-${number}: it was given by name, not by tier`,
        409,
      )
    }
    // A task paused before anyone took it has nobody to take it from.
    const member = task.assignee_id === null ? null : store.participantRow(task.assignee_id)
    if (member !== null) {
      store.db
        .prepare(
          `UPDATE message SET state = 'cancelled'
           WHERE task_id = ? AND recipient_id = ? AND state IN ('queued', 'delivering', 'gated')`,
        )
        .run(task.id, member.id)
    }
    // One statement: the row is never without an assignee in a working
    // state. It remembers the member it was taken from (a session's member).
    store.db
      .prepare(
        `UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?,
           taken_from_id = COALESCE(?, taken_from_id)
         WHERE id = ?`,
      )
      .run(
        member === null
          ? task.body
          : `${task.body}\n\nReassigned from @${member.handle} (${because}); check the working tree for partial changes.`,
        store.at(),
        member === null ? null : (member.member_id ?? member.id),
        task.id,
      )
    store.log(projectId, 'task.released', {
      task: number,
      from: task.state,
      to: 'open',
      member: member?.handle ?? null,
      because,
    })
    const requester = store.participantRow(task.requester_id)
    send(store, projectId, {
      to: requester.handle,
      task: number,
      kind: 'note',
      body:
        member === null
          ? `T-${number} is back on the board (${because}) and waits for ${aPool(task.pool, task.tier)}.`
          : `T-${number} was taken back from @${member.handle} (${because}) and waits for another ${poolName(task.pool, task.tier)}.`,
    })
    return { task: taskById(store, task.id) }
  })
}

/** The assignee's answer finishes the task and is queued for whoever asked for it. */
export function recordResult(store, projectId, number, { body }) {
  requireText(body, 'body', MAX_BODY)
  return store.write(() => {
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ACTIVE_TASK_STATES, 'record a result for')
    const messageId = queue(store, projectId, {
      to: task.requester_id,
      from: task.assignee_id,
      kind: 'result',
      taskId: task.id,
      body,
    })
    store.moveTask(task, 'done', { result: messageId })
    return { task: taskById(store, task.id), message: store.message(messageId) }
  })
}

export function acceptTask(store, projectId, number, { by }) {
  return store.write(() => {
    store.participantByHandle(projectId, by)
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['done'], 'accept')
    withdrawGated(store, task.id, `accepted by @${by}`)
    // What is still on its way to the member (an answer that came after its
    // result, say) has no window left to take it.
    if (task.assignee_id !== null) {
      const member = store.participantRow(task.assignee_id)
      store.db
        .prepare(
          `UPDATE message SET state = 'cancelled', reason = ?
           WHERE task_id = ? AND recipient_id = ? AND state = 'queued'`,
        )
        .run(
          `T-${task.number} was accepted before it reached @${member.handle}`,
          task.id,
          member.id,
        )
    }
    store.moveTask(task, 'accepted', { by })
    releaseWaiting(store, projectId, task.id)
    return taskById(store, task.id)
  })
}

/**
 * The chief (or the human) stops a worker's task without ending it: its
 * window closes on the daemon's next look, whatever was on its way to it is
 * withdrawn, and the task keeps its member, its conversation and its place
 * until it is resumed or cancelled. The chief's own work is not paused.
 */
export function pauseTask(store, projectId, number, { by, because } = {}) {
  if (because !== undefined) requireText(because, 'because', 1000)
  return store.write(() => {
    if (by !== undefined) store.participantByHandle(projectId, by)
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES], 'pause')
    if (task.assignee_id !== null && store.participantRow(task.assignee_id).role === 'chief') {
      throw new LedgerError('own-work', `T-${number} is the chief's own: finish or cancel it`, 409)
    }
    dropQueued(store, task.id)
    store.moveTask(task, 'paused', {
      by: by ?? null,
      ...(because === undefined ? {} : { because }),
    })
    return taskById(store, task.id)
  })
}

/**
 * The daemon holds a task with its window while its member is out of
 * quota: paused, with the time it goes on by itself. The window closes as
 * any paused task's does and comes back on its conversation at the reset.
 */
export function holdTask(store, projectId, number, { until, because }) {
  requireText(because, 'because', 1000)
  if (typeof until !== 'string' || !Number.isFinite(Date.parse(until))) {
    throw new LedgerError('invalid-until', 'a hold names when it ends', 400)
  }
  return store.write(() => {
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['queued', ...ACTIVE_TASK_STATES], 'hold')
    dropQueued(store, task.id)
    store.moveTask(task, 'paused', { by: null, because, until })
    store.db.prepare('UPDATE task SET held_until = ? WHERE id = ?').run(until, task.id)
    return taskById(store, task.id)
  })
}

/** The held tasks whose time has come, oldest first. */
export function heldTasksDue(store, nowIso) {
  return store.db
    .prepare(
      `SELECT project_id, number, assignee_id FROM task
       WHERE state = 'paused' AND held_until IS NOT NULL AND held_until <= ? ORDER BY id`,
    )
    .all(nowIso)
    .map((row) => ({
      projectId: row.project_id,
      number: row.number,
      assigneeId: row.assignee_id,
    }))
}

/** The paused task a participant still holds, or null. */
export function pausedTask(store, participantId) {
  const row = store.db
    .prepare(
      `SELECT project_id, number FROM task WHERE assignee_id = ? AND state = 'paused'
       ORDER BY id LIMIT 1`,
    )
    .get(participantId)
  return row === undefined ? null : task(store, row.project_id, row.number)
}

/** A tell for this task has reached the participant's window since the task was last paused. */
export function toldSincePaused(store, participantId, taskId) {
  return (
    store.db
      .prepare(
        `SELECT 1 FROM message q JOIN task t ON t.id = q.task_id
           WHERE q.recipient_id = ? AND q.task_id = ?
             AND q.kind = 'question' AND q.urgent = 1
             AND q.state IN ('delivering', 'delivered', 'read')
             AND q.created_at >= (SELECT MAX(e.at) FROM event e
               WHERE e.project_id = t.project_id AND e.kind = 'task.state'
                 AND json_extract(e.data, '$.task') = t.number
                 AND json_extract(e.data, '$.to') = 'paused')`,
      )
      .get(participantId, taskId) !== undefined
  )
}

/**
 * A paused task goes on with the words that resume it: into the same
 * window when its session is still there (a brief never delivered goes in
 * first), or back on the board for its tier when the session has ended.
 */
export function resumeTask(store, projectId, number, { by, body }) {
  requireText(body, 'body', MAX_BODY)
  return store.write(() => {
    // The daemon resumes a held task in its own name: no author.
    const author = by === undefined ? null : store.participantByHandle(projectId, by)
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['paused'], 'resume')
    const assignee = task.assignee_id === null ? null : store.participantRow(task.assignee_id)
    if (assignee === null) {
      store.db
        .prepare('UPDATE task SET body = ?, updated_at = ? WHERE id = ?')
        .run(`${task.body}\n\nResumed: ${body}`, store.at(), task.id)
      store.moveTask(task, 'open', { by: by ?? null })
      return { task: taskById(store, task.id), message: null }
    }
    if (assignee.left_at !== null) {
      if (task.pool === null) {
        throw new LedgerError(
          'session-ended',
          `the window that had T-${number} has ended: cancel it and open the work for its tier`,
          409,
        )
      }
      store.db
        .prepare(
          `UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?
           WHERE id = ?`,
        )
        .run(
          `${task.body}\n\nResumed after a pause, in a fresh window (the one that had it ended; check the working tree for partial changes): ${body}`,
          store.at(),
          task.id,
        )
      store.log(projectId, 'task.state', {
        task: number,
        from: 'paused',
        to: 'open',
        by: by ?? null,
      })
      return { task: taskById(store, task.id), message: null }
    }
    const delivered = store.db
      .prepare(
        `SELECT 1 FROM message WHERE task_id = ? AND kind = 'task' AND recipient_id = ?
           AND state IN ('delivered', 'read')`,
      )
      .get(task.id, assignee.id)
    const messageId = queue(store, projectId, {
      to: assignee.id,
      from: author?.id ?? null,
      kind: 'task',
      taskId: task.id,
      body:
        delivered === undefined ? `${deliveryBody(task)}\n\nResumed: ${body}` : `Resumed: ${body}`,
    })
    store.moveTask(task, 'queued', { by: by ?? null, message: messageId })
    return { task: taskById(store, task.id), message: store.message(messageId) }
  })
}

/** A follow-up on a finished or failed task: it goes back to its assignee's queue. */
export function reopenTask(store, projectId, number, { by, body }) {
  requireText(body, 'body', MAX_BODY)
  return store.write(() => {
    const author = store.participantByHandle(projectId, by)
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['done', 'failed'], 'reopen')
    const assignee = store.participantRow(task.assignee_id)
    if (assignee.member_id !== null && assignee.left_at !== null) {
      throw new LedgerError(
        'session-ended',
        `@${assignee.handle} has ended: open the task for its tier instead`,
        409,
      )
    }
    requireActive(assignee)
    withdrawGated(store, task.id, `sent back by @${by}`)
    const messageId = queue(store, projectId, {
      to: task.assignee_id,
      from: author.id,
      kind: 'task',
      taskId: task.id,
      body,
    })
    store.moveTask(task, 'queued', { by, message: messageId })
    return { task: taskById(store, task.id), message: store.message(messageId) }
  })
}

export function cancelTask(store, projectId, number, { by }) {
  return store.write(() => {
    store.participantByHandle(projectId, by)
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES, 'paused'], 'cancel')
    dropQueued(store, task.id)
    store.moveTask(task, 'cancelled', { by })
    return taskById(store, task.id)
  })
}

/** The daemon gives up on a task: its pane died, or its launch never came up. */
export function failTask(store, projectId, number, { reason }) {
  requireText(reason, 'reason', 1000)
  return store.write(() => {
    const task = store.taskRow(projectId, number)
    requireTaskState(task, ['open', 'queued', ...ACTIVE_TASK_STATES], 'fail')
    dropQueued(store, task.id)
    store.moveTask(task, 'failed', { reason })
    return taskById(store, task.id)
  })
}

/**
 * A task and its whole thread, oldest first; null when there is no such
 * task. `cf task get` reads it over the local API; the page reads
 * `taskThatFits`.
 */
export function task(store, projectId, number) {
  const row = store.db
    .prepare(`${TASK_SELECT} WHERE t.project_id = ? AND t.number = ?`)
    .get(projectId, number)
  if (row === undefined) return null
  return {
    ...taskView(row),
    messages: store.db
      .prepare(`${MESSAGE_SELECT} WHERE m.task_id = ? ORDER BY m.id`)
      .all(row.id)
      .map(messageView),
  }
}

/**
 * The task a participant has in progress (working or waiting on an answer),
 * or null. With `queued`, one whose delivery is still being confirmed
 * counts too: a window may ask its first question before the record that
 * confirms the task's arrival is read.
 */
export function activeTask(store, participantId, { queued = false } = {}) {
  const row = store.db
    .prepare(
      `SELECT project_id, number FROM task
       WHERE assignee_id = ? AND state IN (${queued ? "'queued', " : ''}'working', 'waiting')
       ORDER BY id LIMIT 1`,
    )
    .get(participantId)
  return row === undefined ? null : task(store, row.project_id, row.number)
}

function taskRowById(store, id) {
  return store.db.prepare('SELECT * FROM task WHERE id = ?').get(id)
}

function taskById(store, id) {
  return taskView(store.db.prepare(`${TASK_SELECT} WHERE t.id = ?`).get(id))
}

function requireTaskState(task, states, action) {
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
function releaseWaiting(store, projectId, acceptedId) {
  const waiting = store.db
    .prepare(
      `SELECT t.* FROM task t JOIN task_need n ON n.task_id = t.id
       WHERE n.needs_id = ? AND t.state = 'open' AND t.assignee_id IS NOT NULL ORDER BY t.id`,
    )
    .all(acceptedId)
  for (const task of waiting) {
    const still = store.db
      .prepare(
        `SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
         WHERE n.task_id = ? AND d.state != 'accepted'`,
      )
      .get(task.id)
    if (still !== undefined) continue
    const messageId = queue(store, projectId, {
      to: task.assignee_id,
      from: task.requester_id,
      kind: 'task',
      taskId: task.id,
      body: deliveryBody(task),
    })
    store.moveTask(task, 'queued', { message: messageId })
  }
}
