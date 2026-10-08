import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertBuilt, choose, DEFAULT_CLI, NATIVE_CF } from './choice.mjs'

/** Node's CLI, the one `node` selects: its sources, run by name (the door, `bin/cf.mjs`, forwards to the native `cf`). */
const CLI_JS = join(fileURLToPath(new URL('..', import.meta.url)), 'src', 'cli.js')

/**
 * Which `cf` the suites of the CLI run (tests/cli.test.mjs,
 * tests/cf-commands.test.mjs), as `CONSENSFLOW_TEST_CLI` names it (the words
 * are tests/choice.mjs's): `node`, Node's `src/cli.js`; `native` or a JSON
 * array, a command and its arguments, the native `cf` of the build under
 * test; nothing, the tests' default, which is the native one. `kind` is the
 * one chosen. A run labelled with its leg (`CONSENSFLOW_TEST_LEG`) is refused
 * a choice that is not its own, and tests/cli.test.mjs holds the cf that runs
 * to it by the Node processes that start (Node's cf is one; the native cf
 * starts none). Either is run with the environment a test gives it and no
 * other. `node tests/clis.mjs` runs the suites against both. The options are
 * what a test sets to choose in its own words, not the environment's.
 */
export function cliTarget({
  named = process.env.CONSENSFLOW_TEST_CLI,
  leg = process.env.CONSENSFLOW_TEST_LEG,
  fallback = DEFAULT_CLI,
} = {}) {
  const chosen = choose('CONSENSFLOW_TEST_CLI', { named, leg, fallback })
  if (chosen.kind === 'node') {
    return {
      kind: 'node',
      native: false,
      name: "Node's src/cli.js",
      command: process.execPath,
      args: [CLI_JS],
    }
  }
  if (chosen.command === null) assertBuilt()
  const [command, ...args] = chosen.command ?? [NATIVE_CF]
  return {
    kind: 'native',
    native: true,
    name: 'the native cf',
    command,
    args,
  }
}
