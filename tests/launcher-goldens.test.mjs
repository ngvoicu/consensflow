import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { launcherGoldens } from './goldens/launcher/goldens.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))

it('holds the Rust terminal command and stale-hook report to what Node says now: npm run goldens:launcher after a change', () => {
  const { files } = launcherGoldens()
  for (const [relative, text] of Object.entries(files)) {
    assert.equal(readFileSync(join(REPO, ...relative.split('/')), 'utf8'), text, relative)
  }
})
