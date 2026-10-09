/**
 * The proof of the agents screens (tests/agents-proof.mjs) against the native
 * daemon from the checkout, and nothing else (`cf ui`, built and put in bin/ as
 * the app ships it). `npm run test:daemons` runs it with the daemon suites; this
 * is the quick way to hold the agents' API to the one proof, and what the plants
 * of the agents screens (`npm run plants:release`) run. The packaged smoke runs
 * the same proof against the daemon a built app started (`npm run smoke`).
 *
 *   cargo xtask test agents [--offline]
 */
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { buildNativeCf } from './choice.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
buildNativeCf({ offline: process.argv.includes('--offline') })

// The suite is a test runner of its own, even when a test runs this.
const env = { ...process.env }
delete env.NODE_TEST_CONTEXT
const ran = spawnSync(process.execPath, ['--test', 'tests/agents-proof.test.mjs'], {
  cwd: REPO,
  stdio: 'inherit',
  env,
})
process.exit(ran.status ?? 1)
