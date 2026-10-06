import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'

/**
 * The publish step of the release workflow, run as written against a `gh` that
 * records what it is asked: the rule of the feeds (app/scripts/feeds.mjs plans
 * them in the Mac job and writes feeds.txt) applied by the workflow itself, which
 * no hand run of the workflow shows: it publishes nothing.
 */

const WORKFLOW = new URL('../.github/workflows/release.yml', import.meta.url)
const STEP = 'Publish the release, then its update feeds'

/** The script of the step named `name`, as its `run: |` block holds it, read off the workflow's text. */
function stepScript(name) {
  const lines = readFileSync(WORKFLOW, 'utf8').split('\n')
  const start = lines.findIndex((line) => line.trim() === `- name: ${name}`)
  assert.notEqual(start, -1, `release.yml has no step named ${name}`)
  const run = lines.findIndex((line, at) => at > start && line.trim() === 'run: |')
  assert.ok(
    run !== -1 && !lines.slice(start + 1, run).some((line) => /^\s*- /.test(line)),
    `the step ${name} has no run block`,
  )
  const body = []
  let indent = null
  for (const line of lines.slice(run + 1)) {
    const own = line.length - line.trimStart().length
    if (line.trim() !== '') {
      indent ??= own
      if (own < indent) break
    }
    body.push(line.trim() === '' ? '' : line.slice(indent))
  }
  return body.join('\n')
}

// The publish job runs on Linux; its script is kept to what bash 3, the Mac's, runs too.
const skip = process.platform === 'win32' && 'the publish job runs on Linux'

/** A `gh` that logs its arguments, one JSON line each, and says which releases exist. */
const GH = `#!/usr/bin/env node
const { appendFileSync } = require('node:fs')
const args = process.argv.slice(2)
appendFileSync(process.env.GH_LOG, JSON.stringify(args) + '\\n')
if (args[0] === 'release' && args[1] === 'view') {
  process.exit(process.env.GH_EXISTING.split(' ').includes(args[2]) ? 0 : 1)
}
`
const SHA256SUM = `#!/bin/sh
if [ -x /usr/bin/sha256sum ]; then exec /usr/bin/sha256sum "$@"; fi
exec shasum -a 256 "$@"
`

/**
 * Runs the step for the tag `v<version>` with the feeds the Mac job planned
 * (`feeds`, or none: no feeds.txt), where the releases `existing` are on GitHub
 * already: what `gh` was asked, and the files the step wrote.
 */
function publish({ version, feeds, existing = [] }) {
  const root = mkdtempSync(join(tmpdir(), 'cf-publish-'))
  const dist = join(root, 'dist')
  const bin = join(root, 'bin')
  const log = join(root, 'gh.log')
  mkdirSync(join(dist, 'nsis'), { recursive: true })
  mkdirSync(join(dist, 'portable'), { recursive: true })
  mkdirSync(bin)
  for (const [name, text] of [
    ['gh', GH],
    ['sha256sum', SHA256SUM],
  ]) {
    writeFileSync(join(bin, name), text)
    chmodSync(join(bin, name), 0o755)
  }
  const mac = `ConsensFlow_${version}_aarch64`
  for (const file of [
    'notes.txt',
    'latest.json',
    `${mac}.dmg`,
    `${mac}.app.tar.gz`,
    `${mac}.app.tar.gz.sig`,
    `nsis/ConsensFlow_${version}_x64-setup.exe`,
    `portable/ConsensFlow_${version}_x64-portable.exe`,
  ]) {
    writeFileSync(join(dist, file), `${file}\n`)
  }
  if (feeds !== undefined) writeFileSync(join(dist, 'feeds.txt'), `${feeds.join('\n')}\n`)
  const script = join(root, 'step.sh')
  writeFileSync(script, stepScript(STEP))
  const ran = spawnSync('bash', [script], {
    cwd: dist,
    encoding: 'utf8',
    env: {
      ...process.env,
      PATH: `${bin}:${process.env.PATH}`,
      GITHUB_REF_NAME: `v${version}`,
      GH_LOG: log,
      GH_EXISTING: existing.join(' '),
    },
  })
  const asked = existsSync(log)
    ? readFileSync(log, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line))
    : []
  const body = existsSync(join(dist, 'body.md')) ? readFileSync(join(dist, 'body.md'), 'utf8') : ''
  rmSync(root, { recursive: true, force: true })
  return { ran, asked, body, calls: asked.map(([, what, tag]) => [what, tag]) }
}

/** The arguments that follow `flag` in one `gh` call. */
const after = (call, flag) => call[call.indexOf(flag) + 1]

describe('the workflow publishes the feeds its Mac job planned', { skip }, () => {
  it('moves the new feed and the old one for the flip release, and marks the old one pinned', () => {
    const { ran, asked, calls } = publish({
      version: '3.0.0-alpha.80',
      feeds: ['feed-alpha', 'update-alpha'],
      existing: ['update-alpha'],
    })
    assert.equal(ran.status, 0, ran.stderr)
    assert.deepEqual(calls, [
      ['create', 'v3.0.0-alpha.80'],
      ['view', 'feed-alpha'],
      ['create', 'feed-alpha'],
      ['upload', 'feed-alpha'],
      ['view', 'update-alpha'],
      ['upload', 'update-alpha'],
      ['edit', 'update-alpha'],
    ])
    assert.ok(asked[0].includes('--prerelease') && asked[0].includes('--verify-tag'))
    assert.equal(
      after(asked[2], '--title'),
      'Alpha update feed',
      'the new feed is made as the feed',
    )
    assert.ok(asked[2].includes('--prerelease'))
    for (const upload of [asked[3], asked[5]]) {
      assert.deepEqual(upload.slice(3), ['latest.json', '--clobber'])
    }
    assert.equal(after(asked[6], '--title'), 'Alpha update feed, pinned')
    const notes = after(asked[6], '--notes')
    assert.match(
      notes,
      /Pinned to ConsensFlow 3\.0\.0-alpha\.80, the first release to read feed-alpha/,
    )
    assert.match(notes, /from it on read feed-alpha/)
  })

  it('moves the new feed alone for a later release: the old feed is not so much as asked about', () => {
    const { ran, asked, calls } = publish({
      version: '3.0.0-alpha.81',
      feeds: ['feed-alpha'],
      existing: ['feed-alpha', 'update-alpha'],
    })
    assert.equal(ran.status, 0, ran.stderr)
    assert.deepEqual(calls, [
      ['create', 'v3.0.0-alpha.81'],
      ['view', 'feed-alpha'],
      ['upload', 'feed-alpha'],
    ])
    assert.ok(!JSON.stringify(asked).includes('update-'), JSON.stringify(asked))
  })

  it('moves every channel of a stable flip release, each old feed marked pinned', () => {
    const { ran, asked, calls, body } = publish({
      version: '3.0.0',
      feeds: ['feed-alpha', 'feed-stable', 'update-alpha', 'update-stable'],
      existing: ['feed-alpha', 'update-alpha'],
    })
    assert.equal(ran.status, 0, ran.stderr)
    assert.deepEqual(calls, [
      ['create', 'v3.0.0'],
      ['view', 'feed-alpha'],
      ['upload', 'feed-alpha'],
      ['view', 'feed-stable'],
      ['create', 'feed-stable'],
      ['upload', 'feed-stable'],
      ['view', 'update-alpha'],
      ['upload', 'update-alpha'],
      ['edit', 'update-alpha'],
      ['view', 'update-stable'],
      ['create', 'update-stable'],
      ['upload', 'update-stable'],
      ['edit', 'update-stable'],
    ])
    assert.ok(!asked[0].includes('--prerelease'), 'a stable release is not a prerelease')
    assert.equal(after(asked[4], '--title'), 'Stable update feed')
    assert.equal(after(asked[12], '--title'), 'Stable update feed, pinned')
    assert.match(body, /on the Stable channel/)
  })

  it('publishes nothing when the plan is missing', () => {
    const { ran, asked, body } = publish({ version: '3.0.0-alpha.80' })
    assert.equal(ran.status, 1)
    assert.match(ran.stdout, /missing feeds\.txt/)
    assert.deepEqual(asked, [], 'not so much as the versioned release')
    assert.equal(body, '')
  })

  it('tells where the portable exe unpacks its runtime', () => {
    const { body } = publish({
      version: '3.0.0-alpha.80',
      feeds: ['feed-alpha'],
      existing: ['feed-alpha'],
    })
    assert.match(body, /%LOCALAPPDATA%\\dev\.ngvoicu\.consensflow\\portable-runtime/)
  })
})
