import { isCurrency } from './currency.mjs'

/** What is wrong with an entry, each in a few words: nothing for one that is right. */
export function validateEntry(entry, accounts) {
  const errors = []
  if (!entry.date) errors.push('no date')
  if (!accounts.has(entry.account)) errors.push(`unknown account ${entry.account || '(none)'}`)
  if (!Number.isInteger(entry.amount)) errors.push('the amount is no amount')
  if (!isCurrency(entry.currency)) errors.push(`unknown currency ${entry.currency || '(none)'}`)
  return errors
}
