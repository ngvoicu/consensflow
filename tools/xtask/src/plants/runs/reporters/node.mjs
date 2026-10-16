// The plants' reporter of `node --test` (`reporters.rs` writes it into each run's
// folder, and runs the tests in Node's own process, so that every event this reads is
// the runner's and none is read from what a test wrote): a line of JSON for each test
// as Node says it ended, and a last line once the run has.
//
// - A test is named by its own title and the titles of the tests it is in, by Node's
//   ids of each test and of the one it is in, wherever each was written (a helper
//   module that adds a subtest to a test of another file too). Each line is one test
//   registered: a loop that registers two tests of one title and place is two lines.
// - How it ended (`ended`): `failed` where its own code did (it threw, or ran out of
//   its time); `passed` where it passed, and where only tests inside it failed;
//   `skipped` and `todo` where it was marked so, whatever the reason given, an empty
//   one too; `cancelled` where it ended for what is not its own (its parent cancelled
//   it, or ended first; it was aborted; a hook of it failed).
// - A file Node failed whole, or that holds no test, is a test of its own (`whole`):
//   at the top, named by the file's path as Node was given it, at its first line and
//   column.
import { resolve } from 'node:path'

// A test's own code failed, by the `failureType` Node gives it.
const OWN = new Set([
  'testCodeFailure',
  'testTimeoutFailure',
  'callbackAndPromisePresent',
  'multipleCallbackInvocations',
])

// The events that tell a test.
const TOLD = new Set(['test:enqueue', 'test:dequeue', 'test:start', 'test:pass', 'test:fail'])

// Where Node was started, which the paths of the test files it was given are from.
const started = process.cwd()

export default async function* plants(source) {
  // Each test's title and the id of the test it is in, by its id; the run's is 0.
  const tests = new Map()
  const path = (id) => {
    const titles = []
    for (let at = id; at !== 0; at = tests.get(at).parentId) {
      if (!tests.has(at)) {
        throw new Error(`the plants' reporter: Node told no test of the id ${at}`)
      }
      titles.unshift(tests.get(at).name)
    }
    return titles
  }
  for await (const { type, data } of source) {
    if (!TOLD.has(type)) continue
    if (data.testId === undefined || data.parentId === undefined) {
      throw new Error(`the plants' reporter: ${type} came with no ids of tests (Node 26 gives them)`)
    }
    tests.set(data.testId, { name: data.name, parentId: data.parentId })
    if (type !== 'test:pass' && type !== 'test:fail') continue
    const whole =
      data.nesting === 0 &&
      data.line === 1 &&
      data.column === 1 &&
      data.file !== undefined &&
      resolve(started, data.name) === data.file
    const failure = data.details?.error?.failureType
    let ended
    if (data.skip !== undefined) ended = 'skipped'
    else if (data.todo !== undefined) ended = 'todo'
    else if (type === 'test:pass' || failure === 'subtestsFailed') ended = 'passed'
    else if (whole || OWN.has(failure)) ended = 'failed'
    else ended = 'cancelled'
    const line = {
      path: path(data.testId),
      file: data.file ?? null,
      line: data.line ?? null,
      column: data.column ?? null,
      whole,
      ended,
    }
    yield `${JSON.stringify(line)}\n`
  }
  yield `${JSON.stringify({ end: true })}\n`
}
