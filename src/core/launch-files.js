import { readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'

/**
 * What a window leaves in the home under its launch id: the settings and
 * integration files written for it (`integrations/<harness>/<launch>`). A
 * launch id lives as long as its window; nothing else reads these folders,
 * so they go when it does.
 */
const HARNESSES = ['claude', 'pi', 'devin']
const LAUNCH_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/

const folders = (home, launch) =>
  HARNESSES.map((harness) => join(home, 'integrations', harness, launch))

/** A window closed: its files go. An id that is not a launch id names nothing. */
export function forgetLaunch(home, launch) {
  if (typeof launch !== 'string' || !LAUNCH_ID.test(launch)) return
  for (const folder of folders(home, launch)) rmSync(folder, { recursive: true, force: true })
}

/** At start, when no window is open: every launch's files go. */
export function sweepLaunches(home) {
  const parents = HARNESSES.map((harness) => join(home, 'integrations', harness))
  let swept = 0
  for (const parent of parents) {
    let names = []
    try {
      names = readdirSync(parent)
    } catch {
      continue
    }
    for (const name of names) {
      if (!LAUNCH_ID.test(name)) continue
      rmSync(join(parent, name), { recursive: true, force: true })
      swept += 1
    }
  }
  return swept
}
