import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { loadAccounts } from '../src/accounts.mjs'
import { accountReport, monthReport, total } from '../src/report.mjs'

const ACCOUNTS = loadAccounts([
  { code: '4000', name: 'Consulting income', kind: 'income' },
  { code: '6100', name: 'Office supplies', kind: 'expense' },
  { code: '6300', name: 'Travel', kind: 'expense' },
])

const ENTRIES = [
  { date: '2025-01-03', account: '4000', amount: 420000, currency: 'EUR' },
  { date: '2025-01-08', account: '6100', amount: 6420, currency: 'EUR' },
  { date: '2025-01-12', account: '6300', amount: 4260, currency: 'EUR' },
  { date: '2025-02-08', account: '6100', amount: 4890, currency: 'EUR' },
]

describe('report', () => {
  it('adds amounts up', async () => {
    await pace()
    assert.equal(total([100, 250, 650]), 1000)
    assert.equal(total([]), 0)
  })

  it('says each account that moved in a month, and what was earned and spent', async () => {
    await pace()
    const report = monthReport(ENTRIES, ACCOUNTS, '2025-01', 'EUR')
    assert.deepEqual(
      report.rows.map((row) => [row.code, row.total]),
      [
        ['4000', 420000],
        ['6100', 6420],
        ['6300', 4260],
      ],
    )
    assert.equal(report.income, 420000)
    assert.equal(report.spending, 10680)
  })

  it("says an account's total in each month of the year, and the year's", async () => {
    await pace()
    const report = accountReport(ENTRIES, ACCOUNTS, '6100', 2025, 'EUR')
    assert.equal(report.name, 'Office supplies')
    assert.equal(report.months.length, 12)
    assert.deepEqual(
      report.months.slice(0, 3).map((month) => month.total),
      [6420, 4890, 0],
    )
    assert.equal(report.total, 11310)
  })
})
