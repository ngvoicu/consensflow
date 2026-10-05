/**
 * The daemon cases that go through a process, run against both daemons: Node's,
 * then the native one (`cf ui`, built and put in bin/ as the app ships it, behind
 * `CONSENSFLOW_DAEMON=native`). The suites choose the daemon by
 * `CONSENSFLOW_TEST_DAEMON`, a JSON array of a command and its arguments; what
 * only Node's own modules can show, and what the native daemon does not serve
 * yet, they skip for the native one with the reason. The rig's seam is run too:
 * it connects each daemon to the real headless bridge, which is built here.
 *
 *   node tests/daemons.mjs [--offline]
 */
import { execFileSync, spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildCf } from '../app/scripts/build-cf.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const SUITES = [
  'tests/core-daemon.test.mjs',
  'tests/bridge.test.mjs',
  'tests/integration/daemon-seam.test.mjs',
]
const offline = process.argv.includes('--offline')

function run(label, daemon) {
  process.stdout.write(`\n== ${label}\n`)
  const ran = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...SUITES], {
    cwd: REPO,
    stdio: 'inherit',
    env: { ...process.env, CONSENSFLOW_TEST_DAEMON: daemon },
  })
  return ran.status ?? 1
}

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
const node = run('the Node daemon', '')
const native = run('the native daemon', JSON.stringify([cf, 'ui', '--json', '--no-open']))
process.stdout.write(
  `\nNode: ${node === 0 ? 'passed' : 'FAILED'}; native: ${native === 0 ? 'passed' : 'FAILED'}\n`,
)
process.exit(node === 0 && native === 0 ? 0 : 1)
