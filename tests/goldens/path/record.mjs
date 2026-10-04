/**
 * Writes the Rust path module's goldens into crates/cf-base (goldens.mjs says
 * what they hold): `npm run goldens:path`, after a change to the segments or
 * to the Node that computes them.
 */
import { fileURLToPath } from 'node:url'
import { writePathGoldens } from './goldens.mjs'

const CRATE = fileURLToPath(new URL('../../../crates/cf-base', import.meta.url))
const { written, counts } = writePathGoldens(CRATE)
process.stdout.write(
  `${written} golden → ${CRATE} (${counts.joins} joins, ${counts.normalizes} normalizes)\n`,
)
