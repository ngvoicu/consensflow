import assert from 'node:assert/strict'
import test from 'node:test'
import { fits, focusOrder, gridTemplate } from '../src/layout.js'

/**
 * The layout rule (TEST-PANE-37) — pure, no DOM.
 *
 * Panes are counted WITH the lead, so `n` is the total: `gridTemplate(3)`
 * is a lead and TWO workers. Totals 1–6 are the six pictures the spec
 * draws; beyond them `rows = ceil(sqrt(n))`, `cols = ceil(n / rows)`,
 * row-major, lead first.
 *
 * The expected `areas` strings are written out as literals on purpose. A
 * helper that built them the way `src/layout.js` does would test the
 * implementation against itself, and both could drift from the drawing
 * together without a single test going red.
 */

/** The rows of a `grid-template-areas` string, each split into its cells. */
function rowsOf(areas) {
  return [...areas.matchAll(/"([^"]*)"/g)].map((row) => row[1].split(/\s+/).filter(Boolean))
}

/**
 * The distinct cell names in reading order, `.` (an empty cell) dropped.
 * A lead that spans two rows is named twice in `areas` and is still ONE
 * pane, so first appearance wins.
 */
function cellsOf(areas) {
  const named = rowsOf(areas).flat()
  return [...new Set(named.filter((cell) => cell !== '.'))]
}

/** The six pictures, exactly as the spec draws them. */
const PICTURES = [
  { n: 1, rows: 1, cols: 1, areas: '"lead"' },
  { n: 2, rows: 1, cols: 2, areas: '"lead w1"' },
  { n: 3, rows: 2, cols: 2, areas: '"lead w1" "lead w2"' },
  { n: 4, rows: 2, cols: 2, areas: '"lead w1" "w2 w3"' },
  { n: 5, rows: 2, cols: 3, areas: '"lead w1 w2" "lead w3 w4"' },
  { n: 6, rows: 2, cols: 3, areas: '"lead w1 w2" "w3 w4 w5"' },
]

for (const picture of PICTURES) {
  test(`layout: ${picture.n} pane(s) counted with the lead draw the spec's picture`, () => {
    assert.deepEqual(gridTemplate(picture.n), {
      rows: picture.rows,
      cols: picture.cols,
      areas: picture.areas,
    })
  })
}

test('layout: 7 panes leave the table — a 3 x 3 read row-major, the spare cells empty', () => {
  assert.deepEqual(gridTemplate(7), {
    rows: 3,
    cols: 3,
    areas: '"lead w1 w2" "w3 w4 w5" "w6 . ."',
  })
})

test('layout: 9 panes fill a 3 x 3 exactly — not one empty cell', () => {
  assert.deepEqual(gridTemplate(9), {
    rows: 3,
    cols: 3,
    areas: '"lead w1 w2" "w3 w4 w5" "w6 w7 w8"',
  })
})

test('layout: beyond the table rows = ceil(sqrt(n)) and cols = ceil(n / rows)', () => {
  for (let n = 7; n <= 40; n++) {
    const { rows, cols } = gridTemplate(n)
    assert.equal(rows, Math.ceil(Math.sqrt(n)), `rows for ${n}`)
    assert.equal(cols, Math.ceil(n / rows), `cols for ${n}`)
    assert.ok(rows * cols >= n, `${n} panes need at least ${n} cells`)
  }
})

test('layout: the lead cell is always named lead, and named once in the focus order', () => {
  for (let n = 1; n <= 20; n++) {
    const cells = cellsOf(gridTemplate(n).areas)
    assert.equal(cells[0], 'lead', `${n} panes put the lead first`)
    assert.equal(cells.filter((cell) => cell === 'lead').length, 1, `${n} panes name one lead`)
  }
})

test('layout: every areas string is a rectangle of rows x cols cells holding every pane', () => {
  for (let n = 1; n <= 20; n++) {
    const { rows, cols, areas } = gridTemplate(n)
    const grid = rowsOf(areas)
    assert.equal(grid.length, rows, `${n} panes draw ${rows} rows`)
    for (const row of grid) {
      assert.equal(row.length, cols, `${n} panes draw ${cols} columns`)
    }
    assert.equal(cellsOf(areas).length, n, `${n} panes are all placed`)
  }
})

test('layout: fits turns false the pixel the minimum pane no longer honours', () => {
  // 5 panes tile 2 rows x 3 columns, so the smallest cell is a THIRD of the
  // width and a half of the height — 999 x 440 is exactly 3 x 333 by 2 x 220.
  const minPane = { width: 333, height: 220 }
  assert.equal(fits(5, { width: 999, height: 440 }, minPane), true, 'exactly the minimum fits')
  assert.equal(fits(5, { width: 998, height: 440 }, minPane), false, 'one pixel too narrow')
  assert.equal(fits(5, { width: 999, height: 439 }, minPane), false, 'one pixel too short')
  assert.equal(fits(5, { width: 1920, height: 1080 }, minPane), true, 'a real screen fits')
})

test('layout: fits asks the grid, not the count — one area takes 4 panes and refuses 5', () => {
  const area = { width: 800, height: 600 }
  const minPane = { width: 300, height: 250 }
  assert.equal(fits(4, area, minPane), true, '2 x 2 needs 600 x 500')
  assert.equal(fits(5, area, minPane), false, '2 x 3 needs 900 x 500')
})

test('layout: focusOrder is the next/previous order of the focused-pane view', () => {
  assert.deepEqual(focusOrder(1), ['lead'])
  assert.deepEqual(focusOrder(3), ['lead', 'w1', 'w2'])
  assert.deepEqual(focusOrder(6), ['lead', 'w1', 'w2', 'w3', 'w4', 'w5'])
})

test('layout: focusOrder holds every pane once, in the order the grid reads', () => {
  for (let n = 1; n <= 20; n++) {
    const order = focusOrder(n)
    assert.equal(order.length, n, `${n} panes get ${n} stops`)
    assert.deepEqual(order, cellsOf(gridTemplate(n).areas), `focus order for ${n} follows the grid`)
  }
})
