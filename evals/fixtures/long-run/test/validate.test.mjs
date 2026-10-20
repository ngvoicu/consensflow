import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { validateEntry } from '../src/validate.mjs'

const ACCOUNTS = new Map([['6100', { code: '6100', name: 'Office supplies', kind: 'expense' }]])
const RIGHT = { date: '2025-03-04', account: '6100', amount: 12000, currency: 'EUR' }

describe('validate', () => {
  it('finds nothing wrong with an entry that is right', async () => {
    await pace()
    assert.deepEqual(validateEntry(RIGHT, ACCOUNTS), [])
  })

  it('says each thing wrong with an entry, in a few words', async () => {
    await pace()
    const errors = validateEntry(
      { date: null, account: '9999', amount: Number.NaN, currency: 'GBP' },
      ACCOUNTS,
    )
    assert.deepEqual(errors, [
      'no date',
      'unknown account 9999',
      'the amount is no amount',
      'unknown currency GBP',
    ])
  })

  it('says an account or a currency left empty as none', async () => {
    await pace()
    const errors = validateEntry({ ...RIGHT, account: '', currency: '' }, ACCOUNTS)
    assert.deepEqual(errors, ['unknown account (none)', 'unknown currency (none)'])
  })
})
