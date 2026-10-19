/**
 * The slow work, slow on purpose. A real project's integration tests and a real
 * site's image work take minutes; these take as long by waiting: each test case
 * waits `test` milliseconds and each page of the build `page`. `LONGRUN_PACE_MS`
 * sets both, `0` for a quick look; a phase's numbers are those of a full run.
 */
const DEFAULTS = { test: 2000, page: 6000 }

/** How long one piece of `kind` waits, in milliseconds. */
export function paceMs(kind) {
  const said = process.env.LONGRUN_PACE_MS
  return said === undefined || said === '' ? DEFAULTS[kind] : Number(said)
}

/** Waits as long as one piece of `kind` does. */
export function pace(kind = 'test') {
  const ms = paceMs(kind)
  return new Promise((resolve) => setTimeout(resolve, ms))
}
