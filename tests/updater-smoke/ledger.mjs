import assert from 'node:assert/strict'
import { DatabaseSync } from 'node:sqlite'

/**
 * The ledger of a home, read from outside: `consensflow.db` as it is once no
 * daemon holds it, which the ledger's exclusive lock does not allow before. That
 * lock is the one-writer rule of the flip release (a copied home goes Node, then
 * Rust, then Node again), so the smoke reads the file where no daemon runs and
 * asks what it needs of a running one by other means: the lock's refusal
 * (evidence.mjs), and the events the daemon logs as the ledger takes them
 * (`events.jsonl`, appended as they happen, readable at any time).
 *
 * "Whole" is: SQLite finds the file sound, every reference holds, the schema is
 * where the older daemon left it or past it, and what the older daemon wrote is
 * still there as it wrote it. A column that a daemon's restart rewrites by
 * design is not held to its old value: the state a window or a project is in,
 * and when it was last touched.
 */

/** The columns a daemon rewrites when it starts, or a window when it ends. */
const REWRITTEN = new Set([
  'state',
  'updated_at',
  'left_at',
  'ended_at',
  'out_until',
  'out_since',
  'resume_on_start',
  'attempts',
  'reason',
  'receipt',
  'delivered_at',
  'held_until',
])

/** Every table of the ledger with its rows, the schema's version and what SQLite says of the file. */
export function readLedger(file) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const tables = {}
    const names = db
      .prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
      )
      .all()
    for (const { name } of names) {
      tables[name] = db
        .prepare(`SELECT * FROM "${name}" ORDER BY rowid`)
        .all()
        .map((row) => ({ ...row }))
    }
    return {
      version: db.prepare('PRAGMA user_version').get().user_version,
      integrity: db
        .prepare('PRAGMA integrity_check')
        .all()
        .map((row) => row.integrity_check),
      references: db
        .prepare('PRAGMA foreign_key_check')
        .all()
        .map((row) => ({ ...row })),
      tables,
    }
  } finally {
    db.close()
  }
}

/** The file is a sound ledger: SQLite's own check, every reference, and a schema this build knows. */
export function assertSound(ledger, { atLeast = 1 } = {}) {
  assert.deepEqual(ledger.integrity, ['ok'], `SQLite's integrity check: ${ledger.integrity}`)
  assert.deepEqual(ledger.references, [], 'a reference of the ledger does not hold')
  assert.ok(ledger.version >= atLeast, `the schema is at ${ledger.version}, below ${atLeast}`)
  assert.ok(Object.keys(ledger.tables).length > 0, 'the ledger holds no table')
}

/** The ledger holds a project for each of `directories`. */
export function assertProjects(ledger, directories) {
  const held = (ledger.tables.project ?? []).map((row) => row.directory)
  for (const directory of directories) {
    assert.ok(held.includes(directory), `the ledger has no project in ${directory}: ${held}`)
  }
}

/**
 * Nothing the ledger held is gone or changed: each row `before` had is there
 * after, by its `id`, with each column it had, bar the ones a restart rewrites; and
 * the schema has not gone back.
 */
export function assertKept(before, after) {
  assertSound(after, { atLeast: before.version })
  for (const [table, rows] of Object.entries(before.tables)) {
    assert.ok(table in after.tables, `the ledger lost its ${table} table`)
    const kept = new Map(after.tables[table].map((row) => [row.id, row]))
    for (const row of rows) {
      const now = kept.get(row.id)
      assert.ok(now !== undefined, `${table} ${row.id} is gone: ${JSON.stringify(row)}`)
      for (const [column, value] of Object.entries(row)) {
        if (REWRITTEN.has(column)) continue
        assert.deepEqual(
          now[column],
          value,
          `${table} ${row.id}: ${column} was ${JSON.stringify(value)} and is ${JSON.stringify(now[column])}`,
        )
      }
    }
  }
}

/**
 * The ledger events a daemon's trace file holds, in order: its lines of the
 * four keys a ledger event is written with (`at`, `project`, `kind`, `data`),
 * which no other line has.
 */
export function tracedEvents(text) {
  return text.split('\n').flatMap((line) => {
    try {
      const parsed = JSON.parse(line)
      const keys = Object.keys(parsed ?? {})
      return keys.join() === 'at,project,kind,data' ? [parsed] : []
    } catch {
      return []
    }
  })
}

/** Each event the older daemon traced is in the ledger the newer one holds. */
export function assertTraced(events, ledger) {
  const held = (ledger.tables.event ?? []).map((row) =>
    JSON.stringify([row.at, row.project_id, row.kind, JSON.parse(row.data)]),
  )
  for (const event of events) {
    const key = JSON.stringify([event.at, event.project, event.kind, event.data])
    assert.ok(held.includes(key), `the ledger lost an event the daemon traced: ${key}`)
  }
}
