/**
 * How long the page's half of the packaged smoke waits for what it is waiting for.
 *
 * One knob sets it: the patience the smoke's Rust side is given
 * (`CONSENSFLOW_SMOKE_TIMEOUT_MS`), which the smoke tells the app as the step
 * (`CONSENSFLOW_SELFTEST_STEP_MS`, a quarter of it) and the app tells the page
 * as `stepMs`. Told nothing, the page waits {@link STEP_MS}, which is a quarter
 * of the Rust side's 180 s: the page always fails with its own words before the
 * Rust side gives up on hearing from it.
 *
 * A wait that can see progress (a flood of output arriving) also fails when it
 * has seen none for {@link STALL_MS}, so a raised step makes a slow machine
 * wait, never a dead flood.
 */

/** What one step is given when the app told the page nothing. */
export const STEP_MS = 45_000

/** The longest a wait that sees progress goes without any, whatever its step. */
export const STALL_MS = 45_000

export function sleep(ms) {
  return new Promise((wake) => setTimeout(wake, ms))
}

/** The step the app told the page, else {@link STEP_MS}. */
export function stepMsOf(config) {
  const told = config?.stepMs
  return Number.isFinite(told) && told > 0 ? told : STEP_MS
}

/**
 * The page's `until`, with the step it was given.
 *
 * Polls a condition against the live page rather than waiting on an event.
 * The thing being proved is what the human would see on screen, and the only
 * honest source for that is the buffer the renderer is reading from.
 *
 * `progress` is a number that only grows while what is awaited arrives; the
 * wait fails once it has not moved for {@link STALL_MS} (or the whole step, if
 * that is less). `detail` says more of the state, for the words of a failure.
 * `now` and `nap` are the clock and the pause between polls, for a test.
 */
export function waiter({ stepMs = STEP_MS, now = Date.now, nap = sleep } = {}) {
  return async function until(
    what,
    check,
    { timeoutMs = stepMs, note = null, progress = null, detail = null } = {},
  ) {
    const started = now()
    const deadline = started + timeoutMs
    const stallMs = Math.min(STALL_MS, timeoutMs)
    const more = () => (detail === null ? '' : ` (${detail()})`)
    let seen = progress === null ? null : progress()
    let movedAt = started
    let polls = 0
    for (;;) {
      const found = await check()
      if (found !== null && found !== undefined && found !== false) return found
      const at = now()
      if (progress !== null) {
        const latest = progress()
        if (latest !== seen) {
          seen = latest
          movedAt = at
        } else if (at - movedAt > stallMs) {
          throw new Error(
            `${what} stopped making progress: nothing new in ${at - movedAt} ms${more()}`,
          )
        }
      }
      if (at > deadline) throw new Error(`${what} did not happen within ${timeoutMs} ms${more()}`)
      polls += 1
      if (note !== null && polls % 30 === 0) await note(what, polls)
      await nap(100)
    }
  }
}
