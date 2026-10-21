import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { formatMoney, formatShare } from '../src/format.mjs'

describe('format', () => {
  it('shows an amount with its thousands grouped, two decimals and its currency', async () => {
    await pace()
    assert.equal(formatMoney(123450, 'EUR'), '1,234.50 EUR')
    assert.equal(formatMoney(123456789, 'USD'), '1,234,567.89 USD')
    assert.equal(formatMoney(5, 'EUR'), '0.05 EUR')
  })

  it('shows a refund with its sign before it', async () => {
    await pace()
    assert.equal(formatMoney(-3000, 'EUR'), '-30.00 EUR')
    assert.equal(formatMoney(-123450, 'RON'), '-1,234.50 RON')
  })

  it('shows a share as a percentage with two decimals', async () => {
    await pace()
    assert.equal(formatShare(9050), '90.50%')
    assert.equal(formatShare(10612), '106.12%')
    assert.equal(formatShare(0), '0.00%')
  })
})
