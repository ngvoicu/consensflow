import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { loadAccounts } from './accounts.mjs'
import { loadBudgets } from './budget.mjs'
import { parse } from './csv.mjs'
import { entriesFrom } from './journal.mjs'

/** The rows of the CSV file `name` of `folder`. */
function rowsOf(folder, name) {
  return parse(readFileSync(join(folder, name), 'utf8'))
}

/**
 * The books kept in `folder`: the chart of accounts, the journal's entries and
 * the rows it skipped, and the budgets.
 */
export function load(folder) {
  const accounts = loadAccounts(rowsOf(folder, 'accounts.csv'))
  const { entries, skipped } = entriesFrom(rowsOf(folder, 'journal.csv'), accounts)
  const budgets = loadBudgets(rowsOf(folder, 'budgets.csv'))
  return { accounts, entries, skipped, budgets }
}
