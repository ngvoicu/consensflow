/**
 * The packaged smoke (tests/smoke.test.mjs) on both daemons, on the app as it is
 * built: Node's, then the native one. Each leg sets the daemon the app starts
 * (`CONSENSFLOW_DAEMON`) itself, so a variable in the caller's shell chooses
 * nothing, and the smoke holds the daemon that started, which its log names, to
 * the one asked for. It runs against the bundle at `CONSENSFLOW_SMOKE_APP`, or
 * the one `npm --prefix app run build -- --bundles app` leaves.
 *
 *   node tests/smoke-daemons.mjs
 */
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const REPO = fileURLToPath(new URL('..', import.meta.url))

function run(label, daemon) {
  process.stdout.write(`\n== ${label}\n`)
  const env = { ...process.env, CONSENSFLOW_SMOKE: '1', CONSENSFLOW_DAEMON: daemon }
  // The smoke is a test runner of its own, even when a test runs this.
  delete env.NODE_TEST_CONTEXT
  const ran = spawnSync(process.execPath, ['--test', 'tests/smoke.test.mjs'], {
    cwd: REPO,
    stdio: 'inherit',
    env,
  })
  return ran.status ?? 1
}

const node = run('the Node daemon', 'node')
const native = run('the native daemon', 'native')
process.stdout.write(
  `\nNode: ${node === 0 ? 'passed' : 'FAILED'}; native: ${native === 0 ? 'passed' : 'FAILED'}\n`,
)
process.exit(node === 0 && native === 0 ? 0 : 1)
