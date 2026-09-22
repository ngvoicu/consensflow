import assert from 'node:assert/strict'
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
      trace({ kind: 'window.activity', participant: 'lead', state: 'idle' })
      trace({ at: '2026-09-22T10:00:00.000Z', kind: 'task.opened', project: 1, data: { task: 1 } })
      const lines = (await readFile(path.join(dir, 'events.jsonl'), 'utf8')).trim().split('\n')
      const [first, second] = lines.map((line) => JSON.parse(line))
      assert.equal(lines.length, 2)
      assert.match(first.at, /^\d{4}-\d\d-\d\dT/)
      assert.deepEqual(
        [first.kind, first.participant, first.state],
        ['window.activity', 'lead', 'idle'],
      )
      assert.deepEqual(second, {
        at: '2026-09-22T10:00:00.000Z',
        kind: 'task.opened',
        project: 1,
        data: { task: 1 },
      })
      // A trace that cannot write never troubles the daemon.
      eventTrace(path.join(dir, 'missing', 'deeper'))({ kind: 'x' })
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
