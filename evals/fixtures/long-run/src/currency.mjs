import { round } from './money.mjs'

/**
 * What a euro is worth in each currency the books know, in ten-thousandths: 10900
 * is 1.09. Whole numbers, so that a conversion is a product and a quotient of
 * whole numbers, and its only fraction is the one `round` makes whole.
 */
export const RATES = { EUR: 10000, USD: 10900, RON: 49700 }

/** Whether the books know the currency `code`. */
export function isCurrency(code) {
  return Object.hasOwn(RATES, code)
}

/** `cents` of `from` in `to`, made whole by `round`. */
export function convert(cents, from, to) {
  if (from === to) return cents
  if (!isCurrency(from) || !isCurrency(to)) {
    throw new Error(`no rate between ${from} and ${to}`)
  }
  return round((cents * RATES[to]) / RATES[from])
}
