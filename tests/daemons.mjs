/**
 * The daemon cases that go through a process, run against both daemons: Node's,
 * then the native one (`cf ui`, built and put in bin/ as the app ships it). Each
 * leg names its daemon (`node`, `native`) and
 * says which leg it is (tests/legs.mjs, tests/choice.mjs): the suites refuse a
 * selection that is not the leg's own, and hold every daemon they start to it by
 * the start line in its log, which says which ran (`node v…` or `rust …`). The
 * suites say which they found to this runner, which fails a leg in which a
 * daemon started that is not its own, or none was seen to. What
 * only Node's own modules can show, and what the native daemon does not serve
 * yet, they skip for the native one with the reason. The rig's seam and its
 * suites are run too: each daemon on the real headless bridge, which is built
 * here, with a stand-in harness in real windows (a task handed out and its
 * result back, a question and its answer, a chief switched, sessions, a
 * refusal, the human's approval).
 *
 *   node tests/daemons.mjs [--offline]
 */
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildCf } from '../app/scripts/build-cf.mjs'
import { NATIVE_CF } from './choice.mjs'
import { DAEMON_LEGS, runLeg } from './legs.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const SUITES = [
  'tests/agents-proof.test.mjs',
  'tests/core-daemon.test.mjs',
  'tests/bridge.test.mjs',
  'tests/integration/daemon-seam.test.mjs',
  'tests/integration/core-slice.test.mjs',
  'tests/integration/core-tiered.test.mjs',
  'tests/integration/core-questions.test.mjs',
]
const offline = process.argv.includes('--offline')

// What `npm run build:bridge` builds: the pane host's end of the bridge.
execFileSync(
  'cargo',
  [
    'build',
    '--release',
    ...(offline ? ['--offline'] : []),
    '-p',
    'cf-panes',
    '--bin',
    'consensflow-bridge',
  ],
  { cwd: REPO, stdio: 'inherit' },
)
const cf = buildCf({ offline })
// The native leg runs the cf the suites find in bin/: the one just built.
if (cf !== NATIVE_CF) throw new Error(`the native leg runs ${NATIVE_CF}, and ${cf} was built`)
const [node, native] = DAEMON_LEGS.map((leg) =>
  runLeg('CONSENSFLOW_TEST_DAEMON', leg, SUITES, { cwd: REPO }),
)
process.stdout.write(
  `\nNode: ${node === 0 ? 'passed' : 'FAILED'}; native: ${native === 0 ? 'passed' : 'FAILED'}\n`,
)
process.exit(node === 0 && native === 0 ? 0 : 1)
