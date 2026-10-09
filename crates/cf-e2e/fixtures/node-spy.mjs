import { appendFileSync } from 'node:fs'

/**
 * A preload for a test that asks which implementation ran
 * (`NODE_OPTIONS=--import=<this file>`): every Node process that starts says so
 * in the file `CF_TEST_SPY` names, with the script it runs. A native program
 * starts none, and says nothing. Its one user is the CLI suite of cf-e2e
 * (tests/cli/roster.rs), which only hands the variables over: no Node runs
 * when `cf` is native.
 */
if (process.env.CF_TEST_SPY) {
  appendFileSync(process.env.CF_TEST_SPY, `${process.pid}\t${process.argv[1] ?? ''}\n`)
}
