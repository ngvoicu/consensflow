import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  rmSync,
  statSync,
} from 'node:fs'
import { basename, dirname, join, relative } from 'node:path'

/**
 * A built app as the updater smoke looks at it: what the installed app's own
 * check accepts of a bundle (`validate_bundle`, app/src-tauri/src/update_install.rs),
 * its code signature, its bytes, and the archive an update is served as. The
 * flip release's bundle still ships Node, `cf.mjs`, `src` and `hosts`, and the
 * release after it does not: this takes either, and says which it is.
 */

export const IDENTITY = 'dev.ngvoicu.consensflow'

/** What a bundle of the releases before the deletion keeps of Node's, relative to it: the sidecar, and the CLI's sources. */
const NODE_FILES = [
  ['Contents', 'MacOS', 'node'],
  ['Contents', 'Resources', 'cli', 'bin', 'cf.mjs'],
  ['Contents', 'Resources', 'cli', 'src'],
  ['Contents', 'Resources', 'cli', 'hosts'],
]

export function run(program, args, options = {}) {
  try {
    return execFileSync(program, args, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      ...options,
    })
  } catch (cause) {
    const said = cause?.stderr?.toString?.().trim() || cause.message
    throw new Error(`${program} ${args.join(' ')} failed: ${said}`)
  }
}

/** A field of the bundle's `Info.plist`. */
export function plistValue(app, field) {
  return run('/usr/bin/plutil', [
    '-extract',
    field,
    'raw',
    '-o',
    '-',
    join(app, 'Contents', 'Info.plist'),
  ]).trim()
}

/**
 * The app at `app` as the installed app's check takes it: this app's identity,
 * one version in both of the plist's fields, the app's executable, and `cf`, the
 * command a window runs. Whatever Node's files it holds must be all of them: a
 * bundle with a runtime and no `cf.mjs` runs neither implementation of the
 * way back. `cli/package.json` is read where it is, and need not be.
 */
export function inspectBundle(app, label) {
  const plist = join(app, 'Contents', 'Info.plist')
  assert.ok(existsSync(plist), `${label} has no Contents/Info.plist: ${app}`)
  const identifier = plistValue(app, 'CFBundleIdentifier')
  assert.equal(identifier, IDENTITY, `${label} is ${identifier}, not ${IDENTITY}`)
  const version = plistValue(app, 'CFBundleShortVersionString')
  assert.equal(
    plistValue(app, 'CFBundleVersion'),
    version,
    `${label}: the plist's two versions differ`,
  )
  const executable = plistValue(app, 'CFBundleExecutable')
  const binary = join(app, 'Contents', 'MacOS', executable)
  const cf = join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf')
  for (const [what, path] of [
    ['native executable', binary],
    ["window's cf", cf],
  ]) {
    assert.ok(existsSync(path), `${label} has no ${what}: ${path}`)
  }
  const has = NODE_FILES.map((parts) => existsSync(join(app, ...parts)))
  assert.ok(
    has.every(Boolean) || !has.some(Boolean),
    `${label} holds some of Node's files and not all: ${NODE_FILES.map(
      (parts, at) => `${parts.at(-1)} ${has[at] ? 'there' : 'missing'}`,
    ).join(', ')}`,
  )
  const manifest = join(app, 'Contents', 'Resources', 'cli', 'package.json')
  return {
    app,
    label,
    binary,
    cf,
    version,
    node: has[0],
    cliVersion: existsSync(manifest) ? JSON.parse(readFileSync(manifest, 'utf8')).version : null,
  }
}

/** The bundle's code signature verifies, as the installed app verifies an update's. */
export function verifySeal(app) {
  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app])
}

/** The bundle is signed ad hoc: no identity of anyone's, which no certificate of ours could be. */
export function assertAdHoc(app, label) {
  const shown = spawnSync('/usr/bin/codesign', ['-dv', '--verbose=2', app], { encoding: 'utf8' })
  assert.equal(shown.status, 0, `${label}: codesign could not read the signature: ${shown.stderr}`)
  assert.match(shown.stderr, /Signature=adhoc/, `${label} is not signed ad hoc: ${shown.stderr}`)
}

/** Every file of a tree by path, with its digest and mode, and every link by what it names. */
export function digestManifest(root) {
  const entries = []
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const path = join(directory, entry.name)
      const name = relative(root, path)
      if (entry.isDirectory()) {
        visit(path)
      } else if (entry.isSymbolicLink()) {
        entries.push({ name, link: readlinkSync(path) })
      } else if (entry.isFile()) {
        entries.push({
          name,
          sha256: createHash('sha256').update(readFileSync(path)).digest('hex'),
          mode: statSync(path).mode & 0o777,
        })
      } else {
        throw new Error(`unsupported bundle entry in digest manifest: ${path}`)
      }
    }
  }
  visit(root)
  return entries
}

/**
 * The app at `from` copied onto `to`, with its modes, links and attributes (what a
 * disk image's copy keeps), over whatever is at `to`: files of the old app that the
 * new one has not stay where they were.
 */
export function copyOver(from, to) {
  mkdirSync(dirname(to), { recursive: true })
  run('/usr/bin/ditto', [from, to])
  return to
}

/** A copy of the app at `to`, which was not there before: what is at `to` is taken away first. */
export function copyBundle(from, to) {
  rmSync(to, { recursive: true, force: true })
  return copyOver(from, to)
}

/** The update archive of the app at `app`: a gzipped tar of `ConsensFlow.app`, as a release makes it. */
export function archiveOf(app, archive) {
  assert.equal(basename(app), 'ConsensFlow.app', 'an update archive holds ConsensFlow.app')
  mkdirSync(dirname(archive), { recursive: true })
  run('/usr/bin/tar', ['-czf', archive, '-C', dirname(app), basename(app)], {
    env: { ...process.env, COPYFILE_DISABLE: '1' },
  })
  return archive
}

/**
 * Copies of the update that the installed app's check refuses, each in a folder
 * of its own where it is `ConsensFlow.app`, and what it refuses them for:
 *
 * - `without-cf`: no `cli/bin/cf`, the bundle signed again over what is left, so
 *   its seal verifies and only the rule about `cf` can refuse it;
 * - `tampered`: a byte more in `cf` after the signing, so its seal does not.
 */
export const REFUSED = {
  'without-cf': {
    words: /must include cf/,
    make(app) {
      rmSync(join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf'))
      run('/usr/bin/codesign', ['--force', '--deep', '--sign', '-', app])
    },
  },
  tampered: {
    words: /code-signature/,
    make(app) {
      appendFileSync(
        join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf'),
        'changed after signing',
      )
    },
  },
}

export function refusedBundle(kind, from, folder) {
  const app = copyBundle(from, join(folder, kind, 'ConsensFlow.app'))
  REFUSED[kind].make(app)
  return app
}
