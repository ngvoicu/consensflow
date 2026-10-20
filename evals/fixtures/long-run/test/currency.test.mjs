import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { convert, isCurrency, RATES } from '../src/currency.mjs'

describe('currency', () => {
  it('knows the euro, the dollar and the leu, each in ten-thousandths of a euro', async () => {
    await pace()
    assert.deepEqual(Object.keys(RATES), ['EUR', 'USD', 'RON'])
    assert.ok(Object.values(RATES).every(Number.isInteger))
    assert.ok(isCurrency('USD') && !isCurrency('GBP'))
  })

  it('converts at the rate, an amount in its own currency staying as it is', async () => {
    await pace()
    assert.equal(convert(10000, 'EUR', 'USD'), 10900)
    assert.equal(convert(10900, 'USD', 'EUR'), 10000)
    assert.equal(convert(4321, 'EUR', 'EUR'), 4321)
    assert.equal(convert(10000, 'EUR', 'RON'), 49700)
  })

  it('refuses a currency it has no rate for', async () => {
    await pace()
    assert.throws(() => convert(100, 'EUR', 'GBP'), /no rate between EUR and GBP/)
  })
})
