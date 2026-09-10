import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { agentCommand, generateSkill } from '../src/skill.js'

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
    effort: 'max',
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

describe('the private lead skill', () => {
  const md = generateSkill(ROSTER)
  it('has a scoped role description and lists the configured workers', () => {
    assert.match(md, /name: consensflow-lead/)
    for (const agent of ROSTER) {
      assert.ok(md.includes(`| ${agent.name} |`))
      assert.equal(agentCommand(agent), `cf run @${agent.name} "<task>"`)
    }
  })
  it('works with an empty roster and uses explicit discovery', () => {
    assert.match(generateSkill([]), /cf agent list/)
    assert.doesNotMatch(
      md,
      /cf skills (?:install|update)|unsupported version|Resume replies|cf catchup/,
    )
  })
  it('distinguishes new work from continuing a conversation', () => {
    assert.match(md, /--new/)
    assert.match(md, /--session <conversation>/)
    assert.match(md, /cf say <conversation>/)
    assert.match(md, /--prompt-file/)
  })
  it('uses delivered answers and finishes all parts without a new read request', () => {
    assert.match(md, /No retrieval call or new user permission is needed/)
    assert.match(md, /Read every part of that one result/)
    assert.match(md, /immutable delivery ID/)
    assert.match(md, /No delivered answer and no explicit request to read/)
  })
  it('continues independent work and does not infer failure from counters', () => {
    assert.match(md, /continue independent authorized work/)
    assert.match(md, /do not end the overall task|Do not\s+poll or end the overall task/)
    assert.match(md, /`0 runs`/)
    assert.match(md, /does not prove that a worker failed to start/)
    assert.match(md, /execution is uncertain/)
  })
  it('keeps role and policy authority with the app and user', () => {
    assert.match(md, /You do not need to inspect/)
    assert.match(md, /Leave draft text and delivery settings to the user/)
    assert.match(md, /do not manufacture\ncredentials/)
    assert.doesNotMatch(md, /consensflow-pm/)
  })
})
