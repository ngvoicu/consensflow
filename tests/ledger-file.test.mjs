import assert from 'node:assert/strict'
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  addProject,
  fixtureSql,
  loadLedger,
  loadRows,
  MIGRATIONS,
  openLedgerFile,
  SCHEMA_VERSION,
} from './ledger-file.mjs'

/**
 * A ledger file made without the ledger: the native ledger's own migrations
 * run in order, frozen rows loaded into it, and the one project a copy of a
 * home is given by hand.
 */
describe('a ledger file made from the native ledger’s migrations', () => {
  const root = mkdtempSync(join(tmpdir(), 'cf-ledger-file-'))
  after(() => rmSync(root, { recursive: true, force: true }))
  let made = 0
  const file = () => {
    made += 1
    return join(root, `ledger-${made}.db`)
  }
  const pragma = (db, name) => db.prepare(`PRAGMA ${name}`).all()

  it('is at the schema the build knows, which is the number of migrations, and sound', () => {
    const folder = fileURLToPath(new URL('../crates/cf-ledger/migrations/', import.meta.url))
    assert.equal(SCHEMA_VERSION, readdirSync(folder).filter((name) => name.endsWith('.sql')).length)
    assert.equal(MIGRATIONS.length, SCHEMA_VERSION)
    const db = openLedgerFile(file())
    try {
      assert.equal(pragma(db, 'user_version')[0].user_version, SCHEMA_VERSION)
      assert.equal(pragma(db, 'integrity_check')[0].integrity_check, 'ok')
      assert.deepEqual(pragma(db, 'foreign_key_check'), [])
      const tables = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .all()
        .map((row) => row.name)
      for (const table of ['project', 'participant', 'task', 'message', 'transcript', 'event']) {
        assert.ok(tables.includes(table), table)
      }
    } finally {
      db.close()
    }
  })

  it('takes a ledger of an earlier version on from where it was, and leaves one at the version as it is', () => {
    const path = file()
    // The ledger as an older build left it: the first migrations only.
    const old = new DatabaseSync(path)
    for (const [at, migration] of MIGRATIONS.slice(0, 5).entries()) {
      old.exec(migration)
      old.exec(`PRAGMA user_version = ${at + 1}`)
    }
    old.close()
    const db = openLedgerFile(path)
    try {
      assert.equal(pragma(db, 'user_version')[0].user_version, SCHEMA_VERSION)
      // The column migration 9 adds is there.
      assert.ok(pragma(db, 'table_info(participant)').some((column) => column.name === 'designer'))
    } finally {
      db.close()
    }
    const again = openLedgerFile(path)
    try {
      assert.equal(pragma(again, 'user_version')[0].user_version, SCHEMA_VERSION)
    } finally {
      again.close()
    }
  })

  it('refuses a ledger of a newer version, in the words the native ledger says it in', () => {
    const path = file()
    const future = new DatabaseSync(path)
    future.exec(`PRAGMA user_version = ${SCHEMA_VERSION + 1}`)
    future.close()
    assert.throws(
      () => openLedgerFile(path),
      new RegExp(
        `written by a newer ConsensFlow \\(schema ${SCHEMA_VERSION + 1}; this build knows ${SCHEMA_VERSION}\\)`,
      ),
    )
  })

  it('loads the rows of a recording, which leave every reference holding', () => {
    const db = loadLedger(file(), 'controls')
    try {
      assert.deepEqual(pragma(db, 'foreign_key_check'), [])
      assert.equal(db.prepare('SELECT COUNT(*) AS n FROM project').get().n, 1)
      assert.ok(db.prepare('SELECT COUNT(*) AS n FROM message').get().n > 5)
    } finally {
      db.close()
    }
    // A name in the recording that stands for a folder is put where the caller says.
    const home = loadLedger(file(), 'candidate-home', { replacing: { '@WORK@': '/the/work' } })
    try {
      const directories = home
        .prepare('SELECT directory FROM project ORDER BY id')
        .all()
        .map((row) => row.directory)
      assert.deepEqual(
        directories,
        ['site', 'docs', 'billing', 'legacy'].map((n) => `/the/work/${n}`),
      )
    } finally {
      home.close()
    }
    assert.match(fixtureSql('controls'), /^INSERT INTO project /)
    // A folder with a quote in its name (a user called O'Brien) is a string in the rows all the same.
    const quoted = loadLedger(file(), 'candidate-home', { replacing: { '@WORK@': "/o'brien" } })
    try {
      const first = quoted.prepare('SELECT directory FROM project ORDER BY id').get()
      assert.equal(first.directory, "/o'brien/site")
    } finally {
      quoted.close()
    }
  })

  it('refuses rows that leave a reference that does not hold', () => {
    assert.throws(
      () =>
        loadRows(
          file(),
          "INSERT INTO participant (project_id, handle, role, roles, created_at) VALUES (9, 'ghost', 'worker', '[]', 'now');",
        ),
      /leave 1 references that do not hold/,
    )
  })

  describe('a project written by hand, as the ledger’s createProject leaves it', () => {
    const request = {
      directory: '/work/probe',
      name: 'round trip probe',
      chief: { harness: 'claude-code', agent: 'probe-chief' },
      staff: [{ agent: 'probe', harness: 'claude-code', roles: ['worker'], tier: 'standard' }],
    }
    const rows = (db, sql) =>
      db
        .prepare(sql)
        .all()
        .map((row) => ({ ...row }))

    it('has a human, the chief on its agent and each member, and the log of each member’s joining', () => {
      const db = openLedgerFile(file())
      try {
        const id = addProject(db, request)
        assert.deepEqual(
          rows(
            db,
            'SELECT handle, role, roles, agent, harness, designer, tier, left_at FROM participant ORDER BY id',
          ),
          [
            ['human', 'human', '[]', null, null, 0, null],
            ['chief', 'chief', '[]', 'probe-chief', 'claude-code', 0, null],
            ['probe', 'worker', '["worker"]', 'probe', 'claude-code', 0, 'standard'],
          ].map(([handle, role, roles, agent, harness, designer, tier]) => ({
            handle,
            role,
            roles,
            agent,
            harness,
            designer,
            tier,
            left_at: null,
          })),
        )
        assert.deepEqual(rows(db, 'SELECT id, name, directory, state, gate FROM project'), [
          { id, name: 'round trip probe', directory: '/work/probe', state: 'open', gate: 0 },
        ])
        // The member's joining is logged before the project is, as the ledger does.
        assert.deepEqual(
          rows(db, 'SELECT project_id, kind, data FROM event ORDER BY id').map((event) => [
            event.project_id,
            event.kind,
            JSON.parse(event.data),
          ]),
          [
            [
              id,
              'member.added',
              { handle: 'probe', role: 'worker', roles: ['worker'], harness: 'claude-code' },
            ],
            [id, 'project.created', { name: 'round trip probe', directory: '/work/probe' }],
          ],
        )
      } finally {
        db.close()
      }
    })

    it('is all or nothing: a member that cannot join leaves no project behind', () => {
      const db = openLedgerFile(file())
      try {
        const twice = { ...request, staff: [...request.staff, ...request.staff] }
        assert.throws(() => addProject(db, twice), /UNIQUE|constraint/i)
        assert.equal(db.prepare('SELECT COUNT(*) AS n FROM project').get().n, 0)
        assert.equal(db.prepare('SELECT COUNT(*) AS n FROM event').get().n, 0)
      } finally {
        db.close()
      }
    })
  })
})

/**
 * A recording of a real run holds the folder its project worked in and the
 * listings its windows made, so the home folder and the user of the machine
 * that made it: neither is a fixture's to carry. A recording names the home
 * folder `/home/user` and the user `user` (tests/goldens/evals/record.mjs).
 */
describe('the frozen ledgers', () => {
  const folder = fileURLToPath(new URL('./fixtures/ledgers/', import.meta.url))
  const texts = readdirSync(folder).map((name) => [name, readFileSync(join(folder, name), 'utf8')])

  /** What `pattern` finds in a text, by its first group. */
  const found = (text, pattern) => [...text.matchAll(pattern)].map((hit) => hit[1])
  /** A user's name: words, hyphens and dots between them, so the quote or the stop after it is not part of it. */
  const NAME = String.raw`([\w-]+(?:\.[\w-]+)*)`
  const HOME_OF = [
    new RegExp(`/(?:Users|home)/${NAME}`, 'g'),
    new RegExp(String.raw`[A-Za-z]:[\\/]+Users[\\/]+${NAME}`, 'g'),
  ]
  /** The owner column of an `ls -l` listing: `drwxr-xr-x@ 3 owner  group   96 Oct  8 15:31 name`. */
  const OWNER = /(?:^|[\s'])[-dlcbps][-rwxsStT]{9}[@+.]? +\d+ +(\S+) +\S+ +\d/gm

  it('has the recordings to hold', () => {
    assert.ok(texts.length >= 20, `${texts.length} files`)
    assert.ok(texts.some(([name]) => name === 'eval-round-trip.sql'))
  })

  it('say nothing of who made them: the home folder of a machine is /home/user, and its user is user', () => {
    for (const [name, text] of texts) {
      const homes = HOME_OF.flatMap((pattern) => found(text, pattern))
      assert.deepEqual(
        homes.filter((owner) => owner !== 'user'),
        [],
        `${name} holds the home folder of a person`,
      )
      assert.deepEqual(
        found(text, OWNER).filter((owner) => owner !== 'user'),
        [],
        `${name} holds a listing made by a person`,
      )
    }
  })

  it('are held to that by a pattern that finds a person, a home folder and a listing', () => {
    const real = "'/Users/someone/work', 'C:\\Users\\someone\\work', '/home/someone/x'"
    assert.deepEqual(
      HOME_OF.flatMap((pattern) => found(real, pattern)),
      ['someone', 'someone', 'someone'],
    )
    const listing =
      "'\ndrwxr-xr-x@ 3 someone  staff   96 Oct  8 15:31 content\n-rw-r--r-- 1 user  staff  1.0K Oct  8 15:31 a'"
    assert.deepEqual(found(listing, OWNER), ['someone', 'user'])
  })
})
