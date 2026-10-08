/**
 * The native `cf` a window runs (crates/cf), built and put in bin/: the
 * checkout's, the integration suite's, and through prepare-sidecar the app
 * bundle's. The copy there is replaced, never rewritten in place: macOS can
 * kill the next run of a Mach-O changed in place, and Windows refuses to delete
 * a cf.exe a question hook still runs, though it lets one be renamed aside. On
 * macOS the copy is signed ad hoc, as the bundle around it is.
 *
 *   node app/scripts/build-cf.mjs [--offline]
 */
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, readdirSync, renameSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const REPO = fileURLToPath(new URL('../..', import.meta.url))
const NAME = process.platform === 'win32' ? 'cf.exe' : 'cf'

/**
 * Puts the `built` file in `bin` as `name`: the folder made when a clone has
 * none (git keeps no empty folder, and the `cf` is all that is put in this
 * one), the copies set aside earlier taken away, and the copy there replaced.
 * The path it is at.
 */
export function placeCf(built, bin, name) {
  mkdirSync(bin, { recursive: true })
  const placed = join(bin, name)
  // Copies set aside earlier go once nothing runs them any more.
  for (const old of readdirSync(bin).filter((file) => file.startsWith(`${name}.old-`))) {
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
  return placed
}

/** Builds `cf` and puts it in bin/: the path it is at. */
export function buildCf({ offline = false } = {}) {
  execFileSync(
    'cargo',
    ['build', '--release', '--locked', ...(offline ? ['--offline'] : []), '-p', 'cf', '--bin', 'cf'],
    { cwd: REPO, stdio: 'inherit' },
  )
  // The workspace's one build folder (.cargo/config.toml) holds every crate's output.
  const built = join(REPO, 'app', 'src-tauri', 'target', 'release', NAME)
  const placed = placeCf(built, join(REPO, 'bin'), NAME)
  if (process.platform === 'darwin') {
    execFileSync('codesign', ['--force', '--sign', '-', placed], { stdio: 'inherit' })
  }
  return placed
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const placed = buildCf({ offline: process.argv.includes('--offline') })
  process.stdout.write(`cf → ${placed}\n`)
}
