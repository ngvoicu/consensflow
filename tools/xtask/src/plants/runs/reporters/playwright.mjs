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
//   (`outcome`: `expected`, `unexpected`, `flaky`, `skipped`), of its one attempt (the
//   plants' runs ask for no retries); and whether its own body failed (`own`), which
//   only proof says: its attempt failed, no step in a hook has an error (anywhere in the
//   hook: a fixture's steps are the hooks', which Playwright 1.63 says under `Before
//   Hooks` and `After Hooks`, a hook's error said again on each step it is in), and its
//   worker did not exit under it. Anything less is no failure of its own, whatever error
//   came beside, and says why (`why`): a `beforeAll` that threw, a body that failed
//   beside an `afterEach` that threw, a worker that exited, a test out of its time.
// - The errors outside every test (`errors`): a global setup or teardown that threw, a
//   spec that could not be loaded, no test found.
import { writeFileSync } from 'node:fs'

// The first line of an error's words.
const firstLine = (error) => String(error?.message ?? error?.value ?? '').split('\n')[0]

// What Playwright says of a test whose worker exited under it.
const WORKER_EXITED = 'Error: worker process exited unexpectedly'

// Whether a step is a hook's, or in one.
const inHook = (step) => {
  for (let at = step; at; at = at.parent) if (at.category === 'hook') return true
  return false
}

export default class Plants {
  constructor() {
    // By the attempt of a test: the first error of a step in a hook.
    this.hooks = new Map()
    this.errors = []
  }

  onBegin(config, suite) {
    this.suite = suite
  }

  onStepEnd(test, result, step) {
    if (step.error !== undefined && inHook(step) && !this.hooks.has(result)) this.hooks.set(result, step.error)
  }

  onError(error) {
    this.errors.push(firstLine(error))
  }

  // Why the attempt `result` is no failure of its body's own, if it is not: none where
  // it is, or did not fail.
  why(result) {
    if (result === undefined || result.status === 'passed' || result.status === 'skipped') return null
    const hook = this.hooks.get(result)
    if (hook !== undefined) return `a hook failed: ${firstLine(hook)}`
    const exited = result.errors.find((error) => firstLine(error).startsWith(WORKER_EXITED))
    if (exited !== undefined) return `its worker exited: ${firstLine(exited)}`
    if (result.status !== 'failed') return `it ended ${result.status}`
    return null
  }

  onEnd() {
    const tests = this.suite.allTests().map((test) => {
      const last = test.results.at(-1)
      const why = this.why(last)
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
