import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { fromCents, round, share, split, sum, toCents } from '../src/money.mjs'

describe('money', () => {
  it('reads an amount as whole cents, with or without its thousands grouped', async () => {
    await pace()
    assert.equal(toCents('1,234.50'), 123450)
    assert.equal(toCents('12.3'), 1230)
    assert.equal(toCents('-30.00'), -3000)
    assert.equal(toCents('7'), 700)
  })

  it('reads text that is no amount as NaN', async () => {
    await pace()
    for (const text of ['', 'kickoff"', '1.234', '12,34.5.6', 'EUR']) {
      assert.ok(Number.isNaN(toCents(text)), text)
    }
  })

  it('writes cents with two decimals and no grouping', async () => {
    await pace()
    assert.equal(fromCents(123450), '1234.50')
    assert.equal(fromCents(5), '0.05')
    assert.equal(fromCents(-3000), '-30.00')
  })

  it('rounds a fraction of a cent that is not a half to the nearest cent', async () => {
    await pace()
    assert.equal(round(1362.4), 1362)
    assert.equal(round(1362.6), 1363)
    assert.equal(round(-2.4), -2)
  })

  it('sums amounts, and a share is hundredths of a percent', async () => {
    await pace()
    assert.equal(sum([100, 250, -50]), 300)
    assert.equal(share(22500, 25000), 9000)
    assert.equal(share(1, 3), 3333)
    assert.equal(share(5, 0), 0)
  })

  it('splits an amount into parts that differ by a cent at most, the larger first', async () => {
    await pace()
    assert.deepEqual(split(1000, 3), [334, 333, 333])
    assert.deepEqual(split(900, 3), [300, 300, 300])
    assert.equal(sum(split(12345, 7)), 12345)
  })
})
