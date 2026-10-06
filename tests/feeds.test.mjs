import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  channelsOf,
  checkFeeds,
  isFlip,
  membersOf,
  missingForOldApps,
  planFeeds,
} from '../app/scripts/feeds.mjs'

const SCRIPT = fileURLToPath(new URL('../app/scripts/feeds.mjs', import.meta.url))
const MANIFEST = JSON.parse(readFileSync(new URL('../app/feeds.json', import.meta.url), 'utf8'))
const ROOT = 'ConsensFlow.app/Contents/'
/** What an archive the apps before the flip release install holds, as `tar -t` lists it. */
const OLD_LAYOUT = [
  `${ROOT}MacOS/app`,
  `${ROOT}MacOS/node`,
  `${ROOT}Resources/cli/package.json`,
  `${ROOT}Resources/cli/bin/cf.mjs`,
  `${ROOT}Resources/cli/bin/cf`,
  `${ROOT}Resources/cli/hosts/pi-extension/consensflow-delivery.mjs`,
  `${ROOT}Resources/cli/src/core/daemon.js`,
]
/** The same without a byte of Node's: what the release after the flip holds. */
const NODE_FREE = [`${ROOT}MacOS/app`, `${ROOT}Resources/cli/bin/cf`]
const needsTar = { skip: process.platform === 'win32' && 'the release pipeline is macOS and Linux' }

describe('the feeds a release moves', () => {
  it('names the new feeds and the old ones apart, in the manifest the app reads', () => {
    assert.deepEqual(MANIFEST, {
      feeds: { alpha: 'feed-alpha', stable: 'feed-stable' },
      legacy: { alpha: 'update-alpha', stable: 'update-stable' },
    })
  })

  it('takes every release into alpha, and a stable one into stable as well', () => {
    assert.deepEqual(channelsOf('3.0.0-alpha.80'), ['alpha'])
    assert.deepEqual(channelsOf('3.0.0'), ['alpha', 'stable'])
    assert.throws(() => channelsOf('v3.0.0'), /not a semantic version/)
    assert.throws(() => channelsOf('3.0'), /not a semantic version/)
  })

  it('moves the new feed of each channel for a later release, and not the old ones', () => {
    assert.deepEqual(planFeeds({ version: '3.0.0-alpha.81', flip: false, missing: [] }), [
      'feed-alpha',
    ])
    assert.deepEqual(planFeeds({ version: '3.0.1', flip: false, missing: [] }), [
      'feed-alpha',
      'feed-stable',
    ])
  })

  it('moves the old feeds of its channels as well for the flip release, the new ones first', () => {
    assert.deepEqual(planFeeds({ version: '3.0.0-alpha.80', flip: true, missing: [] }), [
      'feed-alpha',
      'update-alpha',
    ])
    assert.deepEqual(planFeeds({ version: '3.0.0', flip: true, missing: [] }), [
      'feed-alpha',
      'feed-stable',
      'update-alpha',
      'update-stable',
    ])
  })

  it('refuses a flip release the old apps would download and then refuse', () => {
    const missing = missingForOldApps(NODE_FREE)
    assert.throws(
      () => planFeeds({ version: '3.0.0-alpha.80', flip: true, missing }),
      /must keep the layout.*MacOS\/node/,
    )
    // A later release may be laid out any way: the old feeds are not its to move.
    assert.deepEqual(planFeeds({ version: '3.0.0-alpha.90', flip: false, missing }), ['feed-alpha'])
  })
})

describe('what the apps before the flip release require of an archive', () => {
  it('is nothing missing from the layout they install', () => {
    assert.deepEqual(missingForOldApps(OLD_LAYOUT), [])
    assert.deepEqual(
      missingForOldApps(OLD_LAYOUT.map((name) => `./${name}`)),
      [],
      'as tar lists them with a leading ./',
    )
  })

  it('names each file or folder that is not there', () => {
    for (const [name, gone] of [
      ['MacOS/node', (n) => n !== `${ROOT}MacOS/node`],
      ['package.json', (n) => !n.endsWith('/package.json')],
      ['cf.mjs', (n) => !n.endsWith('/cf.mjs')],
      ['bin/cf', (n) => !n.endsWith('/bin/cf')],
      ['hosts', (n) => !n.includes('/hosts/')],
      ['src', (n) => !n.includes('/src/')],
    ]) {
      const missing = missingForOldApps(OLD_LAYOUT.filter(gone))
      assert.equal(missing.length, 1, `${name}: ${missing}`)
      assert.ok(missing[0].endsWith(name) || missing[0].endsWith(`${name}/`), `${name}: ${missing}`)
    }
  })

  it('is read off a real archive', needsTar, () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
    try {
      assert.deepEqual(missingForOldApps(membersOf(archive(dir, 'old.tar.gz', OLD_LAYOUT))), [])
      const missing = missingForOldApps(membersOf(archive(dir, 'free.tar.gz', NODE_FREE)))
      assert.equal(missing.length, 5, `${missing}`)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

/** A ustar archive of `files` under `dir`, made as the release makes its own. */
function archive(dir, name, files) {
  const tree = join(dir, `tree-${name}`)
  for (const file of files) {
    mkdirSync(dirname(join(tree, file)), { recursive: true })
    writeFileSync(join(tree, file), file)
  }
  const target = join(dir, name)
  execFileSync('tar', ['--format', 'ustar', '-czf', target, '-C', tree, 'ConsensFlow.app'], {
    env: { ...process.env, COPYFILE_DISABLE: '1' },
  })
  return target
}

/** A repository whose history is: no manifest (v1), the manifest added (v2), more (v3). */
function repository(dir) {
  mkdirSync(dir, { recursive: true })
  const run = (...args) =>
    execFileSync(
      'git',
      ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args],
      { cwd: dir, stdio: 'ignore' },
    )
  run('init', '-q')
  const commit = (file, tag) => {
    mkdirSync(dirname(join(dir, file)), { recursive: true })
    writeFileSync(join(dir, file), tag)
    run('add', file)
    run('commit', '-q', '-m', tag)
    run('tag', tag)
  }
  commit('README', 'v1')
  commit('app/feeds.json', 'v2')
  commit('README', 'v3')
  return dir
}

describe('the flip release', () => {
  it('is the first release whose predecessor held no manifest', () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
    try {
      const repo = repository(dir)
      assert.equal(isFlip({ repo, previous: 'v1' }), true)
      assert.equal(isFlip({ repo, previous: 'v2' }), false)
      assert.equal(isFlip({ repo, previous: 'v3' }), false)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('is not guessed when the release before it cannot be found', () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
    try {
      const repo = repository(dir)
      assert.throws(() => isFlip({ repo, previous: '' }), /fetch the tags/)
      assert.throws(() => isFlip({ repo, previous: undefined }), /fetch the tags/)
      assert.throws(() => isFlip({ repo, previous: 'v9' }), /v9 is not in this repository/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('is planned from the command line: the feeds, one per line, or a refusal', needsTar, () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
    try {
      const repo = repository(join(dir, 'repo'))
      const old = archive(dir, 'old.tar.gz', OLD_LAYOUT)
      const free = archive(dir, 'free.tar.gz', NODE_FREE)
      const plan = (version, archivePath, previous) =>
        spawnSync(
          process.execPath,
          [
            SCRIPT,
            'plan',
            ...['--version', version, '--archive', archivePath, '--previous', previous],
            ...['--repo', repo],
          ],
          { encoding: 'utf8' },
        )
      const flip = plan('3.0.0-alpha.80', old, 'v1')
      assert.equal(flip.status, 0, flip.stderr)
      assert.equal(flip.stdout, 'feed-alpha\nupdate-alpha\n')
      assert.match(flip.stderr, /the flip release/)

      const later = plan('3.0.0-alpha.81', free, 'v2')
      assert.equal(later.status, 0, later.stderr)
      assert.equal(later.stdout, 'feed-alpha\n')
      assert.match(later.stderr, /a later release/)

      const refused = plan('3.0.0-alpha.80', free, 'v1')
      assert.equal(refused.status, 1)
      assert.equal(refused.stdout, '', 'nothing is named to move')
      assert.match(refused.stderr, /must keep the layout/)

      const unknown = plan('3.0.0-alpha.80', old, '')
      assert.equal(unknown.status, 1)
      assert.match(unknown.stderr, /fetch the tags/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

/**
 * GitHub's download address, on this machine. `build` is given the address
 * once it is known and answers the routes: a path mapped to what it serves (a
 * string or a buffer), or to a function of how many times it has been asked
 * for; a path that is not there, or is mapped to nothing, is not found.
 */
async function github(build) {
  let routes = {}
  const server = createServer((request, response) => {
    const route = routes[request.url]
    const body = typeof route === 'function' ? route() : route
    if (body === undefined) {
      response.writeHead(404).end()
      return
    }
    response.writeHead(request.headers.range === undefined ? 200 : 206).end(body)
  })
  await new Promise((done) => server.listen(0, '127.0.0.1', done))
  const base = `http://127.0.0.1:${server.address().port}`
  routes = build(base)
  return {
    base,
    close: () => {
      // The client keeps its connections alive; the server would wait for them.
      server.closeAllConnections()
      return new Promise((done) => server.close(done))
    },
  }
}

/** A release's `latest.json`, naming an archive under `base`. */
function latest(base, version) {
  return JSON.stringify({
    version,
    notes: 'notes',
    pub_date: '2026-10-06T12:00:00Z',
    platforms: {
      'darwin-aarch64': {
        url: `${base}/v${version}/ConsensFlow_${version}_aarch64.app.tar.gz`,
        signature: 'signed',
      },
    },
  })
}

describe('the feeds as the release workflow verifies them', () => {
  const VERSION = '3.0.0-alpha.80'
  const OLDER = '3.0.0-alpha.79'
  const ARCHIVE = Buffer.from('the update archive')
  const archivePath = (version) => `/v${version}/ConsensFlow_${version}_aarch64.app.tar.gz`

  /** What a release's dist folder holds when its feeds are checked. */
  function dist(base, moved) {
    const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
    writeFileSync(join(dir, 'feeds.txt'), `${moved.join('\n')}\n`)
    writeFileSync(join(dir, 'latest.json'), latest(base, VERSION))
    writeFileSync(join(dir, `ConsensFlow_${VERSION}_aarch64.app.tar.gz`), ARCHIVE)
    return dir
  }

  /**
   * The feeds of `serving` (a feed mapped to the version it serves) checked
   * after publishing `moved`; `changes` alters what the server holds.
   */
  async function check(serving, moved, changes = () => ({}), options = {}) {
    let folder
    const server = await github((base) => {
      folder = dist(base, moved)
      const held = {
        [archivePath(VERSION)]: ARCHIVE,
        [archivePath(OLDER)]: 'the older archive',
      }
      for (const [feed, version] of Object.entries(serving)) {
        held[`/${feed}/latest.json`] = latest(base, version)
      }
      return { ...held, ...changes(base) }
    })
    try {
      return await checkFeeds({
        dir: folder,
        version: VERSION,
        base: server.base,
        attempts: 3,
        wait: 5,
        ...options,
      })
    } finally {
      rmSync(folder, { recursive: true, force: true })
      await server.close()
    }
  }

  it('passes the flip release: its new and its old feeds all serve it', async () => {
    const problems = await check({ 'feed-alpha': VERSION, 'update-alpha': VERSION }, [
      'feed-alpha',
      'update-alpha',
    ])
    assert.deepEqual(problems, [])
  })

  it('passes a later release: the new feed serves it, the old one is pinned and served', async () => {
    const problems = await check({ 'feed-alpha': VERSION, 'update-alpha': OLDER }, ['feed-alpha'])
    assert.deepEqual(problems, [])
  })

  it('passes where an old feed of a channel not in use is not there at all', async () => {
    const problems = await check({ 'feed-alpha': VERSION }, ['feed-alpha'])
    assert.deepEqual(problems, [])
  })

  it('waits out a feed that is served stale for a moment', async () => {
    let asked = 0
    const problems = await check({ 'update-alpha': OLDER }, ['feed-alpha'], (base) => ({
      '/feed-alpha/latest.json': () => {
        asked += 1
        return latest(base, asked < 3 ? OLDER : VERSION)
      },
    }))
    assert.deepEqual(problems, [])
    assert.equal(asked, 3, 'read until it served this release')
  })

  it('finds a feed that keeps serving another release', async () => {
    const problems = await check({ 'feed-alpha': OLDER, 'update-alpha': OLDER }, ['feed-alpha'])
    assert.deepEqual(problems, ["feed-alpha does not serve this release's latest.json"])
  })

  it('finds a feed that is not there', async () => {
    const problems = await check({ 'update-alpha': OLDER }, ['feed-alpha'])
    assert.deepEqual(problems, ["feed-alpha does not serve this release's latest.json"])
  })

  it('finds an archive that does not download as the one built', async () => {
    const problems = await check({ 'feed-alpha': VERSION }, ['feed-alpha'], () => ({
      [archivePath(VERSION)]: 'another archive',
    }))
    assert.equal(problems.length, 1)
    assert.match(problems[0], /does not download as the one built/)
  })

  it('finds an old feed that serves this release though the rule does not name it', async () => {
    const problems = await check({ 'feed-alpha': VERSION, 'update-alpha': VERSION }, ['feed-alpha'])
    assert.deepEqual(problems, [
      'update-alpha serves this release, which the rule does not move it to',
    ])
  })

  it('finds a pinned release whose archive is gone', async () => {
    const problems = await check(
      { 'feed-alpha': VERSION, 'update-alpha': OLDER },
      ['feed-alpha'],
      () => ({ [archivePath(OLDER)]: undefined }),
    )
    assert.deepEqual(problems, [
      `update-alpha is pinned to ${OLDER}, whose archive is no longer served`,
    ])
  })

  it('finds an old feed that serves something that is not a latest.json', async () => {
    const problems = await check({ 'feed-alpha': VERSION }, ['feed-alpha'], () => ({
      '/update-alpha/latest.json': '<html>an error page</html>',
    }))
    assert.deepEqual(problems, ['update-alpha serves something that is not a latest.json'])
  })

  it('finds a pinned release that names no archive for the Mac', async () => {
    const problems = await check({ 'feed-alpha': VERSION }, ['feed-alpha'], () => ({
      '/update-alpha/latest.json': JSON.stringify({ version: OLDER, platforms: {} }),
    }))
    assert.deepEqual(problems, [
      `update-alpha is pinned to ${OLDER}, whose archive is no longer served`,
    ])
  })

  it('says what a command lacks, and does nothing', () => {
    for (const [command, lacks] of [
      ['plan', /plan needs --version, --archive and --previous/],
      ['check', /check needs --dir, --version and --base/],
      ['publish', /usage: feeds\.mjs plan\|check/],
    ]) {
      const run = spawnSync(process.execPath, [SCRIPT, command], { encoding: 'utf8' })
      assert.equal(run.status, 1, command)
      assert.match(run.stderr, lacks, command)
      assert.equal(run.stdout, '', command)
    }
  })

  it('is run from the command line, and says what it found', async () => {
    let folder
    const server = await github((base) => {
      folder = dist(base, ['feed-alpha'])
      return {
        [archivePath(VERSION)]: ARCHIVE,
        '/feed-alpha/latest.json': latest(base, VERSION),
      }
    })
    try {
      // Not spawnSync: this process is the server the script reads from.
      const run = (feedsFile) => {
        writeFileSync(join(folder, 'feeds.txt'), feedsFile)
        return new Promise((done) => {
          const child = spawn(process.execPath, [
            SCRIPT,
            'check',
            ...['--dir', folder, '--version', VERSION, '--base', server.base],
            ...['--attempts', '1', '--wait', '1'],
          ])
          let stdout = ''
          let stderr = ''
          child.stdout.on('data', (chunk) => {
            stdout += chunk
          })
          child.stderr.on('data', (chunk) => {
            stderr += chunk
          })
          child.on('close', (status) => done({ status, stdout, stderr }))
        })
      }
      const passed = await run('feed-alpha\n')
      assert.equal(passed.status, 0, passed.stderr)
      assert.match(passed.stdout, /serve this release as the rule says/)
      const failed = await run('feed-alpha\nupdate-alpha\n')
      assert.equal(failed.status, 1)
      assert.match(failed.stderr, /update-alpha does not serve this release's latest\.json/)
    } finally {
      rmSync(folder, { recursive: true, force: true })
      await server.close()
    }
  })
})
