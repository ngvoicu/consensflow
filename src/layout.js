/** Session layout: a fixed lead beside column-major, two-row workers. */
export function gridTemplate(n) {
  if (!Number.isSafeInteger(n) || n < 1) throw new Error('pane count must be positive')
  if (n === 1) return { rows: 1, cols: 1, areas: '"lead"' }
  const rows = n === 2 ? 1 : 2
  const cols = 1 + Math.ceil((n - 1) / 2)
  const grid = Array.from({ length: rows }, (_, row) => [
    'lead',
    ...Array.from({ length: cols - 1 }, (_, column) => {
      const worker = column * 2 + row + 1
      return worker < n ? `w${worker}` : '.'
    }),
  ])
  return { rows, cols, areas: grid.map((row) => `"${row.join(' ')}"`).join(' ') }
}

/** Minimum space for the visible lead and at most two worker columns. */
export function fits(n, area, minPane) {
  const { rows, cols } = gridTemplate(n)
  const units = n === 1 ? 1 : 2 + Math.min(2, cols - 1)
  return area.width >= units * minPane.width && area.height >= rows * minPane.height
}

export function focusOrder(n) {
  return Array.from({ length: n }, (_, index) => (index === 0 ? 'lead' : `w${index}`))
}
