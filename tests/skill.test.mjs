import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { generateSkill } from '../src/skill.js'

const ROSTER = [
  {
    name: 'zeus',
    harness: 'claude',
    model: 'claude-opus-5',
    effort: 'max',
    description: 'Deepest reviewer.',
  },
  {
    name: 'hyperion',
    harness: 'codex',
    model: 'gpt-5.6-sol',
    effort: 'ultra',
  },
  {
    name: 'endymion',
    harness: 'pi',
    model: 'openrouter/moonshotai/kimi-k3',
    effort: 'xhigh',
  },
  {
    name: 'mani',
    harness: 'opencode',
    model: 'openrouter/moonshotai/kimi-k3',
  },
  {
    name: 'loki',
    harness: 'codex',
    model: 'gpt-5.6-luna',
    effort: 'xhigh',
  },
]

describe('one command builder serves the skill and the roster editor', () => {
  it('gives the exact line the skill table will contain', async () => {
    const { agentCommand } = await import('../src/skill.js')
    const command = agentCommand(ROSTER[0])
    assert.ok(generateSkill(ROSTER).includes(command))
    assert.match(command, /claude -p --dangerously-skip-permissions "<question>"/)
  })

  it('has nothing to show for a harness it cannot run', async () => {
    const { agentCommand } = await import('../src/skill.js')
    assert.equal(agentCommand({ name: 'x', harness: 'image', model: 'm' }), undefined)
  })
})

describe('the generated skill carries the live-verified command per engine', () => {
  const md = generateSkill(ROSTER)

  it('is a valid Agent Skills file: frontmatter with name and description', () => {
    assert.match(md, /^---\nname: consensflow\ndescription: /)
    assert.match(md, /\n---\n/)
  })

  it('lists every agent name in the description, so mentions trigger it', () => {
    const description = md.slice(0, md.indexOf('\n---\n'))
    for (const p of ROSTER) assert.ok(description.includes(p.name), `${p.name} missing`)
  })

  it('emits the exact claude command with the billing guard and effort', () => {
    assert.ok(
      md.includes(
        'env -u ANTHROPIC_API_KEY claude -p --dangerously-skip-permissions "<question>" --model claude-opus-5 --effort max',
      ),
    )
  })

  it('emits the exact codex command with the billing guard, trust skip and effort', () => {
    assert.ok(
      md.includes(
        'env -u OPENAI_API_KEY codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox -m gpt-5.6-sol -c model_reasoning_effort="ultra" "<question>"',
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
    assert.ok(md.includes('opencode run --auto --model openrouter/moonshotai/kimi-k3 "<question>"'))
    assert.ok(!md.includes('--variant undefined'))
  })

  it('tells the harness to consult on its own initiative, not only when asked', () => {
    // The roster is worth nothing if the lead only consults when told to.
    assert.match(md, /Reach for an advisor on your own/)
    assert.match(md, /do not need permission to consult/i)
    assert.match(md, /riskiest assumption/i)
    // And it says the one thing this mode cannot do: see the conversation.
    assert.match(md, /cannot see this conversation/i)
  })

  it('gives every agent full permissions, on every engine', () => {
    // An agent is a helper you hand a task to, not a sandboxed reviewer:
    // it may write outside the project and reach the network. The flags are
    // asserted here so a future refactor cannot quietly put a fence back.
    assert.ok(md.includes('claude -p --dangerously-skip-permissions'), 'claude')
    assert.ok(
      md.includes('codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox'),
      'codex',
    )
    assert.ok(md.includes('opencode run --auto'), 'opencode')
    // pi enables its tools by default, so it needs no flag — and must not grow one.
    assert.doesNotMatch(md, /pi [^\n]*--dangerously/)
  })

  it('teaches the harness to self-heal when a name is missing from the table', () => {
    assert.match(md, /cf skills update/)
    assert.match(md, /missing from (the|this) table/i)
  })

  it('keeps the etiquette that survived two product generations', () => {
    assert.match(md, /One agent at a time/i)
    assert.match(md, /Never apply an agent/i)
    assert.match(md, /never edit this file by hand/i)
  })

  it('refuses to generate from an empty roster rather than shipping a blank skill', () => {
    assert.throws(() => generateSkill([]), /empty roster/i)
  })
})
