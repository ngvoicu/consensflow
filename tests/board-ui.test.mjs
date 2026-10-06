import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { columnOf, laneStatus, rowTasks, stateLabel } from '../app/ui/core/board.js'

/** What the page draws of a lane: the receipt and stop redesign's two words (app/ui/core/board.js). */

const task = (number, state) => ({ number, state, requester: 'chief' })
const lane = (activity, tasks, extra = {}) => ({
  participant: { handle: 'zeus-amber-pine', role: 'worker', agent: 'zeus', member: 'zeus' },
  activity,
  tasks,
  ...extra,
})
const board = { open: [], lanes: [] }

describe('a task that waits for an answer', () => {
  it('is at work, with a question pending, while its window goes on working', () => {
    const [card] = rowTasks(lane({ state: 'working' }, [task(7, 'waiting')]), board)
    assert.equal(columnOf(card), 'working')
    assert.equal(stateLabel(card), 'Working, question pending')
    assert.equal(card.state, 'waiting', 'the ledger still says waiting')
  })

  it('is waiting while its window is idle, closed or waiting on its own dialog', () => {
    for (const state of ['idle', 'closed', 'waiting', 'starting', 'out', 'unknown']) {
      const [card] = rowTasks(lane({ state }, [task(7, 'waiting')]), board)
      assert.deepEqual([columnOf(card), stateLabel(card)], ['waiting', 'Waiting'], state)
    }
  })

  it('is the only state a working window changes: the rest read as they are', () => {
    const cards = rowTasks(
      lane({ state: 'working' }, [task(3, 'queued'), task(4, 'working'), task(5, 'paused')]),
      board,
    )
    assert.deepEqual(
      cards.map((card) => [card.number, columnOf(card), stateLabel(card)]),
      [
        [5, 'queued', 'Paused'],
        [4, 'working', 'Working'],
        [3, 'queued', 'Queued'],
      ],
    )
  })
})

describe('a lane whose window did not stop', () => {
  it('says which task it is still on an earlier turn of, before anything else about its window', () => {
    const ignoring = lane({ state: 'working' }, [], {
      unstopped: { task: 7, rounds: 3 },
      holding: true,
    })
    assert.deepEqual(laneStatus(ignoring, board, Date.now()), [
      'unstopped',
      'Did not stop for T-7: still on its earlier turn',
    ])
  })

  it('says nothing of it once the stop is paid', () => {
    const paid = lane({ state: 'working' }, [], { unstopped: undefined })
    assert.deepEqual(laneStatus(paid, board, Date.now()), ['working', 'Working'])
  })
})
