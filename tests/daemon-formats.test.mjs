import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { formats } from './goldens/daemon/formats.mjs'

/**
 * The formats of the daemon's own files (`tests/goldens/daemon/formats.mjs`,
 * `files.json` of the goldens): made the same each time, on a clock that is
 * fixed only while they are made. The tests that hold the checked-in file to
 * what is made now are in `daemon-goldens.test.mjs`.
 */
describe('the formats of the daemon’s own files', () => {
  it('are made the same each time, with the clock fixed while they are made and given back after', () => {
    const clock = Date
    const made = formats()
    assert.equal(Date, clock)
    assert.equal(formats(), made)
    const { clock: fixed, log, trace } = JSON.parse(made)
    assert.equal(fixed, '2026-10-05T10:00:00.123Z')
    assert.equal(
      log.cases[0].expected,
      '2026-10-05T10:00:00.123Z info start pid 4242 node v26.8.1 home /tmp/consensflow\n',
    )
    // A trace line with no time of its own is dated by the fixed clock.
    const idle = trace.cases.find((one) => one.name === 'a window going idle')
    assert.match(idle.expected, /^\{"at":"2026-10-05T10:00:00\.123Z","kind":"window\.activity"/)
    assert.ok(log.rotation.aside !== null && trace.rotation.aside !== null, 'both are moved aside')
    assert.ok(
      !trace.forget.after.includes('"project":2,'),
      'what forget leaves has none of the project',
    )
  })
})
