import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { monthName, monthOf, monthsOf, parseDate } from '../src/dates.mjs'

describe('dates', () => {
  it('takes a date of the calendar written YYYY-MM-DD', async () => {
    await pace()
    assert.equal(parseDate('2025-03-14'), '2025-03-14')
    assert.equal(parseDate(' 2024-02-29 '), '2024-02-29')
  })

  it('refuses a date the calendar has not, or written another way', async () => {
    await pace()
    for (const text of ['2025-02-29', '2025-13-01', '14/03/2025', '2025-3-14', '', undefined]) {
      assert.equal(parseDate(text), null, String(text))
    }
  })

  it('says the month of a date, every month of a year, and a month as a reader reads it', async () => {
    await pace()
    assert.equal(monthOf('2025-03-14'), '2025-03')
    const months = monthsOf(2025)
    assert.equal(months.length, 12)
    assert.deepEqual([months[0], months[11]], ['2025-01', '2025-12'])
    assert.equal(monthName('2025-03'), 'March 2025')
  })
})
