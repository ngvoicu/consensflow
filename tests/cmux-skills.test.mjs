import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { installSkill } from '../src/install.js'
import { syncCmuxSkills } from '../src/mode.js'
import { tempEnv } from './helpers.mjs'

function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

/** What the cloning era left on a machine: manifest-owned cmux files + cache. */
function plantCloningEra(t) {
  stubCli(t, 'claude')
  const installed = installSkill(
    { relPath: 'cmux-core/SKILL.md', content: 'pane control\n', source: 'cmux@abc1234' },
    t.env,
  )
  assert.ok(installed.length > 0, 'fixture: the cmux skill is on the machine')
  const cache = join(t.env.CONSENSFLOW_HOME, 'cache', 'cmux')
  mkdirSync(join(cache, '.git'), { recursive: true })
  writeFileSync(join(cache, 'README.md'), 'checkout\n')
  return { skill: installed[0].path, cache }
}

describe('cmux skills are taken back, never installed — ConsensFlow ships one skill', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('removes what the cloning era installed, and its checkout cache', () => {
    const { skill, cache } = plantCloningEra(t)

    const { commit, report } = syncCmuxSkills(t.env)

    assert.equal(commit, null, 'there is nothing to fetch, so no commit to report')
    assert.equal(existsSync(skill), false, 'the cmux-sourced file is gone')
    assert.equal(existsSync(cache), false, 'the checkout cache is gone with it')
    assert.ok(report.some((row) => row.path === skill))
  })

  it('never touches the skill ConsensFlow itself generates', () => {
    stubCli(t, 'claude')
    const ours = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'the roster skill\n', source: 'consensflow' },
      t.env,
    )

    syncCmuxSkills(t.env)

    assert.ok(existsSync(ours[0].path), 'source consensflow is not source cmux@…')
  })

  it('refuses a user-edited cmux skill without force, like any owned file', () => {
    const { skill } = plantCloningEra(t)
    writeFileSync(skill, 'the user rewrote this\n')

    const refused = syncCmuxSkills(t.env)
    assert.ok(
      refused.report.some((row) => row.path === skill && row.action === 'refused-drifted'),
      'drift is sacred on the way out too',
    )
    assert.equal(readFileSync(skill, 'utf8'), 'the user rewrote this\n')

    const forced = syncCmuxSkills(t.env, { force: true })
    assert.equal(existsSync(skill), false)
    assert.ok(forced.report.some((row) => row.path === skill && row.action !== 'refused-drifted'))
  })

  it('is a quiet no-op on a machine the cloning era never touched', () => {
    const clean = tempEnv()
    try {
      const { report } = syncCmuxSkills(clean.env)
      assert.deepEqual(report, [])
    } finally {
      clean.cleanup()
    }
  })
})
