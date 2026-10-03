import {
  LedgerError,
  requireAgentId,
  requireGate,
  requireHarness,
  requireMember,
  requireText,
} from './model.js'
import { PARTICIPANT_SELECT, participantView } from './views.js'

/**
 * Projects: each with its human, its chief and its staff, open or suspended,
 * gated or not; brought back after a restart of the daemon, logged event by
 * event, and deleted with everything in it once closed.
 */

/**
 * A project with its chief, on the saved agent it runs on, and, when given,
 * its staff (the last project's, usually). A chief with no agent is a lead
 * from before leads were always saved agents: it runs on its harness's
 * default.
 */
export function createProject(store, { directory, name, chief, staff = [], gate = false }) {
  requireText(directory, 'directory', 4096)
  requireText(name, 'name', 100)
  requireHarness(chief?.harness)
  const lead = chief.agent ?? null
  if (lead !== null) requireAgentId(lead)
  requireGate(gate)
  const members = staff.map((member) => ({ ...member, roles: requireMember(member) }))
  return store.write(() => {
    const at = store.at()
    const { lastInsertRowid: id } = store.db
      .prepare(
        `INSERT INTO project (directory, name, state, gate, created_at, updated_at)
         VALUES (?, ?, 'open', ?, ?, ?)`,
      )
      .run(directory, name, gate ? 1 : 0, at, at)
    addParticipant(store, id, { handle: 'human', role: 'human', agent: null, harness: null })
    addParticipant(store, id, {
      handle: 'chief',
      role: 'chief',
      agent: lead,
      harness: chief.harness,
    })
    for (const { agent, harness, designer, roles, tier } of members) {
      addParticipant(store, id, { handle: agent, roles, agent, harness, designer, tier })
    }
    store.log(id, 'project.created', { name, directory })
    return project(store, id)
  })
}

export function project(store, id) {
  const row = store.db.prepare('SELECT * FROM project WHERE id = ?').get(id)
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
    participants: store.db
      .prepare(`${PARTICIPANT_SELECT} WHERE p.project_id = ? AND p.left_at IS NULL ORDER BY p.id`)
      .all(id)
      .map(participantView),
  }
}

export function projects(store) {
  return store.db
    .prepare('SELECT id FROM project ORDER BY id')
    .all()
    .map((row) => project(store, row.id))
}

/** Open or suspend a project by hand; either way it is no longer due a resume. */
export function setProjectState(store, id, state) {
  if (state !== 'open' && state !== 'suspended') {
    throw new LedgerError('invalid-state', `a project is open or suspended, not ${state}`)
  }
  return store.write(() => {
    const from = store.projectRow(id).state
    store.db
      .prepare('UPDATE project SET state = ?, resume_on_start = 0, updated_at = ? WHERE id = ?')
      .run(state, store.at(), id)
    if (from !== state) store.log(id, 'project.state', { from, to: state })
    return project(store, id)
  })
}

/**
 * A closed project goes for good: its participants, conversations, tasks,
 * messages and events with it (the schema cascades). An open one is
 * refused: close it first, so nothing runs while its record disappears.
 */
export function deleteProject(store, id) {
  return store.write(() => {
    const row = store.projectRow(id)
    if (row.state !== 'suspended') {
      throw new LedgerError('project-open', `${row.name} is open: close it first`, 409)
    }
    const count = (sql) => store.db.prepare(sql).get(id).n
    // What goes, for the one line the trace keeps.
    const gone = {
      id: row.id,
      name: row.name,
      directory: row.directory,
      createdAt: row.created_at,
      members: count(
        `SELECT COUNT(*) AS n FROM participant
         WHERE project_id = ? AND agent IS NOT NULL AND member_id IS NULL AND left_at IS NULL
           AND role != 'chief'`,
      ),
      sessions: count(
        'SELECT COUNT(*) AS n FROM participant WHERE project_id = ? AND member_id IS NOT NULL',
      ),
      tasks: count('SELECT COUNT(*) AS n FROM task WHERE project_id = ?'),
      messages: count('SELECT COUNT(*) AS n FROM message WHERE project_id = ?'),
    }
    store.db.prepare('DELETE FROM project WHERE id = ?').run(id)
    return gone
  })
}

/**
 * Human approval required: with the gate on, every message between two
 * agents waits for the human, who passes it on or declines it. What is
 * already gated stays so when the gate goes off; the human decides it.
 */
export function setGate(store, id, gate) {
  requireGate(gate)
  return store.write(() => {
    const from = store.projectRow(id).gate === 1
    store.db
      .prepare('UPDATE project SET gate = ?, updated_at = ? WHERE id = ?')
      .run(gate ? 1 : 0, store.at(), id)
    if (from !== gate) store.log(id, 'project.gate', { from, to: gate })
    return project(store, id)
  })
}

/**
 * At daemon start: the panes of every open project died with the previous
 * process, so each becomes suspended and is marked to come back by itself.
 */
export function suspendForRestart(store) {
  return store.write(() => {
    const open = store.db.prepare(`SELECT id FROM project WHERE state = 'open' ORDER BY id`).all()
    for (const { id } of open) {
      store.db
        .prepare(
          `UPDATE project SET state = 'suspended', resume_on_start = 1, updated_at = ?
           WHERE id = ?`,
        )
        .run(store.at(), id)
      store.log(id, 'project.state', { from: 'open', to: 'suspended', resumeOnStart: true })
    }
    return open.map(({ id }) => project(store, id))
  })
}

/** A resume on start is tried once: after it, success or not, the mark goes. */
export function forgetResume(store, id) {
  return store.write(() => {
    store.projectRow(id)
    store.db.prepare('UPDATE project SET resume_on_start = 0 WHERE id = ?').run(id)
  })
}

export function events(store, projectId, { after = 0, limit = 500 } = {}) {
  return store.db
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

/** A participant joins under a handle nobody in the project has; a member's joining is logged. */
export function addParticipant(
  store,
  projectId,
  { handle, role, roles = [], agent, harness, designer = false, tier = null },
) {
  role ??= roles[0]
  store.projectRow(projectId)
  const taken = store.db
    .prepare('SELECT 1 FROM participant WHERE project_id = ? AND handle = ?')
    .get(projectId, handle)
  if (taken !== undefined) {
    throw new LedgerError('member-exists', `${handle} is already in project ${projectId}`, 409)
  }
  const { lastInsertRowid: id } = store.db
    .prepare(
      `INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(
      projectId,
      handle,
      role,
      JSON.stringify(roles),
      agent,
      harness,
      designer ? 1 : 0,
      tier,
      store.at(),
    )
  if (role !== 'human' && role !== 'chief') {
    store.log(projectId, 'member.added', { handle, role, roles, harness })
  }
  return participantView(store.participantRow(id))
}
