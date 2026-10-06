import assert from 'node:assert/strict'
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { FILE, useNode } from '../src/use-node.js'

/**
 * The Node decider of the way back (`src/use-node.js`) held to the table the Rust
 * one (`crates/cf-base/src/way_back.rs`) is held to by
 * `crates/cf-base/tests/way_back.rs`: each case makes a state at `<home>/use-node`,
 * with the home named by `CONSENSFLOW_HOME` and again by `HOME` alone, and every
 * stray environment on top, among them the `CONSENSFLOW_DAEMON` the product no
 * longer reads. One answer for all of them.
 */
const TABLE = JSON.parse(
  readFileSync(
    fileURLToPath(new URL('../crates/cf-base/tests/way-back.json', import.meta.url)),
    'utf8',
  ),
)

/** Makes `state` at `folder/use-node`; false where this system cannot make it. */
function make(state, folder) {
  const file = join(folder, FILE)
  mkdirSync(folder, { recursive: true })
  switch (state) {
    case 'absent':
      return true
    case 'file':
      writeFileSync(file, 'go back to node\n')
      return true
    case 'empty':
      writeFileSync(file, '')
      return true
    case 'folder':
      mkdirSync(file)
      return true
    case 'unreadable':
      writeFileSync(file, 'go back to node\n')
      chmodSync(file, 0o000)
      return true
    case 'link':
      writeFileSync(join(folder, 'target'), 'go back to node\n')
      symlinkSync(join(folder, 'target'), file)
      return true
    case 'dangling':
      symlinkSync(join(folder, 'nowhere'), file)
      return true
    case 'unsearchable':
      writeFileSync(file, 'go back to node\n')
      chmodSync(folder, 0o000)
      try {
        statSync(file)
      } catch {
        return true
      }
      // A user who may search any folder (root) cannot be kept out of one.
      chmodSync(folder, 0o755)
      return false
    default:
      throw new Error(`the table has a state this test cannot make: ${state}`)
  }
}

/** Gives the folder back its modes, so that it can be removed. */
function unmake(folder) {
  for (const [path, mode] of [
    [folder, 0o755],
    [join(folder, FILE), 0o644],
  ]) {
    try {
      chmodSync(path, mode)
    } catch {}
  }
}

describe('which implementation writes a home, by the Node decider', () => {
  it('has one answer for every case of the table, whatever the environment adds', () => {
    let held = 0
    for (const { name, state, node, unix } of TABLE.cases) {
      if (unix && process.platform === 'win32') continue
      for (const byVariable of [true, false]) {
        const root = mkdtempSync(join(tmpdir(), 'cf-way-back-'))
        try {
          const folder = byVariable ? join(root, 'consensflow') : join(root, 'user', '.consensflow')
          const vars = byVariable
            ? { CONSENSFLOW_HOME: folder }
            : { HOME: join(root, 'user'), USERPROFILE: join(root, 'user') }
          if (!make(state, folder)) continue
          for (const stray of TABLE.strays) {
            assert.equal(
              useNode({ ...vars, ...stray }),
              node,
              `${name} (home by ${byVariable ? 'CONSENSFLOW_HOME' : 'HOME'}, with ${JSON.stringify(stray)})`,
            )
            held += 1
          }
          unmake(folder)
        } finally {
          rmSync(root, { recursive: true, force: true })
        }
      }
    }
    // The table was read: its cases were made and asked, not skipped one by one.
    assert.ok(held >= 2 * TABLE.strays.length * 4, `${held} answers were held`)
  })
})
