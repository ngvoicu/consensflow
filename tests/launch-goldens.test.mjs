import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { launchGoldens } from './goldens/launch/goldens.mjs'

const CRATE = fileURLToPath(new URL('../crates/cf-harness', import.meta.url))

/**
 * Where `actual` first differs from `expected`, a line of each around it: a
 * scenarios file is one long line per scenario, megabytes in all, which an
 * assertion's own message cuts off before the difference.
 */
function firstDifference(actual, expected) {
  const lines = actual.split('\n')
  const wanted = expected.split('\n')
  const line = lines.findIndex((text, at) => text !== wanted[at])
  const at = line === -1 ? lines.length : line
  const [got, want] = [lines[at] ?? '', wanted[at] ?? '']
  let column = 0
  while (column < got.length && got[column] === want[column]) column += 1
  const around = (text) => text.slice(Math.max(0, column - 300), column + 300)
  return `line ${at + 1}, column ${column + 1}:\n  actual   …${around(got)}…\n  expected …${around(want)}…`
}

it('holds the Rust launch to what Node answers now: npm run goldens:launch after a change', async () => {
  for (const [relative, text] of Object.entries(await launchGoldens())) {
    const committed = readFileSync(join(CRATE, ...relative.split('/')), 'utf8')
    if (committed !== text)
      assert.fail(`${relative} differs at ${firstDifference(text, committed)}`)
  }
})
