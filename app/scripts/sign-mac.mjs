#!/usr/bin/env node
/**
 * The Mac release under the Developer ID, so a download opens with no
 * Gatekeeper warning, online or not: every Mach-O in the app signed from the
 * inside out with hardened runtime and a secure timestamp, the app notarized
 * and its ticket stapled, then the DMG Tauri built made again around it,
 * signed, notarized and stapled too.
 *
 * Run after `npm --prefix app run build` on a Mac:
 *   node app/scripts/sign-mac.mjs [--bundle <target/release/bundle>] [--adhoc]
 *
 * The identity comes from the environment, as the release keeps it:
 * APPLE_CERTIFICATE (the .p12, base64) and APPLE_CERTIFICATE_PASSWORD, and the
 * notary's key APPLE_API_KEY (the .p8), APPLE_API_KEY_ID and APPLE_API_ISSUER.
 * Only Apple's tools read it, from a keychain made for this run and deleted
 * after it. `--adhoc` signs the same files with no identity and leaves the
 * notary out: what the tests run.
 */
import { spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import {
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const ENTITLEMENTS = join(APP, 'src-tauri', 'entitlements.plist')
const SECRETS = [
  'APPLE_CERTIFICATE',
  'APPLE_CERTIFICATE_PASSWORD',
  'APPLE_API_KEY',
  'APPLE_API_KEY_ID',
  'APPLE_API_ISSUER',
]
/** How long one notarization may take before the release gives up on it. */
const NOTARY_WAIT = '20m'
const THIN = new Set([0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe])
const UNIVERSAL = new Set([0xcafebabe, 0xcafebabf])

/** Runs a tool and answers its output; a failure names the tool, never its arguments: one may be a password. */
function run(tool, args) {
  const result = spawnSync(tool, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
  if (result.status !== 0) {
    const said = result.stderr?.trim() || result.stdout?.trim() || result.error?.message
    throw new Error(`${tool} ${args[0]} failed: ${said}`)
  }
  return result.stdout
}

function plist(app, key) {
  return run('plutil', [
    '-extract',
    key,
    'raw',
    '-o',
    '-',
    join(app, 'Contents', 'Info.plist'),
  ]).trim()
}

/**
 * Whether `path` is Mach-O code, thin or universal. A Java class file shares
 * the universal magic, with its version where a binary counts its slices.
 */
function isMachO(path) {
  const head = Buffer.alloc(8)
  const file = openSync(path, 'r')
  try {
    if (readSync(file, head, 0, 8, 0) < 8) return false
  } finally {
    closeSync(file)
  }
  const magic = head.readUInt32BE(0)
  return THIN.has(magic) || (UNIVERSAL.has(magic) && head.readUInt32BE(4) < 45)
}

/** Every Mach-O file under `dir`. */
function machOs(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return machOs(path)
    return entry.isFile() && isMachO(path) ? [path] : []
  })
}

function codesign(path, { identity, keychain }, options) {
  run('codesign', [
    '--force',
    identity === '-' ? '--timestamp=none' : '--timestamp',
    '--sign',
    identity,
    ...(keychain ? ['--keychain', keychain] : []),
    ...options,
    path,
  ])
}

/**
 * Signs `app` from the inside out: the Mach-Os it carries, then the bundle,
 * whose seal covers them. The executables in Contents/MacOS, the app's own
 * and its node, carry the app's entitlements, as Tauri signs them; the rest
 * carry none.
 */
function signApp(app, signing) {
  const id = plist(app, 'CFBundleIdentifier')
  const executables = join(app, 'Contents', 'MacOS')
  const main = join(executables, plist(app, 'CFBundleExecutable'))
  for (const path of machOs(app).filter((path) => path !== main)) {
    const entitled = dirname(path) === executables ? ['--entitlements', ENTITLEMENTS] : []
    codesign(path, signing, [
      '--options',
      'runtime',
      '--identifier',
      `${id}.${basename(path)}`,
      ...entitled,
    ])
  }
  codesign(app, signing, ['--options', 'runtime', '--entitlements', ENTITLEMENTS])
}

/** Has Apple's notary check `target`, waits for its answer, and staples the ticket to it. */
function notarize(target, notary, scratch) {
  // The notary takes an app as a zip; the ticket goes on the app itself.
  const upload = target.endsWith('.app') ? join(scratch, `${basename(target)}.zip`) : target
  if (upload !== target) run('ditto', ['-c', '-k', '--keepParent', target, upload])
  console.log(`sign-mac: the notary checks ${basename(target)}`)
  const submitted = spawnSync(
    'xcrun',
    [
      'notarytool',
      'submit',
      upload,
      ...notary,
      '--wait',
      '--timeout',
      NOTARY_WAIT,
      '--output-format',
      'json',
    ],
    { encoding: 'utf8' },
  )
  let answer
  try {
    answer = JSON.parse(submitted.stdout)
  } catch {
    throw new Error(`notarytool submit failed: ${submitted.stderr.trim()}`)
  }
  if (answer.status !== 'Accepted') {
    // The notary's log is the one place that says which file it refused, and why.
    if (answer.id) {
      const log = spawnSync('xcrun', ['notarytool', 'log', answer.id, ...notary], {
        encoding: 'utf8',
      })
      console.error(log.stdout || log.stderr)
    }
    throw new Error(`the notary answered ${answer.status} for ${basename(target)}`)
  }
  run('xcrun', ['stapler', 'staple', target])
  run('xcrun', ['stapler', 'validate', target])
}

/** Detaches `volume`; Spotlight can hold a fresh volume busy for a moment. */
function detach(volume) {
  for (const options of [[], [], [], ['-force']]) {
    if (spawnSync('hdiutil', ['detach', volume, ...options], { stdio: 'ignore' }).status === 0)
      return
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2000)
  }
  throw new Error(`hdiutil detach failed: ${volume} stays mounted`)
}

/**
 * Makes the DMG Tauri built again around the signed `app`: the same volume,
 * its name, icon and Applications link, with the app in it replaced.
 */
function rebuildDmg(dmg, app, scratch) {
  const writable = join(scratch, 'writable.dmg')
  const volume = join(scratch, 'volume')
  mkdirSync(volume)
  run('hdiutil', ['convert', dmg, '-format', 'UDRW', '-ov', '-o', writable])
  run('hdiutil', [
    'attach',
    writable,
    '-readwrite',
    '-noverify',
    '-noautoopen',
    '-nobrowse',
    '-mountpoint',
    volume,
  ])
  try {
    const inside = join(volume, basename(app))
    rmSync(inside, { recursive: true, force: true })
    run('ditto', [app, inside])
    // As Tauri leaves its volume: nothing in it writable by others, no event log.
    run('chmod', ['-R', 'go-w', inside])
    rmSync(join(volume, '.fseventsd'), { recursive: true, force: true })
  } finally {
    detach(volume)
  }
  run('hdiutil', [
    'convert',
    writable,
    '-format',
    'UDZO',
    '-imagekey',
    'zlib-level=9',
    '-ov',
    '-o',
    dmg,
  ])
}

/** What a download meets: every seal holds, and Gatekeeper takes both files as notarized. */
function verify(app, dmg, notarized) {
  run('codesign', ['--verify', '--deep', '--strict', app])
  run('codesign', ['--verify', '--strict', dmg])
  if (!notarized) return
  for (const [path, type] of [
    [app, ['--type', 'execute']],
    [dmg, ['--type', 'open', '--context', 'context:primary-signature']],
  ]) {
    const assessed = spawnSync('spctl', ['--assess', '--verbose=4', ...type, path], {
      encoding: 'utf8',
    })
    if (assessed.status !== 0 || !assessed.stderr.includes('source=Notarized Developer ID'))
      throw new Error(`Gatekeeper refuses ${basename(path)}: ${assessed.stderr.trim()}`)
  }
}

function release({ app, dmg, signing, notary, scratch }) {
  console.log(`sign-mac: signing ${basename(app)}`)
  signApp(app, signing)
  if (notary) notarize(app, notary, scratch)
  console.log(`sign-mac: making ${basename(dmg)} again around it`)
  rebuildDmg(dmg, app, scratch)
  codesign(dmg, signing, [])
  if (notary) notarize(dmg, notary, scratch)
  verify(app, dmg, Boolean(notary))
  console.log(`sign-mac: ${basename(app)} and ${basename(dmg)} signed`)
}

/** The SHA-1 of the one Developer ID Application identity in `keychain`. */
function developerId(keychain) {
  const listed = run('security', ['find-identity', '-v', '-p', 'codesigning', keychain])
  const found = [...listed.matchAll(/^\s*\d+\) ([0-9A-F]{40}) "Developer ID Application: /gm)]
  if (found.length !== 1)
    throw new Error(`the certificate holds ${found.length} Developer ID Application identities`)
  return found[0][1]
}

/**
 * Runs `work` with the release's identity in a keychain of its own, made for
 * this run and deleted after it, with the keychain search list as it was.
 */
function withIdentity(env, scratch, work) {
  const keychain = join(scratch, 'signing.keychain-db')
  const password = randomBytes(24).toString('hex')
  const searched = run('security', ['list-keychains', '-d', 'user'])
    .split('\n')
    .map((line) => line.trim().replace(/^"(.*)"$/, '$1'))
    .filter(Boolean)
  run('security', ['create-keychain', '-p', password, keychain])
  try {
    run('security', ['set-keychain-settings', '-lut', '21600', keychain])
    run('security', ['unlock-keychain', '-p', password, keychain])
    const certificate = join(scratch, 'certificate.p12')
    writeFileSync(certificate, Buffer.from(env.APPLE_CERTIFICATE, 'base64'), { mode: 0o600 })
    run('security', [
      'import',
      certificate,
      '-k',
      keychain,
      '-f',
      'pkcs12',
      '-P',
      env.APPLE_CERTIFICATE_PASSWORD,
      '-T',
      '/usr/bin/codesign',
    ])
    // Without it codesign asks for the keychain's password, in a dialog no one sees.
    run('security', [
      'set-key-partition-list',
      '-S',
      'apple-tool:,apple:,codesign:',
      '-s',
      '-k',
      password,
      keychain,
    ])
    // codesign builds the certificate chain from the keychains it searches.
    run('security', ['list-keychains', '-d', 'user', '-s', keychain, ...searched])
    return work({ identity: developerId(keychain), keychain })
  } finally {
    spawnSync('security', ['list-keychains', '-d', 'user', '-s', ...searched])
    spawnSync('security', ['delete-keychain', keychain])
  }
}

/** The one entry of `dir` named with `extension`. */
function only(dir, extension) {
  const found = readdirSync(dir).filter((name) => name.endsWith(extension))
  if (found.length !== 1)
    throw new Error(`${dir} holds ${found.length} ${extension} files, not one`)
  return join(dir, found[0])
}

const { values } = parseArgs({
  options: {
    bundle: { type: 'string', default: join(APP, 'src-tauri', 'target', 'release', 'bundle') },
    adhoc: { type: 'boolean', default: false },
  },
})
const missing = values.adhoc ? [] : SECRETS.filter((name) => !process.env[name])
if (missing.length > 0) {
  console.error(`sign-mac: ${missing.join(', ')} not set; --adhoc signs with no identity`)
  process.exit(1)
}
// The certificate and the notary's key are written here, and go with it.
const scratch = mkdtempSync(join(tmpdir(), 'sign-mac-'))
try {
  const app = only(join(values.bundle, 'macos'), '.app')
  const dmg = only(join(values.bundle, 'dmg'), '.dmg')
  if (values.adhoc) {
    release({ app, dmg, signing: { identity: '-' }, scratch })
  } else {
    const key = join(scratch, 'notary.p8')
    writeFileSync(key, process.env.APPLE_API_KEY, { mode: 0o600 })
    const notary = [
      '--key',
      key,
      '--key-id',
      process.env.APPLE_API_KEY_ID,
      '--issuer',
      process.env.APPLE_API_ISSUER,
    ]
    withIdentity(process.env, scratch, (signing) => release({ app, dmg, signing, notary, scratch }))
  }
} catch (error) {
  console.error(`sign-mac: ${error.message}`)
  process.exitCode = 1
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
