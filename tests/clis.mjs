/**
 * The suites of the CLI against both CLIs: Node's, `src/cli.js` run by name (the
 * door, `bin/cf.mjs`, forwards every command to the native `cf`), then the native
 * `cf` (built and put in bin/ as the app ships it, answering every standalone verb).
 * The suites choose the CLI by `CONSENSFLOW_TEST_CLI` (tests/cli-target.mjs), which
 * each leg sets itself, with `CONSENSFLOW_TEST_LEG` to say which leg it is
 * (tests/legs.mjs): a variable in the caller's shell does not choose for it, and
 * the suites refuse a selection that is not the leg's own. The product chooses
 * nothing now, so each run is given a home marked as its leg's: one that has the
 * flip release's `use-node` file (Node's leg) or none (the native leg).
 * tests/cli.test.mjs holds the cf that runs to the leg by
 * the Node processes that start (Node's cf is one, the native cf starts none for
 * any verb), and says which it found to this runner, which fails a leg that ran
 * the other. The native cf is named no runtime for any verb, so that one that
 * handed a verb to Node's sources would fail.
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
