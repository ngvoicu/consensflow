import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { openLedger, SCHEMA_VERSION } from '../src/ledger/index.js'
import { MIGRATIONS, migrate } from '../src/ledger/schema.js'
import { busyProject, clock, names, staff, withDir } from './ledger-fixtures.mjs'

/** The ledger's schema and its migrations (src/ledger/schema.js). */

/** Every table, in the order the schema makes them. */
const TABLES = [
  'project',
  'participant',
  'conversation',
  'task',
  'task_need',
  'message',
  'transcript',
  'event',
]
/** The tables whose ids leave the ledger, by name. */
const WITH_IDS = ['conversation', 'event', 'message', 'participant', 'project', 'task']

const highest = (rows) => Math.max(...rows.map((row) => row.id))

/** What a ledger file holds, read without the ledger: its version, its schema and every row. */
function contents(file) {
  const db = new DatabaseSync(file, { readOnly: true })
  const all = (sql) =>
    db
      .prepare(sql)
      .all()
      .map((row) => ({ ...row }))
  try {
    return {
      version: db.prepare('PRAGMA user_version').get().user_version,
      schema: all('SELECT type, name, sql FROM sqlite_master ORDER BY name'),
      rows: Object.fromEntries(
        TABLES.map((table) => [table, all(`SELECT * FROM ${table} ORDER BY rowid`)]),
      ),
    }
  } finally {
    db.close()
  }
}

/** A file at `version` made by that many migrations: an empty ledger as a build of that schema made it. */
function migratedTo(file, version) {
  const db = new DatabaseSync(file)
  migrate(db, MIGRATIONS.slice(0, version))
  db.close()
  return file
}

/** The schema an empty ledger of `version` has. */
const schemaAt = (dir, version) =>
  contents(migratedTo(path.join(dir, `v${version}.db`), version)).schema

/**
 * A ledger as a build of schema `version` left it: made by its first
 * migrations, in WAL mode like every ledger, holding the rows the ledger's
 * own operations write for two busy projects, ids and all, in the columns
 * that schema has.
 */
function ledgerAt(dir, version) {
  const source = path.join(dir, 'source.db')
  const ledger = openLedger(source, { now: clock(), names: names() })
  busyProject(ledger, '/work/app')
  busyProject(ledger, '/work/site')
  ledger.close()
  const file = path.join(dir, 'consensflow.db')
  const db = new DatabaseSync(file)
  db.exec('PRAGMA journal_mode = WAL')
  migrate(db, MIGRATIONS.slice(0, version))
  db.exec('PRAGMA foreign_keys = OFF')
  db.prepare('ATTACH DATABASE ? AS source').run(source)
  for (const table of TABLES) {
    const columns = db
      .prepare(`PRAGMA main.table_info(${table})`)
      .all()
      .map((column) => column.name)
      .join(', ')
    db.exec(`INSERT INTO main.${table} (${columns}) SELECT ${columns} FROM source.${table}`)
  }
  db.exec('DETACH DATABASE source')
  db.close()
  return file
}

describe('the schema', () => {
  it('turns a ledger written before the Chief of Staff into one that says chief, ids kept', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-ledger-chief-'))
    try {
      const file = path.join(dir, 'consensflow.db')
      // A version-4 ledger as the 2026-09-24 build wrote it: `lead` in its rows and its check.
      const old = new DatabaseSync(file)
      old.exec(MIGRATIONS[0].replaceAll("'chief'", "'lead'"))
      for (const migration of MIGRATIONS.slice(1, 4)) old.exec(migration)
      old.exec('PRAGMA user_version = 4')
      const at = '2026-09-24T10:00:00.000Z'
      old.exec(
        `INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES (1, '/work/app', 'app', 'open', 0, '${at}', '${at}')`,
      )
      old.exec(
        `INSERT INTO participant (id, project_id, handle, role, created_at) VALUES (1, 1, 'human', 'human', '${at}'), (2, 1, 'lead', 'lead', '${at}'), (3, 1, 'zeus', 'worker', '${at}')`,
      )
      old.exec(
        `INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, pool, created_at, updated_at) VALUES (1, 1, 'Parser', 'Parser', 2, 3, 'working', 'worker', '${at}', '${at}')`,
      )
      old.close()
      const ledger = openLedger(file)
      try {
        const chief = ledger.project(1).participants.find((p) => p.id === 2)
        assert.deepEqual([chief.handle, chief.role], ['chief', 'chief'])
        assert.deepEqual(
          [ledger.task(1, 1).requester, ledger.task(1, 1).assignee],
          ['chief', 'zeus'],
          'references follow the ids',
        )
        assert.equal(ledger.project(1).participants.length, 3)
      } finally {
        ledger.close()
      }
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })

  it('keeps a migration that leaves a reference dangling from counting: the next start checks again', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const old = new DatabaseSync(file, { enableForeignKeyConstraints: false })
      for (const migration of MIGRATIONS.slice(0, SCHEMA_VERSION - 1)) old.exec(migration)
      old.exec(`PRAGMA user_version = ${SCHEMA_VERSION - 1}`)
      const at = '2026-09-24T10:00:00.000Z'
      old.exec(
        `INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES (1, '/work/app', 'app', 'open', 0, '${at}', '${at}')`,
      )
      // A task asked for by a participant the file does not have.
      old.exec(
        `INSERT INTO task (project_id, number, title, body, requester_id, state, pool, created_at, updated_at) VALUES (1, 1, 'Parser', 'Parser', 99, 'open', 'worker', '${at}', '${at}')`,
      )
      old.close()
      for (const start of ['first', 'next']) {
        assert.throws(() => openLedger(file), { code: 'ledger-broken' }, `the ${start} start`)
      }
      const raw = new DatabaseSync(file, { readOnly: true })
      assert.equal(
        raw.prepare('PRAGMA user_version').get().user_version,
        SCHEMA_VERSION - 1,
        'the version stays where it was',
      )
      raw.close()
    })
  })

  it('refuses what the model never holds, and keeps every reference whole', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedger(file, { now: clock(), names: names() })
      const { project, id } = staff(ledger)
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      const chief = id('chief')
      const zeus = id('zeus')
      ledger.close()
      const raw = new DatabaseSync(file)
      raw.exec('PRAGMA foreign_keys = ON')
      const at = '2026-09-21T10:00:00.000Z'
      assert.throws(() => raw.prepare('UPDATE project SET gate = 2').run(), /CHECK/)
      assert.throws(() => raw.prepare("UPDATE task SET pool = 'judge'").run(), /CHECK/)
      assert.throws(() => raw.prepare("UPDATE task SET state = 'review'").run(), /CHECK/)
      assert.throws(
        () => raw.prepare('UPDATE task SET taken_from_id = 99').run(),
        /FOREIGN KEY/,
        'a task is taken back only from a participant that exists',
      )
      assert.throws(() => raw.prepare('INSERT INTO task_need VALUES (1, 1)').run(), /CHECK/)
      assert.throws(() => raw.prepare('INSERT INTO task_need VALUES (1, 99)').run(), /FOREIGN KEY/)
      const insert = raw.prepare(
        `INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
         VALUES (?, ?, ?, 'note', ?, ?, ?)`,
      )
      assert.throws(() => insert.run(project.id, chief, zeus, 'Held', 'held', at), /CHECK/)
      insert.run(project.id, chief, zeus, 'One', 'delivering', at)
      assert.throws(
        () => insert.run(project.id, chief, zeus, 'Two', 'delivering', at),
        /UNIQUE constraint failed: message.recipient_id/,
        'one delivery at a time per recipient',
      )
      assert.equal(raw.prepare('PRAGMA foreign_key_check').all().length, 0, 'nothing dangles')
      raw.close()
    })
  })
})

describe('schema 6: no id is given twice', () => {
  it('makes the schema 5 had, with AUTOINCREMENT on every table whose ids leave the ledger', async () => {
    await withDir(async (dir) => {
      const [before, after] = [5, 6].map((version) => schemaAt(dir, version))
      assert.deepEqual(
        after.filter((row) => row.sql?.includes(' AUTOINCREMENT')).map((row) => row.name),
        WITH_IDS,
      )
      assert.deepEqual(
        after
          .filter((row) => row.name !== 'sqlite_sequence')
          .map((row) => ({ ...row, sql: row.sql?.replace(' AUTOINCREMENT', '') ?? null })),
        before,
        'nothing else differs',
      )
      assert.deepEqual(
        after.find((row) => row.name === 'sqlite_sequence'),
        { type: 'table', name: 'sqlite_sequence', sql: 'CREATE TABLE sqlite_sequence(name,seq)' },
      )
    })
  })

  it('migrates a schema-5 ledger with every row, id and reference as it was, then gives none of its ids again', async () => {
    await withDir(async (dir) => {
      const file = ledgerAt(dir, 5)
      const before = contents(file)

      const after = contents(migratedTo(file, 6))
      assert.equal(after.version, 6)
      assert.deepEqual(after.rows, before.rows, 'every row, with its id and references')
      assert.deepEqual(after.schema, schemaAt(dir, 6), "the schema is a fresh schema-6 ledger's")
      const raw = new DatabaseSync(file, { readOnly: true })
      assert.deepEqual(raw.prepare('PRAGMA foreign_key_check').all(), [], 'nothing dangles')
      assert.deepEqual(
        raw
          .prepare('SELECT name, seq FROM sqlite_sequence ORDER BY name')
          .all()
          .map((row) => ({ ...row })),
        WITH_IDS.map((name) => ({ name, seq: highest(before.rows[name]) })),
        'each table goes on past its highest id',
      )
      raw.close()

      const ledger = openLedger(file, { now: clock(), names: names() })
      try {
        assert.equal(ledger.integrity(), 'ok')
        // The project with the highest ids goes, and the next one gets none of them.
        const top = ledger.projects().at(-1).id
        ledger.setProjectState(top, 'suspended')
        ledger.deleteProject(top)
        const next = busyProject(ledger, '/work/api')
        for (const [table, ids] of Object.entries(next)) {
          assert.deepEqual(
            ids.filter((id) => id <= highest(before.rows[table])),
            [],
            `every new ${table} id is past the highest the ledger held`,
          )
        }
      } finally {
        ledger.close()
      }
    })
  })

  it('leaves a schema-5 ledger as it was when its migration fails partway, start after start', async () => {
    await withDir(async (dir) => {
      const file = ledgerAt(dir, 5)
      // A message its rebuilt table refuses: the four tables before it are rebuilt when it fails.
      const db = new DatabaseSync(file)
      db.exec('PRAGMA ignore_check_constraints = ON')
      db.exec("UPDATE message SET state = 'held' WHERE id = 1")
      db.close()
      const before = contents(file)
      for (const start of ['first', 'next']) {
        assert.throws(
          () => openLedger(file),
          /CHECK constraint failed: message_state_check/,
          `the ${start} start`,
        )
      }
      assert.deepEqual(contents(file), before, 'its version, its schema and every row')
    })
  })

  it('is refused by a build that knows only schema 5, as a newer ledger always was', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedger(file, { now: clock(), names: names() })
      busyProject(ledger, '/work/app')
      ledger.close()
      const before = contents(file)
      const db = new DatabaseSync(file)
      assert.throws(() => migrate(db, MIGRATIONS.slice(0, 5)), {
        code: 'ledger-newer',
        message: `this home was written by a newer ConsensFlow (schema ${SCHEMA_VERSION}; this build knows 5)`,
      })
      db.close()
      assert.deepEqual(contents(file), before, 'and left as it was')
    })
  })
})

describe('schema 7: a task deleted from the board keeps its row', () => {
  it('adds to the schema 6 had when a task was deleted, and nothing else', async () => {
    await withDir(async (dir) => {
      const [before, after] = [6, 7].map((version) => schemaAt(dir, version))
      const task = (schema) => schema.find((row) => row.name === 'task').sql
      assert.equal(
        task(after),
        task(before).replace('held_until TEXT,', 'held_until TEXT, deleted_at TEXT,'),
      )
      const others = (schema) => schema.filter((row) => row.name !== 'task')
      assert.deepEqual(others(after), others(before), 'nothing else differs')
    })
  })

  it('migrates a schema-6 ledger with every row as it was, and no task deleted', async () => {
    await withDir(async (dir) => {
      const file = ledgerAt(dir, 6)
      const before = contents(file)
      const fresh = path.join(dir, 'fresh.db')
      openLedger(fresh).close()

      openLedger(file).close()
      const after = contents(file)
      assert.equal(after.version, SCHEMA_VERSION)
      assert.deepEqual(
        after.rows,
        { ...before.rows, task: before.rows.task.map((row) => ({ ...row, deleted_at: null })) },
        'every row, with its id and references; every task still on the board',
      )
      assert.deepEqual(after.schema, contents(fresh).schema, "the schema is a fresh ledger's")

      // Its tasks, once finished, leave the board as a new ledger's do.
      const ledger = openLedger(file, { now: clock(), names: names() })
      try {
        const [app] = ledger.projects()
        const lanes = () =>
          ledger
            .board(app.id)
            .lanes.flatMap((lane) => lane.tasks)
            .map((task) => task.number)
        ledger.cancelTask(app.id, 1, { by: 'human' })
        ledger.cancelTask(app.id, 2, { by: 'human' })
        assert.deepEqual(lanes(), [1])
        ledger.deleteTasks(app.id, [1, 2])
        assert.deepEqual(lanes(), [])
      } finally {
        ledger.close()
      }
    })
  })

  it('is refused by a build that knows only schema 6', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const ledger = openLedger(file, { now: clock(), names: names() })
      busyProject(ledger, '/work/app')
      ledger.close()
      const before = contents(file)
      const db = new DatabaseSync(file)
      assert.throws(() => migrate(db, MIGRATIONS.slice(0, 6)), {
        code: 'ledger-newer',
        message: `this home was written by a newer ConsensFlow (schema ${SCHEMA_VERSION}; this build knows 6)`,
      })
      db.close()
      assert.deepEqual(contents(file), before, 'and left as it was')
    })
  })
})
