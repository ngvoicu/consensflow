/**
 * The launch's goldens, as Node answers them: what `crates/cf-harness` is
 * held to for step 3.4 (launch, channels, integration). Every file is
 * deterministic, so the unit suite holds the committed copies equal to what
 * this computes (tests/launch-goldens.test.mjs), and `npm run goldens:launch`
 * writes them again after a change to what they record.
 *
 * - `tests/goldens/launch/tables.json`: the pure functions (`tables.mjs`).
 * - `tests/goldens/launch/scenarios.<platform>.json`: each adapter's
 *   scenarios played (`runner.mjs`), one set per platform, since a launch
 *   names the platform's own paths and programs (`claude.mjs`).
 */
import { claudeScenarios } from './claude.mjs'
import { play } from './runner.mjs'
import { tables } from './tables.mjs'

/** Scenarios played, one a line. */
async function played(scenarios) {
  const lines = []
  for (const scenario of scenarios) lines.push(JSON.stringify(await play(scenario)))
  return `[\n${lines.join(',\n')}\n]\n`
}

/** Every golden's text, by its path under crates/cf-harness. */
export async function launchGoldens() {
  return {
    'tests/goldens/launch/tables.json': `${JSON.stringify(tables(), null, 2)}\n`,
    [`tests/goldens/launch/scenarios.${process.platform}.json`]: await played(claudeScenarios()),
  }
}
