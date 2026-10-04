import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { pathGoldens } from './goldens/path/goldens.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-base', import.meta.url))

it('holds the Rust path module to what Node joins and normalizes now: npm run goldens:path after a change', () => {
  const { files } = pathGoldens()
  for (const [relative, text] of Object.entries(files)) {
    assert.equal(readFileSync(join(CRATE, ...relative.split('/')), 'utf8'), text, relative)
  }
})
