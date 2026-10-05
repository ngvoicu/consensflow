/**
 * Writes the engine's goldens into crates/cf-engine (goldens.mjs says what
 * each holds): `npm run goldens:engine`, after a change to what they record.
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { engineGoldens } from './goldens.mjs'

const CRATE = fileURLToPath(new URL('../../../crates/cf-engine', import.meta.url))
const files = engineGoldens()
for (const [relative, text] of Object.entries(files)) {
  const path = join(CRATE, ...relative.split('/'))
  mkdirSync(join(path, '..'), { recursive: true })
  writeFileSync(path, text)
}
process.stdout.write(`${Object.keys(files).length} goldens → ${CRATE}\n`)
