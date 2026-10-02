import { LedgerError, requireActive } from './model.js'
import { MESSAGE_SELECT, messageView, PARTICIPANT_SELECT } from './views.js'

/**
 * The connection every operation runs on, and what every concern shares: one
 * transaction around an operation, the clock, the event log, the rows looked
 * up by id or handle, and the one way a task changes state. The Ledger holds
 * it privately; nothing outside src/ledger reaches the connection.
 */
export class Store {
  /** Told every event as it is logged: `{at, project, kind, data}`. */
  #trace

  constructor(db, now, names, trace) {
    this.db = db
    this.now = now
    this.names = names
    this.#trace = trace
  }

  close() {
    if (this.db.isOpen) this.db.close()
  }

  /** SQLite's own consistency check of the whole file: 'ok' when sound. */
  integrity() {
    return this.db.prepare('PRAGMA integrity_check').get().integrity_check
  }

  /** One transaction around `work`; an operation called inside another joins it. */
  write(work) {
    if (this.db.isTransaction) return work()
    this.db.exec('BEGIN IMMEDIATE')
    try {
      const result = work()
      this.db.exec('COMMIT')
      return result
    } catch (cause) {
      this.db.exec('ROLLBACK')
      throw cause
    }
  }

  at() {
    return this.now().toISOString()
  }

  log(projectId, kind, data) {
    const at = this.at()
    this.db
      .prepare('INSERT INTO event (project_id, at, kind, data) VALUES (?, ?, ?, ?)')
      .run(projectId, at, kind, JSON.stringify(data))
    this.#trace({ at, project: projectId, kind, data })
  }

  projectRow(id) {
    const row = this.db.prepare('SELECT * FROM project WHERE id = ?').get(id)
    if (row === undefined) throw new LedgerError('unknown-project', `no project ${id}`, 404)
    return row
  }

  participantRow(id) {
    const row = this.db.prepare(`${PARTICIPANT_SELECT} WHERE p.id = ?`).get(id)
    if (row === undefined) throw new LedgerError('unknown-participant', `no participant ${id}`, 404)
    return row
  }

  participantByHandle(projectId, handle) {
    this.projectRow(projectId)
    const row = this.db
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

  taskRow(projectId, number) {
    const row = this.db
      .prepare('SELECT * FROM task WHERE project_id = ? AND number = ?')
      .get(projectId, number)
    if (row === undefined) {
      throw new LedgerError('unknown-task', `no task T-${number} in project ${projectId}`, 404)
    }
    return row
  }

  /** One message, or null. */
  message(id) {
    const row = this.db.prepare(`${MESSAGE_SELECT} WHERE m.id = ?`).get(id)
    return row === undefined ? null : messageView(row)
  }

  /**
   * A task moves to another state, and the log says so. A member leaving, a
   * delivery and a decision all move tasks, so the move lives here, under
   * every concern that makes one.
   */
  moveTask(task, to, detail = {}, kind = 'task.state') {
    this.db
      .prepare('UPDATE task SET state = ?, updated_at = ?, held_until = NULL WHERE id = ?')
      .run(to, this.at(), task.id)
    this.log(task.project_id, kind, { task: task.number, from: task.state, to, ...detail })
  }
}
