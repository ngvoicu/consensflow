/**
 * Runs clippy for Windows from a machine that is not one, as the gate runs it
 * here (`--all-targets -- -D warnings`, tests included): what only Windows
 * compiles (the `cfg(windows)` code, and the unix code a Windows build must
 * leave out) is found in seconds, though nothing is run. The crates to lint are
 * named, else all of the workspace but the app (whose build wants the Windows
 * runtime it ships with, which only a Windows build has). The app is linted
 * when it is named, with its sidecar and resources left out of its Tauri
 * configuration and a resource compiler that compiles nothing, as a check
 * needs neither:
 *
 *   npm run clippy:windows
 *   npm run clippy:windows -- cf-daemon cf-process
 *   npm run clippy:windows -- app
 *
 * It needs the `x86_64-pc-windows-msvc` target (`rustup target add`) and none
 * of Windows' toolchain: the C that SQLite is built from is not compiled (its
 * compiler is `true`) and its archive is made empty by this very file, which
 * cc-rs runs as its archiver.
 */
import { spawnSync } from 'node:child_process'
import { closeSync, mkdtempSync, openSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const TARGET = 'x86_64-pc-windows-msvc'
const AS_ARCHIVER = '--as-archiver'
const args = process.argv.slice(2)

if (args[0] === AS_ARCHIVER) {
  // `ar cq <archive> <objects>`, or `lib -out:<archive> <objects>`.
  const asked = args.slice(1)
  const named = asked.find((arg) => /^[-/]out:/i.test(arg))
  const archive = named === undefined ? asked[1] : named.slice('-out:'.length)
  if (archive !== undefined) closeSync(openSync(archive, 'a'))
} else {
  const archiver = [process.execPath, fileURLToPath(import.meta.url), AS_ARCHIVER].join(' ')
  const crates =
    args.length === 0 ? ['--workspace', '--exclude', 'app'] : args.flatMap((name) => ['-p', name])
  const env = {
    ...process.env,
    [`CC_${TARGET.replaceAll('-', '_')}`]: 'true',
    [`AR_${TARGET.replaceAll('-', '_')}`]: archiver,
  }
  // The app's build script wants the sidecar and the resources a Windows build
  // ships (left out of its configuration here), and a resource compiler for
  // its icon and manifest (one that does nothing, found first on PATH).
  let stand
  if (args.includes('app')) {
    stand = mkdtempSync(join(tmpdir(), 'cf-windows-clippy-'))
    writeFileSync(join(stand, 'llvm-rc'), '#!/bin/sh\nexit 0\n', { mode: 0o755 })
    env.PATH = `${stand}${delimiter}${process.env.PATH}`
    env.TAURI_CONFIG = JSON.stringify({ bundle: { externalBin: null, resources: null } })
  }
  const ran = spawnSync(
    'cargo',
    ['clippy', '--offline', '--target', TARGET, '--all-targets', ...crates, '--', '-D', 'warnings'],
    { stdio: 'inherit', env },
  )
  if (stand !== undefined) rmSync(stand, { recursive: true, force: true })
  process.exit(ran.status ?? 1)
}
