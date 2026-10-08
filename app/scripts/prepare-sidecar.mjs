#!/usr/bin/env node
/**
 * Puts everything the app needs to run on its own into the bundle.
 *
 * The app is the whole installation: someone who downloads it should not
 * then have to install Node, npm, or the CLI. The bundle carries the native
 * `cf` (crates/cf) as a resource, `cli/bin/cf`: it is the daemon the app
 * starts (`cf ui`) and the command every window runs, and nothing else of the
 * CLI travels, no Node and no sources. On Windows it carries Microsoft's own
 * console host beside it.
 */
import { copyFileSync, mkdirSync, rmSync } from 'node:fs'
import { basename, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { buildCf } from './build-cf.mjs'
import { prepareConpty } from './conpty.mjs'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const RESOURCES = join(APP, 'src-tauri', 'resources', 'cli')
const WINDOWS = process.platform === 'win32'

/** Builds `cf` and stages it as the bundle's `cli/bin/cf`: the path it is staged at. */
function stageCf() {
  const built = buildCf()
  rmSync(RESOURCES, { recursive: true, force: true })
  const bin = join(RESOURCES, 'bin')
  mkdirSync(bin, { recursive: true })
  const staged = join(bin, basename(built))
  copyFileSync(built, staged)
  return staged
}

const staged = stageCf()
// Windows: Microsoft's own console host, which the Windows bundle puts beside
// the app (tauri.windows.conf.json) and the portable exe in its runtime.
if (WINDOWS) {
  for (const file of prepareConpty(join(APP, 'src-tauri', 'resources', 'conpty'))) {
    process.stdout.write(`conpty: ${file}\n`)
  }
}

process.stdout.write(`cf → ${staged}\n`)
