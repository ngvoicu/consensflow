import { parseDate } from './dates.mjs'
import { toCents } from './money.mjs'
import { parseTags } from './tags.mjs'
import { validateEntry } from './validate.mjs'

/**
 * The entries the journal's rows make, and the rows that make none, each with
 * its line in the file (the header is line 1) and what is wrong with it. An entry
 * is an amount on an account: positive is what an expense account spent or an
 * income account earned, negative a refund of either.
 */
export function entriesFrom(rows, accounts) {
  const entries = []
  const skipped = []
  rows.forEach((row, at) => {
    const entry = {
      line: at + 2,
      date: parseDate(row.date),
      account: row.account,
      memo: row.memo ?? '',
      amount: toCents(row.amount),
      currency: row.currency || 'EUR',
      tags: parseTags(row.tags),
    }
    const errors = validateEntry(entry, accounts)
    if (errors.length > 0) skipped.push({ line: entry.line, errors })
    else entries.push(entry)
  })
  return { entries, skipped }
}
