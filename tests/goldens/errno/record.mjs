/**
 * Writes the Rust `cf-base`'s errno golden for this platform into
 * crates/cf-base (goldens.mjs says what it holds): `npm run goldens:errno`,
 * after a change of Node or of libuv, and once on each platform the tests
 * run on.
 */
import { fileURLToPath } from 'node:url'
import { writeErrnoGoldens } from './goldens.mjs'

const CRATE = fileURLToPath(new URL('../../../crates/cf-base', import.meta.url))
const { written } = writeErrnoGoldens(CRATE)
process.stdout.write(`${written} golden for ${process.platform} → ${CRATE}\n`)
