import { stringify } from './csv.mjs'
import { fromCents } from './money.mjs'

/** The columns of the journal, as the books keep it. */
export const JOURNAL = ['date', 'account', 'memo', 'amount', 'currency', 'tags']

/** The entries as the journal's CSV: what reading it back gives again. */
export function journalCsv(entries) {
  return stringify(
    entries.map((entry) => ({
      ...entry,
      amount: fromCents(entry.amount),
      tags: entry.tags.map((tag) => `#${tag}`).join(' '),
    })),
    JOURNAL,
  )
}
