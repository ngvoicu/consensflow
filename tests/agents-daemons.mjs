/**
 * The proof of the agents screens (tests/agents-proof.mjs) against each daemon
 * from the checkout, and nothing else: Node's, then the native one (`cf ui`,
 * built and put in bin/ as the app ships it, behind `CONSENSFLOW_DAEMON=native`).
 * `npm run test:daemons` runs it with the daemon suites; this is the quick way
 * to hold the agents' API of both to the one proof, and what the plants of the
 * agents screens (`npm run plants:release`) run. The packaged smoke runs the same
 * proof against the daemon a built app chose (`npm run smoke:daemons`).
 *
 *   node tests/agents-daemons.mjs [--offline]
 */
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildCf } from '../app/scripts/build-cf.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const cf = buildCf({ offline: process.argv.includes('--offline') })

function run(label, daemon) {
  process.stdout.write(`\n== ${label}\n`)
  const ran = spawnSync(process.execPath, ['--test', 'tests/agents-proof.test.mjs'], {
    cwd: REPO,
    stdio: 'inherit',
    env: { ...process.env, CONSENSFLOW_TEST_DAEMON: daemon },
  })
  return ran.status ?? 1
}

const node = run('the Node daemon', '')
const native = run('the native daemon', JSON.stringify([cf, 'ui', '--json', '--no-open']))
process.stdout.write(
  `\nNode: ${node === 0 ? 'passed' : 'FAILED'}; native: ${native === 0 ? 'passed' : 'FAILED'}\n`,
)
process.exit(node === 0 && native === 0 ? 0 : 1)
