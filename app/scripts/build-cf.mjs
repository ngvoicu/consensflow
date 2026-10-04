/**
 * The native `cf` a window runs (crates/cf), built and put in bin/ beside
 * cf.mjs: the checkout's, the integration suite's, and through
 * prepare-sidecar the app bundle's. The copy there is replaced, never
 * rewritten in place: macOS can kill the next run of a Mach-O changed in
 * place, and Windows refuses to delete a cf.exe a question hook still runs,
 * though it lets one be renamed aside. On macOS the copy is signed ad hoc,
 * as the bundle around it is.
 *
 *   node app/scripts/build-cf.mjs [--offline]
 */
import { execFileSync } from 'node:child_process'
import { copyFileSync, readdirSync, renameSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const REPO = fileURLToPath(new URL('../..', import.meta.url))
const NAME = process.platform === 'win32' ? 'cf.exe' : 'cf'

/** Builds `cf` and puts it in bin/: the path it is at. */
export function buildCf({ offline = false } = {}) {
  execFileSync(
    'cargo',
    ['build', '--release', '--locked', ...(offline ? ['--offline'] : []), '-p', 'cf', '--bin', 'cf'],
    { cwd: REPO, stdio: 'inherit' },
  )
  // The workspace's one build folder (.cargo/config.toml) holds every crate's output.
  const built = join(REPO, 'app', 'src-tauri', 'target', 'release', NAME)
  const bin = join(REPO, 'bin')
  const placed = join(bin, NAME)
  // Copies set aside earlier go once nothing runs them any more.
  for (const old of readdirSync(bin).filter((file) => file.startsWith(`${NAME}.old-`))) {
    try {
      rmSync(join(bin, old))
    } catch {}
  }
  try {
    rmSync(placed, { force: true })
  } catch {
    renameSync(placed, `${placed}.old-${Date.now()}`)
  }
  copyFileSync(built, placed)
  if (process.platform === 'darwin') {
    execFileSync('codesign', ['--force', '--sign', '-', placed], { stdio: 'inherit' })
  }
  return placed
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const placed = buildCf({ offline: process.argv.includes('--offline') })
  process.stdout.write(`cf → ${placed}\n`)
}
