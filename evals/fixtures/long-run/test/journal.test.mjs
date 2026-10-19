import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { entriesFrom } from '../src/journal.mjs'

const ACCOUNTS = new Map([
  ['6100', { code: '6100', name: 'Office supplies', kind: 'expense' }],
  ['6200', { code: '6200', name: 'Software', kind: 'expense' }],
])

describe('journal', () => {
  it('makes an entry of each right row, in cents, its currency euros unless it says', async () => {
    await pace()
    const { entries, skipped } = entriesFrom(
      [
        { date: '2025-03-04', account: '6100', memo: 'Desk lamp', amount: '120.00', currency: 'EUR', tags: '#office' },
        { date: '2025-03-07', account: '6200', memo: 'Hosting', amount: '25.00', currency: 'USD', tags: '' },
        { date: '2025-03-09', account: '6100', memo: 'Pens', amount: '4.50', currency: '', tags: '' },
      ],
      ACCOUNTS,
    )
    assert.equal(skipped.length, 0)
    assert.deepEqual(
      entries.map((entry) => [entry.line, entry.amount, entry.currency]),
      [
        [2, 12000, 'EUR'],
        [3, 2500, 'USD'],
        [4, 450, 'EUR'],
      ],
    )
    assert.deepEqual(entries[0].tags, ['office'])
  })

  it('sets a row that makes no entry aside, with its line and what is wrong', async () => {
    await pace()
    const { entries, skipped } = entriesFrom(
      [{ date: '2025-02-30', account: '6100', memo: 'x', amount: 'lots', currency: 'EUR', tags: '' }],
      ACCOUNTS,
    )
    assert.equal(entries.length, 0)
    assert.deepEqual(skipped, [{ line: 2, errors: ['no date', 'the amount is no amount'] }])
  })

  it('takes a negative amount as a refund', async () => {
    await pace()
    const { entries } = entriesFrom(
      [{ date: '2025-03-25', account: '6100', memo: 'Refund', amount: '-30.00', currency: 'EUR', tags: '#refund' }],
      ACCOUNTS,
    )
    assert.equal(entries[0].amount, -3000)
  })
})
