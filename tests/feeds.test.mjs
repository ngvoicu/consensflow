import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  channelsOf,
  checkFeeds,
  checkManifest,
  checkPrerequisites,
  compareVersions,
  feedsMoved,
  MANIFEST,
  membersOf,
  missingForOldApps,
  planFeeds,
  releaseAssets,
  roleOf,
} from '../app/scripts/feeds.mjs'
import {
  archiveOf,
  builtFiles,
  folderOf,
  githubSim,
  latestJson,
  publishedAssets,
} from './github-sim.mjs'

const SCRIPT = fileURLToPath(new URL('../app/scripts/feeds.mjs', import.meta.url))
const ROOT = 'ConsensFlow.app/Contents/'
/** What an archive the apps before the bridge install holds, as `tar -t` lists it. */
const OLD_LAYOUT = [
  `${ROOT}MacOS/app`,
  `${ROOT}MacOS/node`,
  `${ROOT}Resources/cli/package.json`,
  `${ROOT}Resources/cli/bin/cf.mjs`,
  `${ROOT}Resources/cli/bin/cf`,
  `${ROOT}Resources/cli/hosts/pi-extension/consensflow-delivery.mjs`,
  `${ROOT}Resources/cli/src/core/daemon.js`,
]
/** The same without a byte of Node's: what the release after the bridge may hold. */
const NODE_FREE = [`${ROOT}MacOS/app`, `${ROOT}Resources/cli/bin/cf`]
const needsTar = { skip: process.platform === 'win32' && 'the release pipeline is macOS and Linux' }

/**
 * The rule these tests hold the code to, whatever bridge the repository's
 * manifest names today. alpha.80 is built from the code before the bridge: an
 * ordinary release on the old feed, which is what the old feed serves until the
 * bridge reaches it.
 */
const BRIDGE = '3.0.0-alpha.81'
const OLDER = '3.0.0-alpha.80'
const LATER = '3.0.0-alpha.82'
const RULE = checkManifest({
  feeds: { alpha: 'feed-alpha', stable: 'feed-stable' },
  legacy: { alpha: 'update-alpha', stable: 'update-stable' },
  bridge: { version: BRIDGE, legacy: ['alpha'] },
})
const QUICK = { attempts: 2, wait: 1 }

describe('the manifest the app and the release workflow read', () => {
  it('names the feeds of both generations, as an installed app reads them', () => {
    assert.deepEqual(MANIFEST.feeds, { alpha: 'feed-alpha', stable: 'feed-stable' })
    assert.deepEqual(MANIFEST.legacy, { alpha: 'update-alpha', stable: 'update-stable' })
  })

  it('records the bridge, and the old channels in use: alpha, for stable has no release yet', () => {
    assert.match(MANIFEST.bridge.version, /^3\.0\.0-alpha\.\d+$/)
    assert.deepEqual(MANIFEST.bridge.legacy, ['alpha'])
  })

  it('refuses a manifest that does not say what the rule needs', () => {
    const made = (change) => {
      const manifest = JSON.parse(JSON.stringify(RULE))
      change(manifest)
      return manifest
    }
    for (const [change, says] of [
      [(m) => delete m.feeds.stable, /feeds\.stable names no feed/],
      [(m) => delete m.legacy.alpha, /legacy\.alpha names no feed/],
      [(m) => Object.assign(m.legacy, { alpha: 'feed-alpha' }), /a feed is named twice/],
      [(m) => delete m.bridge, /bridge\.version names no release/],
      [(m) => Object.assign(m.bridge, { version: 'v3.0.0' }), /not a semantic version/],
      [(m) => Object.assign(m.bridge, { legacy: [] }), /bridge\.legacy names the channels in use/],
      [
        (m) => Object.assign(m.bridge, { legacy: ['stable'] }),
        /bridge\.legacy names the channels in use that 3\.0\.0-alpha\.81 belongs to \(alpha\)/,
      ],
    ]) {
      assert.throws(() => checkManifest(made(change)), says)
    }
    assert.equal(checkManifest(made(() => {})).bridge.version, BRIDGE)
  })
})

describe('which release comes before which', () => {
  it('orders versions as semantic versioning does', () => {
    const ascending = [
      '1.0.0-alpha',
      '1.0.0-alpha.1',
      '1.0.0-alpha.beta',
      '1.0.0-beta',
      '1.0.0-beta.2',
      '1.0.0-beta.11',
      '1.0.0-rc.1',
      '1.0.0',
      '1.0.1',
      '1.1.0',
      '2.0.0',
      '10.0.0',
    ]
    for (const [at, version] of ascending.entries()) {
      assert.equal(compareVersions(version, version), 0, version)
      for (const larger of ascending.slice(at + 1)) {
        assert.equal(compareVersions(version, larger), -1, `${version} < ${larger}`)
        assert.equal(compareVersions(larger, version), 1, `${larger} > ${version}`)
      }
    }
    assert.equal(compareVersions('3.0.0-alpha.9', '3.0.0-alpha.10'), -1)
    assert.equal(compareVersions(BRIDGE, '3.0.0'), -1)
    assert.equal(compareVersions(OLDER, BRIDGE), -1, 'the release before the bridge')
    assert.equal(compareVersions(LATER, BRIDGE), 1, 'the release after it')
    assert.throws(() => compareVersions('3.0', '3.0.0'), /not a semantic version/)
  })
})

describe('the feeds a release moves', () => {
  it('takes every release into alpha, and a stable one into stable as well', () => {
    assert.deepEqual(channelsOf(BRIDGE), ['alpha'])
    assert.deepEqual(channelsOf('3.0.0'), ['alpha', 'stable'])
    assert.throws(() => channelsOf('v3.0.0'), /not a semantic version/)
    assert.throws(() => channelsOf('3.0'), /not a semantic version/)
  })

  it('tells the bridge from a later release, and refuses one before it', () => {
    assert.equal(roleOf(BRIDGE, RULE), 'bridge')
    assert.equal(roleOf(LATER, RULE), 'later')
    assert.equal(roleOf('3.0.0', RULE), 'later')
    assert.throws(
      () => roleOf(OLDER, RULE),
      /3\.0\.0-alpha\.80 comes before the bridge 3\.0\.0-alpha\.81.*set bridge\.version to the first release made from this tree/,
    )
  })

  it('moves the bridge to the new feed and the old feed of the channel in use, the new first', () => {
    assert.deepEqual(planFeeds({ version: BRIDGE, missing: [], manifest: RULE }), [
      'feed-alpha',
      'update-alpha',
    ])
  })

  it('moves a later release to the new feeds only: no old feed is so much as named', () => {
    assert.deepEqual(planFeeds({ version: LATER, missing: [], manifest: RULE }), ['feed-alpha'])
    assert.deepEqual(feedsMoved('3.0.1', RULE), ['feed-alpha', 'feed-stable'])
  })

  it('moves the old feeds of the channels in use only, even where the bridge is a stable release', () => {
    const stable = checkManifest({ ...RULE, bridge: { version: '3.0.0', legacy: ['alpha'] } })
    assert.deepEqual(feedsMoved('3.0.0', stable), ['feed-alpha', 'feed-stable', 'update-alpha'])
    const both = checkManifest({
      ...RULE,
      bridge: { version: '3.0.0', legacy: ['alpha', 'stable'] },
    })
    assert.deepEqual(feedsMoved('3.0.0', both), [
      'feed-alpha',
      'feed-stable',
      'update-alpha',
      'update-stable',
    ])
  })

  it('refuses a release before the bridge: nothing is planned for it', () => {
    assert.throws(() => planFeeds({ version: OLDER, missing: [], manifest: RULE }), /comes before/)
  })

  it('refuses a bridge the old apps would download and then refuse', () => {
    const missing = missingForOldApps(NODE_FREE)
    assert.throws(
      () => planFeeds({ version: BRIDGE, missing, manifest: RULE }),
      /must keep the layout.*MacOS\/node/,
    )
    // A later release may be laid out any way: the old feeds are not its to move.
    assert.deepEqual(planFeeds({ version: '3.0.0-alpha.90', missing, manifest: RULE }), [
      'feed-alpha',
    ])
  })

  it('names the files a release publishes, in the order it uploads them', () => {
    assert.deepEqual(releaseAssets('3.0.0-alpha.81'), {
      dmg: 'ConsensFlow_3.0.0-alpha.81_aarch64.dmg',
      archive: 'ConsensFlow_3.0.0-alpha.81_aarch64.app.tar.gz',
      signature: 'ConsensFlow_3.0.0-alpha.81_aarch64.app.tar.gz.sig',
      metadata: 'latest.json',
      installer: 'nsis/ConsensFlow_3.0.0-alpha.81_x64-setup.exe',
      portable: 'portable/ConsensFlow_3.0.0-alpha.81_x64-portable.exe',
    })
  })
})

describe('what the apps before the bridge require of an archive', () => {
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

/** A ustar archive of `files` in `dir`, made as the release makes its own. */
function archive(dir, name, files) {
  const target = join(dir, name)
  writeFileSync(target, archiveOf(files))
  return target
}

/** The script, run: it ends with { status, stdout, stderr }. Not spawnSync: this process may be the server it reads. */
function run(args) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [SCRIPT, ...args])
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

describe('the plan, from the command line', () => {
  /** A version that comes after, and one before, the bridge the repository records. */
  const after = '99.0.0-alpha.1'
  const before = '1.0.0'
  const bridge = MANIFEST.bridge.version
  const plan = (version, archivePath, ...more) =>
    spawnSync(
      process.execPath,
      [SCRIPT, 'plan', '--version', version, '--archive', archivePath, ...more],
      {
        encoding: 'utf8',
      },
    )

  it(
    'names the feeds one per line: the bridge to both generations, a later release to the new',
    needsTar,
    () => {
      const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
      try {
        const old = archive(dir, 'old.tar.gz', OLD_LAYOUT)
        const free = archive(dir, 'free.tar.gz', NODE_FREE)
        const moving = plan(bridge, old)
        assert.equal(moving.status, 0, moving.stderr)
        assert.equal(moving.stdout, `feed-alpha\n${MANIFEST.legacy.alpha}\n`)
        assert.match(moving.stderr, /is the bridge/)

        const later = plan(after, free)
        assert.equal(later.status, 0, later.stderr)
        assert.equal(later.stdout, 'feed-alpha\n')
        assert.match(later.stderr, /a later release/)

        const refused = plan(bridge, free)
        assert.equal(refused.status, 1)
        assert.equal(refused.stdout, '', 'nothing is named to move')
        assert.match(refused.stderr, /must keep the layout/)
      } finally {
        rmSync(dir, { recursive: true, force: true })
      }
    },
  )

  it('refuses a release before the bridge, which a hand run only says it would', needsTar, () => {
    const dir = mkdtempSync(join(tmpdir(), 'cf-feeds-'))
    try {
      const old = archive(dir, 'old.tar.gz', OLD_LAYOUT)
      const refused = plan(before, old)
      assert.equal(refused.status, 1)
      assert.equal(refused.stdout, '')
      assert.match(refused.stderr, /comes before the bridge/)

      const dry = plan(before, old, '--dry-run')
      assert.equal(dry.status, 0, 'a hand run is not failed by it')
      assert.equal(dry.stdout, '')
      assert.match(dry.stderr, /feeds \(a tag would be refused\): .*comes before the bridge/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('says what a command lacks, and does nothing', async () => {
    for (const [command, lacks] of [
      ['plan', /plan needs --version and --archive/],
      ['prerequisites', /prerequisites needs --version and --base/],
      ['check', /check needs --dir, --version and --base/],
      ['publish', /usage: feeds\.mjs plan\|prerequisites\|check/],
    ]) {
      const ran = await run([command])
      assert.equal(ran.status, 1, command)
      assert.match(ran.stderr, lacks, command)
      assert.equal(ran.stdout, '', command)
    }
  })
})

/** GitHub as it is once the bridge has crossed: its release published, update-alpha serving it. */
async function crossed(version = BRIDGE) {
  const github = await githubSim()
  const files = builtFiles(github.base, version)
  github.release(`v${version}`, { assets: publishedAssets(files) })
  github.release('update-alpha', {
    prerelease: true,
    assets: { 'latest.json': files['latest.json'] },
  })
  return { github, files }
}

/** The path a release's file is downloaded from. */
const download = (version, name) => `/v${version}/${name}`
const ARCHIVE = (version) => `ConsensFlow_${version}_aarch64.app.tar.gz`
const INSTALLER = (version) => `ConsensFlow_${version}_x64-setup.exe`
const PORTABLE = (version) => `ConsensFlow_${version}_x64-portable.exe`

describe('before a later release moves a feed: the old feeds serve the bridge, with its files', () => {
  /** What `checkPrerequisites` finds for `version`, once `change` has altered GitHub. */
  async function found(change = () => {}, version = LATER) {
    const { github, files } = await crossed()
    try {
      await change(github, files)
      return await checkPrerequisites({ version, base: github.base, manifest: RULE, ...QUICK })
    } finally {
      await github.close()
    }
  }
  const feedOf = (github, version) =>
    github.release('update-alpha', {
      prerelease: true,
      assets: { 'latest.json': latestJson(github.base, version) },
    })

  it('passes once the old feed serves the bridge and its files download as published', async () => {
    assert.deepEqual(await found(), [])
  })

  it('refuses where the old feed still serves the release before the bridge', async () => {
    // The bridge's tag is in the repository, and its release failed before it moved update-alpha.
    assert.deepEqual(await found((github) => feedOf(github, OLDER)), [
      'update-alpha serves 3.0.0-alpha.80, not the bridge 3.0.0-alpha.81: the apps that read it do not reach the bridge',
    ])
  })

  it('refuses where the old feed names some other release than the bridge, an earlier Node-free one among them', async () => {
    const problems = await found((github) => feedOf(github, '3.0.0-alpha.78'))
    assert.match(problems[0], /^update-alpha serves 3\.0\.0-alpha\.78, not the bridge/)
    const newer = await found((github) => feedOf(github, '3.0.0-alpha.85'))
    assert.match(newer[0], /^update-alpha serves 3\.0\.0-alpha\.85, not the bridge/)
  })

  it('refuses a feed that names the bridge but is not its latest.json byte for byte', async () => {
    const problems = await found((github, files) =>
      github.release('update-alpha', {
        prerelease: true,
        assets: { 'latest.json': files['latest.json'].replace('notes', 'other notes') },
      }),
    )
    assert.deepEqual(problems, [
      "update-alpha names the bridge 3.0.0-alpha.81, but is not the bridge's own latest.json byte for byte",
    ])
  })

  it('refuses a feed that is not there: a required feed is not absent', async () => {
    const problems = await found((github) => github.override('/update-alpha/latest.json', 404))
    assert.deepEqual(problems, [
      'update-alpha cannot be read (HTTP 404): the apps before the bridge read it',
    ])
  })

  it('refuses a feed that fails to load, whichever way it fails', async () => {
    for (const [instead, says] of [
      [503, /^update-alpha cannot be read \(HTTP 503\)/],
      [500, /^update-alpha cannot be read \(HTTP 500\)/],
      ['reset', /^update-alpha cannot be read \(unreachable: /],
    ]) {
      const problems = await found((github) =>
        github.override('/update-alpha/latest.json', instead),
      )
      assert.equal(problems.length, 1, `${instead}: ${problems}`)
      assert.match(problems[0], says)
    }
  })

  it('refuses a feed that serves something that is not a latest.json', async () => {
    const problems = await found((github) =>
      github.release('update-alpha', {
        prerelease: true,
        assets: { 'latest.json': '<html>an error page</html>' },
      }),
    )
    assert.deepEqual(problems, ['update-alpha serves something that is not a latest.json'])
  })

  it('refuses a bridge archive that serves junk, though its address answers', async () => {
    const problems = await found((github) =>
      github.override(download(BRIDGE, ARCHIVE(BRIDGE)), 'junk, not the archive'),
    )
    assert.deepEqual(problems, [
      `the bridge's ${ARCHIVE(BRIDGE)} does not download as the one its SHA256SUMS lists`,
    ])
  })

  it("refuses the bridge's Windows installer or portable that is gone, or serves junk", async () => {
    for (const name of [INSTALLER(BRIDGE), PORTABLE(BRIDGE)]) {
      const gone = await found((github) => github.override(download(BRIDGE, name), 404))
      assert.deepEqual(gone, [`the bridge's ${name} cannot be downloaded (HTTP 404)`])
      const junk = await found((github) => github.override(download(BRIDGE, name), 'junk'))
      assert.deepEqual(junk, [
        `the bridge's ${name} does not download as the one its SHA256SUMS lists`,
      ])
    }
  })

  it("refuses where the bridge's own latest.json is not there to hold the feed to", async () => {
    const problems = await found((github) => github.override(download(BRIDGE, 'latest.json'), 404))
    assert.deepEqual(problems, [
      `the bridge's v${BRIDGE}/latest.json cannot be read (HTTP 404), so no feed can be held to it`,
    ])
  })

  it('refuses a bridge whose latest.json names another release, or an archive elsewhere', async () => {
    /** The bridge published with a latest.json that `wrong` makes of its own, and update-alpha serving it. */
    const published = (wrong) => (github, files) => {
      const text = wrong(files['latest.json'])
      github.release(`v${BRIDGE}`, { assets: { ...publishedAssets(files), 'latest.json': text } })
      github.release('update-alpha', { prerelease: true, assets: { 'latest.json': text } })
    }
    const elsewhere = await found(
      published((text) =>
        text.replace(/"url":"[^"]*"/, '"url":"https://example.invalid/a.tar.gz"'),
      ),
    )
    assert.match(elsewhere[0], /names https:\/\/example\.invalid\/a\.tar\.gz for the Mac, not /)
    const another = await found(
      published((text) => text.replace(`"version":"${BRIDGE}"`, '"version":"3.0.0-alpha.99"')),
    )
    assert.deepEqual(another, [
      `the bridge's v${BRIDGE}/latest.json names 3.0.0-alpha.99, not ${BRIDGE}`,
    ])
  })

  it('refuses where SHA256SUMS is gone, or lists no file the old apps need', async () => {
    const gone = await found((github) => github.override(download(BRIDGE, 'SHA256SUMS'), 404))
    assert.deepEqual(gone, [`the bridge's v${BRIDGE}/SHA256SUMS cannot be read (HTTP 404)`])
    const without = await found((github, files) => {
      const sums = publishedAssets(files).SHA256SUMS
      const trimmed = sums
        .split('\n')
        .filter((line) => !line.endsWith(INSTALLER(BRIDGE)))
        .join('\n')
      github.release(`v${BRIDGE}`, { assets: { ...publishedAssets(files), SHA256SUMS: trimmed } })
    })
    assert.deepEqual(without, [`the bridge's v${BRIDGE}/SHA256SUMS lists no ${INSTALLER(BRIDGE)}`])
  })

  it('waits out an old feed served stale for a moment', async () => {
    let asked = 0
    const passed = await found((github) =>
      github.override('/update-alpha/latest.json', () => {
        asked += 1
        return asked < 2 ? latestJson(github.base, OLDER) : undefined
      }),
    )
    assert.deepEqual(passed, [])
    assert.equal(asked, 2, 'read again until it served the bridge')
  })

  it('has nothing to ask of the feeds for the bridge itself, and reads none', async () => {
    const { github } = await crossed()
    try {
      feedOf(github, OLDER)
      const problems = await checkPrerequisites({
        version: BRIDGE,
        base: github.base,
        manifest: RULE,
        ...QUICK,
      })
      assert.deepEqual(problems, [])
      assert.deepEqual(github.requests, [])
    } finally {
      await github.close()
    }
  })

  it('refuses a release before the bridge', async () => {
    const problems = await found(() => {}, OLDER)
    assert.equal(problems.length, 1)
    assert.match(problems[0], /comes before the bridge/)
  })
})

/**
 * A release `version` built and published, its feeds moved as a run that
 * finished leaves them: the bridge crossed (`crossed`), and for a later release
 * its own files and feed-alpha serving it. `edit` alters what was built before
 * anything is published. `check` is `checkFeeds` on it.
 */
async function afterPublishing(version, edit = (files) => files) {
  const { github, files } = await crossed()
  let built = files
  if (version !== BRIDGE) {
    built = edit(builtFiles(github.base, version))
    github.release(`v${version}`, { assets: publishedAssets(built) })
  }
  github.release('feed-alpha', {
    prerelease: true,
    assets: { 'latest.json': built['latest.json'] },
  })
  const dir = folderOf({ ...built, SHA256SUMS: publishedAssets(built).SHA256SUMS })
  return {
    github,
    files: built,
    dir,
    check: (options = {}) =>
      checkFeeds({ dir, version, base: github.base, manifest: RULE, ...QUICK, ...options }),
    done: async () => {
      rmSync(dir, { recursive: true, force: true })
      await github.close()
    },
  }
}

describe('after a release is published: the files and the feeds serve what the rule says', () => {
  /** What `checkFeeds` finds for `version`, once `change` has altered GitHub. */
  async function found(version, change = () => {}) {
    const world = await afterPublishing(version)
    try {
      await change(world.github, world)
      return await world.check()
    } finally {
      await world.done()
    }
  }

  it('passes a later release: its files and its feed, the old feed pinned to the bridge', async () => {
    assert.deepEqual(await found(LATER), [])
  })

  it('passes the bridge: the new feed and the old feed both serve it', async () => {
    assert.deepEqual(await found(BRIDGE), [])
  })

  it('finds the bridge not carried to the old feed: it still serves the release before', async () => {
    const problems = await found(BRIDGE, (github) =>
      github.release('update-alpha', {
        prerelease: true,
        assets: { 'latest.json': latestJson(github.base, OLDER) },
      }),
    )
    assert.deepEqual(problems, [
      'update-alpha serves 3.0.0-alpha.80, not the bridge 3.0.0-alpha.81: the apps that read it do not reach the bridge',
    ])
  })

  it('finds the old feed no longer pinned to the bridge after a later release', async () => {
    const problems = await found(LATER, (github) =>
      github.release('update-alpha', {
        prerelease: true,
        assets: { 'latest.json': latestJson(github.base, LATER) },
      }),
    )
    assert.match(problems[0], /^update-alpha serves 3\.0\.0-alpha\.82, not the bridge/)
  })

  it('finds a required old feed that is not there, or fails to load', async () => {
    for (const [instead, says] of [
      [404, /^update-alpha cannot be read \(HTTP 404\)/],
      [503, /^update-alpha cannot be read \(HTTP 503\)/],
      ['reset', /^update-alpha cannot be read \(unreachable: /],
    ]) {
      const problems = await found(LATER, (github) =>
        github.override('/update-alpha/latest.json', instead),
      )
      assert.equal(problems.length, 1, `${instead}: ${problems}`)
      assert.match(problems[0], says)
    }
  })

  it('finds a file of the release that does not download as the one built, whichever it is', async () => {
    for (const name of [
      ARCHIVE(LATER),
      INSTALLER(LATER),
      PORTABLE(LATER),
      `ConsensFlow_${LATER}_aarch64.dmg`,
    ]) {
      const problems = await found(LATER, (github) =>
        github.override(download(LATER, name), 'another file'),
      )
      assert.deepEqual(
        problems,
        [`${name} does not download from v${LATER} as the one built`],
        name,
      )
    }
  })

  it('finds a file of the release that is not served, or whose address fails', async () => {
    const gone = await found(LATER, (github) =>
      github.override(download(LATER, PORTABLE(LATER)), 404),
    )
    assert.deepEqual(gone, [`${PORTABLE(LATER)} cannot be downloaded from v${LATER} (HTTP 404)`])
    const failing = await found(LATER, (github) =>
      github.override(download(LATER, INSTALLER(LATER)), 502),
    )
    assert.deepEqual(failing, [
      `${INSTALLER(LATER)} cannot be downloaded from v${LATER} (HTTP 502)`,
    ])
  })

  it('finds a new feed that keeps serving another release, is not there, or fails', async () => {
    const other = await found(LATER, (github) =>
      github.release('feed-alpha', {
        prerelease: true,
        assets: { 'latest.json': latestJson(github.base, OLDER) },
      }),
    )
    assert.deepEqual(other, ["feed-alpha does not serve this release's latest.json"])
    for (const [instead, says] of [
      [404, "feed-alpha does not serve this release's latest.json (HTTP 404)"],
      [503, "feed-alpha does not serve this release's latest.json (HTTP 503)"],
    ]) {
      const problems = await found(LATER, (github) =>
        github.override('/feed-alpha/latest.json', instead),
      )
      assert.deepEqual(problems, [says])
    }
    const dropped = await found(LATER, (github) =>
      github.override('/feed-alpha/latest.json', 'reset'),
    )
    assert.match(
      dropped[0],
      /^feed-alpha does not serve this release's latest\.json \(unreachable: /,
    )
  })

  it('finds a latest.json that names no archive of this release', async () => {
    const world = await afterPublishing(LATER, (files) => ({
      ...files,
      'latest.json': files['latest.json'].replace(
        /"url":"[^"]*"/,
        '"url":"https://example.invalid/a.tar.gz"',
      ),
    }))
    try {
      const problems = await world.check()
      assert.equal(problems.length, 1, `${problems}`)
      assert.match(problems[0], /names https:\/\/example\.invalid\/a\.tar\.gz for the Mac, not /)
    } finally {
      await world.done()
    }
  })

  it('waits out a feed that is served stale for a moment', async () => {
    let asked = 0
    const problems = await found(LATER, (github, { files }) =>
      github.override('/feed-alpha/latest.json', () => {
        asked += 1
        return asked < 2 ? latestJson(github.base, OLDER) : files['latest.json']
      }),
    )
    assert.deepEqual(problems, [])
    assert.equal(asked, 2, 'read until it served this release')
  })

  it('lets the old feed of a channel not in use be absent, and finds it serving this release', async () => {
    assert.deepEqual(await found(LATER), [], 'update-stable is not there')
    const serving = await found(LATER, (github, { files }) =>
      github.release('update-stable', {
        prerelease: true,
        assets: { 'latest.json': files['latest.json'] },
      }),
    )
    assert.deepEqual(serving, [
      'update-stable serves this release, which the rule does not move it to',
    ])
    const failing = await found(LATER, (github) =>
      github.override('/update-stable/latest.json', 503),
    )
    assert.deepEqual(failing, ['update-stable cannot be read (HTTP 503)'])
    const junk = await found(LATER, (github) =>
      github.release('update-stable', { prerelease: true, assets: { 'latest.json': 'oops' } }),
    )
    assert.deepEqual(junk, ['update-stable serves something that is not a latest.json'])
  })

  it('downloads each file once, though the bridge is the release being checked', async () => {
    const world = await afterPublishing(BRIDGE)
    try {
      assert.deepEqual(await world.check(), [])
      const archive = download(BRIDGE, ARCHIVE(BRIDGE))
      assert.equal(world.github.requests.filter((path) => path === archive).length, 1)
    } finally {
      await world.done()
    }
  })

  it('refuses a release before the bridge', async () => {
    const world = await afterPublishing(LATER)
    try {
      const problems = await checkFeeds({
        dir: world.dir,
        version: OLDER,
        base: world.github.base,
        manifest: RULE,
        ...QUICK,
      })
      assert.match(problems[0], /comes before the bridge/)
    } finally {
      await world.done()
    }
  })
})

describe('the checks, from the command line', () => {
  const after = '99.0.0-alpha.1'
  const bridge = MANIFEST.bridge.version

  /** The repository's own bridge, crossed, and a later release published after it. */
  async function world() {
    const github = await githubSim()
    const files = builtFiles(github.base, bridge)
    github.release(`v${bridge}`, { assets: publishedAssets(files) })
    github.release('update-alpha', {
      prerelease: true,
      assets: { 'latest.json': files['latest.json'] },
    })
    const built = builtFiles(github.base, after)
    github.release(`v${after}`, { assets: publishedAssets(built) })
    github.release('feed-alpha', {
      prerelease: true,
      assets: { 'latest.json': built['latest.json'] },
    })
    const dir = folderOf({ ...built, SHA256SUMS: publishedAssets(built).SHA256SUMS })
    return { github, dir }
  }

  it('says the prerequisites hold, or what they lack: a refusal fails, a hand run only says it', async () => {
    const { github, dir } = await world()
    try {
      const base = ['--version', after, '--base', github.base, '--attempts', '2', '--wait', '1']
      const holds = await run(['prerequisites', ...base])
      assert.equal(holds.status, 0, holds.stderr)
      assert.match(
        holds.stdout,
        /may move its feeds \(the old feeds serve the bridge, with its files\)/,
      )

      github.override(`/${MANIFEST.legacy.alpha}/latest.json`, 404)
      const refused = await run(['prerequisites', ...base])
      assert.equal(refused.status, 1)
      assert.equal(refused.stdout, '')
      assert.match(refused.stderr, /^feeds: update-alpha cannot be read \(HTTP 404\)/m)

      const dry = await run(['prerequisites', ...base, '--dry-run'])
      assert.equal(dry.status, 0, 'a hand run is not failed by it')
      assert.match(dry.stderr, /^feeds \(a tag would be refused\): update-alpha cannot be read/m)

      const bridged = await run(['prerequisites', '--version', bridge, '--base', github.base])
      assert.equal(bridged.status, 0, bridged.stderr)
      assert.match(bridged.stdout, /it is the bridge/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
      await github.close()
    }
  })

  it('says the feeds serve what the rule says once published, or what they do not', async () => {
    const { github, dir } = await world()
    try {
      const base = [
        '--dir',
        dir,
        '--version',
        after,
        '--base',
        github.base,
        '--attempts',
        '1',
        '--wait',
        '1',
      ]
      const passed = await run(['check', ...base])
      assert.equal(passed.status, 0, passed.stderr)
      assert.match(passed.stdout, /serve this release as the rule says/)

      github.override(download(after, PORTABLE(after)), 'junk')
      const failed = await run(['check', ...base])
      assert.equal(failed.status, 1)
      assert.match(failed.stderr, /does not download from v99\.0\.0-alpha\.1 as the one built/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
      await github.close()
    }
  })
})
