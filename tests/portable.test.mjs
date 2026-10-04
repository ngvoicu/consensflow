import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { crc32, gunzipSync } from 'node:zlib'

const SCRIPT = fileURLToPath(new URL('../app/scripts/portable.mjs', import.meta.url))
// The script's own tar. On Windows a bare `tar` may be Git's GNU tar, which
// reads `C:` as a remote host.
const TAR =
  process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
    : 'tar'

/**
 * A release folder as `tauri build` leaves it on Windows, build leftovers
 * included: the test helper is there only when someone built it.
 */
function release(dir, { missing = [] } = {}) {
  const files = {
    'ConsensFlow.exe': 'app',
    'consensflow-bridge.exe': 'bridge',
    'node.exe': 'node',
    'cli/bin/cf.exe': 'cf',
    'cli/src/core/daemon.js': 'cli',
    'app.pdb': 'debug',
    'deps/app.d': 'dep',
    'nsis/installer.nsi': 'nsis',
  }
  for (const [path, body] of Object.entries(files)) {
    if (missing.includes(path)) continue
    mkdirSync(join(dir, path, '..'), { recursive: true })
    writeFileSync(join(dir, path), body)
  }
}

describe('the portable Windows exe', () => {
  // The layout is app/src-tauri/src/portable.rs's, which reads it back.
  it('is the app, then its runtime as a gzip-compressed tar, then the length and the tag', () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-portable-'))
    try {
      release(join(dir, 'release'))
      execFileSync(process.execPath, [
        SCRIPT,
        '--release',
        join(dir, 'release'),
        '--out',
        join(dir, 'out'),
        '--version',
        '3.0.0-alpha.99',
      ])
      const exe = readFileSync(join(dir, 'out', 'ConsensFlow_3.0.0-alpha.99_x64-portable.exe'))
      assert.equal(exe.subarray(0, 3).toString(), 'app', 'the app first, byte for byte')
      assert.equal(exe.subarray(-8).toString('latin1'), 'CFPAYLD1')
      const length = Number(exe.readBigUInt64LE(exe.length - 16))
      assert.equal(3 + length + 16, exe.length)

      const payload = exe.subarray(3, 3 + length)
      const tar = gunzipSync(payload)
      // The app names its runtime folder by this CRC, the tar's, from the
      // gzip trailer.
      assert.equal(payload.readUInt32LE(payload.length - 8), crc32(tar))
      writeFileSync(join(dir, 'runtime.tar'), tar)
      const listed = execFileSync(TAR, ['-tf', join(dir, 'runtime.tar')], { encoding: 'utf8' })
        .split(/\r?\n/) // Windows' tar ends its lines with CRLF
        .filter((line) => line !== '' && !line.endsWith('/'))
        .sort()
      assert.deepEqual(listed, ['cli/bin/cf.exe', 'cli/src/core/daemon.js', 'node.exe'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('refuses a release folder missing a piece, and names the build that makes it', () => {
    for (const piece of ['node.exe', 'cli/bin/cf.exe']) refusesWithout(piece)
  })
})

/** The portable build of a release folder without `piece`: refused, naming it. */
function refusesWithout(piece) {
  const dir = mkdtempSync(join(tmpdir(), 'cf-portable-'))
  try {
    release(join(dir, 'release'), { missing: [piece] })
    const run = spawnSync(
      process.execPath,
      [SCRIPT, '--release', join(dir, 'release'), '--out', join(dir, 'out'), '--version', '1.0.0'],
      { encoding: 'utf8' },
    )
    assert.equal(run.status, 1)
    assert.match(run.stderr, /is missing .*npm --prefix app run build/)
    assert.ok(run.stderr.includes(join(...piece.split('/'))), run.stderr)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}
