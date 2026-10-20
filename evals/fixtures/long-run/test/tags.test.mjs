import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { hasTag, parseTags } from '../src/tags.mjs'

describe('tags', () => {
  it('reads the tags of a text without their # and in lower case', async () => {
    await pace()
    assert.deepEqual(parseTags('#travel  #client:ACME'), ['travel', 'client:acme'])
    assert.deepEqual(parseTags('no tags # here'), [])
    assert.deepEqual(parseTags(undefined), [])
  })

  it('finds a tag of an entry whatever its case', async () => {
    await pace()
    const entry = { tags: ['travel', 'client:acme'] }
    assert.ok(hasTag(entry, 'Client:ACME'))
    assert.ok(!hasTag(entry, 'meals'))
  })
})
