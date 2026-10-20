import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { JOURNAL, journalCsv } from '../src/export.mjs'

describe('export', () => {
  it("writes the entries as the journal's CSV, with the journal's columns", async () => {
    await pace()
    const text = journalCsv([
      { date: '2025-03-25', account: '6100', memo: 'Refund', amount: -3000, currency: 'EUR', tags: ['office', 'refund'] },
    ])
    assert.equal(text, `${JOURNAL.join(',')}\n2025-03-25,6100,Refund,-30.00,EUR,#office #refund\n`)
  })

  it('quotes a memo that holds a comma', async () => {
    await pace()
    const text = journalCsv([
      { date: '2025-04-09', account: '6300', memo: 'Flight Bucharest, Cluj', amount: 31200, currency: 'EUR', tags: [] },
    ])
    assert.ok(text.includes('"Flight Bucharest, Cluj",312.00'), text)
  })
})
