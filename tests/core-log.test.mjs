import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { daemonLog } from '../src/core/log.js'

/** The daemon's own log: dated lines, an error's stack under its line, one old file kept. */
describe('the daemon log', () => {
  it('dates each line, keeps an error’s stack under it, and rotates once past its limit', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-log-'))
    try {
      let at = 0
      const log = daemonLog(dir, {
        limit: 200,
        now: () => new Date(Date.UTC(2026, 8, 24, 14, 0, at++)),
      })
      log.info('start pid 1')
      log.error('pass failed', new Error('boom'))
      const text = await readFile(log.file, 'utf8')
      assert.match(
        text,
        /^2026-09-24T14:00:00\.000Z info start pid 1\n2026-09-24T14:00:01\.000Z error pass failed\n {4}Error: boom\n {8}at /,
      )
      for (let i = 0; i < 6; i += 1) log.warn(`slow pass ${i}, forty characters of padding here…`)
      // Lines of some 75 bytes past a 200-byte limit: the file turns over every
      // three, the one before it is kept, and no older one.
      const fresh = await readFile(log.file, 'utf8')
      assert.ok(!fresh.includes('start pid 1'), 'a fresh file began')
      assert.ok(fresh.split('\n').filter(Boolean).length <= 3, 'and holds only what came after')
      assert.ok(existsSync(`${log.file}.1`), 'the full one was kept aside')
      assert.ok(!existsSync(`${log.file}.2`), 'and only one')
      // A home that cannot be written loses the line, not the run.
      daemonLog(path.join(dir, 'missing', 'deeper')).error('x', new Error('y'))
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
