import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

/**
 * The Agents page's browser tests on a `cf` built from the sources as they are
 * now: the page is the daemon's (`crates/cf-daemon/src/screens/pages/agents.html`),
 * built into `cf`, so a plant in it is seen only by a `cf` built again. Takes the
 * words of the tests to run (Playwright's `-g`). A plant's run starts it with
 * `node tests/agents-page-built.mjs <words>`, and says it `builds`.
 */

const REPO = fileURLToPath(new URL('..', import.meta.url))
const words = process.argv[2]
if (!words) throw new Error('say which of the Agents page tests to run')
const run = (program, args) => execFileSync(program, args, { cwd: REPO, stdio: 'inherit' })
run('cargo', ['xtask', 'build-cf', '--offline'])
run(process.execPath, [
  'app/node_modules/@playwright/test/cli.js',
  'test',
  'app/tests/harnesses.spec.mjs',
  '-g',
  words,
])
