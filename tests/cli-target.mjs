import { assertBuilt, choose, NATIVE_CF } from './choice.mjs'

/**
 * Which `cf` the suites of the CLI run (tests/cli.test.mjs,
 * tests/cf-commands.test.mjs): the native `cf` this checkout builds, or the
 * command `CONSENSFLOW_TEST_CLI` names (a JSON array, a command and its
 * arguments; the words are tests/choice.mjs's). It is run with the environment
 * a test gives it and no other, and is named no runtime for any verb. `named` is
 * what a test sets to choose in its own words, not the environment's.
 */
export function cliTarget({ named = process.env.CONSENSFLOW_TEST_CLI } = {}) {
  const chosen = choose('CONSENSFLOW_TEST_CLI', named)
  if (chosen === null) assertBuilt()
  const [command, ...args] = chosen ?? [NATIVE_CF]
  return { name: 'the native cf', command, args }
}
