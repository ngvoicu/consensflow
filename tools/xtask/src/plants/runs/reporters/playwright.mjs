// The plants' reporter of Playwright's tests (`reporters.rs` writes it into each run's
// folder, and the run names it beside the list it prints, and where it writes in
// `PLANTS_PLAYWRIGHT_REPORT`): once the run has ended, one JSON report of every test
// registered and how it ended, and of the errors outside every test. A run ended
// before then writes none.
//
// - A test is named by the titles of the groups it is in, from the first one in its
//   file on, and its own (the titles of the project and of the file are none of it),
//   and told from another of its name by its file and place.
// - How it ended, in each project it was given to: Playwright's word on it
//   (`outcome`: `expected`, `unexpected`, `flaky`, `skipped`), and whether its own body
//   failed (`own`), which only proof says (both reviewers' look at round 7: three rounds
//   inferred that the body ran from what the errors and the hooks said, and each had a
//   hole): it had one attempt, which failed, an assertion of its body failed it (a
//   step `expect` with an error, under no hook and no fixture: in the body, or in a
//   `test.step` of it; and that error among the attempt's, so not one the body caught,
//   nor one a retrying `toPass` passed after), no step in a hook has an error (anywhere in the hook: a
//   fixture's steps are the hooks', which Playwright 1.63 says under `Before Hooks` and
//   `After Hooks`, a hook's error said again on each step it is in), and its worker did
//   not exit under it. Anything less is no failure of its own, whatever error came
//   beside, and says why (`why`): a test run again (the plants' runs ask for no retries,
//   and a group's `retries` is over that), a `beforeAll` that threw, a body that failed
//   beside an `afterEach` that threw, a worker that exited, a test out of its time, and
//   a body that threw, a worker's fixture or an import that failed before it, with no
//   assertion of it failed. A test whose body fails by a thrown error alone is so never
//   caught.
// - The errors outside every test (`errors`): a global setup or teardown that threw, a
//   spec that could not be loaded, no test found.
import { writeFileSync } from 'node:fs'

// The first line of an error's words.
const firstLine = (error) => String(error?.message ?? error?.value ?? '').split('\n')[0]

// What Playwright says of a test whose worker exited under it.
const WORKER_EXITED = 'Error: worker process exited unexpectedly'

// Whether a step is of one of `categories`, or in one.
const under = (step, ...categories) => {
  for (let at = step; at; at = at.parent) if (categories.includes(at.category)) return true
  return false
}

// Whether a step is an assertion of the body's own that failed: what failed the test is
// said by the attempt's errors (`asserted`).
const failedAssertion = (step) =>
  step.category === 'expect' && step.error !== undefined && !under(step, 'hook', 'fixture')

export default class Plants {
  constructor() {
    // By the attempt of a test: the first error of a step in a hook.
    this.hooks = new Map()
    // By the attempt of a test: the first lines of the errors of the assertions of the
    // body's own that failed in it.
    this.assertions = new Map()
    this.errors = []
  }

  onBegin(config, suite) {
    this.suite = suite
  }

  onStepEnd(test, result, step) {
    if (step.error !== undefined && under(step, 'hook') && !this.hooks.has(result)) this.hooks.set(result, step.error)
    if (failedAssertion(step)) {
      const lines = this.assertions.get(result) ?? []
      this.assertions.set(result, [...lines, firstLine(step.error)])
    }
  }

  onError(error) {
    this.errors.push(firstLine(error))
  }

  // Whether an assertion of the body's own failed the attempt `result`: its error is
  // among the attempt's (Calliope's look at round 8: one caught by the body, or one a
  // retrying `toPass` passed after, is not).
  asserted(result) {
    const failed = result.errors.map(firstLine)
    return (this.assertions.get(result) ?? []).some((line) => failed.includes(line))
  }

  // Why the last attempt of `test` is no failure of its body's own, if it is not: none
  // where it is, or did not fail and was not run again.
  why(test) {
    if (test.results.length > 1 || test.results.some((result) => result.retry > 0)) {
      return `it was retried (${test.results.length} attempts)`
    }
    const result = test.results.at(-1)
    if (result === undefined || result.status === 'passed' || result.status === 'skipped') return null
    const hook = this.hooks.get(result)
    if (hook !== undefined) return `a hook failed: ${firstLine(hook)}`
    const exited = result.errors.find((error) => firstLine(error).startsWith(WORKER_EXITED))
    if (exited !== undefined) return `its worker exited: ${firstLine(exited)}`
    if (result.status !== 'failed') return `it ended ${result.status}`
    if (!this.asserted(result)) return `no assertion of its body failed: ${firstLine(result.errors[0])}`
    return null
  }

  onEnd() {
    const tests = this.suite.allTests().map((test) => {
      const last = test.results.at(-1)
      const why = this.why(test)
      return {
        path: test.titlePath().slice(3),
        file: test.location.file,
        line: test.location.line,
        column: test.location.column,
        project: test.parent.project()?.name ?? '',
        outcome: test.outcome(),
        own: last?.status === 'failed' && why === null,
        why,
      }
    })
    writeFileSync(
      process.env.PLANTS_PLAYWRIGHT_REPORT,
      `${JSON.stringify({ tests, errors: this.errors, end: true })}\n`,
    )
  }

  printsToStdio() {
    return false
  }
}
