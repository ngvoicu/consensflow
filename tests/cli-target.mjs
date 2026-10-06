import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { stageBundle } from './bundle.mjs'
import { assertBuilt, choose, chooseHome, DEFAULT_CLI, NATIVE_CF } from './choice.mjs'

/** Node's CLI, the one `node` selects: the door, which runs Node's own CLI in a home that has taken the way back. */
const CF_MJS = join(fileURLToPath(new URL('..', import.meta.url)), 'bin', 'cf.mjs')

/**
 * The verbs the native cf still hands to Node's sources, which it runs on the
 * Node of the bundle it is in: a checkout's bin/ is none, so they are run from
 * a bundle staged for it (`bundle`). They leave this set when the native cf
 * answers them.
 */
const HANDED_ON = new Set(['setup', 'doctor'])

/** The bundle the verbs handed on are run from, staged once and removed when the process ends. */
let staged = null
function bundle() {
  if (staged === null) {
    staged = stageBundle()
    process.on('exit', staged.cleanup)
  }
  return staged
}

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
 * Either is run with the environment a test gives it and no other. `node
 * tests/clis.mjs` runs the suites against both. The options are what a test
 * sets to choose in its own words, not the environment's.
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
      name: "Node's bin/cf.mjs (use-node in the home)",
      command: process.execPath,
      args: [CF_MJS],
      given: false,
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
    // A command a test names is its own to run, bundle and all.
    given: chosen.command !== null,
  }
}

/**
 * The program and words a run of `args` is. The native cf's are those of the
 * target, but for a verb it hands on to Node's sources: that one is run by the
 * native cf of a bundle that has a Node (`HANDED_ON`).
 */
export function cliRun(target, args) {
  if (target.native && !target.given && HANDED_ON.has(args[0])) {
    return { command: bundle().cf, args }
  }
  return { command: target.command, args: [...target.args, ...args] }
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
