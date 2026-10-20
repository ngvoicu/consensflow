import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { config } from '../src/config.mjs'
import { load } from '../src/index.mjs'

describe('the books of 2025', () => {
  it('load: ten accounts, six budgets, and the journal of a year', async () => {
    await pace()
    const books = load(config.data)
    assert.equal(books.accounts.size, 10)
    assert.equal(books.budgets.size, 6)
    assert.ok(books.entries.length >= 100, `${books.entries.length} entries`)
    assert.ok(books.entries.every((entry) => entry.date.startsWith('2025-')))
  })

  it('every budget is of an expense account of the chart', async () => {
    await pace()
    const books = load(config.data)
    for (const account of books.budgets.keys()) {
      assert.equal(books.accounts.get(account)?.kind, 'expense', account)
    }
  })
})
