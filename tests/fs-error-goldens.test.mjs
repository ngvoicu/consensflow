import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { errnoGoldens } from './goldens/errno/goldens.mjs'
import { unwritableGoldens } from './goldens/unwritable/goldens.mjs'

const CRATES = fileURLToPath(new URL('../crates', import.meta.url))

/** Holds each file of `crate`, by its path under it, equal to what Node computes now. */
function holdsEqual(crate, files) {
  for (const [relative, text] of Object.entries(files)) {
    assert.equal(readFileSync(join(CRATES, crate, ...relative.split('/')), 'utf8'), text, relative)
  }
}

it("holds the Rust errno golden to libuv's names and words now: npm run goldens:errno after a change", () => {
  holdsEqual('cf-base', errnoGoldens().files)
})

it('holds the Rust failed-save golden to what the roster says now: npm run goldens:unwritable after a change', () => {
  holdsEqual('cf-catalog', unwritableGoldens().files)
})
