import assert from 'node:assert/strict'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { installSkill, syncCmuxSkills } from '../src/install.js'
import { tempEnv } from './helpers.mjs'

test('retired cmux integration neither installs nor deletes global skill files', () => {
  const t = tempEnv()
  try {
    const file = join(t.env.HOME, '.claude/skills/cmux-core/SKILL.md')
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(file, 'manual global content')
    assert.deepEqual(installSkill({ content: 'replacement', source: 'cmux@old' }, t.env), [])
    assert.deepEqual(syncCmuxSkills(t.env, { force: true }).report, [])
    assert.equal(readFileSync(file, 'utf8'), 'manual global content')
  } finally {
    t.cleanup()
  }
})
