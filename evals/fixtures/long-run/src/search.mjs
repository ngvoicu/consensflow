import { monthOf } from './dates.mjs'
import { hasTag } from './tags.mjs'

/**
 * The entries a query finds: each of its words must hold. `tag:travel` is a tag,
 * `account:6300` an account, `month:2025-03` a month, and any other word is in the
 * memo, whatever its case.
 */
export function search(entries, query) {
  const words = String(query ?? '')
    .split(/\s+/)
    .filter((word) => word !== '')
  return entries.filter((entry) =>
    words.every((word) => {
      const [field, value] = word.includes(':') ? word.split(/:(.*)/s) : [null, word]
      if (field === 'tag') return hasTag(entry, value)
      if (field === 'account') return entry.account === value
      if (field === 'month') return monthOf(entry.date) === value
      return entry.memo.toLowerCase().includes(word.toLowerCase())
    }),
  )
}
