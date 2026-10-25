import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { STALL_MS, STEP_MS, stepMsOf, waiter } from '../app/ui/core/selftest-wait.js'
import { runSelftest } from '../app/ui/selftest.js'

/**
 * How long the page's half of the packaged smoke waits (app/ui/core/selftest-wait.js):
 * the step the smoke's patience tells it, and the stall that fails a flood which has stopped.
 * The Rust side's tests hold the numbers it tells (crates/cf-e2e/tests/smoke/machine.rs) and
 * the app's the way they reach the page (app/src-tauri/src/lib.rs); the smoke itself, on a
 * built app, holds the flood.
 */

/** A clock that moves only when the waiter pauses, so a minute of waiting takes no time. */
function fakeTime() {
  const time = { at: 1_000_000 }
  return {
    time,
    now: () => time.at,
    nap: async (ms) => {
      time.at += ms
    },
  }
}

/** A flood that sends `rate` bytes every `every` ms of fake time until `until` ms from the start. */
function flood(clock, { rate, until, every = 100 }) {
  const start = clock.time.at
  return {
    bytes: () => Math.floor(Math.min(clock.time.at - start, until) / every) * rate,
    drained: () => clock.time.at - start >= until,
  }
}

describe('the step the page is given', () => {
  it('is what the app told it, and 45 s where the app told it nothing usable', () => {
    assert.equal(stepMsOf({ stepMs: 135_000 }), 135_000)
    assert.equal(stepMsOf({ stepMs: 250 }), 250)
    for (const told of [
      undefined,
      null,
      {},
      { stepMs: null },
      { stepMs: 0 },
      { stepMs: -5 },
      { stepMs: Number.NaN },
      { stepMs: Number.POSITIVE_INFINITY },
      { stepMs: '135000' },
    ]) {
      assert.equal(stepMsOf(told), STEP_MS, JSON.stringify(told))
    }
    assert.equal(STEP_MS, 45_000)
  })
})

describe('a flood that is slow', () => {
  // On a loaded machine: 70 s of it, a steady 25 KB a second, or 50 KB in a burst every 20 s with
  // nothing between (a pause is no stop while it is shorter than the stall limit).
  const floods = {
    steady: (clock) => flood(clock, { rate: 2_500, until: 70_000 }),
    'in bursts': (clock) => flood(clock, { rate: 50_000, until: 70_000, every: 20_000 }),
  }

  for (const [kind, slow] of Object.entries(floods)) {
    it(`fails at the fixed 45 s, and drains when the step the smoke was given reaches the page (${kind})`, async () => {
      const fixed = fakeTime()
      const fixedFlood = slow(fixed)
      await assert.rejects(
        waiter({ stepMs: stepMsOf({}), now: fixed.now, nap: fixed.nap })(
          'the flood drained',
          () => (fixedFlood.drained() ? { last: 4096 } : null),
          { progress: fixedFlood.bytes },
        ),
        /the flood drained did not happen within 45000 ms/,
      )

      const told = fakeTime()
      const toldFlood = slow(told)
      const found = await waiter({
        stepMs: stepMsOf({ stepMs: 135_000 }),
        now: told.now,
        nap: told.nap,
      })('the flood drained', () => (toldFlood.drained() ? { last: 4096 } : null), {
        progress: toldFlood.bytes,
      })
      assert.deepEqual(found, { last: 4096 })
    })
  }
})

describe('a flood that has stopped', () => {
  it('fails when no byte has come for the stall limit, long before a raised step is over', async () => {
    const clock = fakeTime()
    // 10 s of output, then nothing, with a step of 10 minutes.
    const dying = flood(clock, { rate: 2_500, until: 10_000 })
    const start = clock.time.at
    let said = ''
    await assert.rejects(
      waiter({ stepMs: 600_000, now: clock.now, nap: clock.nap })('the flood drained', () => null, {
        progress: dying.bytes,
        detail: () => `${dying.bytes()} bytes, 3 acks, page hidden: false`,
      }),
      (failure) => {
        said = failure.message
        return true
      },
    )
    assert.match(said, /^the flood drained stopped making progress: nothing new in \d+ ms/)
    assert.match(said, /\(250000 bytes, 3 acks, page hidden: false\)$/)
    const waited = clock.time.at - start
    assert.ok(waited > 10_000 + STALL_MS && waited < 10_000 + STALL_MS + 1_000, `${waited}`)
  })

  it('fails at the default step as a stopped flood, not as a slow one, if nothing came at all', async () => {
    const clock = fakeTime()
    await assert.rejects(
      waiter({ now: clock.now, nap: clock.nap })('the flood drained', () => null, {
        progress: () => 0,
      }),
      /the flood drained stopped making progress/,
    )
  })

  it('is told apart from a wait that has no progress to see, which waits out its step', async () => {
    const clock = fakeTime()
    const start = clock.time.at
    await assert.rejects(
      waiter({ stepMs: 90_000, now: clock.now, nap: clock.nap })(
        'the chief window appeared',
        () => null,
      ),
      /the chief window appeared did not happen within 90000 ms$/,
    )
    assert.ok(clock.time.at - start > 90_000)
  })
})

describe('a wait', () => {
  it('says how it is going every 30 polls, and returns what the check found', async () => {
    const clock = fakeTime()
    const noted = []
    let polls = 0
    const found = await waiter({ now: clock.now, nap: clock.nap })(
      'the harness banner rendered',
      () => {
        polls += 1
        return polls > 65 ? 'CFSMOKE-READY x' : null
      },
      { note: async (what, count) => noted.push([what, count]) },
    )
    assert.equal(found, 'CFSMOKE-READY x')
    assert.deepEqual(noted, [
      ['the harness banner rendered', 30],
      ['the harness banner rendered', 60],
    ])
  })
})

describe('the self-test of the page', () => {
  it('waits the step it was told and says so, not the 45 s it has when told nothing', {
    timeout: 10_000,
  }, async () => {
    const globals = { document: globalThis.document, location: globalThis.location }
    globalThis.document = { hidden: false, querySelectorAll: () => [] }
    globalThis.location = { protocol: 'tauri:' }
    try {
      const reports = []
      const started = Date.now()
      await runSelftest({
        config: { dir: '/workspace', tag: 'tag', stepMs: 150 },
        invoke: async (name, body) => {
          if (name === 'selftest_report') reports.push(body)
        },
        daemon: async () => ({ ok: true, project: { id: 'project' } }),
        refresh: async () => {},
        // No window of the chief's ever appears: the first wait is the one that ends it.
        registry: { emulators: new Map() },
        sendInput: async () => {},
        onAck: () => {},
        onOutput: () => {},
      })
      const failed = reports.find((report) => report.event === 'failed')
      assert.match(failed?.data.error, /the chief window appeared did not happen within 150 ms$/)
      assert.ok(Date.now() - started < 10_000, `${Date.now() - started} ms`)
    } finally {
      for (const [name, value] of Object.entries(globals)) {
        if (value === undefined) delete globalThis[name]
        else globalThis[name] = value
      }
    }
  })
})
