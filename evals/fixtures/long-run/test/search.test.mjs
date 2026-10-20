import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { search } from '../src/search.mjs'

const ENTRIES = [
  { date: '2025-01-12', account: '6300', memo: 'Train to Cluj', tags: ['travel'] },
  { date: '2025-02-15', account: '6400', memo: 'Team lunch', tags: ['meals'] },
  { date: '2025-03-12', account: '6300', memo: 'Train to Cluj', tags: ['travel', 'client:acme'] },
]

describe('search', () => {
  it('finds entries by a word of their memo, whatever its case', async () => {
    await pace()
    assert.equal(search(ENTRIES, 'train').length, 2)
    assert.equal(search(ENTRIES, 'LUNCH').length, 1)
  })

  it('finds entries by tag, account and month, every word holding', async () => {
    await pace()
    assert.equal(search(ENTRIES, 'tag:travel').length, 2)
    assert.equal(search(ENTRIES, 'tag:client:acme').length, 1)
    assert.equal(search(ENTRIES, 'account:6300 month:2025-03').length, 1)
    assert.equal(search(ENTRIES, 'account:6400 train').length, 0)
  })

  it('finds every entry for an empty query', async () => {
    await pace()
    assert.equal(search(ENTRIES, '  ').length, 3)
  })
})
