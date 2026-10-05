/**
 * `npm run bench:records-memory`: what a first look at a big Claude
 * transcript costs, release-built, each measure in a process of its own (a
 * process remembers the most memory it ever held). The measures are the
 * ignored tests of `crates/cf-harness/src/claude/record/tests/memory.rs`,
 * which say what each reads: a first look and the memory it peaks at; the same
 * look in its parts, each timed apart; the look after it, which finds nothing
 * new; and the look after a record that has the transcript read again.
 *
 * The transcript is a synthetic one, built like a 327 MB one of 125,000
 * lines. Nothing reaches the network, and nothing is written but the
 * synthetic transcript, in a temporary folder.
 *
 *   npm run bench:records-memory [-- --runs 3 --lines 125000 --transcript FILE --node --repo DIR]
 *
 * `--transcript` names a real one to read instead (read only; its file name
 * is its session). `--node` has Node's reader take a first look at it too, for
 * its time and memory to be read beside (`records-node.mjs`). `--runs`
 * repeats the first look. `--repo` names another checkout of this repository
 * to measure, one from before a change: it needs this module's files
 * (`tests/memory.rs`, `tests/synthetic.rs`, and their `mod` lines in
 * `tests.rs`), and the line `read_on_with(… Transcript::parse …)` of
 * `first_look_parts` read as `read_on(…)` where the code is from before the
 * parser of a line was chosen.
 */
import { spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const HERE = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const MODULE = 'claude::record::tests::memory'

const { values } = parseArgs({
  options: {
    runs: { type: 'string', default: '1' },
    lines: { type: 'string' },
    transcript: { type: 'string' },
    repo: { type: 'string' },
    node: { type: 'boolean', default: false },
  },
})
const REPO = values.repo ?? HERE
const env = { ...process.env }
if (values.lines !== undefined) env.CF_RECORDS_MEMORY_LINES = values.lines
if (values.transcript !== undefined) env.CF_RECORDS_MEMORY_TRANSCRIPT = values.transcript

/** One measure, run alone: the lines it printed. */
function measure(name) {
  const run = spawnSync(
    'cargo',
    ['test', '--offline', '--release', '-p', 'cf-harness', '--lib', `${MODULE}::${name}`].concat([
      '--',
      '--ignored',
      '--nocapture',
      '--exact',
    ]),
    { cwd: REPO, env, encoding: 'utf8', maxBuffer: 1 << 26 },
  )
  if (run.status !== 0) {
    process.stderr.write(run.stdout + run.stderr)
    throw new Error(`${name} failed`)
  }
  return run.stdout
    .split('\n')
    .filter((line) => line !== '' && !line.startsWith('running') && !line.startsWith('test '))
}

if (values.node) {
  if (values.transcript === undefined) throw new Error('--node needs --transcript FILE')
  const run = spawnSync(
    process.execPath,
    [join(HERE, 'tests', 'bench', 'records-node.mjs'), values.transcript],
    {
      cwd: HERE,
      encoding: 'utf8',
    },
  )
  if (run.status !== 0) throw new Error(run.stderr)
  console.log("node's first look:")
  for (const line of run.stdout.trimEnd().split('\n')) console.log(`  ${line}`)
}

const runs = Number(values.runs)
for (const name of ['first_look', 'first_look_parts']) {
  for (let run = 1; run <= runs; run++) {
    console.log(`${name.replaceAll('_', ' ')}${runs > 1 ? ` (run ${run})` : ''}:`)
    for (const line of measure(name)) console.log(`  ${line}`)
  }
}
for (const name of ['unchanged_look', 'reread']) {
  console.log(`${name.replace('_', ' ')}:`)
  for (const line of measure(name)) console.log(`  ${line}`)
}
