/**
 * The harness admin's goldens, as Node answers them: what `crates/cf-harness`
 * is held to for step 3.6 (the admin, and the detection beside it). Every file
 * is deterministic, so the unit suite holds the committed copies equal to what
 * this computes (tests/admin-goldens.test.mjs), and `npm run goldens:admin`
 * writes them again after a change to what they record.
 *
 * - `tests/goldens/admin/tables.json`: the layouts that are no more than
 *   a text, which hold on every system, as Node answers them for one
 *   (`tables.mjs`).
 * - `tests/goldens/admin/scenarios.<platform>.json`: each scenario played
 *   (`world.mjs`), one set per platform, since a CLI is found at the
 *   platform's own places under its own names and a path is joined with its
 *   own separator. The Windows one is recorded on Windows.
 */
import { caches } from './caches.mjs'
import { detections } from './detections.mjs'
import { feedScenarios } from './feeds.mjs'
import { layouts } from './layouts.mjs'
import { nodeTests } from './node-tests.mjs'
import { probes } from './probes.mjs'
import { sourceScenarios } from './sources.mjs'
import { tables } from './tables.mjs'
import { updates } from './updates.mjs'
import { play } from './world.mjs'

/** Scenarios played, one a line. */
async function played(scenarios) {
  const lines = []
  for (const scenario of scenarios) lines.push(JSON.stringify(await play(scenario)))
  return `[\n${lines.join(',\n')}\n]\n`
}

/** Every golden's text, by its path under crates/cf-harness. */
export async function adminGoldens() {
  const names = new Set()
  const scenarios = [
    ...nodeTests(),
    ...layouts(),
    ...sourceScenarios(),
    ...probes(),
    ...caches(),
    ...updates(),
    ...feedScenarios(),
    ...detections(),
  ]
  for (const { name } of scenarios) {
    if (names.has(name)) throw new Error(`two scenarios are called ${name}`)
    names.add(name)
  }
  return {
    'tests/goldens/admin/tables.json': `${JSON.stringify(tables(), null, 2)}\n`,
    [`tests/goldens/admin/scenarios.${process.platform}.json`]: await played(scenarios),
  }
}
