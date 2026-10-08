/**
 * The packaged smoke (tests/smoke.test.mjs) on the app as it is built, or on
 * the one it is given: the app is the whole of the product, and it ships one
 * daemon, so this runs the smoke once.
 *
 *   npm run smoke                       # the bundle `npm --prefix app run build -- --bundles app` leaves
 *   npm run smoke -- --app <path>       # a built ConsensFlow.app: the signed one, or a copy
 *
 * `CONSENSFLOW_SMOKE_APP` names the app as well, for a workflow's environment;
 * `--app` wins.
 */
import { spawnSync } from 'node:child_process'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const { values } = parseArgs({ options: { app: { type: 'string' } } })

const env = { ...process.env, CONSENSFLOW_SMOKE: '1' }
if (values.app !== undefined) env.CONSENSFLOW_SMOKE_APP = resolve(values.app)
// The smoke is a test runner of its own, even when a test runs this.
delete env.NODE_TEST_CONTEXT
const ran = spawnSync(process.execPath, ['--test', 'tests/smoke.test.mjs'], {
  cwd: REPO,
  stdio: 'inherit',
  env,
})
process.exit(ran.status ?? 1)
