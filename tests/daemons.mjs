/**
 * The daemon cases that go through a process, run against the native daemon
 * (`cf ui`, built and put in bin/ as the app ships it). The suites hold every
 * daemon they start to the native one by the start line in its log, which says
 * `rust …`. The rig's seam and its suites are run too: the daemon on the real
 * headless bridge, which is built here, with a stand-in harness in real
 * windows (a task handed out and its result back, a question and its answer, a
 * chief switched, sessions, a refusal, the human's approval, the board after a
 * re-plan, a tell and a cancel, a window with no login whose screen the
 * failure quotes).
 *
 *   cargo xtask test daemons [--offline]
 */
import { execFileSync, spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildNativeCf } from './choice.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const SUITES = [
  'tests/agents-proof.test.mjs',
  'tests/core-daemon.test.mjs',
  'tests/bridge.test.mjs',
  'tests/integration/daemon-seam.test.mjs',
  'tests/integration/core-slice.test.mjs',
  'tests/integration/core-tiered.test.mjs',
  'tests/integration/core-questions.test.mjs',
  'tests/integration/core-board.test.mjs',
  'tests/integration/core-tells.test.mjs',
  'tests/integration/window-says-why.test.mjs',
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
// The suites run the cf they find in bin/: the one just built.
buildNativeCf({ offline })
// The suites are a test runner of their own, even when a test runs this.
const env = { ...process.env }
delete env.NODE_TEST_CONTEXT
const ran = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...SUITES], {
  cwd: REPO,
  stdio: 'inherit',
  env,
})
process.exit(ran.status ?? 1)
