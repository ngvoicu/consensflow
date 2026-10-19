/**
 * Money is whole cents, held as integers: a float never holds an amount. What
 * is read from a file is text ("1,234.50", "-30.00"), and it becomes cents once,
 * here; what is shown is made from cents by `format.mjs`.
 */

/** The cents a text amount says, or NaN for text that is no amount. */
export function toCents(text) {
  const clean = String(text ?? '')
    .trim()
    .replaceAll(',', '')
  if (!/^-?\d+(\.\d{1,2})?$/.test(clean)) return Number.NaN
  const negative = clean.startsWith('-')
  const [whole, part = ''] = clean.replace('-', '').split('.')
  const cents = Number(whole) * 100 + Number(part.padEnd(2, '0'))
  return negative ? -cents : cents
}

/** Cents as text with two decimals and no grouping: 123450 is "1234.50". */
export function fromCents(cents) {
  const sign = cents < 0 ? '-' : ''
  const whole = Math.abs(cents)
  return `${sign}${Math.floor(whole / 100)}.${String(whole % 100).padStart(2, '0')}`
}

/**
 * A number of cents that may have a fraction (a conversion, a share), made
 * whole.
 *
 * BUG (docs/bugs.md, 1): a half rounds up, always; the conventions say a half
 * rounds to the even neighbour.
 */
export function round(value) {
  return Math.floor(value + 0.5)
}

/** The sum of amounts in cents. */
export function sum(amounts) {
  return amounts.reduce((total, amount) => total + amount, 0)
}

/** `part` as a share of `whole`, in hundredths of a percent: 9050 is 90.50%. */
export function share(part, whole) {
  if (whole === 0) return 0
  return round((part * 10000) / whole)
}

/** `cents` split into `parts` amounts that differ by a cent at most, the larger first. */
export function split(cents, parts) {
  const each = Math.trunc(cents / parts)
  let left = cents - each * parts
  return Array.from({ length: parts }, () => {
    if (left <= 0) return each
    left -= 1
    return each + 1
  })
}
