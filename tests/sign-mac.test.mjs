import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'

const SCRIPT = fileURLToPath(new URL('../app/scripts/sign-mac.mjs', import.meta.url))
const ID = 'dev.ngvoicu.consensflow'
const DMG = 'ConsensFlow_3.0.0-alpha.99_aarch64.dmg'
/** Where the release's app keeps the Mach-Os it carries, by name. */
const CODE = { app: 'MacOS/app', node: 'MacOS/node', cf: 'Resources/cli/bin/cf' }

/** A bundle folder as Tauri leaves one: the app, with the release's three Mach-Os, and a DMG of it. */
function writeBundle(dir) {
  const app = join(dir, 'macos', 'ConsensFlow.app')
  const contents = join(app, 'Contents')
  for (const folder of ['MacOS', 'Resources/cli/bin', 'Resources/cli/src'])
    mkdirSync(join(contents, folder), { recursive: true })
  writeFileSync(
    join(contents, 'Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${ID}</string><key>CFBundleExecutable</key><string>app</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n`,
  )
  for (const path of Object.values(CODE)) copyFileSync('/usr/bin/true', join(contents, path))
  writeFileSync(join(contents, 'Resources', 'cli', 'src', 'main.js'), 'export {}\n')
  const volume = join(dir, 'volume-source')
  mkdirSync(volume)
  execFileSync('ditto', [app, join(volume, 'ConsensFlow.app')])
  symlinkSync('/Applications', join(volume, 'Applications'))
  writeFileSync(join(volume, '.VolumeIcon.icns'), 'icon\n')
  mkdirSync(join(dir, 'dmg'))
  execFileSync('hdiutil', [
    ...['create', '-quiet', '-volname', 'ConsensFlow', '-srcfolder', volume],
    ...['-fs', 'HFS+', '-format', 'UDZO', '-size', '20m', join(dir, 'dmg', DMG)],
  ])
  return app
}

/** What `codesign --display` says of `path`, and the entitlements it carries. */
function signature(path) {
  const shown = spawnSync('codesign', ['-dvvv', path], { encoding: 'utf8' }).stderr
  const entitlements = execFileSync('codesign', ['-d', '--entitlements', '-', '--xml', path], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  })
  return { shown, entitlements, cdhash: shown.match(/^CDHash=(\w+)$/m)?.[1] }
}

it('signs every Mach-O of the app from the inside out, then makes the DMG again around it', {
  skip: process.platform !== 'darwin' && 'macOS only',
}, () => {
  const dir = mkdtempSync(join(tmpdir(), 'sign-mac-test-'))
  try {
    const app = writeBundle(dir)
    execFileSync(process.execPath, [SCRIPT, '--bundle', dir, '--adhoc'], { stdio: 'pipe' })

    for (const [name, path] of Object.entries(CODE)) {
      const { shown, entitlements } = signature(join(app, 'Contents', path))
      assert.match(shown, /flags=0x10002\(adhoc,runtime\)/, `${name} runs hardened`)
      const identifier = name === 'app' ? ID : `${ID}.${name}`
      assert.match(shown, new RegExp(`^Identifier=${identifier}$`, 'm'))
      // V8's JIT in node needs these; Tauri gives the app's own executable the same.
      if (name === 'cf') assert.equal(entitlements, '')
      else assert.match(entitlements, /com\.apple\.security\.cs\.allow-jit/, name)
    }
    execFileSync('codesign', ['--verify', '--deep', '--strict', app])

    const dmg = join(dir, 'dmg', DMG)
    assert.match(signature(dmg).shown, /^Format=disk image$/m)
    const volume = join(dir, 'volume')
    mkdirSync(volume)
    execFileSync('hdiutil', [
      ...['attach', dmg, '-readonly', '-noverify', '-noautoopen', '-nobrowse'],
      ...['-mountpoint', volume],
    ])
    try {
      assert.deepEqual(readdirSync(volume).sort(), [
        '.VolumeIcon.icns',
        'Applications',
        'ConsensFlow.app',
      ])
      assert.equal(signature(join(volume, 'ConsensFlow.app')).cdhash, signature(app).cdhash)
    } finally {
      execFileSync('hdiutil', ['detach', volume, '-force'], { stdio: 'ignore' })
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

it('refuses to sign the release without its identity, naming what is missing', () => {
  const env = { PATH: process.env.PATH, APPLE_API_KEY_ID: 'KEY' }
  const result = spawnSync(process.execPath, [SCRIPT, '--bundle', tmpdir()], {
    encoding: 'utf8',
    env,
  })
  assert.equal(result.status, 1)
  assert.match(
    result.stderr,
    /APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_API_KEY, APPLE_API_ISSUER not set/,
  )
})
