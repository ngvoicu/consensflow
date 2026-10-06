import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * The shell scripts the workflows run, parsed. A hand run of a workflow shows a
 * step's syntax only once it gets there (the publish step never, short of a tag),
 * so a slip in one is found on release day; `bash -n` finds it here. PowerShell's
 * steps (`shell: pwsh`) are the Windows runner's to parse.
 */

const WORKFLOWS = fileURLToPath(new URL('../.github/workflows/', import.meta.url))
const STEP = /^ {6}- /
const RUN = /^( {6}- | {8})run: /

/** The `run` of each step of a workflow that bash runs, as `{ name, script }`, read off the text. */
function bashSteps(text) {
  const lines = text.split('\n')
  const starts = lines.flatMap((line, at) => (STEP.test(line) ? [at] : []))
  const steps = []
  for (const [index, start] of starts.entries()) {
    const body = lines.slice(start, starts[index + 1] ?? lines.length)
    if (body.some((line) => /^ {8}shell: pwsh$/.test(line))) continue
    const run = body.findIndex((line) => RUN.test(line))
    if (run === -1) continue
    const first = body[run].replace(RUN, '')
    const named = body.find((line) => /^( {6}- | {8})name: /.test(line))
    const name = named === undefined ? first : named.replace(/^( {6}- | {8})name: /, '')
    if (first !== '|') {
      steps.push({ name, script: first })
      continue
    }
    const script = []
    let indent = null
    for (const line of body.slice(run + 1)) {
      const own = line.length - line.trimStart().length
      if (line.trim() !== '') {
        indent ??= own
        if (own < indent) break
      }
      script.push(line.trim() === '' ? '' : line.slice(indent))
    }
    steps.push({ name, script: script.join('\n') })
  }
  return steps
}

const hasBash = spawnSync('bash', ['--version']).error === undefined

describe('the scripts of the workflows', { skip: !hasBash && 'there is no bash here' }, () => {
  it('are read off every workflow, the publish step among them', () => {
    const release = bashSteps(readFileSync(join(WORKFLOWS, 'release.yml'), 'utf8'))
    assert.ok(release.length >= 10, `release.yml: ${release.length} bash steps found`)
    for (const name of [
      'Notes, and the update feed for this build',
      'Publish the release, then its update feeds',
      'The feeds serve this release, and its archive downloads',
    ]) {
      assert.ok(
        release.some((step) => step.name === name),
        `release.yml has no bash step named ${name}`,
      )
    }
    const windows = bashSteps(readFileSync(join(WORKFLOWS, 'windows-build.yml'), 'utf8'))
    assert.ok(
      windows.every((step) => !step.name.includes('portable exe')),
      'the PowerShell step is not read as bash',
    )
  })

  it('all parse as bash (bash -n)', () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-workflows-'))
    try {
      for (const file of readdirSync(WORKFLOWS).filter((name) => name.endsWith('.yml'))) {
        for (const [index, { name, script }] of bashSteps(
          readFileSync(join(WORKFLOWS, file), 'utf8'),
        ).entries()) {
          const path = join(dir, `${file}-${index}.sh`)
          writeFileSync(path, script)
          const checked = spawnSync('bash', ['-n', path], { encoding: 'utf8' })
          assert.equal(checked.status, 0, `${file}: ${name}\n${checked.stderr}`)
        }
      }
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
