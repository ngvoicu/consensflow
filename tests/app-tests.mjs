/**
 * Runs the unit tests of the app crate where the app cannot be built as it
 * ships: a worktree may not have the resources that the app's Tauri
 * configuration names (the bundled `cf`), and its build script refuses to go
 * on without them. A unit test needs none, so they are left out of the
 * configuration for this run, as tests/windows-clippy.mjs leaves them out for
 * the app's lint. Cargo runs in the app's folder, as CI's does.
 *
 *   npm run test:app
 *   npm run test:app -- portable::
 *
 * What follows `--` is handed to the test binaries: a filter, `--nocapture`.
 */
import { spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const APP = join(dirname(fileURLToPath(import.meta.url)), '..', 'app', 'src-tauri')
const ran = spawnSync('cargo', ['test', '--offline', '--', ...process.argv.slice(2)], {
  cwd: APP,
  stdio: 'inherit',
  env: {
    ...process.env,
    TAURI_CONFIG: JSON.stringify({ bundle: { resources: null } }),
  },
})
process.exit(ran.status ?? 1)
