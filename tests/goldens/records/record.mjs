/**
 * Writes the harness records' goldens into crates/cf-harness (goldens.mjs
 * says what each holds): `npm run goldens:records`, after a change to a
 * reader, to quota, or to a fixture.
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

process.env.TZ = 'America/Los_Angeles'
const { fileBytes, filePath, recordsGoldens } = await import('./goldens.mjs')

const CRATE = fileURLToPath(new URL('../../../crates/cf-harness', import.meta.url))
const files = await recordsGoldens()
for (const [relative, text] of Object.entries(files)) {
  const path = join(CRATE, ...filePath(relative).split('/'))
  mkdirSync(join(path, '..'), { recursive: true })
  writeFileSync(path, fileBytes(relative, text))
}
process.stdout.write(`${Object.keys(files).length} goldens → ${CRATE}\n`)
