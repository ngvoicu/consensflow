import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { deliveryText, markerOf } from '../src/core/delivery-text.js'

/** How a message reads in its recipient's pane, and the header that proves it arrived. */
describe('the delivered text', () => {
  it('names the message, the task and the sender, and tells the reader how to answer a question', () => {
    const base = { id: 12, taskNumber: 3, sender: 'zeus', body: 'Which format?' }
    assert.equal(
      deliveryText({ ...base, kind: 'question' }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 "…"',
    )
    const options = [{ question: 'Which?', header: 'Format', options: [], multiple: false }]
    assert.equal(
      deliveryText({ ...base, kind: 'question', questions: options }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 "…" (a label or your own words)',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'question', questions: [...options, ...options] }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 "…" (a label or your own words; one line per question)',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'note', sender: null, taskNumber: null, body: 'hi' }),
      '[ConsensFlow m-12 · note from ConsensFlow]\nhi',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'result', body: 'Parser done' }),
      '[ConsensFlow m-12 · T-3 · result from @zeus]\nParser done\n\nDecide with: cf task accept T-3 · cf task reopen T-3 "…"',
      'a result says what to do with it: it is not a request',
    )
  })

  it('sends a long body as its opening and the command that reads the rest', () => {
    const text = deliveryText({
      id: 7,
      taskNumber: 1,
      sender: 'zeus',
      kind: 'result',
      body: 'x'.repeat(40_000),
    })
    assert.ok(text.length < 16_000)
    assert.match(
      text,
      /\n… \(40000 characters; read all of it with: cf inbox read m-7\)\n\nDecide with: /,
    )
    const whole = deliveryText({
      id: 8,
      taskNumber: 1,
      sender: 'chief',
      kind: 'task',
      body: 'y'.repeat(16_000),
    })
    assert.ok(whole.endsWith('y'.repeat(16_000)), 'a body of 16,000 characters goes whole')
  })

  it('tells the reader of an urgent question that its task waits for it, and who resumes the task', () => {
    const tell = { id: 12, taskNumber: 3, sender: 'chief', kind: 'question', body: 'Stop: use v2' }
    assert.equal(
      deliveryText({ ...tell, urgent: true }),
      '[ConsensFlow m-12 · T-3 · question from @chief]\nStop: use v2\n\nT-3 is paused for this. Run in your shell: cf answer m-12 "…"; the chief resumes the task.',
    )
    assert.equal(
      deliveryText({ ...tell, urgent: true, taskNumber: null }),
      '[ConsensFlow m-12 · question from @chief]\nStop: use v2\n\nRun in your shell: cf answer m-12 "…"',
      'with no task, nothing is paused',
    )
  })

  it("starts with the marker a window's record is searched for, which names no other message", () => {
    const text = deliveryText({ id: 12, taskNumber: 3, sender: 'chief', kind: 'task', body: 'x' })
    assert.ok(text.startsWith(markerOf(12)))
    assert.ok(!text.startsWith(markerOf(1)), 'm-1 is not m-12')
    // A window that did not keep the · still shows the marker: Devin on
    // Windows got it as |, and before that lost it.
    for (const kept of [text.replace(/·/g, '|'), text.replace(/·/g, '')]) {
      assert.ok(kept.startsWith(markerOf(12)))
    }
  })
})
