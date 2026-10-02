import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { admission, windowText } from '../src/adapters/shared.js'

/**
 * What every adapter shares (`src/adapters/shared.js`): the text a window is
 * given, which the pane host must be able to take, and how a send's answer
 * reads as a delivery outcome.
 */
describe('the text a window is given', () => {
  it('keeps whole characters: the half of an emoji a cut left behind is dropped', () => {
    // The dispatcher cuts a long body at 3,000 code units; an emoji across
    // the cut leaves its first half, which no JSON frame to the host carries.
    const cut = `${'a'.repeat(2999)}\ud83d\n… (4200 characters; read all of it with: cf inbox read m-9)`
    assert.equal(cut.isWellFormed(), false)
    const sent = windowText(cut)
    assert.equal(sent.isWellFormed(), true)
    assert.equal(
      sent,
      `${'a'.repeat(2999)}\n… (4200 characters; read all of it with: cf inbox read m-9)`,
    )
    assert.equal(windowText('done \u{1F600}'), 'done \u{1F600}', 'a whole emoji stays')
    assert.equal(windowText('\udc00 tail'), ' tail', 'so does a half without its first')
  })

  it('shows every control character but tab and newline, which the pane host refuses', () => {
    assert.equal(
      windowText('red \u001b[31mtext\u001b[0m\r\nnext\tcolumn\r50%\r60%\u0007\u007f\u0085'),
      'red ␛[31mtext␛[0m\nnext\tcolumn␍50%␍60%␇␡\ufffd',
    )
    const controls = Array.from({ length: 0xa0 }, (_, code) => String.fromCharCode(code))
      .filter((character) => /\p{Cc}/u.test(character))
      .join('')
    assert.deepEqual(
      [...windowText(controls)].filter((character) => /\p{Cc}/u.test(character)),
      ['\t', '\n'],
    )
  })
})

describe('a delivery outcome', () => {
  it("reads the bridge's own deadline or end as uncertain, never as a refusal", () => {
    assert.deepEqual(admission({ ok: true }, 'refused'), { admitted: true })
    assert.deepEqual(admission({ ok: false, error: 'deadline' }, 'refused'), {
      admitted: null,
      reason: 'deadline',
    })
    assert.deepEqual(admission({ ok: false, error: 'eof' }, 'refused'), {
      admitted: null,
      reason: 'eof',
    })
    assert.deepEqual(admission({ ok: false, error: 'stale pane' }, 'refused'), {
      admitted: false,
      reason: 'stale pane',
    })
  })
})
