import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { MANIFEST } from '../app/scripts/feeds.mjs'
import {
  archiveOf,
  builtFiles,
  folderOf,
  githubSim,
  latestJson,
  publishedAssets,
} from './github-sim.mjs'

/**
 * The release workflow (.github/workflows/release.yml): the steps that publish,
 * run as written, since a hand run of the workflow shows none of them (it
 * publishes nothing) and a slip in one is found on release day. The logic is
 * app/scripts/feeds.mjs's and app/scripts/publish.mjs's, held to its cases in
 * tests/feeds.test.mjs and tests/publish.test.mjs; here the workflow's own text
 * is held to calling it right: the steps' scripts, run by bash with a `gh` that
 * is GitHub as tests/github-sim.mjs has it, and the places the workflow says
 * who may publish.
 */

const REPO = fileURLToPath(new URL('..', import.meta.url))
const WORKFLOW = new URL('../.github/workflows/release.yml', import.meta.url)
const DOWNLOADS = 'https://github.com/$GITHUB_REPOSITORY/releases/download'
const PUBLISH = 'Publish the release, then its update feeds'
const CHECK = 'The feeds serve this release, and its archive downloads'
const PREREQUISITES = 'The old feeds serve the bridge, for a release after it'
const NOTES = 'Notes, and the update feed for this build'

const workflow = () => readFileSync(WORKFLOW, 'utf8')

/** The script of the step named `name`, as its `run: |` block holds it, read off the workflow's text. */
function stepScript(name) {
  const lines = workflow().split('\n')
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

/** The text of the job `name`, to the next job. */
function job(name) {
  const lines = workflow().split('\n')
  const start = lines.indexOf(`  ${name}:`)
  assert.notEqual(start, -1, `release.yml has no job ${name}`)
  const end = lines.findIndex((line, at) => at > start && /^ {2}\S/.test(line))
  return lines.slice(start, end === -1 ? undefined : end).join('\n')
}

const skip = process.platform === 'win32' && 'the release jobs run on macOS and Linux'

/** The step's script with a short patience given to its `feeds.mjs <command>`, which waits a minute for a feed that is not right. */
function quickly(script, command) {
  const patient = script.replace(
    `feeds.mjs ${command}`,
    `feeds.mjs ${command} --attempts 2 --wait 1`,
  )
  assert.notEqual(patient, script, `the step does not run feeds.mjs ${command}`)
  return patient
}

describe('who may publish', () => {
  it('is the push of a tag: not a hand run, though it is on a tag', () => {
    const condition = /^ {4}if: (.*)$/m.exec(job('publish'))?.[1]
    assert.ok(condition, 'the publish job has no condition')
    const needed = condition.split('&&').map((part) => part.trim())
    assert.ok(needed.includes("github.event_name == 'push'"), condition)
    assert.ok(needed.includes("github.ref_type == 'tag'"), condition)
    assert.ok(!/\|\|/.test(condition), 'no way round it')
  })

  it('is the push of a tag for the group of releases that go out one at a time, too', () => {
    const group = /^ {2}group: (.*)$/m.exec(workflow())?.[1]
    assert.ok(group, 'the workflow has no concurrency group')
    const [tagged, trial] = group
      .replace(/^\$\{\{ | \}\}$/g, '')
      .split('||')
      .map((part) => part.trim())
    assert.ok(tagged.includes("github.event_name == 'push'"), group)
    assert.ok(tagged.includes("github.ref_type == 'tag'"), group)
    assert.match(trial, /^format\('release-trial-\{0\}', github\.run_id\)$/)
  })

  it('keeps every call of gh in the publisher, which is tested: the workflow only runs it', () => {
    assert.ok(
      !/\bgh release\b/.test(workflow().replace(/^ *#.*$/gm, '')),
      'a gh release call in a step',
    )
    assert.ok(!workflow().includes('--clobber'), 'a file replaced by deleting it first')
  })

  it('has the publish job check out what it runs, and nothing else of the tree', () => {
    const checkout = job('publish')
    for (const file of ['app/scripts/feeds.mjs', 'app/scripts/publish.mjs', 'app/feeds.json']) {
      assert.ok(checkout.includes(`            ${file}\n`), `the sparse checkout lacks ${file}`)
    }
  })

  it('checks the feeds after it publishes them, and asks the old feeds before it builds', () => {
    const lines = workflow().split('\n')
    const at = (name) => lines.findIndex((line) => line.trim() === `- name: ${name}`)
    assert.ok(at(PUBLISH) !== -1 && at(PUBLISH) < at(CHECK), 'the check follows the publishing')
    const mac = job('mac')
    const steps = mac.split('\n')
    const first = (text) => steps.findIndex((line) => line.includes(text))
    assert.ok(first(`name: ${PREREQUISITES}`) !== -1, 'the Mac job has no early check')
    assert.ok(first(`name: ${PREREQUISITES}`) < first('- run: npm ci'), 'before the build')
    assert.ok(first(`name: ${PREREQUISITES}`) < first(`name: ${NOTES}`))
  })
})

/** Runs `script` with bash, `env` over a PATH that finds this Node and the given folders. */
function bash(script, { cwd, env, path = [] }) {
  const root = mkdtempSync(join(tmpdir(), 'cf-step-'))
  const file = join(root, 'step.sh')
  writeFileSync(file, script)
  return new Promise((done) => {
    const child = spawn('bash', [file], {
      cwd,
      env: {
        ...env,
        PATH: [...path, dirname(process.execPath), '/usr/bin', '/bin'].join(delimiter),
      },
    })
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', (chunk) => {
      stdout += chunk
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk
    })
    child.on('close', (status) => {
      rmSync(root, { recursive: true, force: true })
      done({ status, stdout, stderr })
    })
  })
}

/** A `gh` that asks the simulator, in this process, what to do: it is GitHub as the tests have it. */
const GH = `#!/usr/bin/env node
;(async () => {
  const args = process.argv.slice(2)
  const asked = await fetch(process.env.GH_SIM + '/__gh', {
    method: 'POST',
    body: JSON.stringify({ args, cwd: process.cwd() }),
  })
  const answer = await asked.json()
  process.stdout.write(answer.stdout)
  process.stderr.write(answer.stderr)
  process.exitCode = answer.status
})()
`

describe('the publish job, run as written', { skip }, () => {
  const OLD_LAYOUT = [
    'ConsensFlow.app/Contents/MacOS/node',
    'ConsensFlow.app/Contents/Resources/cli/package.json',
    'ConsensFlow.app/Contents/Resources/cli/bin/cf.mjs',
    'ConsensFlow.app/Contents/Resources/cli/bin/cf',
    'ConsensFlow.app/Contents/Resources/cli/hosts/pi-extension/consensflow-delivery.mjs',
    'ConsensFlow.app/Contents/Resources/cli/src/core/daemon.js',
  ]
  /** The bridge the repository records, built into a folder, its update-alpha serving an earlier release. */
  async function world() {
    const github = await githubSim()
    const version = MANIFEST.bridge.version
    github.release(MANIFEST.legacy.alpha, {
      prerelease: true,
      assets: { 'latest.json': latestJson(github.base, '3.0.0-alpha.1') },
    })
    const files = builtFiles(github.base, version)
    files[`ConsensFlow_${version}_aarch64.app.tar.gz`] = archiveOf(OLD_LAYOUT)
    const dist = folderOf(files)
    const root = mkdtempSync(join(tmpdir(), 'cf-gh-'))
    mkdirSync(join(root, 'bin'))
    writeFileSync(join(root, 'bin', 'gh'), GH)
    chmodSync(join(root, 'bin', 'gh'), 0o755)
    /** Runs a step of the publish job, in the folder the jobs built into. */
    const step = (name, env = {}) =>
      bash(stepScript(name).replaceAll(DOWNLOADS, github.base), {
        cwd: dist,
        path: [join(root, 'bin')],
        env: {
          GITHUB_WORKSPACE: REPO,
          GITHUB_REF_NAME: `v${version}`,
          GITHUB_REPOSITORY: github.repo,
          GH_REPO: github.repo,
          GITHUB_EVENT_NAME: 'push',
          GITHUB_REF_TYPE: 'tag',
          GH_SIM: github.base,
          ...env,
        },
      })
    return {
      github,
      version,
      step,
      done: async () => {
        rmSync(dist, { recursive: true, force: true })
        rmSync(root, { recursive: true, force: true })
        await github.close()
      },
    }
  }

  it('publishes the bridge and moves its feeds, and the step after it finds them right', async () => {
    const { github, version, step, done } = await world()
    try {
      const published = await step(PUBLISH)
      assert.equal(published.status, 0, `${published.stdout}${published.stderr}`)
      assert.match(
        published.stdout,
        new RegExp(
          `^publish: ${version.replaceAll('.', '\\.')}: the release was created; feed-alpha created, update-alpha replaced$`,
          'm',
        ),
      )
      assert.equal(github.isDraft(`v${version}`), false)
      for (const feed of ['feed-alpha', 'update-alpha']) {
        assert.deepEqual(github.names(feed), ['latest.json'], feed)
      }
      const checked = await step(CHECK)
      assert.equal(checked.status, 0, `${checked.stdout}${checked.stderr}`)
      assert.match(checked.stdout, /feeds: serve this release as the rule says/)
    } finally {
      await done()
    }
  })

  it('is run again after a failure, and finishes the work', async () => {
    const { github, version, step, done } = await world()
    try {
      github.fail((args) => args.includes(MANIFEST.legacy.alpha) && 'the network went away')
      const cut = await step(PUBLISH)
      assert.equal(cut.status, 1)
      assert.match(cut.stderr, /the network went away/)
      github.fail(() => undefined)
      const again = await step(PUBLISH)
      assert.equal(again.status, 0, `${again.stdout}${again.stderr}`)
      assert.match(again.stdout, /the release was kept; feed-alpha kept, update-alpha replaced/)
      assert.equal(
        github.calls.filter((args) => args[1] === 'create' && args[2] === `v${version}`).length,
        1,
      )
    } finally {
      await done()
    }
  })

  it('publishes nothing for a hand run on a tag: the step refuses of itself', async () => {
    const { github, step, done } = await world()
    try {
      const ran = await step(PUBLISH, { GITHUB_EVENT_NAME: 'workflow_dispatch' })
      assert.equal(ran.status, 1)
      assert.match(
        ran.stderr,
        /only the push of a version tag publishes; this is workflow_dispatch on tag/,
      )
      assert.deepEqual(github.calls, [])
    } finally {
      await done()
    }
  })
})

describe('the Mac job, as a hand run and as a tag run', { skip }, () => {
  const FEEDS = { alpha: 'feed-alpha', stable: 'feed-stable' }
  const LEGACY = { alpha: 'update-alpha', stable: 'update-stable' }
  const BRIDGE = '3.0.0-alpha.80'
  /** A checkout of the scripts and the manifest, at the version `package.json` says. */
  function project(version) {
    const root = mkdtempSync(join(tmpdir(), 'cf-mac-'))
    mkdirSync(join(root, 'app', 'scripts'), { recursive: true })
    writeFileSync(join(root, 'package.json'), JSON.stringify({ version }))
    copyFileSync(
      join(REPO, 'app', 'scripts', 'feeds.mjs'),
      join(root, 'app', 'scripts', 'feeds.mjs'),
    )
    writeFileSync(
      join(root, 'app', 'feeds.json'),
      JSON.stringify({
        feeds: FEEDS,
        legacy: LEGACY,
        bridge: { version: BRIDGE, legacy: ['alpha'] },
      }),
    )
    return root
  }
  const env = (event, github) => ({
    GITHUB_EVENT_NAME: event,
    GITHUB_REPOSITORY: github.repo,
  })

  it('asks the old feeds before the build: a tag is refused, a hand run is told what a tag would be', async () => {
    const github = await githubSim()
    // The bridge's tag is made and its release failed: update-alpha serves the release before it.
    github.release('update-alpha', {
      prerelease: true,
      assets: { 'latest.json': latestJson(github.base, '3.0.0-alpha.79') },
    })
    github.release(`v${BRIDGE}`, { assets: publishedAssets(builtFiles(github.base, BRIDGE)) })
    // The workflow gives the command no patience of its own: a read that does not serve the bridge is read again for a minute.
    const script = quickly(stepScript(PREREQUISITES), 'prerequisites').replaceAll(
      DOWNLOADS,
      github.base,
    )
    const root = project('3.0.0-alpha.81')
    try {
      const tag = await bash(script, { cwd: root, env: env('push', github) })
      assert.equal(tag.status, 1, `${tag.stdout}${tag.stderr}`)
      assert.match(tag.stderr, /^feeds: update-alpha serves 3\.0\.0-alpha\.79, not the bridge/m)

      const hand = await bash(script, { cwd: root, env: env('workflow_dispatch', github) })
      assert.equal(hand.status, 0, `${hand.stdout}${hand.stderr}`)
      assert.match(
        hand.stderr,
        /^feeds \(a tag would be refused\): update-alpha serves 3\.0\.0-alpha\.79/m,
      )

      // The bridge itself has nothing to ask, whatever the feeds serve.
      writeFileSync(join(root, 'package.json'), JSON.stringify({ version: BRIDGE }))
      const bridge = await bash(script, { cwd: root, env: env('push', github) })
      assert.equal(bridge.status, 0, `${bridge.stdout}${bridge.stderr}`)
      assert.match(bridge.stdout, /it is the bridge/)
    } finally {
      rmSync(root, { recursive: true, force: true })
      await github.close()
    }
  })

  it('plans the feeds from the archive: a tag is refused for a release before the bridge, a hand run is told', async () => {
    const dir = folderOf({ 'archive.tar.gz': archiveOf(['ConsensFlow.app/Contents/MacOS/app']) })
    const notes = stepScript(NOTES)
    const plan = notes.slice(notes.indexOf('dry=()'))
    assert.ok(plan.includes('feeds.mjs plan'), 'the step plans the feeds')
    const root = project('3.0.0-alpha.79')
    const github = await githubSim()
    const run = (event, version) =>
      bash(`set -euo pipefail\nversion=${version}\nout=${dir}\narchive=archive.tar.gz\n${plan}`, {
        cwd: root,
        env: env(event, github),
      })
    try {
      const tag = await run('push', '3.0.0-alpha.79')
      assert.equal(tag.status, 1, `${tag.stdout}${tag.stderr}`)
      assert.match(
        tag.stderr,
        /^feeds: 3\.0\.0-alpha\.79 comes before the bridge 3\.0\.0-alpha\.80/m,
      )
      assert.equal(tag.stdout, '')

      const hand = await run('workflow_dispatch', '3.0.0-alpha.79')
      assert.equal(hand.status, 0, `${hand.stdout}${hand.stderr}`)
      assert.match(
        hand.stderr,
        /^feeds \(a tag would be refused\): 3\.0\.0-alpha\.79 comes before the bridge/m,
      )

      const later = await run('push', '3.0.0-alpha.81')
      assert.equal(later.status, 0, `${later.stdout}${later.stderr}`)
      assert.equal(later.stdout, 'feed-alpha\n')
    } finally {
      rmSync(dir, { recursive: true, force: true })
      rmSync(root, { recursive: true, force: true })
      await github.close()
    }
  })
})
