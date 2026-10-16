#!/usr/bin/env node

import { readdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Writes `fixtures/specs/<name>.tree.json` for each fixture folder: its tree as
 * the daemon answers `specs.tree` (each folder's entries by name, in byte order,
 * a file with its size, no dotfile). The page's stand-in answers with these
 * (`specs-fixtures.mjs`), and the daemon's own test reads each folder and holds
 * its answer to them (`crates/cf-daemon/src/page/tests/viewer.rs`), so a fixture
 * changed and this not run fails there. Run it after changing a fixture:
 *
 *   node app/tests/specs-trees.mjs
 */

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), 'fixtures', 'specs')
/** The fixture folders: one per Specmint variant, and the hostile one. */
const NAMES = ['core', 'core-html', 'tdd', 'tdd-html', 'hostile']

/** What `folder` (`at` in the tree) holds, as the daemon lists it. */
function tree(folder, at = '') {
  return readdirSync(join(folder, at))
    .filter((name) => !name.startsWith('.'))
    .sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)))
    .map((name) => {
      const path = at === '' ? name : `${at}/${name}`
      const found = statSync(join(folder, path))
      return found.isDirectory()
        ? { name, path, entries: tree(folder, path) }
        : { name, path, size: found.size }
    })
}

for (const name of NAMES) {
  const written = `${JSON.stringify(tree(join(FIXTURES, name)), null, 2)}\n`
  writeFileSync(join(FIXTURES, `${name}.tree.json`), written)
  process.stdout.write(`fixtures/specs/${name}.tree.json\n`)
}
