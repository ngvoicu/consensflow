/**
 * Writes the Rust launcher's goldens into crates/cf-launcher and crates/cf-harness
 * (goldens.mjs says what each holds): `npm run goldens:launcher`, after a change
 * to the terminal command, the stale-hook report, or Node.
 */
import { fileURLToPath } from 'node:url'
import { writeLauncherGoldens } from './goldens.mjs'

const REPO = fileURLToPath(new URL('../../..', import.meta.url))
const { written } = writeLauncherGoldens(REPO)
process.stdout.write(`${written} goldens → ${REPO}crates\n`)
