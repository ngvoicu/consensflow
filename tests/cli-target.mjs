import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** The CLI the suites run when none is named: Node's. */
const CF_MJS = join(fileURLToPath(new URL('..', import.meta.url)), 'bin', 'cf.mjs')

/** The verbs the native cf still hands to Node's sources, which need the runtime named. */
const HANDED_ON = new Set(['setup', 'doctor'])

/**
 * Which `cf` the suites of the CLI run (tests/cli.test.mjs,
 * tests/cf-commands.test.mjs): Node's `bin/cf.mjs` by default; with
 * `CONSENSFLOW_TEST_CLI` set to a JSON array, a command and its arguments, the
 * native `cf` of the build under test, with the switch it answers the standalone
 * verbs behind (`CONSENSFLOW_DAEMON=native`) in its environment. Either is run
 * with the environment a test gives it and no other. `node tests/clis.mjs` runs
 * the suites against both.
 */
export function cliTarget() {
  const named = process.env.CONSENSFLOW_TEST_CLI
  if (named === undefined || named === '') {
    return {
      native: false,
      name: "Node's bin/cf.mjs",
      command: process.execPath,
      args: [CF_MJS],
      env: {},
    }
  }
  const [command, ...args] = JSON.parse(named)
  if (typeof command !== 'string' || args.some((arg) => typeof arg !== 'string')) {
    throw new Error('CONSENSFLOW_TEST_CLI is a JSON array of strings: a command and its arguments')
  }
  return {
    native: true,
    name: 'the native cf (CONSENSFLOW_DAEMON=native)',
    command,
    args,
    env: { CONSENSFLOW_DAEMON: 'native' },
  }
}

/**
 * The environment a run of `args` is given: the test's, the target's, and for
 * a verb the native cf hands on to Node's sources the runtime to run them on
 * (it names none of its own; the app does when it opens a pane). Every other
 * verb is given none, so that a native cf that handed it on would fail.
 */
export function cliEnv(target, args, env) {
  const runtime =
    target.native && HANDED_ON.has(args[0]) ? { CONSENSFLOW_NODE: process.execPath } : {}
  return { ...env, ...target.env, ...runtime }
}
