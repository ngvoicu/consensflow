/**
 * The suites of the CLI against both CLIs: Node's `bin/cf.mjs`, then the native
 * `cf` (built and put in bin/ as the app ships it, answering the standalone verbs
 * behind `CONSENSFLOW_DAEMON=native`). The suites choose the CLI by
 * `CONSENSFLOW_TEST_CLI` (tests/cli-target.mjs), which each leg sets, or clears,
 * itself: a variable in the caller's shell does not choose for it. What the native
 * cf still hands to Node's sources (`setup` and `doctor`) runs on the runtime of
 * this process, and every other verb is given none, so that a native cf that
 * handed it on would fail.
 *
 *   node tests/clis.mjs [--offline]
 */
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildCf } from '../app/scripts/build-cf.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const SUITES = ['tests/cli.test.mjs', 'tests/cf-commands.test.mjs']

function run(label, cli) {
  process.stdout.write(`\n== ${label}\n`)
  const env = { ...process.env, CONSENSFLOW_TEST_CLI: cli }
  // The suites are a test runner of their own, even when a test runs this.
  delete env.NODE_TEST_CONTEXT
  const ran = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...SUITES], {
    cwd: REPO,
    stdio: 'inherit',
    env,
  })
  return ran.status ?? 1
}

const cf = buildCf({ offline: process.argv.includes('--offline') })
const node = run("Node's cf.mjs", '')
const native = run('the native cf', JSON.stringify([cf]))
process.stdout.write(
  `\nNode: ${node === 0 ? 'passed' : 'FAILED'}; native: ${native === 0 ? 'passed' : 'FAILED'}\n`,
)
process.exit(node === 0 && native === 0 ? 0 : 1)
