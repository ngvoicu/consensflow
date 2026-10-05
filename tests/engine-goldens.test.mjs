import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { engineGoldens } from './goldens/engine/goldens.mjs'
import { firstDifference } from './helpers.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-engine', import.meta.url))

it('holds the Rust engine to what Node answers now: npm run goldens:engine after a change', () => {
  for (const [relative, text] of Object.entries(engineGoldens())) {
    const committed = readFileSync(join(CRATE, ...relative.split('/')), 'utf8')
    if (committed !== text)
      assert.fail(`${relative} differs at ${firstDifference(text, committed)}`)
  }
})
