import { convert } from './currency.mjs'
import { entriesOf } from './ledger.mjs'
import { share, toCents } from './money.mjs'

/** The monthly budget of each account its rows name, in cents of euros. */
export function loadBudgets(rows) {
  const budgets = new Map()
  for (const row of rows) {
    const monthly = toCents(row.monthly)
    if (!Number.isInteger(monthly) || monthly <= 0) {
      throw new Error(`account ${row.account}'s budget is no amount: ${row.monthly}`)
    }
    budgets.set(row.account, monthly)
  }
  return budgets
}

/**
 * What each budgeted account spent of its budget in `month`, in `currency`: the
 * account, what it spent, its budget, and the share (hundredths of a percent).
 */
export function usage(entries, budgets, month, currency) {
  return [...budgets].map(([account, monthly]) => {
    const spent = entriesOf(entries, { account, month }).reduce(
      (total, entry) => total + convert(entry.amount, entry.currency, currency),
      0,
    )
    const budget = convert(monthly, 'EUR', currency)
    return { account, spent, budget, share: share(spent, budget) }
  })
}
