import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { adminGoldens } from './goldens/admin/goldens.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-harness', import.meta.url))

it('holds the Rust harness admin to what Node answers now: npm run goldens:admin after a change', async () => {
  for (const [relative, text] of Object.entries(await adminGoldens())) {
    const file = join(CRATE, ...relative.split('/'))
    assert.ok(
      existsSync(file),
      `${relative} is not there: record it on this system with npm run goldens:admin`,
    )
    assert.equal(readFileSync(file, 'utf8'), text, relative)
  }
})
