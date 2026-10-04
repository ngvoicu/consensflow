import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { catalogGoldens } from './goldens/catalog/goldens.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-catalog', import.meta.url))

it('holds the Rust catalog to what Node computes now: npm run goldens:catalog after a change', () => {
  const { files } = catalogGoldens()
  for (const [relative, text] of Object.entries(files)) {
    assert.equal(readFileSync(join(CRATE, ...relative.split('/')), 'utf8'), text, relative)
  }
})
