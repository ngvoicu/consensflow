/**
 * Runs clippy for Windows from a machine that is not one, as the gate runs it
 * here (`--all-targets -- -D warnings`, tests included): what only Windows
 * compiles (the `cfg(windows)` code, and the unix code a Windows build must
 * leave out) is found in seconds, though nothing is run. The crates to lint are
 * named, else all of the workspace but the app (whose build wants the Windows
 * runtime it ships with, which only a Windows build has):
 *
 *   npm run clippy:windows
 *   npm run clippy:windows -- cf-daemon cf-process
 *
 * It needs the `x86_64-pc-windows-msvc` target (`rustup target add`) and none
 * of Windows' toolchain: the C that SQLite is built from is not compiled (its
 * compiler is `true`) and its archive is made empty by this very file, which
 * cc-rs runs as its archiver.
 */
import { spawnSync } from 'node:child_process'
import { closeSync, openSync } from 'node:fs'
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
  const ran = spawnSync(
    'cargo',
    ['clippy', '--offline', '--target', TARGET, '--all-targets', ...crates, '--', '-D', 'warnings'],
    {
      stdio: 'inherit',
      env: {
        ...process.env,
        [`CC_${TARGET.replaceAll('-', '_')}`]: 'true',
        [`AR_${TARGET.replaceAll('-', '_')}`]: archiver,
      },
    },
  )
  process.exit(ran.status ?? 1)
}
