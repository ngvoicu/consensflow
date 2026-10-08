import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { fileURLToPath } from 'node:url'

/**
 * A ledger file made without the ledger, for the tests and tools that need
 * one to read or to start a daemon on: the schema is the native ledger's own
 * migrations (crates/cf-ledger/migrations, the files `schema.rs` includes), run
 * in order as `migrate` runs them, and the rows are a frozen fixture
 * (tests/fixtures/ledgers) or a few written by hand. Nothing here writes
 * what the ledger's operations write, but for the one project `addProject` adds.
 */

const MIGRATIONS_FOLDER = fileURLToPath(new URL('../crates/cf-ledger/migrations/', import.meta.url))
const FIXTURES_FOLDER = fileURLToPath(new URL('./fixtures/ledgers/', import.meta.url))

const names = readdirSync(MIGRATIONS_FOLDER)
  .filter((name) => /^\d{4}\.sql$/.test(name))
  .sort()
names.forEach((name, at) => {
  if (name !== `${String(at + 1).padStart(4, '0')}.sql`) {
    throw new Error(`the ledger's migrations are numbered in order: ${name} is the ${at + 1}th`)
  }
})

/** The ledger's migrations, in order. */
export const MIGRATIONS = names.map((name) => readFileSync(join(MIGRATIONS_FOLDER, name), 'utf8'))

/** The schema version this build's ledger knows: `PRAGMA user_version` after a start. */
export const SCHEMA_VERSION = MIGRATIONS.length

/** The text of the frozen ledger `name` (tests/fixtures/ledgers/<name>.sql). */
export const fixtureSql = (name) => readFileSync(join(FIXTURES_FOLDER, `${name}.sql`), 'utf8')

/** The recorded answer of the frozen ledger `name` (tests/fixtures/ledgers/<name>.json). */
export const fixtureJson = (name) =>
  JSON.parse(readFileSync(join(FIXTURES_FOLDER, `${name}.json`), 'utf8'))

/**
 * The ledger file `file`, made if it is not there, open at the schema this
 * build knows: the migrations after its version run in order, each in a
 * transaction that sets the version, with references unchecked while they run,
 * and a ledger of a newer version refused (`migrate`, crates/cf-ledger/src/schema.rs).
 * `wal` has it in write-ahead mode, as a ledger in use is.
 */
export function openLedgerFile(file, { wal = false } = {}) {
  const db = new DatabaseSync(file)
  if (wal) db.exec('PRAGMA journal_mode = WAL')
  const { user_version: version } = db.prepare('PRAGMA user_version').get()
  if (version > SCHEMA_VERSION) {
    db.close()
    throw new Error(
      `this home was written by a newer ConsensFlow (schema ${version}; this build knows ${SCHEMA_VERSION})`,
    )
  }
  db.exec('PRAGMA foreign_keys = OFF')
  for (let at = version; at < SCHEMA_VERSION; at += 1) {
    db.exec('BEGIN IMMEDIATE')
    db.exec(MIGRATIONS[at])
    db.exec(`PRAGMA user_version = ${at + 1}`)
    db.exec('COMMIT')
  }
  return db
}

/**
 * The rows `sql` inserts, loaded into the new ledger file `file`, open: they
 * must leave every reference holding. `replacing` names text of `sql`, which
 * is in a string, and what it stands for here (a folder's placeholder).
 */
export function loadRows(file, sql, { wal = false, replacing = {} } = {}) {
  const db = openLedgerFile(file, { wal })
  let rows = sql
  for (const [token, text] of Object.entries(replacing)) {
    rows = rows.replaceAll(token, text.replaceAll("'", "''"))
  }
  db.exec(rows)
  const broken = db.prepare('PRAGMA foreign_key_check').all()
  if (broken.length > 0) {
    db.close()
    throw new Error(`the rows leave ${broken.length} references that do not hold`)
  }
  db.exec('PRAGMA foreign_keys = ON')
  return db
}

/** The frozen ledger `name` loaded into the new file `file`, open (see `loadRows`). */
export const loadLedger = (file, name, options) => loadRows(file, fixtureSql(name), options)

/**
 * A project with its human, its chief and its staff, as the ledger's
 * `createProject` leaves it, written by hand into the open ledger `db`
 * (a copy of a home to start a daemon on, before any has): the rows of the
 * project, each participant and the log of each member's joining. The id of
 * the project.
 */
export function addProject(db, { directory, name, chief, staff }) {
  const at = new Date().toISOString()
  db.exec('BEGIN IMMEDIATE')
  try {
    const { lastInsertRowid: project } = db
      .prepare(
        `INSERT INTO project (directory, name, state, gate, created_at, updated_at)
         VALUES (?, ?, 'open', 0, ?, ?)`,
      )
      .run(directory, name, at, at)
    const join = db.prepare(
      `INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, created_at)
       VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)`,
    )
    const log = db.prepare('INSERT INTO event (project_id, at, kind, data) VALUES (?, ?, ?, ?)')
    join.run(project, 'human', 'human', '[]', null, null, null, at)
    join.run(project, 'chief', 'chief', '[]', chief.agent, chief.harness, null, at)
    for (const { agent, harness, roles, tier } of staff) {
      join.run(project, agent, roles[0], JSON.stringify(roles), agent, harness, tier, at)
      log.run(
        project,
        at,
        'member.added',
        JSON.stringify({ handle: agent, role: roles[0], roles, harness }),
      )
    }
    log.run(project, at, 'project.created', JSON.stringify({ name, directory }))
    db.exec('COMMIT')
    return Number(project)
  } catch (cause) {
    db.exec('ROLLBACK')
    throw cause
  }
}
