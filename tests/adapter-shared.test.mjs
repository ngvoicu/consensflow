import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { claudeCodeAdapter } from '../src/adapters/claude-code.js'
import { codexAdapter } from '../src/adapters/codex.js'
import { devinAdapter } from '../src/adapters/devin.js'
import { openCodeAdapter } from '../src/adapters/opencode.js'
import { piAdapter } from '../src/adapters/pi.js'
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
  it('reads a refusal only where the channel says nothing reached the harness', () => {
    assert.deepEqual(admission({ ok: true, admitted: true }, 'refused'), { admitted: true })
    assert.deepEqual(
      admission(
        { ok: false, admitted: false, bytesWritten: 0, error: 'stale-generation', cause: 'gone' },
        'refused',
      ),
      { admitted: false, reason: 'gone' },
    )
    assert.deepEqual(admission({ ok: false, admitted: false }, 'refused'), {
      admitted: false,
      reason: 'refused',
    })
    assert.deepEqual(
      admission({ ok: false, admitted: null, error: 'uncertain', cause: 'cut off' }, 'refused'),
      { admitted: null, reason: 'cut off' },
    )
    // An answer that does not say may have reached the harness: sending it
    // again at once is how Pi got a message twice.
    assert.deepEqual(admission({ ok: false, error: 'transport', cause: 'EISDIR' }, 'refused'), {
      admitted: null,
      reason: 'EISDIR',
    })
  })
})

describe("a harness's own record, read with no window open", () => {
  it('asks each harness reader for the conversation it names', async () => {
    const env = { HOME: '/home/a' }
    for (const [make, kind] of [
      [claudeCodeAdapter, 'claude-code'],
      [codexAdapter, 'codex'],
      [piAdapter, 'pi'],
      [openCodeAdapter, 'opencode'],
      [devinAdapter, 'devin'],
    ]) {
      const asked = []
      const answers = async (...args) => {
        asked.push(args)
        return { items: [{ id: 'u1', role: 'user', text: 'hello' }] }
      }
      const record = await make({ env, answers }).record({ conversation: { nativeSession: 's-1' } })
      assert.deepEqual(asked, [[kind, 's-1', env]], kind)
      assert.deepEqual(
        record.items.map((item) => item.id),
        ['u1'],
        kind,
      )
    }
  })
})
