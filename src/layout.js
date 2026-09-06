/**
 * The pane layout rule (IMPL-PANE-38) — pure: no DOM, no I/O, no clock.
 *
 * Panes are counted WITH the lead. `n` is the TOTAL, so `gridTemplate(3)`
 * is a lead and two workers, not a lead and three; a page that counts only
 * the workers draws the wrong picture. Totals 1–6 are the six the spec
 * draws by hand, because the pretty arrangement at those sizes is a taste
 * call and not a formula — twice the lead is full-height beside a stack,
 * which no `ceil(sqrt(n))` would ever produce. From 7 on the formula takes
 * over: `rows = ceil(sqrt(n))`, `cols = ceil(n / rows)`, filled row-major
 * with the lead first.
 *
 * `areas` is a ready-to-assign CSS `grid-template-areas` value — quoted
 * rows separated by a space, `element.style.gridTemplateAreas = areas` and
 * the browser does the rest. Each pane gets a cell NAME rather than a
 * coordinate, and the lead's is always `lead`, so the page places a pane by
 * setting `grid-area` and never recomputes a position. A cell no pane fills
 * is `.`, CSS's own token for an empty one.
 *
 * The formula never leaves a whole row empty (checked for n = 7…400), so
 * every drawn row carries at least one pane: `cols = ceil(n / rows)` forces
 * `rows * cols - n < rows`, and the spare cells always land at the end of
 * the last row.
 */

/** CSS's token for a grid cell no pane fills. */
const EMPTY = '.'

/**
 * The six pictures, counted WITH the lead. Kept as cell grids rather than
 * strings so `rows` and `cols` are read off the drawing itself and cannot
 * disagree with it.
 */
const PICTURES = new Map([
  [1, [['lead']]],
  [2, [['lead', 'w1']]],
  [
    3,
    [
      ['lead', 'w1'],
      ['lead', 'w2'],
    ],
  ],
  [
    4,
    [
      ['lead', 'w1'],
      ['w2', 'w3'],
    ],
  ],
  [
    5,
    [
      ['lead', 'w1', 'w2'],
      ['lead', 'w3', 'w4'],
    ],
  ],
  [
    6,
    [
      ['lead', 'w1', 'w2'],
      ['w3', 'w4', 'w5'],
    ],
  ],
])

/** The name of the pane at `index` in reading order: the lead, then `w1`, `w2` … */
function cellName(index) {
  return index === 0 ? 'lead' : `w${index}`
}

/** A grid of cell names as one `grid-template-areas` value. */
function serialize(grid) {
  return grid.map((row) => `"${row.join(' ')}"`).join(' ')
}

/**
 * The grid for `n` panes counted WITH the lead (`n >= 1` — a tab always has
 * its lead, so there is no empty case to draw).
 *
 * @param {number} n — the total pane count, the lead included.
 * @returns {{rows: number, cols: number, areas: string}} — `areas` is a CSS
 *   `grid-template-areas` value naming the lead's cell `lead` and each
 *   worker's `w1`, `w2` …, with `.` for a cell no pane fills.
 */
export function gridTemplate(n) {
  const drawn = PICTURES.get(n)
  if (drawn) {
    return { rows: drawn.length, cols: drawn[0].length, areas: serialize(drawn) }
  }

  const rows = Math.ceil(Math.sqrt(n))
  const cols = Math.ceil(n / rows)
  const grid = []
  for (let row = 0; row < rows; row++) {
    const cells = []
    for (let col = 0; col < cols; col++) {
      const index = row * cols + col
      cells.push(index < n ? cellName(index) : EMPTY)
    }
    grid.push(cells)
  }
  return { rows, cols, areas: serialize(grid) }
}

/**
 * Can `n` panes tile `area` and still honour `minPane`? When this is false
 * the page keeps every process and shows the focused-pane view instead.
 *
 * The grid decides, not the count: `fits` asks `gridTemplate` for the same
 * `rows` and `cols` the page will draw, so the two can never disagree about
 * what has to fit. A lead spanning two rows (3 and 5 panes) is the BIGGEST
 * cell there is, so it never sets the limit — every other cell is one row by
 * one column, and that is what `minPane` has to clear. Compared by
 * multiplication rather than by dividing the area, so whole pixels stay
 * whole and the boundary is exact.
 *
 * @param {number} n — the total pane count, the lead included.
 * @param {{width: number, height: number}} area — the pane area, in pixels.
 * @param {{width: number, height: number}} minPane — the smallest pane the
 *   page will draw, in pixels. Exactly the minimum still fits.
 * @returns {boolean}
 */
export function fits(n, area, minPane) {
  const { rows, cols } = gridTemplate(n)
  return area.width >= cols * minPane.width && area.height >= rows * minPane.height
}

/**
 * The next/previous order of the focused-pane view — the same order the
 * grid reads, the lead first. A lead that spans two rows is named twice in
 * `areas` and is still one stop here.
 *
 * @param {number} n — the total pane count, the lead included.
 * @returns {string[]} — a fresh array of cell names, `['lead', 'w1', …]`.
 */
export function focusOrder(n) {
  return Array.from({ length: n }, (_, index) => cellName(index))
}
