import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import {
  assertApp,
  assertLedgerHeld,
  assertOnlyProbesRefused,
  assertStartedDaemon,
  daemonEvidence,
  daemonRows,
  kindOfCommand,
} from './updater-smoke/evidence.mjs'
import {
  assertKept,
  assertProjects,
  assertSound,
  assertTraced,
  readLedger,
  tracedEvents,
} from './updater-smoke/ledger.mjs'

/**
 * The updater smoke's readers of evidence, held to what each exists to refuse.
 * The smoke itself needs two built apps and minutes; what it takes as proof that
 * an app is up, that its daemon is ready and alone, that the ledger has one
 * holder and that the ledger is whole is decided here, on what a machine looks
 * like when each is true and when it is not (the terminal command's is in
 * tests/updater-smoke-launchers.test.mjs). `npm run plants:release -- updater`
 * takes each check out and expects a test of these to fail.
 */

const APP = '/box/Applications/ConsensFlow.app'
const NODE_DAEMON = `${APP}/Contents/MacOS/node ${APP}/Contents/Resources/cli/bin/cf.mjs ui --json --no-open`
const NATIVE_DAEMON = `${APP}/Contents/Resources/cli/bin/cf ui --json --no-open`
const row = (pid, ppid, command, state = 'S') => ({ pid, ppid, state, command })
const START = {
  node: (pid) => `2026-10-07T05:33:49.964Z info start pid ${pid} node v26.7.0 home /box/state\n`,
  rust: (pid) =>
    `2026-10-07T05:33:52.788Z info start pid ${pid} rust 3.0.0-alpha.81 home /box/state\n`,
}
const refused = (pid) => `${START.rust(pid)}2026-10-07T05:33:52.790Z info exit 1\n`

describe('the app and its daemon, as the process table shows them', () => {
  const app = row(100, 1, `${APP}/Contents/MacOS/app`)

  it('is Node’s daemon when the app’s child runs cf.mjs and the log says Node', () => {
    const found = daemonEvidence({
      log: START.node(200),
      table: [app, row(200, 100, NODE_DAEMON)],
      app: 100,
      bundle: APP,
    })
    assert.deepEqual([found.pid, found.kind, found.runtime], [200, 'node', 'node v26.7.0'])
  })

  it('is the native daemon when the app’s child runs cf and the log says Rust', () => {
    const found = daemonEvidence({
      log: START.rust(300),
      table: [app, row(300, 100, NATIVE_DAEMON)],
      app: 100,
      bundle: APP,
    })
    assert.deepEqual([found.pid, found.kind], [300, 'native'])
  })

  it('takes the daemon an update started after the one it replaced, whose start line is still in the log', () => {
    const found = daemonEvidence({
      log: START.node(200) + START.rust(300),
      table: [app, row(300, 100, NATIVE_DAEMON)],
      app: 100,
      bundle: APP,
    })
    assert.equal(found.pid, 300)
  })

  it('refuses an app with no daemon of its own running: nothing is ready yet', () => {
    assert.throws(
      () => daemonEvidence({ log: START.node(200), table: [app], app: 100, bundle: APP }),
      /has 0 daemons/,
    )
  })

  it('refuses a daemon that is not the app’s child', () => {
    assert.throws(
      () =>
        daemonEvidence({
          log: START.node(200),
          table: [app, row(200, 1, NODE_DAEMON)],
          app: 100,
          bundle: APP,
        }),
      /has 0 daemons/,
    )
  })

  it('refuses a daemon of another bundle: it is not this app’s', () => {
    assert.throws(
      () =>
        daemonEvidence({
          log: START.node(200),
          table: [app, row(200, 100, NODE_DAEMON.replaceAll(APP, '/elsewhere/ConsensFlow.app'))],
          app: 100,
          bundle: APP,
        }),
      /has 0 daemons/,
    )
  })

  it('refuses a home with a second daemon running: one daemon holds the ledger', () => {
    assert.throws(
      () =>
        daemonEvidence({
          log: START.node(200) + START.node(201),
          table: [app, row(200, 100, NODE_DAEMON), row(201, 77, NODE_DAEMON)],
          app: 100,
          bundle: APP,
        }),
      /one daemon serves the home/,
    )
  })

  it('refuses a daemon that never logged its start', () => {
    assert.throws(
      () =>
        daemonEvidence({
          log: '',
          table: [app, row(200, 100, NODE_DAEMON)],
          app: 100,
          bundle: APP,
        }),
      /logged no start line/,
    )
    assert.throws(
      () =>
        daemonEvidence({
          log: START.node(999),
          table: [app, row(200, 100, NODE_DAEMON)],
          app: 100,
          bundle: APP,
        }),
      /logged no start line/,
    )
  })

  it('refuses a log that says one daemon where the process is the other', () => {
    assert.throws(
      () =>
        daemonEvidence({
          log: START.rust(200),
          table: [app, row(200, 100, NODE_DAEMON)],
          app: 100,
          bundle: APP,
        }),
      /the log says rust/,
    )
  })

  it('refuses a daemon that logged an error', () => {
    assert.throws(
      () =>
        daemonEvidence({
          log: `${START.rust(300)}2026-10-07T05:33:53.000Z error the agents file could not be used\n`,
          table: [app, row(300, 100, NATIVE_DAEMON)],
          app: 100,
          bundle: APP,
        }),
      /logged errors/,
    )
  })

  it('refuses an earlier daemon of the app that was refused its start, and not a probe’s refusal', () => {
    const log = refused(250) + START.rust(300)
    const table = [app, row(300, 100, NATIVE_DAEMON)]
    assert.throws(
      () => daemonEvidence({ log, table, app: 100, bundle: APP }),
      /failed to start/,
      'the ledger refused a daemon the app started',
    )
    const found = daemonEvidence({ log, table, app: 100, bundle: APP, probes: [250] })
    assert.equal(found.pid, 300, 'a second ConsensFlow the smoke tried is refused by design')
  })

  it('reads a process by its command: Node’s daemon runs cf.mjs, the native one is cf', () => {
    assert.equal(kindOfCommand(NODE_DAEMON), 'node')
    assert.equal(kindOfCommand(NATIVE_DAEMON), 'native')
    assert.equal(daemonRows([row(1, 0, `${APP}/Contents/MacOS/app`)], APP).length, 0)
  })

  it('takes the app for the process that is the bundle’s executable, and no other', () => {
    assert.equal(assertApp([app], 100, `${APP}/Contents/MacOS/app`).pid, 100)
    assert.throws(() => assertApp([], 100, `${APP}/Contents/MacOS/app`), /is not running/)
    assert.throws(
      () => assertApp([row(100, 1, '/bin/sleep 5')], 100, `${APP}/Contents/MacOS/app`),
      /is not the app/,
    )
    assert.throws(
      () =>
        assertApp(
          [row(100, 1, `${APP}/Contents/MacOS/app`, 'Z')],
          100,
          `${APP}/Contents/MacOS/app`,
        ),
      /is not running/,
    )
  })
})

describe('the app’s log naming the daemon it started', () => {
  // Paths are made as the platform makes them: the app writes them so.
  const CF = join(
    '/box',
    'Applications',
    'ConsensFlow.app',
    'Contents',
    'Resources',
    'cli',
    'bin',
    'cf',
  )
  const said = (cf) => `consensflow: starting the daemon: ${cf} ui --json --no-open`
  const FLIPS =
    'consensflow: starting the native daemon: the default, there is no /box/state/use-node'

  it('says the bundle’s cf, by its path', () => {
    assertStartedDaemon(`${said(CF)}\n`, CF)
    // The log is written on by every app of the home: the line of the one that started it is among them.
    assertStartedDaemon(`${FLIPS}\n${said(CF)}\nconsensflow: something else\n`, CF)
  })

  it('does not take the flip’s sentence, another bundle’s cf, or silence', () => {
    for (const log of [
      '',
      `${FLIPS}\n`,
      `consensflow: starting Node's daemon: /box/state/use-node is there, the way back to Node\n`,
      `${said(join('/box', 'Other.app', 'Contents', 'Resources', 'cli', 'bin', 'cf'))}\n`,
      'consensflow: the terminal command is not repaired\n',
    ]) {
      assert.throws(() => assertStartedDaemon(log, CF), /does not say the app started/, log)
    }
  })
})

describe('the daemons of a home, once every app is gone', () => {
  const log = START.node(200) + START.rust(300) + refused(250)

  it('were refused nothing but the probes the smoke tried', () => {
    assertOnlyProbesRefused(log, new Set([250]))
    assertOnlyProbesRefused(START.node(200) + START.rust(300), new Set())
  })

  it('say so when a daemon the app started was refused the ledger, or a probe was not', () => {
    assert.throws(() => assertOnlyProbesRefused(log, new Set()), /were refused the ledger/)
    assert.throws(
      () => assertOnlyProbesRefused(START.node(200) + refused(250), new Set([250, 251])),
      /were refused the ledger/,
    )
  })
})

describe('the ledger’s one holder, as a second ConsensFlow finds it', () => {
  const DB = '/box/state/consensflow.db'
  const held = {
    code: 1,
    signal: null,
    out: '',
    err: `cf: another ConsensFlow has ${DB} open\n`,
  }

  it('is refused in the ledger’s own words, with no handle line out', () => {
    assertLedgerHeld(held, DB)
  })

  it('is no proof when the second one ran, or was refused for something else', () => {
    for (const [what, attempt, words] of [
      ['a second owner', { ...held, code: 0, err: '' }, /ended 0/],
      ['a killed probe', { ...held, code: null, signal: 'SIGKILL' }, /never ended/],
      [
        'a missing program',
        { ...held, err: 'cf: command not found\n' },
        /not for the ledger's lock/,
      ],
      [
        'another home’s ledger',
        { ...held, err: 'cf: another ConsensFlow has /elsewhere/consensflow.db open\n' },
        /not for the ledger's lock/,
      ],
      [
        'a handle line',
        { ...held, out: '{"url":"http://x","token":"t"}\n' },
        /printed a handle line/,
      ],
    ]) {
      assert.throws(() => assertLedgerHeld(attempt, DB), words, what)
    }
  })
})

/** A ledger with the tables the smoke reads, and the rows a run of it leaves. */
function withLedger(body, { version = 10, rows } = {}) {
  const folder = mkdtempSync(join(tmpdir(), 'cf-ledger-evidence-'))
  try {
    const file = join(folder, 'consensflow.db')
    const db = new DatabaseSync(file)
    db.exec(`
      CREATE TABLE project (id INTEGER PRIMARY KEY, directory TEXT NOT NULL, name TEXT NOT NULL, state TEXT NOT NULL, updated_at TEXT NOT NULL);
      CREATE TABLE participant (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL REFERENCES project (id), handle TEXT NOT NULL, left_at TEXT);
      CREATE TABLE event (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL REFERENCES project (id), at TEXT NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
      PRAGMA user_version = ${version};
    `)
    for (const sql of rows ?? [
      "INSERT INTO project VALUES (1, '/w/a', 'a', 'open', 't1'), (2, '/w/b', 'b', 'open', 't1')",
      "INSERT INTO participant VALUES (1, 1, 'chief', NULL), (2, 2, 'chief', NULL)",
      `INSERT INTO event VALUES (1, 1, 't1', 'project.created', '{"name":"a"}'), (2, 2, 't2', 'project.created', '{"name":"b"}')`,
    ]) {
      db.exec(sql)
    }
    db.close()
    return body(file)
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
}

/** The ledger as `body` leaves it, then read. */
const changed = (file, ...statements) => {
  const db = new DatabaseSync(file)
  for (const sql of statements) db.exec(sql)
  db.close()
  return readLedger(file)
}

describe('the ledger, read from outside once no daemon holds it', () => {
  it('is whole: sound, its projects there, and nothing it held lost or changed', () => {
    withLedger((file) => {
      const before = readLedger(file)
      assertSound(before, { atLeast: 10 })
      assertProjects(before, ['/w/a', '/w/b'])
      const after = changed(
        file,
        "UPDATE project SET state = 'suspended', updated_at = 't9'",
        "UPDATE participant SET left_at = 't9'",
        "INSERT INTO event VALUES (3, 1, 't9', 'project.state', '{}')",
        'PRAGMA user_version = 11',
      )
      assertKept(before, after)
    })
  })

  it('is no longer whole when a row is gone, a stable column changed, a table lost or the schema went back', () => {
    const lost = [
      [
        'a project is gone',
        [
          'DELETE FROM event WHERE project_id = 2',
          'DELETE FROM participant WHERE project_id = 2',
          'DELETE FROM project WHERE id = 2',
        ],
        /(event|participant|project) 2 is gone/,
      ],
      [
        'a project’s directory changed',
        ["UPDATE project SET directory = '/w/other' WHERE id = 1"],
        /project 1: directory was "\/w\/a"/,
      ],
      ['an event was rewritten', ["UPDATE event SET data = '{}' WHERE id = 1"], /event 1: data/],
      ['a table is gone', ['DROP TABLE participant'], /lost its participant table/],
      ['the schema went back', ['PRAGMA user_version = 9'], /schema is at 9, below 10/],
    ]
    for (const [what, statements, words] of lost) {
      withLedger((file) => {
        const before = readLedger(file)
        assert.throws(() => assertKept(before, changed(file, ...statements)), words, what)
      })
    }
  })

  it('is not sound when its schema is below the older daemon’s, a reference is broken or it holds no table', () => {
    withLedger(
      (file) => {
        assert.throws(() => assertSound(readLedger(file), { atLeast: 10 }), /below 10/)
      },
      { version: 9 },
    )
    withLedger((file) => {
      const orphan = changed(
        file,
        'PRAGMA foreign_keys = OFF',
        "INSERT INTO event VALUES (9, 77, 't', 'x', '{}')",
      )
      assert.throws(() => assertSound(orphan), /reference of the ledger/)
    })
    assert.throws(
      () => assertSound({ integrity: ['ok'], references: [], version: 10, tables: {} }),
      /holds no table/,
    )
    assert.throws(
      () =>
        assertSound({
          integrity: ['*** in database main ***'],
          references: [],
          version: 10,
          tables: { a: [] },
        }),
      /integrity check/,
    )
  })

  it('has a project for each directory asked for, and says which has none', () => {
    withLedger((file) => {
      assert.throws(
        () => assertProjects(readLedger(file), ['/w/a', '/w/missing']),
        /no project in \/w\/missing/,
      )
    })
  })

  it('reads the events a daemon traced from its trace file, and only the ledger’s', () => {
    const lines = [
      '{"at":"t1","project":1,"kind":"project.created","data":{"name":"a"}}',
      '{"at":"t1","kind":"window.activity","project":1,"participant":"chief","state":"idle"}',
      'not json',
      '',
      '{"at":"t2","project":2,"kind":"project.created","data":{"name":"b"}}',
    ].join('\n')
    const events = tracedEvents(lines)
    assert.deepEqual(
      events.map((event) => [event.project, event.kind]),
      [
        [1, 'project.created'],
        [2, 'project.created'],
      ],
    )
    withLedger((file) => {
      const ledger = readLedger(file)
      assertTraced(events, ledger)
      assert.throws(
        () => assertTraced([{ at: 't3', project: 1, kind: 'project.state', data: {} }], ledger),
        /lost an event/,
      )
      assert.throws(
        () =>
          assertTraced(
            [{ at: 't1', project: 1, kind: 'project.created', data: { name: 'x' } }],
            ledger,
          ),
        /lost an event/,
      )
    })
  })
})
