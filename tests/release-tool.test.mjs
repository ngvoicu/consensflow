import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * How the release workflow (.github/workflows/release.yml) uses the release
 * tool, tools/cf-release: it is built in a step of its own that holds no key,
 * and the steps that need it run the binary by its path. A step that holds a key
 * never builds anything, and never goes through `cargo run`: that would build,
 * and run, whatever the lockfile names with the key in its environment. The
 * tool's own behaviour is its crate's tests'; this holds the workflow's text.
 */

const WORKFLOW = fileURLToPath(new URL('../.github/workflows/release.yml', import.meta.url))
const BUILD = 'cargo build --release --locked -p cf-release'
const TOOL = 'app/src-tauri/target/release/cf-release'

/** The Mac job's steps as `{ name, text }`, comments left out. */
function macSteps() {
  const lines = readFileSync(WORKFLOW, 'utf8').split('\n')
  const start = lines.indexOf('  mac:')
  assert.notEqual(start, -1, 'release.yml has no mac job')
  const end = lines.findIndex((line, at) => at > start && /^ {2}\S/.test(line))
  const job = lines.slice(start, end === -1 ? undefined : end).filter((line) => !/^\s*#/.test(line))
  const steps = []
  for (const line of job) {
    if (/^ {6}- /.test(line)) steps.push([])
    steps.at(-1)?.push(line)
  }
  return steps.map((step) => {
    const named = step.find((line) => /^( {6}- | {8})name: /.test(line))
    return {
      name: named === undefined ? step[0].trim() : named.replace(/^( {6}- | {8})name: /, ''),
      text: step.join('\n'),
    }
  })
}

const holdsKey = (step) => /\bsecrets\./.test(step.text)

describe('the release tool in the release workflow', () => {
  it('is built with the lockfile in a step of its own that holds no key', () => {
    const steps = macSteps()
    const builders = steps.filter((step) => step.text.includes(BUILD))
    assert.equal(builders.length, 1, `${builders.length} steps build the tool`)
    const [builder] = builders
    assert.ok(!holdsKey(builder), 'the step that builds the tool holds a key')
    assert.ok(!/\bcargo run\b/.test(builder.text))
    assert.equal(
      builder.text.trim().split('\n').length,
      2,
      'the step builds the tool and does nothing else',
    )
  })

  it('is built before every step that holds a key', () => {
    const steps = macSteps()
    const built = steps.findIndex((step) => step.text.includes(BUILD))
    const keyed = steps.flatMap((step, at) => (holdsKey(step) ? [at] : []))
    assert.ok(keyed.length >= 2, 'the steps that hold a key are found')
    for (const at of keyed) assert.ok(built < at, `${steps[at].name} comes before the build`)
  })

  it('never has a step that holds a key build or run anything through cargo', () => {
    for (const step of macSteps().filter(holdsKey)) {
      assert.ok(!/\bcargo\b/.test(step.text), `${step.name} goes through cargo with a key`)
    }
  })

  it("is what the notes step runs, by its path, to read the version and to make the feed's entry", () => {
    const steps = macSteps()
    const notes = steps.find((step) => step.name === 'Notes, and the update feed for this build')
    assert.ok(notes, 'the notes step is found')
    assert.ok(
      notes.text.includes(`release_tool="$GITHUB_WORKSPACE/${TOOL}"`),
      'the tool is named by its path',
    )
    assert.ok(notes.text.includes('version=$("$release_tool" version)'), 'the version is its')
    assert.ok(
      notes.text.includes('"$release_tool" prepare-update --repo "$PWD"'),
      'the entry is its',
    )
    assert.ok(!/\bcargo\b/.test(notes.text), 'the step goes through cargo')
    assert.ok(!notes.text.includes('prepare-update.mjs'), 'the step runs the script it replaced')
    assert.ok(
      steps.findIndex((step) => step.text.includes(BUILD)) < steps.indexOf(notes),
      'the tool is built before the step that runs it',
    )
  })
})
