import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { identity, isMember, laneOrder, laneStatus } from '../app/ui/core/board.js'

/**
 * What the page's own checks that name the chief say in a project with no lead (the leads
 * design, step 0): who counts as a member, where the chief's lane sits, what its lane says
 * of a window that would not open, a meeting and a missing model, and how its identity
 * reads. A later step turns each check into a question of the participant's seat; these
 * answers stay as they are. The checks the page keeps out of reach of a plain import (the
 * board's rows, the dock's chip, the terminals' tools) are held by the page suite in
 * Chromium (`app/tests/core-page.spec.mjs`).
 */

const human = { id: 1, handle: 'human', role: 'human', harness: null, member: null }
const chief = {
  id: 2,
  handle: 'chief',
  role: 'chief',
  harness: 'claude-code',
  model: 'claude-opus-5',
  member: null,
  memberId: null,
}
const zeus = {
  id: 3,
  handle: 'zeus',
  role: 'worker',
  roles: ['worker'],
  tier: 'standard',
  harness: 'claude-code',
  member: null,
  memberId: null,
  model: 'claude-sonnet-5',
}
const session = { ...zeus, id: 5, handle: 'zeus-amber-pine', member: 'zeus', memberId: 3 }

describe('who counts as a member', () => {
  it('is a participant on a harness that is neither the chief, nor the human, nor a session', () => {
    assert.equal(isMember(zeus), true)
    assert.equal(isMember(chief), false)
    assert.equal(isMember(human), false)
    assert.equal(isMember(session), false)
  })
})

describe('the order of the lanes', () => {
  it('puts the chief first, then the human, then each member with its sessions right under it', () => {
    const lane = (participant) => ({ participant })
    const later = { ...zeus, id: 4, handle: 'diana', memberId: null }
    const ordered = laneOrder([
      lane(later),
      lane(session),
      lane(zeus),
      lane(chief),
      lane(human),
    ]).map(({ participant }) => participant.handle)
    assert.deepEqual(ordered, ['chief', 'human', 'zeus', 'zeus-amber-pine', 'diana'])
  })
})

describe("the chief's lane status", () => {
  const board = { project: {}, held: [], lanes: [], open: [] }
  const NOW = Date.UTC(2026, 10, 27, 12, 0, 0)

  it("says a window that could not be opened is the chief's, with what to do", () => {
    const lane = {
      participant: chief,
      activity: { state: 'closed', reason: "The chief's window could not be opened: no claude" },
    }
    assert.deepEqual(laneStatus(lane, board, NOW), [
      'closed',
      "The chief's window could not be opened: no claude. Close then Resume the project to try again.",
    ])
    const member = { ...lane, participant: zeus }
    assert.notEqual(laneStatus(member, board, NOW)?.[0], 'missing')
    assert.notEqual(
      laneStatus(member, board, NOW)?.[1],
      "The chief's window could not be opened: no claude. Close then Resume the project to try again.",
    )
  })

  it('says a meeting for the chief only, with how many messages it holds', () => {
    const meeting = { project: { meeting: { since: 1 } }, held: [{}, {}], lanes: [], open: [] }
    assert.deepEqual(laneStatus({ participant: chief, activity: { state: 'idle' } }, meeting, NOW), [
      'meeting',
      'In a meeting · 2 messages held',
    ])
    const quiet = { project: { meeting: { since: 1 } }, held: [], lanes: [], open: [] }
    assert.deepEqual(laneStatus({ participant: chief, activity: { state: 'idle' } }, quiet, NOW), [
      'meeting',
      'In a meeting · nothing held yet',
    ])
    assert.notEqual(
      laneStatus({ participant: zeus, activity: { state: 'idle' } }, meeting, NOW)?.[0],
      'meeting',
    )
  })

  it('asks for a model for a chief the update could not tell, and for a member in its own words', () => {
    const lane = { participant: { ...chief, needsEdit: true }, activity: { state: 'closed' } }
    assert.deepEqual(laneStatus(lane, board, NOW), [
      'missing',
      "Needs a model: ConsensFlow couldn't tell which model this chief used before the update. Pick one with Switch chief; what was written to the chief waits until then",
    ])
    const [state, words] = laneStatus(
      { participant: { ...zeus, needsEdit: true }, activity: { state: 'closed' } },
      board,
      NOW,
    )
    assert.equal(state, 'missing')
    assert.ok(!words.includes('Switch chief'), words)
  })
})

describe("the chief's identity", () => {
  it('reads its harness and its model, and a member its role, tier and harness', () => {
    assert.equal(identity(chief), 'Claude Code · claude-opus-5')
    assert.equal(identity(zeus).startsWith('worker · standard · '), true)
  })
})
