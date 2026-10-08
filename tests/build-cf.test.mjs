import assert from 'node:assert/strict'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { placeCf } from '../app/scripts/build-cf.mjs'

/**
 * Where the native `cf` is put (`npm run build:cf`): bin/, which a clone may not
 * have (git keeps no empty folder, and nothing else is tracked in it once the
 * Node CLI is gone).
 */
describe('placing the built cf in bin/', () => {
  const root = mkdtempSync(join(tmpdir(), 'cf-place-'))
  after(() => rmSync(root, { recursive: true, force: true }))
  const built = join(root, 'built-cf')
  writeFileSync(built, 'the new cf')

  it('makes the folder a clone does not have, and puts the cf in it', () => {
    const bin = join(root, 'fresh', 'bin')
    assert.equal(existsSync(bin), false)
    const placed = placeCf(built, bin, 'cf')
    assert.equal(placed, join(bin, 'cf'))
    assert.equal(readFileSync(placed, 'utf8'), 'the new cf')
  })

  it('replaces the cf there and takes away the copies set aside earlier', () => {
    const bin = join(root, 'used', 'bin')
    mkdirSync(bin, { recursive: true })
    writeFileSync(join(bin, 'cf'), 'the old cf')
    writeFileSync(join(bin, 'cf.old-1760000000000'), 'one set aside')
    writeFileSync(join(bin, 'other'), 'not a copy of the cf')
    const placed = placeCf(built, bin, 'cf')
    assert.equal(readFileSync(placed, 'utf8'), 'the new cf')
    assert.deepEqual(readdirSync(bin).sort(), ['cf', 'other'])
  })
})
