/**
 * The suites of the CLI against both CLIs: Node's `bin/cf.mjs`, then the native
 * `cf` (built and put in bin/ as the app ships it, answering the standalone verbs
 * behind `CONSENSFLOW_DAEMON=native`). The suites choose the CLI by
 * `CONSENSFLOW_TEST_CLI` (tests/cli-target.mjs), which each leg sets itself, with
 * `CONSENSFLOW_TEST_LEG` to say which leg it is (tests/legs.mjs): a variable in
 * the caller's shell does not choose for it, and the suites refuse a selection
 * that is not the leg's own. tests/cli.test.mjs holds the cf that runs to the
 * leg by the Node processes that start (Node's cf is one, the native cf starts
 * none for the catalog), and says which it found to this runner, which fails a
 * leg that ran the other. What the native cf still hands to Node's
 * sources (`setup` and `doctor`) runs on the runtime of this process, and every
 * other verb is given none, so that a native cf that handed it on would fail.
 *
 *   node tests/clis.mjs [--offline]
 */
import { fileURLToPath } from 'node:url'
import { buildCf } from '../app/scripts/build-cf.mjs'
import { NATIVE_CF } from './choice.mjs'
import { CLI_LEGS, runLeg } from './legs.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const SUITES = ['tests/cli.test.mjs', 'tests/cf-commands.test.mjs']

const cf = buildCf({ offline: process.argv.includes('--offline') })
// The native leg runs the cf the suites find in bin/: the one just built.
if (cf !== NATIVE_CF) throw new Error(`the native leg runs ${NATIVE_CF}, and ${cf} was built`)
const [node, native] = CLI_LEGS.map((leg) =>
  runLeg('CONSENSFLOW_TEST_CLI', leg, SUITES, { cwd: REPO }),
)
process.stdout.write(
  `\nNode: ${node === 0 ? 'passed' : 'FAILED'}; native: ${native === 0 ? 'passed' : 'FAILED'}\n`,
)
process.exit(node === 0 && native === 0 ? 0 : 1)
