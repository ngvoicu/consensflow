import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { teamTable, workTierList } from '../src/skill.js'

describe('the team table a coordinator reads', () => {
  it('shows one row per member: name, work tier and tags, and nothing to pick a member by', () => {
    const md = teamTable(
      [
        {
          name: 'zeus',
          harness: 'claude',
          model: 'claude-opus-5',
          effort: 'max',
          tags: ['coding', 'rust'],
          description: 'Deepest reviewer.',
        },
        { name: 'diana', harness: 'codex', model: 'gpt-5.6-luna', effort: 'low' },
      ],
      'workers',
    )
    assert.equal(
      md,
      [
        '| Member | Work tier | Tags |',
        '|---|---|---|',
        '| zeus | Standard work | coding, rust |',
        '| diana | Light work | coding, small-changes |',
      ].join('\n'),
    )
    assert.doesNotMatch(md, /claude-opus-5|Deepest reviewer|gpt-5\.6/)
  })

  it('says so when the team is empty', () => {
    assert.match(teamTable([], 'advisors'), /^No saved advisors are available\./)
  })

  it('lists the four work tiers, one line each', () => {
    const lines = workTierList().split('\n')
    assert.equal(lines.length, 4)
    assert.match(lines[0], /^- Critical work: /)
    assert.match(lines[3], /^- Light work: /)
  })
})
