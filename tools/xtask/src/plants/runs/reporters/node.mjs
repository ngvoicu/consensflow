// The plants' reporter of `node --test` (`reporters.rs` writes it into each run's
// folder): a line of JSON for each test as Node says it ended, and a last line once
// the run has. A test is named by its own title and the titles of the tests it is
// in; it failed where its own code did, not where only tests inside it failed. What
// a test prints is an event of its own, which is not read.
export default async function* plants(source) {
  // The titles that a test starting at each depth is inside, by file.
  const around = new Map()
  for await (const { type, data } of source) {
    if (type === 'test:start') {
      const titles = (around.get(data.file) ?? []).slice(0, data.nesting)
      around.set(data.file, [...titles, data.name])
    } else if (type === 'test:pass' || type === 'test:fail') {
      const titles = (around.get(data.file) ?? []).slice(0, data.nesting)
      const failure = data.details?.error?.failureType
      const line = {
        path: [...titles, data.name],
        file: data.file ?? null,
        line: data.line ?? null,
        column: data.column ?? null,
        failed: type === 'test:fail' && failure !== 'subtestsFailed',
        skipped: Boolean(data.skip || data.todo),
      }
      yield `${JSON.stringify(line)}\n`
    }
  }
  yield `${JSON.stringify({ end: true })}\n`
}
