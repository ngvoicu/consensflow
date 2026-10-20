import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { loadBudgets, usage } from '../src/budget.mjs'

const BUDGETS = loadBudgets([
  { account: '6100', monthly: '500.00' },
  { account: '6400', monthly: '250.00' },
])

const ENTRIES = [
  { date: '2025-12-08', account: '6100', amount: 12050, currency: 'EUR' },
  { date: '2025-12-09', account: '6100', amount: 41000, currency: 'EUR' },
  { date: '2025-12-15', account: '6400', amount: 23680, currency: 'EUR' },
  { date: '2025-11-15', account: '6400', amount: 14020, currency: 'EUR' },
]

describe('budget', () => {
  it("reads each account's monthly budget in cents of euros", async () => {
    await pace()
    assert.deepEqual([...BUDGETS], [
      ['6100', 50000],
      ['6400', 25000],
    ])
  })

  it('refuses a budget that is no amount above zero', async () => {
    await pace()
    assert.throws(() => loadBudgets([{ account: '6100', monthly: 'lots' }]), /no amount/)
    assert.throws(() => loadBudgets([{ account: '6100', monthly: '0' }]), /no amount/)
  })

  it('says what each account spent of its budget in a month, and the share', async () => {
    await pace()
    assert.deepEqual(usage(ENTRIES, BUDGETS, '2025-12', 'EUR'), [
      { account: '6100', spent: 53050, budget: 50000, share: 10610 },
      { account: '6400', spent: 23680, budget: 25000, share: 9472 },
    ])
  })

  it('compares in the report currency, the budget converted at the rate', async () => {
    await pace()
    const [office] = usage(ENTRIES, BUDGETS, '2025-12', 'USD')
    assert.equal(office.budget, 54500)
    assert.equal(office.share, 10610)
  })
})
