import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { engineGoldens } from './goldens/engine/goldens.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-engine', import.meta.url))

it('holds the Rust engine to what Node answers now: npm run goldens:engine after a change', () => {
  for (const [relative, text] of Object.entries(engineGoldens())) {
    assert.equal(readFileSync(join(CRATE, ...relative.split('/')), 'utf8'), text, relative)
  }
})
