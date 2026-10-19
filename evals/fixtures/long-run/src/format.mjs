import { fromCents } from './money.mjs'

/** Cents as a reader sees them: thousands grouped with commas, two decimals, the currency after. */
export function formatMoney(cents, currency) {
  const [whole, part] = fromCents(Math.abs(cents)).split('.')
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',')
  return `${cents < 0 ? '-' : ''}${grouped}.${part} ${currency}`
}

/** A share in hundredths of a percent, as a reader sees it: 9050 is "90.50%". */
export function formatShare(hundredths) {
  return `${(hundredths / 100).toFixed(2)}%`
}
