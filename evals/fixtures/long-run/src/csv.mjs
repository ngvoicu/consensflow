/**
 * CSV as the books and the bank's export write it: a header line, then a line to
 * a row, a field in double quotes where it holds a comma.
 */

/**
 * The rows of `text`, each an object by the header's names.
 *
 * BUG (docs/bugs.md, 2): every line is split at every comma, so a field in quotes
 * that holds one is split in two, and every field after it moves a column.
 */
export function parse(text) {
  const lines = String(text)
    .split(/\r?\n/)
    .filter((line) => line.trim() !== '')
  if (lines.length === 0) return []
  const [header, ...rows] = lines
  const names = header.split(',').map((name) => name.trim())
  return rows.map((line) => {
    const fields = line.split(',')
    return Object.fromEntries(names.map((name, at) => [name, (fields[at] ?? '').trim()]))
  })
}

/** A field as a line of CSV holds it: in quotes where it has to be. */
function quote(value) {
  const text = String(value ?? '')
  return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text
}

/** `rows` as CSV, with the header `names` and their fields in that order. */
export function stringify(rows, names) {
  const lines = [names.join(','), ...rows.map((row) => names.map((name) => quote(row[name])).join(','))]
  return `${lines.join('\n')}\n`
}
