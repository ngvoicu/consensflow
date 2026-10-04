import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'

process.env.TZ = 'America/Los_Angeles'
const { committedText, filePath, recordsGoldens } = await import('./goldens/records/goldens.mjs')

const CRATE = fileURLToPath(new URL('../crates/cf-harness', import.meta.url))

it('holds the Rust records to what Node reads now: npm run goldens:records after a change', async () => {
  const files = await recordsGoldens()
  for (const [relative, text] of Object.entries(files)) {
    const bytes = readFileSync(join(CRATE, ...filePath(relative).split('/')))
    assert.equal(committedText(relative, bytes), text, relative)
  }
})
