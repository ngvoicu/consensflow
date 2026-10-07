import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * The legs of the dual runners (tests/daemons.mjs, tests/clis.mjs): what each is
 * called, what it selects, and what it says it is. A leg's suites are given both
 * (tests/choice.mjs): the selection alone would leave a leg whose selector went
 * missing to run the tests' default, and once that is the native one both legs
 * would run it, one of them under Node's name. With the label too, the suites
 * refuse the selection that is not the leg's own, and the daemon that starts is
 * held to the leg (tests/integration/daemon-seam.test.mjs). The runner holds the
 * leg to it as well, by what its suites say they ran (`ranProblem`), for the
 * leg that lost its selector and its label both, whose suites would all agree.
 */
export const DAEMON_LEGS = [
  { label: 'the Node daemon', selector: 'node', leg: 'node' },
  { label: 'the native daemon', selector: 'native', leg: 'native' },
]

export const CLI_LEGS = [
  { label: "Node's cf.mjs", selector: 'node', leg: 'node' },
  { label: 'the native cf', selector: 'native', leg: 'native' },
]

/**
 * The environment the suites of a leg run in: the caller's, but for the two
 * variables that are the leg's own, which a variable in the caller's shell
 * does not choose for it. `variable` is the one that selects
 * (`CONSENSFLOW_TEST_DAEMON`, `CONSENSFLOW_TEST_CLI`).
 */
export function legEnv(variable, { selector, leg }, base = process.env) {
  return { ...base, [variable]: selector, CONSENSFLOW_TEST_LEG: leg }
}

/**
 * What a leg's suites said they ran (`noteRan` in tests/choice.mjs: a word a
 * line, in `said`): a problem when none said anything, or one ran that is not
 * the leg's own; null when every one was.
 */
export function ranProblem({ label, leg }, said) {
  const ran = said.split('\n').filter(Boolean)
  if (ran.length === 0) return `${label}: its suites did not say which implementation they ran`
  const other = ran.find((kind) => kind !== leg)
  return other === undefined ? null : `${label}: a suite ran the ${other} one`
}

/**
 * One leg of a dual runner: its suites as `node --test` in `cwd` (their output
 * to `stdio`), in the leg's environment on top of `base`, and then the leg held
 * to what they said they ran. The runner's own words go to `say`. The exit
 * status it answers is 1 when the suites passed and the leg was not its own.
 */
export function runLeg(
  variable,
  leg,
  suites,
  { cwd, base = process.env, stdio = 'inherit', say = (text) => process.stdout.write(text) },
) {
  say(`\n== ${leg.label} (${variable}=${leg.selector}, CONSENSFLOW_TEST_LEG=${leg.leg})\n`)
  const folder = mkdtempSync(join(tmpdir(), 'consensflow-leg-'))
  const said = join(folder, 'ran')
  try {
    const env = { ...legEnv(variable, leg, base), CONSENSFLOW_TEST_RAN: said }
    // The suites are a test runner of their own, even when a test runs this.
    delete env.NODE_TEST_CONTEXT
    const ran = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...suites], {
      cwd,
      stdio,
      env,
    })
    const problem = ranProblem(leg, existsSync(said) ? readFileSync(said, 'utf8') : '')
    if (problem !== null) say(`${problem}\n`)
    return (ran.status ?? 1) === 0 && problem === null ? 0 : 1
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
}
