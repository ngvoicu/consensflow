import assert from 'node:assert/strict'
import test from 'node:test'
import { fits, focusOrder, gridTemplate } from '../src/layout.js'

for (const [n, rows, cols, areas] of [
  [1, 1, 1, '"lead"'],
  [2, 1, 2, '"lead w1"'],
  [3, 2, 2, '"lead w1" "lead w2"'],
  [5, 2, 3, '"lead w1 w3" "lead w2 w4"'],
  [6, 2, 4, '"lead w1 w3 w5" "lead w2 w4 ."'],
  [10, 2, 6, '"lead w1 w3 w5 w7 w9" "lead w2 w4 w6 w8 ."'],
])
  test(`session layout places ${n} panes in two-row worker columns`, () => {
    assert.deepEqual(gridTemplate(n), { rows, cols, areas })
  })

test('large sessions keep every worker exactly once without additional rows', () => {
  for (let n = 3; n <= 100; n++) {
    const layout = gridTemplate(n)
    assert.equal(layout.rows, 2)
    const workers = layout.areas.match(/w\d+/g)
    assert.equal(workers.length, n - 1)
    assert.equal(new Set(workers).size, n - 1)
  }
})

test('visible space accounts for a double-width lead and two worker columns', () => {
  const minimum = { width: 300, height: 200 }
  assert.equal(fits(20, { width: 1200, height: 400 }, minimum), true)
  assert.equal(fits(20, { width: 1199, height: 400 }, minimum), false)
  assert.equal(fits(20, { width: 1200, height: 399 }, minimum), false)
  assert.deepEqual(focusOrder(4), ['lead', 'w1', 'w2', 'w3'])
})
