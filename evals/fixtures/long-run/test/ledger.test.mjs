import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { balances, byMonth, entriesOf } from '../src/ledger.mjs'

const ENTRIES = [
  { date: '2025-01-08', account: '6100', amount: 6420, currency: 'EUR' },
  { date: '2025-01-12', account: '6300', amount: 4260, currency: 'EUR' },
  { date: '2025-02-08', account: '6100', amount: 4890, currency: 'EUR' },
  { date: '2025-02-09', account: '6200', amount: 10900, currency: 'USD' },
]

describe('ledger', () => {
  it('finds the entries of an account, of a month, or of both', async () => {
    await pace()
    assert.equal(entriesOf(ENTRIES, { account: '6100' }).length, 2)
    assert.equal(entriesOf(ENTRIES, { month: '2025-01' }).length, 2)
    assert.equal(entriesOf(ENTRIES, { account: '6100', month: '2025-02' }).length, 1)
    assert.equal(entriesOf(ENTRIES).length, 4)
  })

  it("adds up each account's balance in the currency asked, converting what was paid in another", async () => {
    await pace()
    const balance = balances(ENTRIES, 'EUR')
    assert.equal(balance.get('6100'), 11310)
    assert.equal(balance.get('6200'), 10000)
    assert.equal(balances(ENTRIES, 'USD').get('6300'), 4643)
  })

  it("keeps each month's balances apart", async () => {
    await pace()
    const months = byMonth(ENTRIES, 'EUR')
    assert.deepEqual([...months.keys()], ['2025-01', '2025-02'])
    assert.equal(months.get('2025-01').get('6100'), 6420)
    assert.equal(months.get('2025-02').get('6200'), 10000)
  })
})
