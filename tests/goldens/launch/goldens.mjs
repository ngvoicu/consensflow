/**
 * The launch's goldens, as Node answers them: what `crates/cf-harness` is
 * held to for step 3.4 (launch, channels, integration). Every file is
 * deterministic, so the unit suite holds the committed copies equal to what
 * this computes (tests/launch-goldens.test.mjs), and `npm run goldens:launch`
 * writes them again after a change to what they record.
 *
 * - `tests/goldens/launch/tables.json`: the pure functions (`tables.mjs`).
 */
import { tables } from './tables.mjs'

/** Every golden's text, by its path under crates/cf-harness. */
export function launchGoldens() {
  return {
    'tests/goldens/launch/tables.json': `${JSON.stringify(tables(), null, 2)}\n`,
  }
}
