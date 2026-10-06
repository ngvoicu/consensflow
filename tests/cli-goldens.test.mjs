import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { cliGoldens } from './goldens/cli/goldens.mjs'
import { firstDifference } from './helpers.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))

it('holds the Rust CLI to what Node answers now: npm run goldens:cli after a change', async () => {
  for (const [relative, text] of Object.entries(await cliGoldens())) {
    const file = join(REPO, ...relative.split('/'))
    assert.ok(
      existsSync(file),
      `${relative} is not there: record it on this system with npm run goldens:cli`,
    )
    const recorded = readFileSync(file, 'utf8')
    assert.ok(recorded === text, `${relative}: ${firstDifference(recorded, text)}`)
  }
})
