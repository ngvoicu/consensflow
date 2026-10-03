import { currentConversation } from './conversations.js'
import {
  HELD_TASK_STATES,
  LedgerError,
  MEMBER_ROLES,
  requireFittingRoles,
  requireMember,
  requireRoles,
  requireText,
  TIERS,
} from './model.js'
import { addParticipant, project } from './projects.js'
import { dropQueued, send } from './queue.js'
import { PARTICIPANT_SELECT, participantView } from './views.js'

/**
 * The staff: members who join, change roles, follow the roster's tiers, run
 * out of quota and leave, and the sessions their tasks run in, each a named
 * window from its first task until the human ends it. It answers who may
 * take a task: the members of a pool and tier, with what the daemon ranks
 * them by.
 */

const HOLDS_WORK = `SELECT 1 FROM task WHERE assignee_id = ? AND state IN (${HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')})`

/** A member joins the staff, or rejoins it in the roles, harness, designer flag and tier given now. */
export function addMember(
  store,
  projectId,
  { agent, harness, designer = false, role, roles, tier },
) {
  roles = requireMember({ agent, harness, designer, role, roles, tier })
  return store.write(() => {
    const left = store.db
      .prepare(
        'SELECT * FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NOT NULL',
      )
      .get(projectId, agent)
    if (left === undefined) {
      return addParticipant(store, projectId, {
        handle: agent,
        roles,
        agent,
        harness,
        designer,
        tier,
      })
    }
    store.db
      .prepare(
        `UPDATE participant SET role = ?, roles = ?, harness = ?, designer = ?, tier = ?,
           left_at = NULL
         WHERE id = ?`,
      )
      .run(roles[0], JSON.stringify(roles), harness, designer ? 1 : 0, tier, left.id)
    store.log(projectId, 'member.added', { handle: agent, roles, harness, rejoined: true })
    return participantView(store.participantRow(left.id))
  })
}

/**
 * Members follow the roster: each active member's tier (and its sessions')
 * becomes what its saved agent has now, since the app's catalog may have
 * moved the model. Says which members changed; an agent the roster no
 * longer has leaves its member as it is.
 */
export function refreshMemberTiers(store, tierOf) {
  return store.write(() => {
    const members = store.db
      .prepare(
        `SELECT id, project_id, handle, agent, tier FROM participant
         WHERE agent IS NOT NULL AND member_id IS NULL AND left_at IS NULL AND role != 'chief'
         ORDER BY id`,
      )
      .all()
    const changed = []
    for (const member of members) {
      const tier = tierOf(member.agent) ?? null
      if (tier === null || tier === member.tier) continue
      store.db
        .prepare('UPDATE participant SET tier = ? WHERE id = ? OR member_id = ?')
        .run(tier, member.id, member.id)
      store.log(member.project_id, 'member.tier', {
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

/**
 * A member's roles change in place. A role it gains must fit its agent; one
 * it holds already stays though it does not (a member from before an image
 * designer had to be an image agent), until the human drops it.
 */
export function setRoles(store, projectId, handle, roles) {
  roles = requireRoles(roles)
  return store.write(() => {
    const member = store.participantByHandle(projectId, handle)
    requireMemberRow(store, member.id, 'changes roles')
    if (!MEMBER_ROLES.includes(member.role)) {
      throw new LedgerError(
        'not-a-member',
        `${handle} is the project's ${member.role}, not a member of its staff`,
        409,
      )
    }
    const held = JSON.parse(member.roles)
    requireFittingRoles(
      member.agent,
      member.designer === 1,
      roles.filter((role) => !held.includes(role)),
    )
    store.db
      .prepare('UPDATE participant SET role = ?, roles = ? WHERE id = ?')
      .run(roles[0], JSON.stringify(roles), member.id)
    store.log(projectId, 'member.roles', { handle, roles })
    return participantView(store.participantRow(member.id))
  })
}

/**
 * A member leaves the staff. Its open tasks are cancelled, with the messages
 * still on their way to it and its unread questions; its coordinator and
 * whoever asked for those tasks are told, if their windows run.
 */
export function removeMember(store, projectId, handle) {
  return store.write(() => {
    const member = store.participantByHandle(projectId, handle)
    requireMemberRow(store, member.id, 'leaves the staff')
    if (!MEMBER_ROLES.includes(member.role)) {
      throw new LedgerError(
        'not-a-member',
        `${handle} is the project's ${member.role}, not a member of its staff`,
        409,
      )
    }
    const sessions = store.db
      .prepare(`${PARTICIPANT_SELECT} WHERE p.member_id = ? AND p.left_at IS NULL ORDER BY p.id`)
      .all(member.id)
    const windows = [member, ...sessions]
    const open = store.db
      .prepare(
        `SELECT t.*, q.handle AS requester FROM task t JOIN participant q ON q.id = t.requester_id
         WHERE t.assignee_id IN (${windows.map(() => '?').join(', ')})
           AND t.state IN ('queued', 'working', 'waiting')
         ORDER BY t.number`,
      )
      .all(...windows.map((row) => row.id))
    for (const task of open) {
      dropQueued(store, task.id)
      store.moveTask(task, 'cancelled', { reason: `@${handle} left the staff` })
    }
    for (const row of windows) {
      store.db
        .prepare(
          `UPDATE message SET state = 'cancelled'
           WHERE (recipient_id = ? AND state IN ('queued', 'delivering', 'gated'))
              OR (sender_id = ? AND kind = 'question' AND state IN ('queued', 'gated'))`,
        )
        .run(row.id, row.id)
    }
    for (const session of sessions) closeSession(store, session, `@${handle} left the staff`)
    store.db.prepare('UPDATE participant SET left_at = ? WHERE id = ?').run(store.at(), member.id)
    const cancelled = open.map((task) => task.number)
    store.log(projectId, 'member.left', { handle, cancelled })
    // Only work that went with it is worth a word, and only to whoever asked for it.
    if (cancelled.length > 0) {
      const body = `@${handle} left the staff; it takes no more tasks. Cancelled with it: ${cancelled.map((number) => `T-${number}`).join(', ')}.`
      for (const requester of new Set(open.map((task) => task.requester))) {
        if (requester !== 'human') tellIfRunning(store, projectId, requester, body)
      }
    }
    return { member: participantView(store.participantRow(member.id)), cancelled }
  })
}

/** The members of the newest project that has any: the staff a new project starts from. */
export function lastStaff(store) {
  const member = `role IN (${MEMBER_ROLES.map((role) => `'${role}'`).join(', ')})
    AND left_at IS NULL AND member_id IS NULL`
  return store.db
    .prepare(
      `SELECT agent, harness, role, roles FROM participant
       WHERE project_id = (SELECT MAX(project_id) FROM participant WHERE ${member})
         AND ${member}
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

/**
 * Whether a member has a task on its hands: one task per member session
 * ends when this is false. Paused work counts: its window stays for the
 * resumption.
 */
export function holdsWork(store, participantId) {
  return (
    store.db
      .prepare(
        `SELECT 1 FROM task WHERE assignee_id = ? AND state IN (${[...HELD_TASK_STATES, 'paused'].map((state) => `'${state}'`).join(', ')})`,
      )
      .get(participantId) !== undefined
  )
}

/** The active members an open task may go to, with what the daemon ranks them by. */
export function candidates(store, projectId, number) {
  const task = store.taskRow(projectId, number)
  return members(store, projectId, task.pool)
    .filter((member) => task.tier === null || member.tier === task.tier)
    .map((member) => ({ ...member, hadIt: member.id === task.taken_from_id }))
}

/**
 * The active members of one role, in join order, with what the daemon ranks
 * them by: their tier, how many tasks they have taken, whether one is on
 * their hands now, and until when they are out of quota.
 */
export function members(store, projectId, role) {
  const held = HELD_TASK_STATES.map((state) => `'${state}'`).join(', ')
  return store.db
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
      harness: row.harness,
      tier: row.tier,
      roles: JSON.parse(row.roles),
      taken: row.taken,
      sessions: row.sessions,
      outUntil: row.out_until,
    }))
}

/**
 * A member's new session: its own participant, named after the member,
 * with the member's agent, harness, designer flag and tier and the role its
 * task needs. It starts from nothing and ends with its work (CORE-19).
 */
export function startSession(store, projectId, member, role) {
  const free = (handle) =>
    store.db
      .prepare('SELECT 1 FROM participant WHERE project_id = ? AND handle = ?')
      .get(projectId, handle) === undefined
  let name = store.names()
  for (let attempt = 1; attempt < 16 && !free(`${member.handle}-${name}`); attempt += 1) {
    name = store.names()
  }
  // A name is never used twice (an ended session keeps its row), so a
  // member about a thousand sessions in draws only taken ones: the last
  // name drawn then takes the first number free.
  let handle = `${member.handle}-${name}`
  for (let number = 2; !free(handle); number += 1) handle = `${member.handle}-${name}-${number}`
  const { lastInsertRowid: id } = store.db
    .prepare(
      `INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, member_id, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(
      projectId,
      handle,
      role,
      member.roles,
      member.agent,
      member.harness,
      member.designer,
      member.tier,
      member.id,
      store.at(),
    )
  store.log(projectId, 'session.started', { handle, member: member.handle, role })
  return store.participantRow(id)
}

/** A session ends: it leaves the project and its conversation closes. */
function closeSession(store, session, reason) {
  const at = store.at()
  store.db.prepare('UPDATE participant SET left_at = ? WHERE id = ?').run(at, session.id)
  store.db
    .prepare('UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL')
    .run(at, session.id)
  store.log(session.project_id, 'session.ended', {
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
export function endSession(store, projectId, handle, { by }) {
  return store.write(() => {
    store.participantByHandle(projectId, by)
    const session = store.participantByHandle(projectId, handle)
    if (session.member_id === null) {
      throw new LedgerError('not-a-session', `@${handle} is not a session`, 409)
    }
    if (store.db.prepare(HOLDS_WORK).get(session.id) !== undefined) {
      throw new LedgerError(
        'session-busy',
        `@${handle} still holds work: accept, cancel or pause it first`,
        409,
      )
    }
    closeSession(store, session, `ended by @${by}`)
    return project(store, projectId)
  })
}

/** The session that did T-`after`, still there and with nothing on its hands. */
export function continuableSession(store, projectId, after) {
  const previous = store.taskRow(projectId, after)
  const session = previous.assignee_id === null ? null : store.participantRow(previous.assignee_id)
  if (session === null || session.member_id === null || session.left_at !== null) {
    throw new LedgerError(
      'session-ended',
      `the session that did T-${after} has ended: open the task for its tier instead`,
      409,
    )
  }
  if (holdsWork(store, session.id)) {
    throw new LedgerError(
      'session-busy',
      `@${session.handle} is still on its work: wait for its result, or open the task for its tier`,
      409,
    )
  }
  return session
}

/** A member of the staff, never one of its sessions. */
export function requireMemberRow(store, participantId, does) {
  const row = store.participantRow(participantId)
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
export function markOut(store, participantId, { until, reason }) {
  if (Number.isNaN(Date.parse(until))) {
    throw new LedgerError('invalid-time', `not a time: ${JSON.stringify(until)}`)
  }
  requireText(reason, 'reason', 1000)
  return store.write(() => {
    const member = store.participantRow(participantId)
    store.db
      .prepare('UPDATE participant SET out_until = ?, out_since = ? WHERE id = ?')
      .run(until, store.at(), member.id)
    store.log(member.project_id, 'member.out', { handle: member.handle, until, reason })
    return participantView(store.participantRow(member.id))
  })
}

/**
 * The tier a task for `pool` goes to: the one asked, when somebody holds it;
 * else the nearest one somebody does, the next one up before the next one
 * down. A pool with no tier (the designer) keeps none.
 */
export function nearestTier(store, projectId, pool, tier) {
  if (tier === null || tier === undefined || membersOfTier(store, projectId, pool, tier).length > 0)
    return tier
  const at = TIERS.indexOf(tier)
  const near = TIERS.filter((other) => other !== tier).sort(
    (a, b) =>
      Math.abs(TIERS.indexOf(a) - at) - Math.abs(TIERS.indexOf(b) - at) ||
      TIERS.indexOf(a) - TIERS.indexOf(b),
  )
  return near.find((other) => membersOfTier(store, projectId, pool, other).length > 0) ?? tier
}

/** The staff's members of one role and tier (any tier when null), whatever role they were saved with first. */
export function membersOfTier(store, projectId, pool, tier) {
  return store.db
    .prepare(
      `SELECT * FROM participant p
       WHERE p.project_id = ? AND (? IS NULL OR p.tier = ?) AND p.left_at IS NULL AND p.member_id IS NULL
         AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
       ORDER BY p.id`,
    )
    .all(projectId, tier, tier, pool)
}

/** A note from ConsensFlow, for a participant whose window has already started. */
function tellIfRunning(store, projectId, handle, body) {
  const row = store.db
    .prepare('SELECT id FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NULL')
    .get(projectId, handle)
  if (row === undefined || currentConversation(store, row.id) === null) return
  send(store, projectId, { to: handle, body, kind: 'note' })
}
