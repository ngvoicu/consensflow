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
  tags: ['coding', 'rust'],
}

describe('role instructions for the new core', () => {
  for (const role of ['lead', 'pm', 'advisor', 'worker', 'reviewer']) {
    it(`teach the ${role} only the board's commands`, () => {
      const text = roleInstructions(role, [zeus])
      assert.match(text, new RegExp(`^---\\nname: consensflow-${role}\\n`))
      assert.doesNotMatch(text, OLD_COMMANDS)
      assert.doesNotMatch(text, /\{\{/, 'every placeholder is filled')
      assert.match(
        text,
        /## Your commands\n\nRun each of these in your shell \(your Bash or terminal tool\)/,
        `${role} knows the commands are shell commands`,
      )
      if (role === 'lead' || role === 'pm') {
        for (const command of [
          'cf task add --tier',
          'cf task add --self',
          'cf task review',
          'cf answer',
          'cf task done',
          'cf ask --human',
          'cf team',
        ])
          assert.ok(text.includes(command), `${role} learns ${command}`)
        assert.doesNotMatch(text, /cf task add @/, 'no agent gives another a task by name')
        assert.match(text, /never read another agent's\s+session files/i)
        assert.match(
          text,
          /\| zeus \| [^|]+ \| coding, rust \|/,
          'the team table: name, tier, tags',
        )
        assert.doesNotMatch(text, /claude-sonnet-5|Careful implementer/, 'no model, no description')
        assert.match(text, /^## Your commands$/m, 'the command card comes first')
        const next = role === 'lead' ? '## What you do' : '## How work moves'
        assert.ok(text.indexOf('## Your commands') < text.indexOf(next), `${role}: card first`)
        assert.match(text, /Cross-model review/)
      } else {
        assert.ok(text.includes('cf ask'), `${role} can ask`)
        assert.match(text, /^## Your commands$/m, 'the command card comes first')
        assert.match(text, /final message of your turn/)
        assert.match(text, /never to another member/)
        assert.match(text, /never read another agent's\s+session files/i)
        assert.doesNotMatch(text, /cf task add/)
        if (role === 'reviewer') assert.match(text, /VERDICT: pass/)
      }
    })
  }

  it('says so when the team is empty', () => {
    assert.match(roleInstructions('lead', []), /No saved workers are available/)
    for (const role of ['lead', 'pm']) {
      const text = roleInstructions(role, [zeus])
      assert.match(text, /cf task add --after T-3/, `${role}: the one way to continue a window`)
      assert.match(text, /only when its context matters/, `${role}: and when`)
    }
    assert.match(roleInstructions('lead', [zeus]), /## What you do/)
    assert.match(roleInstructions('lead', [zeus]), /## What you never do/)
    assert.match(
      roleInstructions('lead', [zeus]),
      /## What you never do\n\n- Give a task to a worker by name, or write a task with one worker in mind/,
    )
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
