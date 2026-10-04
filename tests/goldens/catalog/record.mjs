/**
 * Writes the Rust catalog's goldens into crates/cf-catalog (goldens.mjs says
 * what each holds): `npm run goldens:catalog`, after a change to the
 * presets, the catalog or the roster.
 */
import { fileURLToPath } from 'node:url'
import { writeCatalogGoldens } from './goldens.mjs'

const CRATE = fileURLToPath(new URL('../../../crates/cf-catalog', import.meta.url))
const { written, skipped } = writeCatalogGoldens(CRATE)
process.stdout.write(
  `${written} goldens → ${CRATE} (${skipped} inputs left out: Node throws a TypeError on them)\n`,
)
