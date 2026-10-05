/**
 * The digest `npm run parity:records` compares of a reading
 * (`tests/parity/digest.mjs`): what it holds, and that a lone surrogate
 * anywhere in it, a member's name too, is made whole and said.
 */
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { test } from 'node:test'
import { digest, wellFormed } from './parity/digest.mjs'

const OURS = ['missing session id', 'unreadable: no claude session ']
const sha = (text) => createHash('sha256').update(text).digest('hex')

/** A reading of one item, `fields` over the item's. */
function reading(fields = {}) {
  return {
    items: [{ id: 'u1', role: 'user', text: 'Hello', complete: true, at: 3, ...fields }],
    inFlight: false,
    asking: false,
    failed: false,
    quota: null,
    settlement: { state: 'settled' },
  }
}

test('a reading is its items, never their text, and what it says of the turn', () => {
  assert.deepEqual(digest(reading(), OURS), {
    items: [['u1', 'user', true, [3], 5, sha('Hello'), false]],
    inFlight: false,
    asking: false,
    failed: false,
    quota: null,
    settlement: 'settled',
    wellFormed: true,
  })
})

test('a time that is none is not a time that is null', () => {
  const [none, held] = [{ at: undefined }, { at: null }].map(
    (fields) => digest(reading(fields), OURS).items[0][3],
  )
  assert.deepEqual(none, [])
  assert.deepEqual(held, [null])
})

test('a reason of our own is compared whole, a platform`s as its class', () => {
  assert.equal(
    digest({ unknown: true, reason: 'unreadable: no claude session s' }, OURS).reason,
    'unreadable: no claude session s',
  )
  assert.equal(
    digest({ unknown: true, reason: 'unreadable: EACCES: permission denied' }, OURS).reason,
    'unreadable: «platform»',
  )
})

test('a lone surrogate is made whole, as the Rust readers hold it, and said', () => {
  const half = digest(reading({ text: 'half \ud800 of a pair' }), OURS)
  assert.equal(half.wellFormed, false)
  assert.deepEqual(half.items[0].slice(4, 6), [16, sha('half \ufffd of a pair')])
  // Anywhere: an id, a time's text, a member's name in a time that is an object.
  for (const fields of [{ id: 'u\udfff' }, { at: '\ud83d' }, { at: { '\ud800': 1 } }]) {
    const made = digest(reading(fields), OURS)
    assert.equal(made.wellFormed, false, JSON.stringify(fields))
    // Nothing in the digest is a lone half any more: walked again, it is whole.
    assert.equal(wellFormed(made)[1], true, JSON.stringify(fields))
  }
  assert.deepEqual(wellFormed({ '\ud800': ['\udc00x'] }), [{ '\ufffd': ['\ufffdx'] }, false])
  // A whole pair is no lone half.
  assert.equal(digest(reading({ text: '\ud83d\ude00' }), OURS).wellFormed, true)
})
