import { convert } from './currency.mjs'
import { monthOf } from './dates.mjs'

/** The entries of an account, or of a month, or of both. */
export function entriesOf(entries, { account, month } = {}) {
  return entries.filter(
    (entry) =>
      (account === undefined || entry.account === account) &&
      (month === undefined || monthOf(entry.date) === month),
  )
}

/** Each account's balance, in `currency`: what its entries add up to. */
export function balances(entries, currency) {
  const balance = new Map()
  for (const entry of entries) {
    const amount = convert(entry.amount, entry.currency, currency)
    balance.set(entry.account, (balance.get(entry.account) ?? 0) + amount)
  }
  return balance
}

/** Each month's balances, in `currency`: by month, then by account. */
export function byMonth(entries, currency) {
  const months = new Map()
  for (const entry of entries) {
    const month = monthOf(entry.date)
    if (!months.has(month)) months.set(month, [])
    months.get(month).push(entry)
  }
  return new Map([...months].map(([month, of]) => [month, balances(of, currency)]))
}
