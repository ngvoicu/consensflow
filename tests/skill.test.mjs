import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { teamTable, workTierList } from '../src/skill.js'

describe('the staff table the chief reads', () => {
  it('shows one row per member: name, roles and work tier, and nothing to pick a member by', () => {
    const md = teamTable([
      { name: 'zeus', roles: ['worker', 'reviewer'], workTier: 'standard' },
      { name: 'diana', roles: ['advisor'], workTier: 'light' },
    ])
    assert.equal(
      md,
      [
        '| Member | Roles | Work tier |',
        '|---|---|---|',
        '| zeus | worker, reviewer | Standard work |',
        '| diana | advisor | Light work |',
      ].join('\n'),
    )
  })

  it('says so when the staff is empty', () => {
    assert.match(teamTable([]), /^Nobody is on the staff yet/)
  })

  it('lists the four work tiers, one line each', () => {
    const lines = workTierList().split('\n')
    assert.equal(lines.length, 4)
    assert.match(lines[0], /^- Critical work: /)
    assert.match(lines[3], /^- Light work: /)
  })
})
