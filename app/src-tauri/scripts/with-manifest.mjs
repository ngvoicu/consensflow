// Cargo's runner on Windows (see ../.cargo/config.toml): a unit-test binary
// gets tests.manifest beside it as its external manifest, then runs as asked.
// Anything else, the app included, runs untouched: an embedded manifest wins.
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync } from 'node:fs'
import { basename, join } from 'node:path'

const [executable, ...args] = process.argv.slice(2)
if (/^(app_lib|consensflow_bridge)-.*\.exe$/i.test(basename(executable))) {
  const manifest = `${executable}.manifest`
  if (!existsSync(manifest)) copyFileSync(join(import.meta.dirname, '..', 'tests.manifest'), manifest)
}
const result = spawnSync(executable, args, { stdio: 'inherit' })
process.exit(result.status ?? 1)
