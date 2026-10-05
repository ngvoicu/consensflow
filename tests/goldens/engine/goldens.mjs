/**
 * The engine's goldens, as Node answers them: what `crates/cf-engine` is held
 * to for step 3.5 (delivery text, handoff, role instructions). Every file is
 * deterministic, so the unit suite holds the committed copy equal to what
 * this computes (tests/engine-goldens.test.mjs), and `npm run goldens:engine`
 * writes it again after a change to what it records.
 *
 * - `tests/goldens/text.json`: the engine's texts (`text.mjs`).
 */
import { text } from './text.mjs'

/** Every golden's text, by its path under crates/cf-engine. */
export function engineGoldens() {
  return {
    'tests/goldens/text.json': `${JSON.stringify(text(), null, 2)}\n`,
  }
}
