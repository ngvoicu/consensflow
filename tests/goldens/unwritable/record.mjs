/**
 * Writes the Rust `cf-catalog`'s failed-save golden for this platform into
 * crates/cf-catalog (goldens.mjs says what it holds): `npm run
 * goldens:unwritable`, after a change to `saveDocument` or to Node, and once
 * on each platform the tests run on.
 */
import { fileURLToPath } from 'node:url'
import { writeUnwritableGoldens } from './goldens.mjs'

const CRATE = fileURLToPath(new URL('../../../crates/cf-catalog', import.meta.url))
const { written } = writeUnwritableGoldens(CRATE)
process.stdout.write(`${written} golden for ${process.platform} → ${CRATE}\n`)
