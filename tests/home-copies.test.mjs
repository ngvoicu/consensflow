import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { after, before, describe, it } from 'node:test'
import { errorLines, errorsWithCause, linesOf } from './choice.mjs'
import {
  copyHome,
  differences,
  digestTree,
  mask,
  snapshot,
  within,
} from './integration/home-copies.mjs'
import { buildHome } from './integration/home-fixture.mjs'

/**
 * What a run on a copy of a home is made of, without a daemon (the daemons are
 * started by tests/integration/home-round-trip.test.mjs).
 */

const seen = (more = {}) => ({
  root: '/tmp/copy',
  handles: new Set(['human', 'chief', 'zeus']),
  started: Date.parse('2026-10-06T10:00:00.000Z'),
  last: 40,
  ...more,
})

describe('what a copy shows, masked', () => {
  it('leaves what was written before the run, and says the times since as one', () => {
    const shown = { before: '2026-10-06T09:59:59.999Z', since: '2026-10-06T10:00:00.000Z' }
    assert.deepEqual(mask(shown, seen()), { before: shown.before, since: '«now»' })
  })

  it('says the root, the tokens and the ports as what they are, and nothing else', () => {
    const token = 'ab'.repeat(24)
    assert.equal(
      mask(`/tmp/copy/projects/1 on 127.0.0.1:51234 with ${token}`, seen()),
      '«root»/projects/1 on 127.0.0.1:«port» with «token»',
    )
    assert.equal(mask('abc123', seen()), 'abc123')
    assert.equal(mask(42, seen()), 42)
    assert.equal(mask(null, seen()), null)
  })

  it('says a session made since by what it is, and keeps the handles the copy held', () => {
    const held = seen({ handles: new Set(['human', 'chief', 'zeus', 'zeus-amber-pine']) })
    assert.equal(mask('worker-brisk-birch', held), '«new session»')
    assert.equal(mask('zeus-amber-pine', held), 'zeus-amber-pine')
    assert.equal(mask('@zeus and @chief', held), '@zeus and @chief')
    assert.equal(mask('brisk-birch', held), 'brisk-birch', 'two words are no session')
    assert.equal(mask('Worker-Brisk-Birch', held), 'Worker-Brisk-Birch')
  })

  it('says the ids of messages written since as new, and puts them first in a fixed order', () => {
    const shown = {
      messages: [
        { id: 41, body: 'b' },
        { id: 39, body: 'old' },
        { id: 42, body: 'a', replyTo: 41 },
      ],
    }
    assert.deepEqual(mask(shown, seen()), {
      messages: [
        { id: '«new»', body: 'a', replyTo: '«new»' },
        { id: '«new»', body: 'b' },
        { id: 39, body: 'old' },
      ],
    })
    // Another list keeps its order, and its ids are the ids of rows that were there.
    assert.deepEqual(mask({ tasks: [{ id: 41 }, { id: 39 }] }, seen(), null), {
      tasks: [{ id: '«new»' }, { id: 39 }],
    })
  })

  it('says a pane’s generation, and a receipt’s process, as what they are', () => {
    assert.deepEqual(mask({ pane: { generation: 1760000000123, id: 'p1-chief' } }, seen()), {
      pane: { generation: '«generation»', id: 'p1-chief' },
    })
    assert.deepEqual(mask({ receipt: { item: 'sess-4242-7' } }, seen()), {
      receipt: { item: 'sess-«pid»-7' },
    })
  })
})

describe('where two shows differ', () => {
  it('names each place by its path, and whose each side is', () => {
    const a = { projects: [{ id: 1, state: 'open' }], windows: ['p1-chief'], same: 1 }
    const b = {
      projects: [{ id: 1, state: 'suspended' }],
      windows: ['p1-chief', 'p2-chief'],
      same: 1,
    }
    assert.deepEqual(
      [...differences(a, b)],
      [
        '/projects/0/state: Node "open", native "suspended"',
        '/windows/1: Node undefined, native "p2-chief"',
      ],
    )
    assert.deepEqual([...differences(a, a)], [])
    assert.deepEqual([...differences(1, 2, '/x', ['before', 'after'])], ['/x: before 1, after 2'])
    // An object is not a list, and a missing side is a difference.
    assert.equal([...differences({}, [])].length, 1)
    assert.equal([...differences(null, { a: 1 })].length, 1)
  })
})

describe('what one daemon wrote in a log that every start writes to', () => {
  const log = [
    '2026-10-06T10:00:00.000Z info start pid 11 node v26.8.1 home /h',
    '2026-10-06T10:00:01.000Z error a pass failed',
    '    at stack line that says error handling',
    '2026-10-06T10:00:02.000Z info stop: stdin ended; rss 90 MB',
    '2026-10-06T10:01:00.000Z info start pid 12 rust 3.0.0 home /h',
    '2026-10-06T10:01:01.000Z warn a pass took 6000 ms',
    '2026-10-06T10:01:02.000Z error the bridge failed',
    '2026-10-06T10:02:00.000Z info start pid 13 node v26.8.1 home /h',
    '',
  ].join('\n')

  it('is the lines from its start to the next start', () => {
    assert.deepEqual(linesOf(log, 11).length, 4)
    assert.equal(linesOf(log, 12).length, 3)
    assert.deepEqual(linesOf(log, 13), [
      '2026-10-06T10:02:00.000Z info start pid 13 node v26.8.1 home /h',
    ])
    assert.deepEqual(linesOf(log, 99), [])
  })

  it('says an error only for a line whose level it is', () => {
    assert.deepEqual(errorLines(linesOf(log, 11)), ['2026-10-06T10:00:01.000Z error a pass failed'])
    assert.deepEqual(errorLines(linesOf(log, 12)), [
      '2026-10-06T10:01:02.000Z error the bridge failed',
    ])
    assert.deepEqual(errorLines(linesOf(log, 13)), [])
  })

  it('says what is logged under an error with it, when something is', () => {
    assert.deepEqual(errorsWithCause(linesOf(log, 11)), [
      '2026-10-06T10:00:01.000Z error a pass failed at stack line that says error handling',
    ])
    assert.deepEqual(errorsWithCause(linesOf(log, 12)), [
      '2026-10-06T10:01:02.000Z error the bridge failed',
    ])
    assert.deepEqual(errorsWithCause(linesOf(log, 13)), [])
  })
})

describe('a folder left as it was', () => {
  let dir
  before(() => {
    dir = mkdtempSync(join(tmpdir(), 'cf-digest-'))
  })
  after(() => rmSync(dir, { recursive: true, force: true }))

  it('has one digest for what it holds, however it was filled', () => {
    const [one, two] = [join(dir, 'one'), join(dir, 'two')]
    for (const folder of [one, two]) mkdirSync(join(folder, 'sub'), { recursive: true })
    writeFileSync(join(one, 'a.txt'), 'a')
    writeFileSync(join(one, 'sub', 'b.txt'), 'b')
    writeFileSync(join(two, 'sub', 'b.txt'), 'b')
    writeFileSync(join(two, 'a.txt'), 'a')
    assert.equal(digestTree(one), digestTree(two))
  })

  it('changes with a byte, a name, a file added or taken away, and an empty folder', () => {
    const folder = join(dir, 'held')
    mkdirSync(folder)
    writeFileSync(join(folder, 'a.txt'), 'a')
    const digests = new Set([digestTree(folder)])
    writeFileSync(join(folder, 'a.txt'), 'b')
    digests.add(digestTree(folder))
    writeFileSync(join(folder, 'c.txt'), 'b')
    digests.add(digestTree(folder))
    rmSync(join(folder, 'a.txt'))
    digests.add(digestTree(folder))
    mkdirSync(join(folder, 'empty'))
    digests.add(digestTree(folder))
    assert.equal(digests.size, 5)
  })

  it('says a folder is within a root when it is the root or under it, not beside it', () => {
    assert.ok(within('/a/root', '/a/root'))
    assert.ok(within('/a/root', '/a/root/projects/1'))
    assert.ok(!within('/a/root', '/a/root-two'))
    assert.ok(!within('/a/root', '/a/other'))
    assert.ok(!within('/a/root', '/'))
  })
})

describe('a copy of a home', () => {
  let dir
  let built
  let taken
  let untouched
  const roots = []
  before(() => {
    dir = mkdtempSync(join(tmpdir(), 'cf-copy-fixture-'))
    built = buildHome(dir)
    built.close()
    taken = snapshot(built.home)
    untouched = [digestTree(built.home), digestTree(built.work)]
  })
  after(() => {
    for (const root of [dir, taken, ...roots]) rmSync(root, { recursive: true, force: true })
  })
  const copied = (options) => {
    const copy = copyHome(taken, options)
    roots.push(copy.root)
    return copy
  }
  const projects = (copy) => {
    const db = new DatabaseSync(join(copy.home, 'consensflow.db'), { readOnly: true })
    try {
      return db
        .prepare('SELECT id, name, directory FROM project ORDER BY id')
        .all()
        .map((row) => ({ ...row }))
    } finally {
      db.close()
    }
  }

  it('holds every project of the snapshot, each in a folder under its own root', () => {
    const copy = copied()
    const held = projects(copy)
    assert.deepEqual(
      held.map((project) => project.name),
      ['site', 'docs', 'billing', 'legacy'],
    )
    for (const project of held) {
      assert.ok(within(copy.root, project.directory), project.directory)
      assert.ok(!within(built.work, project.directory), 'not one of the home’s own folders')
    }
    assert.equal(copy.probe, null)
    assert.deepEqual([digestTree(built.home), digestTree(built.work)], untouched)
  })

  it('knows what the masks need of it: the handles it holds and its last message', () => {
    const { seen: known } = copied()
    assert.equal(known.started, 0)
    for (const handle of ['human', 'chief', 'builder', 'retired-one', 'zeus', 'diana']) {
      assert.ok(known.handles.has(handle), handle)
    }
    // Read from the snapshot, not the home: reading a ledger in write-ahead mode
    // makes files beside it, and the home must be left as it is.
    const db = new DatabaseSync(join(taken, 'consensflow.db'), { readOnly: true })
    const { last } = db.prepare('SELECT max(id) AS last FROM message').get()
    db.close()
    assert.equal(known.last, last)
  })

  it('adds the probe, a project of the trip’s own with agents of its own, when asked', () => {
    const copy = copied({ probe: true })
    const held = projects(copy)
    assert.deepEqual(held.at(-1), {
      id: copy.probe.project,
      name: 'round trip probe',
      directory: copy.probe.directory,
    })
    assert.ok(within(copy.root, copy.probe.directory))
    const roster = JSON.parse(readFileSync(join(copy.home, 'agents.json'), 'utf8'))
    assert.deepEqual(
      roster.agents.filter((agent) => agent.id.startsWith('probe')).map((agent) => agent.id),
      ['probe-chief', 'probe'],
    )
    assert.ok(copy.seen.handles.has('probe'), 'its worker is a handle the masks know')
    // The agents file the copy starts with is the one it was given, and the probe's agents.
    assert.notEqual(copy.roster, copied().roster)
    // None of it reached the home.
    assert.deepEqual([digestTree(built.home), digestTree(built.work)], untouched)
  })

  /** A snapshot of the same home with an agents file of the test's own. */
  const withAgents = (text) => {
    const folder = snapshot(built.home)
    roots.push(folder)
    writeFileSync(join(folder, 'agents.json'), text)
    return folder
  }

  it('refuses a home that has an agent of the probe’s names, as the human’s own', () => {
    const agents = [{ id: 'probe', kind: 'codex', model: 'gpt-5.6-luna' }]
    assert.throws(
      () => copyHome(withAgents(JSON.stringify({ schemaVersion: 1, agents })), { probe: true }),
      /the home has an agent named probe, which the trip's probe runs on/,
    )
    // A copy for parity adds nothing to the agents, and is not refused.
    assert.doesNotThrow(() => copied().root)
  })

  it('refuses an agents file that is not JSON, and takes a home with none', () => {
    assert.throws(
      () => copyHome(withAgents('{"agents": ['), { probe: true }),
      /agents\.json is not JSON: the trip cannot add its probe's agents to it/,
    )
    const bare = snapshot(built.home)
    roots.push(bare)
    rmSync(join(bare, 'agents.json'))
    const copy = copyHome(bare, { probe: true })
    roots.push(copy.root)
    const roster = JSON.parse(readFileSync(join(copy.home, 'agents.json'), 'utf8'))
    assert.deepEqual(
      roster.agents.map((agent) => agent.id),
      ['probe-chief', 'probe'],
    )
  })
})

describe('a snapshot of a home in use', () => {
  // The ledger's own file holds none of what was written since it went into
  // write-ahead mode: a snapshot that left the other file out would be a home
  // with nothing in it. Whether Windows lets an open ledger's files be copied
  // is not known here, so the suites above copy a home that is closed.
  const windows = process.platform === 'win32' && "Windows' hold on an open ledger's files"
  it('carries the ledger’s write-ahead file, and a copy shows what it held', {
    skip: windows,
  }, () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-in-use-'))
    const roots = [dir]
    try {
      const built = buildHome(dir)
      let taken
      try {
        taken = snapshot(built.home)
      } finally {
        built.close()
      }
      roots.push(taken)
      assert.ok(statSync(join(taken, 'consensflow.db-wal')).size > 0)
      const copy = copyHome(taken)
      roots.push(copy.root)
      const db = new DatabaseSync(join(copy.home, 'consensflow.db'), { readOnly: true })
      try {
        const names = db.prepare('SELECT name FROM project ORDER BY id').all()
        assert.deepEqual(
          names.map((row) => row.name),
          ['site', 'docs', 'billing', 'legacy'],
        )
      } finally {
        db.close()
      }
      // Without it, the same ledger file is a home with no project table at all.
      rmSync(join(taken, 'consensflow.db-wal'))
      assert.throws(() => copyHome(taken), /no such table: project/)
    } finally {
      for (const root of roots) rmSync(root, { recursive: true, force: true })
    }
  })
})
