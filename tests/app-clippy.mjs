/**
 * Lints the app crate for this machine, where the app cannot be built as it ships:
 * a worktree may not have the resources that the app's Tauri configuration names
 * (the bundled `cf`), and its build script refuses to go on without them. A lint
 * needs none, so they are left out of the configuration for this run, as
 * tests/app-tests.mjs leaves them out for the app's tests and
 * tests/windows-clippy.mjs for the app's lint on Windows. Tests included, warnings
 * denied, as the gate lints the rest of the workspace:
 *
 *   npm run clippy:app
 */
import { spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const APP = join(dirname(fileURLToPath(import.meta.url)), '..', 'app', 'src-tauri')
const ran = spawnSync(
  'cargo',
  ['clippy', '--offline', '--all-targets', '--', '-D', 'warnings', ...process.argv.slice(2)],
  {
    cwd: APP,
    stdio: 'inherit',
    env: {
      ...process.env,
      TAURI_CONFIG: JSON.stringify({ bundle: { resources: null } }),
    },
  },
)
process.exit(ran.status ?? 1)
