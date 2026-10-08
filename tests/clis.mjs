/**
 * The suites of the CLI against the native `cf` (built and put in bin/ as the
 * app ships it, answering every verb). The suites choose the cf by
 * `CONSENSFLOW_TEST_CLI` (tests/cli-target.mjs), and tests/cli.test.mjs holds
 * it to the native one by the Node processes that start: the native cf starts
 * none for any verb, and is named no runtime, so one that handed a verb to Node
 * would fail.
 *
 *   node tests/clis.mjs [--offline]
 */
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildCf } from '../app/scripts/build-cf.mjs'
import { NATIVE_CF } from './choice.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const SUITES = ['tests/cli.test.mjs', 'tests/cf-commands.test.mjs']

const cf = buildCf({ offline: process.argv.includes('--offline') })
// The suites run the cf they find in bin/: the one just built.
if (cf !== NATIVE_CF) throw new Error(`the suites run ${NATIVE_CF}, and ${cf} was built`)
// The suites are a test runner of their own, even when a test runs this.
const env = { ...process.env }
delete env.NODE_TEST_CONTEXT
const ran = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...SUITES], {
  cwd: REPO,
  stdio: 'inherit',
  env,
})
process.exit(ran.status ?? 1)
