/**
 * Writes the harness admin's goldens into crates/cf-harness (goldens.mjs says
 * what each holds): `npm run goldens:admin`, after a change to what they
 * record, and once on each platform the tests run on.
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { adminGoldens } from './goldens.mjs'

const CRATE = fileURLToPath(new URL('../../../crates/cf-harness', import.meta.url))
const files = await adminGoldens()
for (const [relative, text] of Object.entries(files)) {
  const path = join(CRATE, ...relative.split('/'))
  mkdirSync(join(path, '..'), { recursive: true })
  writeFileSync(path, text)
}
process.stdout.write(`${Object.keys(files).length} goldens → ${CRATE}\n`)
