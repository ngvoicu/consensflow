import assert from 'node:assert/strict'
import { existsSync, statSync } from 'node:fs'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { eventTrace } from '../src/core/trace.js'

/** The event file in the home: what the daemon does, one JSON line each, for whoever watches from outside. */
describe('the event trace', () => {
  it('appends one JSON line per entry, dated unless the entry is', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-trace-'))
    try {
      const trace = eventTrace(dir)
      trace({ kind: 'window.activity', participant: 'chief', state: 'idle' })
      trace({ at: '2026-09-22T10:00:00.000Z', kind: 'task.opened', project: 1, data: { task: 1 } })
      const lines = (await readFile(path.join(dir, 'events.jsonl'), 'utf8')).trim().split('\n')
      const [first, second] = lines.map((line) => JSON.parse(line))
      assert.equal(lines.length, 2)
      assert.match(first.at, /^\d{4}-\d\d-\d\dT/)
      assert.deepEqual(
        [first.kind, first.participant, first.state],
        ['window.activity', 'chief', 'idle'],
      )
      assert.deepEqual(second, {
        at: '2026-09-22T10:00:00.000Z',
        kind: 'task.opened',
        project: 1,
        data: { task: 1 },
      })
      // A trace that cannot write never troubles the daemon.
      eventTrace(path.join(dir, 'missing', 'deeper'))({ kind: 'x' })
      // A deleted project's lines go; the record of the deletion, which names
      // no project, and everyone else's stay.
      trace({ kind: 'task.opened', project: 2, data: { task: 1 } })
      trace({ kind: 'project.deleted', project: null, data: { id: 1, name: 'app' } })
      trace.forget(1)
      const after = (await readFile(path.join(dir, 'events.jsonl'), 'utf8'))
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line))
      assert.deepEqual(
        after.map((entry) => [entry.kind, entry.project]),
        [
          ['window.activity', undefined],
          ['task.opened', 2],
          ['project.deleted', null],
        ],
      )
      eventTrace(path.join(dir, 'missing')).forget(1)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })

  it('keeps one older file past its limit, and forgets a deleted project in both', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-trace-'))
    try {
      const trace = eventTrace(dir, { limit: 300 })
      const file = path.join(dir, 'events.jsonl')
      // Lines of some 80 bytes past a 300-byte limit: the file turns over
      // every four, the one before it is kept, and no older one.
      for (let task = 1; task <= 12; task += 1) {
        trace({ kind: 'task.opened', project: (task % 2) + 1, data: { task } })
      }
      assert.ok(statSync(file).size < 400, 'the file stays near its limit')
      assert.ok(existsSync(`${file}.1`), 'the full one was kept aside')
      assert.ok(!existsSync(`${file}.2`), 'and only one')
      const entries = async (name) =>
        (await readFile(name, 'utf8'))
          .trim()
          .split('\n')
          .map((line) => JSON.parse(line))
      const tasks = async (name, project) =>
        (await entries(name)).filter((e) => e.project === project).map((e) => e.data.task)
      assert.deepEqual(await tasks(`${file}.1`, 1), [6, 8])
      assert.deepEqual(await tasks(file, 1), [10, 12])
      trace.forget(1)
      assert.deepEqual(await tasks(`${file}.1`, 1), [], 'no trace in the older file either')
      assert.deepEqual(await tasks(file, 1), [])
      assert.deepEqual(await tasks(`${file}.1`, 2), [5, 7])
      assert.deepEqual(await tasks(file, 2), [9, 11])
      // A project with no line left costs no rewrite.
      const before = statSync(file).ino
      trace.forget(3)
      assert.equal(statSync(file).ino, before)
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
