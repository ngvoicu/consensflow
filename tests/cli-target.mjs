import { assertBuilt, choose, chooseHome, DEFAULT_CLI, NATIVE_CF, NODE_CF } from './choice.mjs'

/**
 * Which `cf` the suites of the CLI run (tests/cli.test.mjs,
 * tests/cf-commands.test.mjs), as `CONSENSFLOW_TEST_CLI` names it (the words
 * are tests/choice.mjs's): `node`, Node's `bin/cf.mjs`; `native` or a JSON
 * array, a command and its arguments, the native `cf` of the build under
 * test; nothing, the tests' default, which is the native one. The product
 * chooses by the file in the home, not by the environment: `cliEnv` makes the
 * choice in the home a run is given (`chooseHome`), so a run never leaves to the
 * product's own default which cf answers. `kind` is the one chosen. A run
 * labelled with its leg (`CONSENSFLOW_TEST_LEG`) is refused a choice that is
 * not its own, and tests/cli.test.mjs holds the cf that runs to it by the
 * Node processes that start (Node's cf is one; the native cf starts none).
 * Either is run with the environment a test gives it and no other, and the
 * native cf is named no runtime for any verb, so that one that handed a verb
 * to Node's sources would fail (none is, now that `setup` and `doctor` are
 * answered too). `node tests/clis.mjs` runs the suites against both. The
 * options are what a test sets to choose in its own words, not the
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
      name: "Node's bin/cf.mjs (use-node in the home)",
      command: process.execPath,
      args: [NODE_CF],
    }
  }
  if (chosen.command === null) assertBuilt()
  const [command, ...args] = chosen.command ?? [NATIVE_CF]
  return {
    kind: 'native',
    name: 'the native cf',
    command,
    args,
  }
}

/**
 * The environment a run is given: the test's, and the home in it made the
 * target's choice (`chooseHome`: the file for Node's cf, none for the native
 * one). The home is the test's own: a run needs one, since the product looks in
 * it for the way back and nothing of the machine's is anybody's to look in.
 */
export function cliEnv(target, env) {
  chooseHome(target.kind, env.CONSENSFLOW_HOME)
  return { ...env }
}
