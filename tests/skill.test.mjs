import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { generateSkill } from '../src/skill.js'

const ROSTER = [
  {
    name: 'zeus',
    runtime: 'claude',
    model: 'claude-opus-5',
    effort: 'max',
    permission: 'workspace-write',
    description: 'Deepest reviewer.',
  },
  {
    name: 'hyperion',
    runtime: 'codex',
    model: 'gpt-5.6-sol',
    effort: 'ultra',
    permission: 'workspace-write',
  },
  {
    name: 'endymion',
    runtime: 'pi',
    model: 'openrouter/moonshotai/kimi-k3',
    effort: 'xhigh',
    permission: 'workspace-write',
  },
  {
    name: 'mani',
    runtime: 'opencode',
    model: 'openrouter/moonshotai/kimi-k3',
    permission: 'workspace-write',
  },
  {
    name: 'loki',
    runtime: 'codex',
    model: 'gpt-5.6-luna',
    effort: 'xhigh',
    permission: 'full-auto',
  },
]

describe('the generated skill carries the live-verified command per engine', () => {
  const md = generateSkill(ROSTER)

  it('is a valid Agent Skills file: frontmatter with name and description', () => {
    assert.match(md, /^---\nname: consensflow\ndescription: /)
    assert.match(md, /\n---\n/)
  })

  it('lists every participant name in the description, so mentions trigger it', () => {
    const description = md.slice(0, md.indexOf('\n---\n'))
    for (const p of ROSTER) assert.ok(description.includes(p.name), `${p.name} missing`)
  })

  it('emits the exact claude command with the billing guard and effort', () => {
    assert.ok(
      md.includes(
        'env -u ANTHROPIC_API_KEY claude -p "<question>" --model claude-opus-5 --effort max',
      ),
    )
  })

  it('emits the exact codex command with the billing guard, trust skip and effort', () => {
    assert.ok(
      md.includes(
        'env -u OPENAI_API_KEY codex exec --skip-git-repo-check -m gpt-5.6-sol -c model_reasoning_effort="ultra" "<question>"',
      ),
    )
  })

  it('emits the exact pi command with thinking', () => {
    assert.ok(
      md.includes(
        'pi --no-session --model openrouter/moonshotai/kimi-k3 --thinking xhigh -p "<question>"',
      ),
    )
  })

  it('emits the exact opencode command, effort-less when none is set', () => {
    assert.ok(md.includes('opencode run --model openrouter/moonshotai/kimi-k3 "<question>"'))
    assert.ok(!md.includes('--variant undefined'))
  })

  it('adds the danger flag only to full-auto participants, and marks the row', () => {
    const lokiRow = md.split('\n').find((line) => line.includes('**loki**'))
    assert.ok(lokiRow.includes('--dangerously-bypass-approvals-and-sandbox'))
    assert.ok(/full-auto/i.test(lokiRow))
    const hyperionRow = md.split('\n').find((line) => line.includes('**hyperion**'))
    assert.ok(!hyperionRow.includes('--dangerously'))
  })

  it('teaches the agent to self-heal when a name is missing from the table', () => {
    assert.match(md, /cf skills update/)
    assert.match(md, /missing from (the|this) table/i)
  })

  it('keeps the etiquette that survived two product generations', () => {
    assert.match(md, /One participant at a time/i)
    assert.match(md, /Never apply a participant/i)
    assert.match(md, /never edit this file by hand/i)
  })

  it('refuses to generate from an empty roster rather than shipping a blank skill', () => {
    assert.throws(() => generateSkill([]), /empty roster/i)
  })
})
