#!/usr/bin/env node
/**
 * The portable Windows build: what the NSIS installer puts in
 * %LOCALAPPDATA%\ConsensFlow, minus its uninstaller, zipped under one
 * ConsensFlow folder, to unpack anywhere and run without installing. Its
 * data lives where the installed app's does (%USERPROFILE%\.consensflow), and
 * the app installs no update in place on Windows, so a portable copy is never
 * swapped for an installed one.
 *
 * Run after `npm --prefix app run build` on Windows:
 *   node app/scripts/portable.mjs [--release <dir>] [--out <dir>] [--version <x>]
 */
import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const RELEASE = join(APP, 'src-tauri', 'target', 'release')

/** The installed layout (2026-09-30, %LOCALAPPDATA%\ConsensFlow), without uninstall.exe. */
const LAYOUT = ['ConsensFlow.exe', 'node.exe', 'cli']

const { values } = parseArgs({
  options: {
    release: { type: 'string', default: RELEASE },
    out: { type: 'string', default: join(RELEASE, 'bundle', 'portable') },
    version: { type: 'string' },
  },
})
const version =
  values.version ??
  JSON.parse(readFileSync(join(APP, 'src-tauri', 'tauri.conf.json'), 'utf8')).version

for (const name of LAYOUT) {
  if (!existsSync(join(values.release, name))) {
    console.error(
      `portable: ${name} is missing from ${values.release}; build first with npm --prefix app run build`,
    )
    process.exit(1)
  }
}

const staging = mkdtempSync(join(tmpdir(), 'cf-portable-'))
try {
  const folder = join(staging, 'ConsensFlow')
  for (const name of LAYOUT) cpSync(join(values.release, name), join(folder, name), { recursive: true })
  mkdirSync(values.out, { recursive: true })
  const zip = join(values.out, `ConsensFlow_${version}_x64-portable.zip`)
  rmSync(zip, { force: true })
  // Windows' own tar.exe writes a zip for a .zip name (-a), as the Mac's does.
  const tar =
    process.platform === 'win32'
      ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
      : 'tar'
  execFileSync(tar, ['-a', '-c', '-f', zip, '-C', staging, 'ConsensFlow'])
  console.log(`portable: ${zip}`)
} finally {
  rmSync(staging, { recursive: true, force: true })
}
