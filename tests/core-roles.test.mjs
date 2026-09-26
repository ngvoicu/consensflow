import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { roleInstructions } from '../src/core/roles.js'
import { roleConfiguration } from '../src/role-skills.js'

/**
 * The instructions each window of the new core starts with (TEST-BDC-11): the
 * board's commands for every role, and for coordinators the staff they choose
 * from, the work tiers and the review rule. Nothing from the old transport.
 */
const OLD_COMMANDS = /\bcf (run|say|attach|read|results|projects|chief (send|read))\b/
const zeus = { name: 'zeus', roles: ['worker', 'reviewer'], workTier: 'standard' }

describe('role instructions for the new core', () => {
  for (const role of ['chief', 'advisor', 'worker', 'reviewer', 'designer']) {
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
      if (role === 'chief') {
        for (const command of [
          'cf task add --tier',
          'cf task add --advice --tier',
          'cf task add --design',
          'cf task add --self',
          'cf task add --review --tier',
          'cf answer',
          'cf task done',
          'cf ask --human',
          'cf staff',
        ])
          assert.ok(text.includes(command), `${role} learns ${command}`)
        assert.doesNotMatch(text, /cf task add @/, 'no agent gives another a task by name')
        assert.match(text, /never read another agent's\s+session files/i)
        assert.match(
          text,
          /\| Member \| Roles \| Work tier \|\n\|---\|---\|---\|\n\| zeus \| worker, reviewer \| Standard work \|/,
          'the staff table: name, roles, tier',
        )
        assert.doesNotMatch(text, /tags|PM\b/, 'no tags, no PM')
        assert.match(text, /^## Your commands$/m, 'the command card comes first')
        assert.ok(text.indexOf('## Your commands') < text.indexOf('## What you do'), 'card first')
        assert.match(text, /## Reviews\n\nNothing is reviewed unless you ask\./)
        assert.match(text, /run `cf staff` only when it\s+may have changed since/)
        assert.match(text, /do not run the command to see the refusal: ask/)
        assert.doesNotMatch(text, /VERDICT|review policy|cf task review/)
      } else {
        assert.ok(text.includes('cf ask'), `${role} can ask`)
        assert.match(text, /^## Your commands$/m, 'the command card comes first')
        assert.match(text, /final message of your turn/)
        assert.match(text, /never to another member/)
        assert.match(text, /never read another agent's\s+session files/i)
        assert.doesNotMatch(text, /cf task add/)
        assert.doesNotMatch(text, /PM\b|coordinator/, `${role} answers to the chief`)
        if (role === 'reviewer') assert.match(text, /delivers them to the chief, who decides/)
        assert.doesNotMatch(text, /VERDICT|Review round/, 'a review is a task: no verdict line')
        if (role === 'advisor') assert.match(text, /You advise this project's chief/)
        if (role === 'designer') assert.match(text, /image generation tool/)
      }
    })
  }

  it('says so when the staff is empty', () => {
    assert.match(roleInstructions('chief', []), /Nobody is on the staff yet/)
    const text = roleInstructions('chief', [zeus])
    assert.match(text, /cf task add --after T-3/, 'the one way to continue a window')
    assert.match(text, /only when its context matters/, 'and when')
    assert.match(roleInstructions('chief', [zeus]), /## What you do/)
    assert.match(roleInstructions('chief', [zeus]), /## What you never do/)
    assert.match(
      roleInstructions('chief', [zeus]),
      /## What you never do\n\n- Give a task to a worker by name, or write a task with one worker in mind/,
    )
  })

  it("names this window's cf by its full path, for a shell that finds another cf first", () => {
    for (const role of ['chief', 'worker', 'advisor', 'reviewer', 'designer']) {
      const text = roleInstructions(role, [], { cf: '/opt/consensflow/bin/cf' })
      assert.match(
        text,
        /## This window's cf\n\nHere `cf` is \/opt\/consensflow\/bin\/cf\. If `cf` says a command is unknown, or answers as another program, another `cf` comes first on this shell's PATH: run \/opt\/consensflow\/bin\/cf instead\.\n$/,
        role,
      )
    }
    assert.doesNotMatch(
      roleInstructions('worker', []),
      /This window's cf/,
      'only when the daemon says',
    )
  })

  it("tells the chief its harness's own subagents are not the staff", () => {
    assert.match(
      roleInstructions('chief', []),
      /Hand work to your harness's own subagents or task tool: the board, the\s+human and the staff never see that work/,
    )
  })

  it('refuses an unknown role', () => {
    assert.throws(() => roleInstructions('king', []), /no role instructions for king/)
  })

  it('keeps the authorization boundary in the chief text, ships no harness payload and no personal name', async () => {
    const skill = roleInstructions('chief', [
      { name: 'zeus', roles: ['worker'], workTier: 'critical' },
    ])
    assert.match(skill, /authorized work/)
    assert.match(skill, /never type into another window or launch agents/)
    assert.match(skill, /only the human gives you work, here in your terminal/)
    assert.match(skill, /the human never accepts work on the board/)
    // One role text per role, read by the core: no host payload carries a second copy.
    const root = path.resolve(import.meta.dirname, '..')
    assert.equal(existsSync(path.join(root, 'hosts', 'claude')), false, 'no claude payload')
    assert.equal(existsSync(path.join(root, 'hosts', 'pi')), false, 'no pi payload')
    // The personal name must not appear in anything that ships.
    for (const base of ['hosts', 'bin', 'src', 'skill'].map((d) => path.join(root, d))) {
      for (const file of await readdir(base, { recursive: true })) {
        const content = await readFile(path.join(base, file), 'utf8').catch(() => '')
        assert.doesNotMatch(content, /Gabriel/, `${base}/${file}`)
      }
    }
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
      assert.match(
        file.replaceAll('\\', '/'),
        /roles\/worker\/\.claude\/skills\/consensflow-worker\/SKILL\.md$/,
      )
    } finally {
      await rm(home, { recursive: true, force: true })
    }
  })
})
