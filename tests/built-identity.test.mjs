import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

import {
  builtIdentityIn,
  refusalOf,
  SMOKE,
  smokeBuildConfig,
} from '../app/scripts/built-identity.mjs'

const RELEASE = 'dev.ngvoicu.consensflow'
const exeSaying = (identity) =>
  Buffer.concat([
    Buffer.from('MZ\0the app\0'),
    Buffer.from(`\0consensflow-built-identity:${identity};\0`),
    Buffer.from('\0the rest of it\0dev.ngvoicu.consensflow\0'),
  ])

test('an app says the identity its build was made with, and nothing where it says none', () => {
  assert.equal(builtIdentityIn(exeSaying(SMOKE)), SMOKE)
  assert.equal(builtIdentityIn(exeSaying(RELEASE)), RELEASE)
  // The release's name elsewhere in the file is no identity of the build's.
  assert.equal(builtIdentityIn(Buffer.from('MZ\0dev.ngvoicu.consensflow\0')), null)
  // Words cut short, or two that differ, say nothing a test can trust.
  assert.equal(builtIdentityIn(Buffer.from('MZ consensflow-built-identity:dev.ngvoicu.cons')), null)
  assert.equal(builtIdentityIn(Buffer.concat([exeSaying(SMOKE), exeSaying(RELEASE)])), null)
})

test('a test starts only an app of the smoke identity: not the release, nor one that says none', () => {
  const folder = mkdtempSync(join(tmpdir(), 'cf-built-identity-'))
  try {
    const write = (name, bytes) => {
      const path = join(folder, name)
      writeFileSync(path, bytes)
      return path
    }
    assert.equal(refusalOf(write('smoke.exe', exeSaying(SMOKE))), null)
    for (const [name, bytes] of [
      ['release.exe', exeSaying(RELEASE)],
      ['candidate.exe', exeSaying(`${RELEASE}.candidate`)],
      ['shipped.exe', Buffer.from('MZ\0an exe built before the app said its identity\0')],
    ]) {
      const said = refusalOf(write(name, bytes))
      assert.match(
        said ?? '',
        /is not started: a test starts only an app of the smoke identity/,
        name,
      )
    }
    assert.match(refusalOf(join(folder, 'missing.exe')) ?? '', /is not started/)
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
})

test("a smoke's build is given the smoke identity, and an app-alone build no step before it", () => {
  assert.deepEqual(smokeBuildConfig(), { identifier: SMOKE })
  assert.deepEqual(smokeBuildConfig({ appAlone: true }), {
    identifier: SMOKE,
    build: { beforeBuildCommand: null },
  })
})
