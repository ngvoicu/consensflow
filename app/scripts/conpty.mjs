#!/usr/bin/env node
/**
 * Microsoft's own console host for the app's terminals on Windows:
 * `conpty.dll` and `OpenConsole.exe`, built from the Windows Terminal
 * repository (microsoft/terminal, MIT) and published by Microsoft on NuGet as
 * Microsoft.Windows.Console.ConPTY. Beside the program that opens the
 * terminals, portable-pty loads them instead of the system's console host,
 * so every Windows the app runs on (Windows 10's builds too) has the same,
 * current one, as VS Code and WezTerm ship theirs.
 *
 * Pinned, and the package checked against its SHA-256 before anything in it
 * is used; cached in app/.cache. Run on Windows:
 *   node app/scripts/conpty.mjs --into <dir>
 */
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const CACHE = join(APP, '.cache')

/** The package, and its SHA-256 as nuget.org serves it (`.nupkg`, signed by Microsoft). */
export const CONPTY_VERSION = '1.25.260930003'
const CONPTY_SHA256 = '02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee'

/** What the app takes of it, for x64 Windows: where the package keeps each, and its name beside the app. */
export const CONPTY_FILES = [
  ['runtimes/win-x64/native/conpty.dll', 'conpty.dll'],
  ['build/native/runtimes/x64/OpenConsole.exe', 'OpenConsole.exe'],
]

/** Puts the console host's two files into `into`, fetched and checked first when they are not cached. */
export function prepareConpty(into) {
  const name = `microsoft.windows.console.conpty.${CONPTY_VERSION}`
  const archive = join(CACHE, `${name}.nupkg`)
  const extracted = join(CACHE, name)
  mkdirSync(CACHE, { recursive: true })
  if (!existsSync(archive)) {
    const url = `https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/${CONPTY_VERSION}/${name}.nupkg`
    process.stdout.write(`fetching ${url}\n`)
    execFileSync('curl', ['-fsSL', '-o', archive, url], { stdio: ['ignore', 'inherit', 'inherit'] })
  }
  const actual = createHash('sha256').update(readFileSync(archive)).digest('hex')
  if (actual !== CONPTY_SHA256) {
    rmSync(archive, { force: true })
    throw new Error(
      `${archive} is not the package Microsoft published (SHA-256 ${actual}, not ${CONPTY_SHA256}): deleted it; run again`,
    )
  }
  if (!CONPTY_FILES.every(([inside]) => existsSync(join(extracted, ...inside.split('/'))))) {
    mkdirSync(extracted, { recursive: true })
    // A .nupkg is a zip, which Windows' own tar reads; a GNU tar first on
    // PATH (Git Bash's) reads `D:\...` as a remote host, so Windows names its own.
    const tar =
      process.platform === 'win32'
        ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
        : 'tar'
    execFileSync(tar, ['-xf', archive, '-C', extracted, ...CONPTY_FILES.map(([inside]) => inside)], {
      stdio: 'inherit',
    })
  }
  mkdirSync(into, { recursive: true })
  for (const [inside, named] of CONPTY_FILES) {
    copyFileSync(join(extracted, ...inside.split('/')), join(into, named))
  }
  return CONPTY_FILES.map(([, named]) => join(into, named))
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { values } = parseArgs({ options: { into: { type: 'string' } } })
  if (!values.into) {
    console.error('usage: node app/scripts/conpty.mjs --into <dir>')
    process.exit(2)
  }
  for (const file of prepareConpty(values.into)) process.stdout.write(`conpty: ${file}\n`)
}
