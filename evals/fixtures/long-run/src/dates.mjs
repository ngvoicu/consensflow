/** The names of the months, January first. */
const NAMES = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
]

/** `text` if it is a date of the calendar written `YYYY-MM-DD`, else null. */
export function parseDate(text) {
  const said = String(text ?? '').trim()
  const found = /^(\d{4})-(\d{2})-(\d{2})$/.exec(said)
  if (!found) return null
  const [year, month, day] = found.slice(1).map(Number)
  const date = new Date(Date.UTC(year, month - 1, day))
  const same =
    date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day
  return same ? said : null
}

/** The month a date is in: "2025-03-14" is "2025-03". */
export function monthOf(date) {
  return date.slice(0, 7)
}

/** Every month of `year`, January first: "2025-01" … "2025-12". */
export function monthsOf(year) {
  return NAMES.map((_, at) => `${year}-${String(at + 1).padStart(2, '0')}`)
}

/** A month as a reader sees it: "2025-03" is "March 2025". */
export function monthName(month) {
  return `${NAMES[Number(month.slice(5)) - 1]} ${month.slice(0, 4)}`
}
