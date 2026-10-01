import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const SCRIPT = fileURLToPath(new URL('../app/scripts/portable.mjs', import.meta.url))

/** A release folder as `tauri build` leaves it on Windows, build leftovers included. */
function release(dir, { missing = [] } = {}) {
  const files = {
    'ConsensFlow.exe': 'app',
    'consensflow-bridge.exe': 'bridge',
    'node.exe': 'node',
    'cli/bin/cf.cmd': 'cf',
    'cli/src/core/cli.js': 'cli',
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

describe('the portable Windows zip', () => {
  it('holds what the installer installs, minus its uninstaller, in one ConsensFlow folder', () => {
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
      const zip = join(dir, 'out', 'ConsensFlow_3.0.0-alpha.99_x64-portable.zip')
      const listed = execFileSync('tar', ['-tf', zip], { encoding: 'utf8' })
        .split('\n')
        .filter((line) => line !== '' && !line.endsWith('/'))
        .sort()
      assert.deepEqual(listed, [
        'ConsensFlow/ConsensFlow.exe',
        'ConsensFlow/cli/bin/cf.cmd',
        'ConsensFlow/cli/src/core/cli.js',
        'ConsensFlow/consensflow-bridge.exe',
        'ConsensFlow/node.exe',
      ])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('refuses a release folder missing a piece, and names the build that makes it', () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-portable-'))
    try {
      release(join(dir, 'release'), { missing: ['node.exe'] })
      const run = spawnSync(
        process.execPath,
        [
          SCRIPT,
          '--release',
          join(dir, 'release'),
          '--out',
          join(dir, 'out'),
          '--version',
          '1.0.0',
        ],
        { encoding: 'utf8' },
      )
      assert.equal(run.status, 1)
      assert.match(run.stderr, /node\.exe is missing .*npm --prefix app run build/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
