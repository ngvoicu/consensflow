import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { openLedger, SCHEMA_VERSION } from '../src/ledger/index.js'
import { MIGRATIONS } from '../src/ledger/schema.js'
import { clock, names, staff, withDir } from './ledger-fixtures.mjs'

/** The ledger's schema and its migrations (src/ledger/schema.js). */

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
