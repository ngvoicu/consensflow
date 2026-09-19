import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { roleInstructions } from '../src/core/roles.js'
import { roleConfiguration } from '../src/role-skills.js'

/**
 * The instructions each window of the new core starts with (TEST-BDC-11): the
 * board's commands for every role, and for coordinators the team they choose
 * from, the work tiers and the review rule. Nothing from the old transport.
 */
const OLD_COMMANDS = /\bcf (run|say|attach|read|results|projects|lead (send|read))\b/
const zeus = {
  name: 'zeus',
  harness: 'claude',
  model: 'claude-sonnet-5',
  effort: 'high',
  description: 'Careful implementer',
}

describe('role instructions for the new core', () => {
  for (const role of ['lead', 'pm', 'advisor', 'worker', 'reviewer']) {
    it(`teach the ${role} only the board's commands`, () => {
      const text = roleInstructions(role, [zeus])
      assert.match(text, new RegExp(`^---\\nname: consensflow-${role}\\n`))
      assert.doesNotMatch(text, OLD_COMMANDS)
      assert.doesNotMatch(text, /\{\{/, 'every placeholder is filled')
      if (role === 'lead' || role === 'pm') {
        for (const command of [
          'cf task add',
          'cf answer',
          'cf task done',
          'cf ask --human',
          'cf team',
        ])
          assert.ok(text.includes(command), `${role} learns ${command}`)
        assert.match(text, /\| zeus \| claude-sonnet-5/)
        assert.match(text, /Cross-model review/)
      } else {
        assert.ok(text.includes('cf ask'), `${role} can ask`)
        assert.match(text, /final message of your turn/)
        assert.doesNotMatch(text, /cf task add/)
      }
    })
  }

  it('says so when the team is empty', () => {
    assert.match(roleInstructions('lead', []), /No saved workers are available/)
    assert.match(roleInstructions('pm', []), /No saved advisors are available/)
  })

  it('refuses an unknown role', () => {
    assert.throws(() => roleInstructions('king', []), /no role instructions for king/)
  })

  it('are written where each harness loads them, for any role', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-core-roles-'))
    try {
      const env = { HOME: home, CONSENSFLOW_HOME: path.join(home, 'consensflow') }
      const configured = await roleConfiguration('claude-code', {
        role: 'worker',
        env,
        content: 'WORKER TEXT',
      })
      const file = configured.args[configured.args.indexOf('--append-system-prompt-file') + 1]
      assert.equal(await readFile(file, 'utf8'), 'WORKER TEXT')
      assert.match(file, /roles\/worker\/\.claude\/skills\/consensflow-worker\/SKILL\.md$/)
    } finally {
      await rm(home, { recursive: true, force: true })
    }
  })
})
