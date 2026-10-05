import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { launchGoldens } from './goldens/launch/goldens.mjs'
import { firstDifference } from './helpers.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-harness', import.meta.url))

it('holds the Rust launch to what Node answers now: npm run goldens:launch after a change', async () => {
  for (const [relative, text] of Object.entries(await launchGoldens())) {
    const committed = readFileSync(join(CRATE, ...relative.split('/')), 'utf8')
    if (committed !== text)
      assert.fail(`${relative} differs at ${firstDifference(text, committed)}`)
  }
})
