import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { choose, DEFAULT_CLI, NATIVE_CF } from './choice.mjs'

/** Node's CLI, the one `node` selects. */
const CF_MJS = join(fileURLToPath(new URL('..', import.meta.url)), 'bin', 'cf.mjs')

/**
 * Which `cf` the suites of the CLI run (tests/cli.test.mjs,
 * tests/cf-commands.test.mjs), as `CONSENSFLOW_TEST_CLI` names it (the words
 * are tests/choice.mjs's): `node`, Node's `bin/cf.mjs`; `native` or a JSON
 * array, a command and its arguments, the native `cf` of the build under
 * test; nothing, the tests' default. Each has the switch the product reads
 * (`CONSENSFLOW_DAEMON`) said in its environment, Node's too: no run leaves to
 * the product's own default which cf answers. `kind` is the one chosen. A run
 * labelled with its leg (`CONSENSFLOW_TEST_LEG`) is refused a choice that is
 * not its own, and tests/cli.test.mjs holds the cf that runs to it by the
 * Node processes that start (Node's cf is one; the native cf starts none).
 * Either is run with the environment a test gives it and no other, and the
 * native cf is named no runtime for any verb, so that one that handed a verb
 * to Node's sources would fail. `node tests/clis.mjs` runs the suites against
 * both. The options are what a test sets to choose in its own words, not the
 * environment's.
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
      name: "Node's bin/cf.mjs",
      command: process.execPath,
      args: [CF_MJS],
      env: { CONSENSFLOW_DAEMON: 'node' },
    }
  }
  const [command, ...args] = chosen.command ?? [NATIVE_CF]
  return {
    kind: 'native',
    native: true,
    name: 'the native cf (CONSENSFLOW_DAEMON=native)',
    command,
    args,
    env: { CONSENSFLOW_DAEMON: 'native' },
  }
}
