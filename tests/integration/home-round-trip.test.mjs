import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { SCHEMA_VERSION } from '../../src/ledger/index.js'
import { digestTree, parity, roundTrip, snapshot } from './home-copies.mjs'
import { buildHome } from './home-fixture.mjs'

/**
 * The flip's way back, on a home built by a fixture (tests/integration/home-fixture.mjs):
 * a copy that goes to Node's daemon, then the native one, then Node's again
 * (tests/integration/home-copies.mjs). Each daemon is chosen in its own words,
 * whatever CONSENSFLOW_TEST_DAEMON says, so one run is all three; a leg of
 * `npm run test:daemons` has nothing to add to it. No real home is read.
 */

const LIAR = fileURLToPath(new URL('./liar-daemon.mjs', import.meta.url))
const PARITY = fileURLToPath(new URL('../live/home-parity.mjs', import.meta.url))
/**
 * Settled is four looks in a row, a second apart (a pass of the dispatcher is a
 * second: a window still drawing its screen is not settled sooner). The runs
 * that plant a fault need less: what they look for is in the problems the fault
 * makes, whatever else is still moving.
 */
const SETTLE = { limitMs: 120_000, looks: 4, everyMs: 1000 }
const BRIEF = { limitMs: 120_000, looks: 2, everyMs: 1000 }

describe('a copy of a home that goes Node, then native, then Node again', () => {
  let dir
  let built
  let taken
  let untouched
  const roots = []

  before(() => {
    dir = mkdtempSync(join(tmpdir(), 'cf-home-fixture-'))
    built = buildHome(dir)
    // The home is closed before it is copied, so that this runs on every
    // platform; that a copy of a home in use carries what its write-ahead file
    // holds is tests/home-copies.test.mjs's.
    built.close()
    taken = snapshot(built.home)
    untouched = [digestTree(built.home), digestTree(built.work)]
  })

  after(() => {
    for (const root of [taken, dir, ...roots]) rmSync(root, { recursive: true, force: true })
  })

  /** A trip on the home, its copy to be removed once the suite is over. */
  async function trip(options = {}) {
    const ran = await roundTrip(taken, { settle: SETTLE, ...options })
    roots.push(ran.copy.root)
    return ran
  }
  const ledger = (copy, run) => {
    const db = new DatabaseSync(join(copy.home, 'consensflow.db'))
    try {
      return run(db)
    } finally {
      db.close()
    }
  }

  it('starts each daemon, which reads what the one before left, and touches nothing of the home', async () => {
    const ran = await trip()
    assert.deepEqual(ran.problems, [])

    // The daemons were, in turn, Node's, the native one and Node's: each by its
    // own start line, and the log of the one home says so in order.
    assert.deepEqual(
      ran.starts.map((start) => [start.select, start.daemon.kind]),
      [
        ['node', 'node'],
        ['native', 'native'],
        ['node', 'node'],
      ],
    )
    const log = readFileSync(join(ran.copy.home, 'daemon.log'), 'utf8')
    assert.deepEqual(
      [...log.matchAll(/^\S+ info start pid \d+ (node v|rust )\S+ home /gm)].map((hit) => hit[1]),
      ['node v', 'rust ', 'node v'],
    )
    assert.deepEqual(
      ran.starts.map((start) => start.pid),
      [...log.matchAll(/^\S+ info start pid (\d+) /gm)].map((hit) => Number(hit[1])),
    )
    assert.deepEqual(
      ran.starts.flatMap((start) => start.errors),
      [],
      'nothing was logged as an error',
    )

    // The projects open when the app quit came back, in each start, in windows
    // of the copy: the chief of each (the probe's too).
    const { open } = built.projects
    const probe = ran.copy.probe.project
    const chiefs = [...open, probe].map((id) => `p${id}-chief`).sort()
    for (const start of ran.starts) assert.deepEqual(start.windows, chiefs)
    // Each chief came back on its own conversation, the way back too. One that
    // was never spoken to has none of Claude's to resume (the stand-in writes a
    // transcript at its first turn): its window opens again under the id it had.
    // The probe's chief is spoken to by the native daemon's work, and Node's,
    // on the way back, resumes the conversation that left.
    const asked = `p${probe}-chief`
    for (const id of chiefs) {
      const session = ran.starts[0].sessions[id]
      assert.match(session, /^[\w-]+$/, id)
      assert.deepEqual(
        ran.starts.map((start) => start.sessions[id]),
        [session, session, session],
        id,
      )
      assert.deepEqual(
        ran.starts.map((start) => start.launches[id]),
        id === asked
          ? ['--session-id', '--session-id', '--resume']
          : ['--session-id', '--session-id', '--session-id'],
        id,
      )
    }
    const first = ran.starts[0].entry
    assert.deepEqual(
      first.projects.map((project) => [project.name, project.state]),
      [
        ['site', 'open'],
        ['docs', 'open'],
        ['billing', 'suspended'],
        ['legacy', 'suspended'],
        ['round trip probe', 'open'],
      ],
    )
    // What the closed ones hold is shown whole: a gate with a brief and a
    // question behind it, and a member whose agent left the roster.
    const legacy = first.shown[built.projects.closed[1]].board.board
    assert.equal(legacy.gated.length, 2)
    assert.ok(legacy.lanes.some((lane) => lane.agentMissing === true))

    // The native daemon handed out the probe's task and delivered its result;
    // Node's, on the way back, shows it as the native one left it.
    assert.equal(ran.starts[1].worked, 'ROUND-TRIP')
    const [, native, back] = ran.starts
    assert.notDeepEqual(native.entry.shown[probe], native.exit.shown[probe])
    assert.deepEqual(
      Object.keys(native.exit.shown).filter(
        (id) => JSON.stringify(native.entry.shown[id]) !== JSON.stringify(native.exit.shown[id]),
      ),
      [String(probe)],
      'the work moved the probe and nothing else',
    )
    const results = back.entry.shown[probe].inbox.messages.filter((m) => m.kind === 'result')
    assert.deepEqual(
      results.map((m) => [m.body, m.state]),
      [['ROUND-TRIP', 'delivered']],
    )
    assert.equal(back.entry.shown[probe].board.board.lanes.at(-1).tasks[0].state, 'done')

    // The schema is the one Node's build knows after every start, and the
    // agents file an older build wrote was folded by the first start, once: the
    // two after it found it as it was left, and left it so.
    for (const start of ran.starts) {
      assert.deepEqual(start.facts, { schema: SCHEMA_VERSION, integrity: 'ok', broken: 0 })
    }
    const [one, two, three] = ran.starts
    assert.equal(one.rosterBefore, ran.copy.roster)
    assert.notEqual(one.rosterBefore, one.roster, 'the first start folded the file')
    assert.deepEqual([two.rosterBefore, three.rosterBefore], [one.roster, two.roster])
    assert.deepEqual([two.roster, three.roster], [one.roster, one.roster])

    // The home, and the project folders, were never touched.
    assert.deepEqual([digestTree(built.home), digestTree(built.work)], untouched)
  })

  it('refuses a start whose daemon is not the one it names', async () => {
    const ran = await trip({
      order: ['native'],
      probe: false,
      commands: { 0: JSON.stringify([process.execPath, LIAR]) },
      settle: BRIEF,
    })
    assert.equal(ran.problems.length, 1)
    assert.match(
      ran.problems[0],
      /^start 1 \(native\) did not succeed: .*the native daemon was asked for, but the start line in its log says node v0\.0\.0/,
    )
  })

  // A home from a newer build, which neither daemon may open: the native one
  // refuses it on its standard error and ends with 1, and Node's says so in its
  // log and ends with 0 (its refusal is a rejection it logged); either way the
  // start says why.
  for (const [order, refuses] of [
    [['node', 'native'], 'native'],
    [['native', 'node'], 'node'],
  ]) {
    it(`fails the ${refuses} daemon that cannot open what the one before left`, async () => {
      const ran = await trip({
        order,
        probe: false,
        settle: BRIEF,
        before: {
          1: (copy) => ledger(copy, (db) => db.exec(`PRAGMA user_version = ${SCHEMA_VERSION + 1}`)),
        },
      })
      assert.equal(ran.problems.length, 1, JSON.stringify(ran.problems))
      assert.match(
        ran.problems[0],
        new RegExp(`^start 2 \\(${refuses}\\) did not succeed: .*written by a newer ConsensFlow`),
      )
    })
  }

  it('fails a start that does not read what the one before left', async () => {
    const ran = await trip({
      order: ['node', 'native'],
      probe: false,
      settle: BRIEF,
      // A daemon that lost a note of the human's: the next one shows no such note.
      before: {
        1: (copy) =>
          ledger(copy, (db) =>
            db
              .prepare('DELETE FROM message WHERE body = ?')
              .run('T-1 is accepted: the parser is in.'),
          ),
      },
    })
    const lost = ran.problems.filter((problem) => problem.includes('/shown/1/human/'))
    assert.ok(lost.length > 0, JSON.stringify(ran.problems))
    for (const problem of lost) {
      assert.match(
        problem,
        /^start 2 \(native\) did not read what the one before left: \/shown\/1\/human\/.*start 1 \(node\) left .*start 2 \(native\) read/,
      )
    }
  })

  it('fails a start that opens a window outside its copy', async () => {
    const folder = join(built.work, 'docs')
    const ran = await trip({
      order: ['node'],
      probe: false,
      settle: BRIEF,
      // A copy that kept the project's own folder.
      before: {
        0: (copy) =>
          ledger(copy, (db) =>
            db.prepare("UPDATE project SET directory = ? WHERE name = 'docs'").run(folder),
          ),
      },
    })
    assert.equal(ran.problems.length, 1, JSON.stringify(ran.problems))
    assert.equal(ran.problems[0], `start 1 (node) opened a window in ${folder}, outside its copy`)
    assert.deepEqual(digestTree(built.work), untouched[1], 'the stand-in did not write in it')
  })

  it('shows the same of a home on a copy of its own for each daemon, and folds its agents file alike', async () => {
    const ran = await parity(taken, { settle: SETTLE })
    roots.push(...ran.roots)
    assert.deepEqual([ran.differences, ran.problems], [[], []])
    const { node, native } = ran.runs
    assert.deepEqual([node.daemon.kind, native.daemon.kind], ['node', 'native'])
    for (const run of [node, native]) {
      assert.deepEqual(run.errors, [], run.select)
      assert.deepEqual(run.facts, { schema: SCHEMA_VERSION, integrity: 'ok', broken: 0 })
      assert.equal(run.windows.length, 2, 'the chiefs of the two projects that were open')
    }
    // The file an older build wrote, folded by each daemon on its own copy to the same bytes.
    assert.notEqual(node.rosterBefore, node.roster)
    assert.equal(native.rosterBefore, node.rosterBefore)
    assert.equal(native.roster, node.roster)
    assert.deepEqual([digestTree(built.home), digestTree(built.work)], untouched)
  })

  it('finds what one daemon shows that the other does not', async () => {
    const ran = await parity(taken, {
      settle: BRIEF,
      // A copy that lost a note of the human's, as a daemon that lost it would show.
      before: {
        native: (copy) =>
          ledger(copy, (db) =>
            db
              .prepare('DELETE FROM message WHERE body = ?')
              .run('T-1 is accepted: the parser is in.'),
          ),
      },
    })
    roots.push(...ran.roots)
    assert.deepEqual(ran.problems, [])
    // What is lost is found, as the human's inbox of the project it was lost
    // from, by the side that lost it: whatever else is still moving is noise.
    const lost = ran.differences.filter((found) => found.startsWith('/shown/1/human/'))
    assert.ok(lost.length > 0, JSON.stringify(ran.differences))
    for (const found of lost) assert.match(found, /^\/shown\/1\/human\/.*: Node .*, native /)
  })

  it('fails a daemon that cannot open its copy, and has nothing of its to compare', async () => {
    const ran = await parity(taken, {
      settle: BRIEF,
      // A copy of a newer build's: the daemon that opens it is refused, and says so.
      before: {
        native: (copy) =>
          ledger(copy, (db) => db.exec(`PRAGMA user_version = ${SCHEMA_VERSION + 1}`)),
      },
    })
    roots.push(...ran.roots)
    assert.equal(ran.problems.length, 1, JSON.stringify(ran.problems))
    assert.match(
      ran.problems[0],
      /^the native daemon did not succeed: .*written by a newer ConsensFlow/,
    )
    assert.deepEqual(Object.keys(ran.runs), ['node'])
    assert.deepEqual(ran.differences, [], 'nothing to compare it with')
  })

  it('runs as the command the live home is run with, and says what it did', () => {
    const ran = spawnSync(process.execPath, [PARITY, '--home', built.home, '--looks', '3'], {
      encoding: 'utf8',
      env: process.env,
    })
    assert.equal(ran.status, 0, ran.stdout + ran.stderr)
    // The parity's two copies hold the two open projects, and the trip's the probe too.
    assert.match(
      ran.stdout,
      /^Node \(node v[\d.]+\): settled in \d+ s; 2 windows; 0 errors logged$/m,
    )
    assert.match(
      ran.stdout,
      /^native \(rust [\w.-]+\): settled in \d+ s; 2 windows; 0 errors logged$/m,
    )
    assert.match(ran.stdout, /^0 differences$/m)
    assert.match(ran.stdout, /^1 Node \(node v[\d.]+\): settled in \d+ s; 3 windows; 0 errors/m)
    assert.match(
      ran.stdout,
      /^2 native \(rust [\w.-]+\): settled in \d+ s; 3 windows; 0 errors logged; handed out and delivered ROUND-TRIP$/m,
    )
    assert.match(ran.stdout, /^3 Node \(node v[\d.]+\): settled in \d+ s; 3 windows; 0 errors/m)
    assert.match(ran.stdout, /^0 problems$/m)
    assert.deepEqual([digestTree(built.home), digestTree(built.work)], untouched)
  })
})
