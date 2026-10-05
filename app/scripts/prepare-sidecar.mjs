#!/usr/bin/env node
/**
 * Puts everything the app needs to run on its own into the bundle.
 *
 * The app is the whole installation: someone who downloads it should not
 * then have to install Node, npm, or the CLI. So the bundle carries an
 * official Node build as a Tauri sidecar and the CLI's own sources as
 * resources, the native `cf` a window runs among them, and the app runs the
 * same code the terminal would.
 *
 * The system's Node is deliberately not copied: package-manager builds link
 * against libraries that only exist on the machine that installed them, so a
 * copied binary dies on any other Mac. The official tarball is
 * self-contained.
 */
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { buildCf } from './build-cf.mjs'
import { prepareConpty } from './conpty.mjs'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const REPO = dirname(APP)
const CACHE = join(APP, '.cache')
const BINARIES = join(APP, 'src-tauri', 'binaries')
const RESOURCES = join(APP, 'src-tauri', 'resources', 'cli')

/**
 * The Node the app ships, pinned so a build is reproducible, and each
 * archive's SHA-256 as nodejs.org publishes it (its SHASUMS256.txt): the
 * app runs this binary with the human's full permissions, so one that is
 * not the published build is never bundled.
 */
const NODE_VERSION = 'v26.7.0'
const NODE_SHA256 = {
  'darwin-arm64': '7ee659a7768e641bbfd5360940660b8e8fd0052f77488f365562bac522fc15d4',
  'darwin-x64': 'f279d1ed28ce57f7788bf23435d2ad7fdd7438904ad5c4d8a1081a7cde3d4b96',
  'linux-arm64': '925aa6157dd37542d0d7f2e28b7bf61e7b39284411210b0498bc3788db4aef68',
  'linux-x64': 'bd6b6c31e377bad9ad579bed72e5bc11f4c879ac9452ad51d30e646ea3d828df',
  'win-x64': 'd3bd72755141ed32bbcd841228ee81897c8a98d50dfa7dae2179399a0a7c90f8',
}

const TRIPLES = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
  'win32-x64': 'x86_64-pc-windows-msvc',
}
const WINDOWS = process.platform === 'win32'

function platformKey() {
  return `${process.platform}-${process.arch}`
}

function targetTriple() {
  const triple = TRIPLES[platformKey()]
  if (triple === undefined) {
    throw new Error(`no sidecar mapping for ${platformKey()} yet`)
  }
  return triple
}

/**
 * The official Node build for this platform, from nodejs.org's archive: a
 * tarball with `bin/node` inside, or on Windows a zip with `node.exe` at its
 * root. Both `curl` and `tar` ship with Windows 10 and later, and its `tar`
 * reads zips.
 */
function fetchNode() {
  const key = WINDOWS ? 'win-x64' : platformKey()
  const name = `node-${NODE_VERSION}-${key}`
  const archive = join(CACHE, WINDOWS ? `${name}.zip` : `${name}.tar.gz`)
  const extracted = join(CACHE, name)

  mkdirSync(CACHE, { recursive: true })
  if (!existsSync(archive)) {
    const url = `https://nodejs.org/dist/${NODE_VERSION}/${name}.${WINDOWS ? 'zip' : 'tar.gz'}`
    process.stdout.write(`fetching ${url}\n`)
    execFileSync('curl', ['-fsSL', '-o', archive, url], { stdio: ['ignore', 'inherit', 'inherit'] })
  }
  verify(archive, NODE_SHA256[key])
  if (!existsSync(extracted)) {
    // Windows' own tar reads a zip; a GNU tar first on PATH (Git Bash's, in CI)
    // reads `D:\...` as a remote host and fails, so Windows names its own.
    const tar = WINDOWS ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar'
    execFileSync(tar, ['-xf', archive, '-C', CACHE], { stdio: 'inherit' })
  }
  return WINDOWS ? join(extracted, 'node.exe') : join(extracted, 'bin', 'node')
}

/** Refuses an archive that is not the one nodejs.org published, deleting it so the next run fetches it again. */
function verify(archive, expected) {
  if (expected === undefined) throw new Error(`no published SHA-256 pinned for ${archive}`)
  const actual = createHash('sha256').update(readFileSync(archive)).digest('hex')
  if (actual === expected) return
  rmSync(archive, { force: true })
  throw new Error(
    `${archive} is not the Node nodejs.org published (SHA-256 ${actual}, not ${expected}): deleted it; run again`,
  )
}

function copyCli() {
  rmSync(RESOURCES, { recursive: true, force: true })
  mkdirSync(RESOURCES, { recursive: true })
  for (const part of ['bin', 'src', 'hosts', 'skill']) {
    const from = join(REPO, part)
    if (existsSync(from)) cpSync(from, join(RESOURCES, part), { recursive: true })
  }
  // package.json travels too: the CLI reads its own version from it.
  const manifest = JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8'))
  writeFileSync(
    join(RESOURCES, 'package.json'),
    `${JSON.stringify({ name: manifest.name, version: manifest.version, type: 'module' }, null, 2)}\n`,
  )
  return manifest.version
}

const triple = targetTriple()
const node = fetchNode()
mkdirSync(BINARIES, { recursive: true })
// Tauri names a sidecar by its target triple, and on Windows expects `.exe`.
const sidecar = join(BINARIES, `node-${triple}${WINDOWS ? '.exe' : ''}`)
cpSync(node, sidecar)
if (!WINDOWS) execFileSync('chmod', ['+x', sidecar])
// A window's `cf`, native, in bin/ before bin/ is copied.
buildCf()
const version = copyCli()
// Windows: Microsoft's own console host, which the Windows bundle puts beside
// the app (tauri.windows.conf.json) and the portable exe in its runtime.
if (WINDOWS) {
  for (const file of prepareConpty(join(APP, 'src-tauri', 'resources', 'conpty'))) {
    process.stdout.write(`conpty: ${file}\n`)
  }
}

process.stdout.write(`sidecar: ${sidecar}\n`)
process.stdout.write(`cli ${version} → ${RESOURCES}\n`)
