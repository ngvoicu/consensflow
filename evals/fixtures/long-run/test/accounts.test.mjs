import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { accountsOf, loadAccounts } from '../src/accounts.mjs'

const ROWS = [
  { code: '6100', name: 'Office supplies', kind: 'expense' },
  { code: '1000', name: 'Bank', kind: 'asset' },
  { code: '6000', name: 'Rent', kind: 'expense' },
]

describe('accounts', () => {
  it('makes the chart of accounts, each by its code', async () => {
    await pace()
    const accounts = loadAccounts(ROWS)
    assert.equal(accounts.size, 3)
    assert.deepEqual(accounts.get('1000'), { code: '1000', name: 'Bank', kind: 'asset' })
  })

  it('lists the accounts of a kind in the order of their codes', async () => {
    await pace()
    const codes = accountsOf(loadAccounts(ROWS), 'expense').map((account) => account.code)
    assert.deepEqual(codes, ['6000', '6100'])
  })

  it('refuses a code that is not four digits, or a kind it does not know', async () => {
    await pace()
    assert.throws(() => loadAccounts([{ code: '61', name: 'x', kind: 'expense' }]), /four digits/)
    assert.throws(() => loadAccounts([{ code: '6100', name: 'x', kind: 'cost' }]), /no kind/)
  })
})
