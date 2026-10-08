import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { fakeExecutable, tempEnv } from './helpers.mjs'

/** The stand-ins the tests put on PATH in place of a harness's CLI. */
describe('a stand-in CLI that counts how often it is run', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  mkdirSync(t.env.PATH, { recursive: true })

  it('adds a line to its log for each run, and says what it was told to', {
    skip: process.platform === 'win32' && 'a .cmd is run by cmd.exe, which a test of its own holds',
  }, () => {
    const log = join(t.root, 'runs')
    const file = fakeExecutable(join(t.env.PATH, 'pi'), { output: '1.2.3', log })
    for (let run = 1; run <= 3; run += 1) {
      assert.equal(execFileSync(file, ['--version'], { encoding: 'utf8' }), '1.2.3\n')
      assert.equal(readFileSync(log, 'utf8').split('\n').filter(Boolean).length, run)
    }
  })

  it('keeps no log when it is asked for none', {
    skip: process.platform === 'win32' && 'a .cmd is run by cmd.exe, which a test of its own holds',
  }, () => {
    const file = fakeExecutable(join(t.env.PATH, 'codex'), { output: '2.0.0' })
    assert.equal(execFileSync(file, [], { encoding: 'utf8' }), '2.0.0\n')
    assert.equal(readFileSync(file, 'utf8').includes('>>'), false)
  })
})
