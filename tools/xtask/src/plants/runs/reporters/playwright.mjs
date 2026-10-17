// The plants' reporter of Playwright's tests (`reporters.rs` writes it into each run's
// folder, and the run names it beside the list it prints, and where it writes in
// `PLANTS_PLAYWRIGHT_REPORT`): once the run has ended, one JSON report of every test
// registered and how it ended, and of the errors outside every test. A run ended
// before then writes none.
//
// - A test is named by the titles of the groups it is in, from the first one in its
//   file on, and its own (the titles of the project and of the file are none of it),
//   and told from another of its name by its file and place.
// - How it ended, in each project it was given to: Playwright's last word on it,
//   across its retries (`outcome`: `expected`, `unexpected`, `flaky`, `skipped`); and,
//   of its last run, whether its own body failed (`own`). An error a hook or a fixture
//   threw is that one's, which Playwright 1.63 says in a step of the test's own run
//   (`Before Hooks`, `After Hooks`), the hook's first (`hook`); the body's are the
//   errors of the run beyond those. A `beforeAll` that threw failed a test whose body
//   never ran.
// - The errors outside every test (`errors`): a global setup or teardown that threw, a
//   spec that could not be loaded, no test found.
import { writeFileSync } from 'node:fs'

// The first line of an error's words.
const firstLine = (error) => String(error?.message ?? error?.value ?? '').split('\n')[0]

export default class Plants {
  constructor() {
    // The errors hooks and fixtures threw, by the run of a test they came in.
    this.hooks = new Map()
    this.errors = []
  }

  onBegin(config, suite) {
    this.suite = suite
  }

  onStepEnd(test, result, step) {
    if (step.category !== 'hook' || step.parent !== undefined || step.error === undefined) return
    const hooks = this.hooks.get(result) ?? { count: 0, first: firstLine(step.error) }
    hooks.count += 1
    this.hooks.set(result, hooks)
  }

  onError(error) {
    this.errors.push(firstLine(error))
  }

  onEnd() {
    const tests = this.suite.allTests().map((test) => {
      const last = test.results.at(-1)
      const hooks = (last && this.hooks.get(last)) ?? { count: 0, first: null }
      return {
        path: test.titlePath().slice(3),
        file: test.location.file,
        line: test.location.line,
        column: test.location.column,
        project: test.parent.project()?.name ?? '',
        outcome: test.outcome(),
        own: last !== undefined && last.errors.length > hooks.count,
        hook: hooks.first,
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
