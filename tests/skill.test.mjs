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
  const md = generateSkill(ROSTER)

  it('gives the exact line the skill table will contain', async () => {
    const { agentCommand } = await import('../src/skill.js')
    const command = agentCommand(ROSTER[0])
    assert.equal(command, `cf run @${ROSTER[0].name} "<task>"`)
    for (const agent of ROSTER) {
      assert.equal(agentCommand(agent), `cf run @${agent.name} "<task>"`)
    }
  })

  it('lists every agent name in the description, so mentions trigger it', () => {
    const description = md.slice(0, md.indexOf('\n---\n'))
    for (const p of ROSTER) assert.ok(description.includes(p.name), `${p.name} missing`)
  })

  it('tells the harness to consult on its own initiative, not only when asked', () => {
    // The roster is worth nothing if the lead only consults when told to.
    assert.match(md, /Reach for an advisor on your own/)
    assert.match(md, /do not need permission to consult/i)
    assert.match(md, /riskiest assumption/i)
    // And it says the one thing this mode cannot do: see the conversation.
    assert.match(md, /cannot see this conversation/i)
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

describe('the skill teaches one verb, with the flags that make a spawn', () => {
  const md = generateSkill([
    { name: 'zeus', harness: 'claude', model: 'claude-opus-5', effort: 'max' },
    { name: 'pygmalion', harness: 'image', model: 'gpt-image-2' },
  ])

  it('spawns every agent the same way, image agents included', () => {
    assert.match(md, /cf run @<name> "<task>"/)
    assert.match(md, /pygmalion/)
    assert.match(md, /gpt-image-2/)
    assert.doesNotMatch(md, /codex exec/)
    assert.doesNotMatch(md, /env -u ANTHROPIC_API_KEY/)
  })

  it('says the brief and the handoff are the lead to give', () => {
    assert.match(md, /--brief/)
    assert.match(md, /--handoff-file/)
    assert.match(md, /You are the one holding it/)
  })
})
