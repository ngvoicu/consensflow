#!/usr/bin/env node
/**
 * Puts everything the app needs to run on its own into the bundle.
 *
 * The app is the whole installation: someone who downloads it should not
 * then have to install Node, npm, or the CLI. So the bundle carries an
 * official Node build as a Tauri sidecar and the CLI's own sources as
 * resources, and the app runs the same code the terminal would.
 *
 * The system's Node is deliberately not copied: package-manager builds link
 * against libraries that only exist on the machine that installed them, so a
 * copied binary dies on any other Mac. The official tarball is
 * self-contained.
 */
import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const REPO = dirname(APP)
const CACHE = join(APP, '.cache')
const BINARIES = join(APP, 'src-tauri', 'binaries')
const RESOURCES = join(APP, 'src-tauri', 'resources', 'cli')

/** The Node the app ships. Pinned so a build is reproducible. */
const NODE_VERSION = process.env.CONSENSFLOW_NODE_VERSION ?? 'v26.7.0'

const TRIPLES = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
}

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

function fetchNode() {
  const key = platformKey().replace('-', '-')
  const name = `node-${NODE_VERSION}-${key}`
  const archive = join(CACHE, `${name}.tar.gz`)
  const extracted = join(CACHE, name)

  mkdirSync(CACHE, { recursive: true })
  if (!existsSync(archive)) {
    const url = `https://nodejs.org/dist/${NODE_VERSION}/${name}.tar.gz`
    process.stdout.write(`fetching ${url}\n`)
    execFileSync('curl', ['-fsSL', '-o', archive, url], { stdio: ['ignore', 'inherit', 'inherit'] })
  }
  if (!existsSync(extracted)) {
    execFileSync('tar', ['-xzf', archive, '-C', CACHE], { stdio: 'inherit' })
  }
  return join(extracted, 'bin', 'node')
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
const sidecar = join(BINARIES, `node-${triple}`)
cpSync(node, sidecar)
execFileSync('chmod', ['+x', sidecar])
const version = copyCli()

process.stdout.write(`sidecar: ${sidecar}\n`)
process.stdout.write(`cli ${version} → ${RESOURCES}\n`)
