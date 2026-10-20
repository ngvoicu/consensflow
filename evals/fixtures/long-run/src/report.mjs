import { accountsOf } from './accounts.mjs'
import { convert } from './currency.mjs'
import { monthsOf } from './dates.mjs'
import { entriesOf } from './ledger.mjs'

/**
 * What amounts add up to, for a report.
 *
 * BUG (docs/bugs.md, 3): each amount counts by its size, so a refund (a negative
 * amount) adds to a total instead of taking from it.
 */
export function total(amounts) {
  return amounts.reduce((sum, amount) => sum + Math.abs(amount), 0)
}

/** The amounts of `entries` in `currency`. */
function amountsIn(entries, currency) {
  return entries.map((entry) => convert(entry.amount, entry.currency, currency))
}

/**
 * A month of the books, in `currency`: each account that moved, with its total,
 * and what was earned and spent in all.
 */
export function monthReport(entries, accounts, month, currency) {
  const of = entriesOf(entries, { month })
  const rows = [...accounts.values()]
    .map((account) => ({
      ...account,
      total: total(amountsIn(entriesOf(of, { account: account.code }), currency)),
      count: entriesOf(of, { account: account.code }).length,
    }))
    .filter((row) => row.count > 0)
  const sumOf = (kind) =>
    accountsOf(accounts, kind).reduce(
      (sum, account) => sum + total(amountsIn(entriesOf(of, { account: account.code }), currency)),
      0,
    )
  return { month, rows, income: sumOf('income'), spending: sumOf('expense') }
}

/** An account over the year, in `currency`: its total in each month, and in all. */
export function accountReport(entries, accounts, code, year, currency) {
  const account = accounts.get(code)
  const months = monthsOf(year).map((month) => ({
    month,
    total: total(amountsIn(entriesOf(entries, { account: code, month }), currency)),
  }))
  return { ...account, months, total: total(months.map((month) => month.total)) }
}
