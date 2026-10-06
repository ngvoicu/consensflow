import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
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
import { fileURLToPath } from 'node:url'
import { checkFeeds, checkManifest } from '../app/scripts/feeds.mjs'
import { publishRelease } from '../app/scripts/publish.mjs'
import { builtFiles, folderOf, githubSim, latestJson, publishedAssets } from './github-sim.mjs'

/**
 * The publisher (app/scripts/publish.mjs), run against GitHub as
 * tests/github-sim.mjs has it: every half-done state a run can be cut short
 * in, finished by running it again; no file of a feed ever deleted for its
 * replacement; and the rule's refusals, which move nothing.
 */

const SCRIPT = fileURLToPath(new URL('../app/scripts/publish.mjs', import.meta.url))
/** alpha.80 is built from the code before the bridge: an ordinary release, which the old feed serves until the bridge reaches it. */
const BRIDGE = '3.0.0-alpha.81'
const OLDER = '3.0.0-alpha.80'
const LATER = '3.0.0-alpha.82'
const RULE = checkManifest({
  feeds: { alpha: 'feed-alpha', stable: 'feed-stable' },
  legacy: { alpha: 'update-alpha', stable: 'update-stable' },
  bridge: { version: BRIDGE, legacy: ['alpha'] },
})
const QUICK = { attempts: 2, wait: 1 }
const ROOT = 'ConsensFlow.app/Contents/'
/** What the old apps check an archive for, as `tar -t` would list it: the archive is not read here. */
const OLD_LAYOUT = [
  `${ROOT}MacOS/node`,
  `${ROOT}Resources/cli/package.json`,
  `${ROOT}Resources/cli/bin/cf.mjs`,
  `${ROOT}Resources/cli/bin/cf`,
  `${ROOT}Resources/cli/hosts/pi-extension/consensflow-delivery.mjs`,
  `${ROOT}Resources/cli/src/core/daemon.js`,
]
const LATEST = 'latest.json'
const NEXT = 'latest.next.json'
const PREVIOUS = 'latest.previous.json'

/**
 * GitHub before `version` is published, and the release built in a folder:
 * `serving` is the release update-alpha serves, which for a later release is
 * the bridge (published, and crossed), and for the bridge the one before it.
 * `publish()` runs the publisher on it.
 */
async function before(version, { serving, members = () => OLD_LAYOUT } = {}) {
  const github = await githubSim()
  if (version !== BRIDGE) {
    github.release(`v${BRIDGE}`, { assets: publishedAssets(builtFiles(github.base, BRIDGE)) })
  }
  const feed = serving ?? (version === BRIDGE ? OLDER : BRIDGE)
  github.release('update-alpha', {
    prerelease: true,
    assets: { [LATEST]: latestJson(github.base, feed) },
  })
  const files = builtFiles(github.base, version)
  const dir = folderOf(files)
  const log = []
  return {
    github,
    files,
    dir,
    log,
    /** Publishes the release, as the workflow's step does. */
    publish: (extra = {}) =>
      publishRelease({
        dir,
        tag: `v${version}`,
        base: github.base,
        repo: github.repo,
        gh: github.gh,
        manifest: RULE,
        members,
        patience: QUICK,
        log: (line) => log.push(line),
        ...extra,
      }),
    /** What the feeds serve afterwards, as the workflow's last step asks. */
    check: () => checkFeeds({ dir, version, base: github.base, manifest: RULE, ...QUICK }),
    done: async () => {
      rmSync(dir, { recursive: true, force: true })
      await github.close()
    },
  }
}

/** Runs `body` on the world `before` makes, whichever way it ends. */
async function within(version, options, body) {
  const world = await before(version, options)
  try {
    return await body(world)
  } finally {
    await world.done()
  }
}

/** The calls that change anything. */
const changes = (github) =>
  github.calls.filter(
    ([group, verb]) =>
      group === 'api' || ['create', 'upload', 'delete-asset', 'edit'].includes(verb),
  )
/** The calls that name `text` among their arguments. */
const mentioning = (github, text) => github.calls.filter((args) => args.includes(text))

/**
 * The most moments in a row, each after a `gh` call, at which a feed lacked its
 * latest.json after having had it: `had` says it had it before the first call.
 */
function gap(github, feed, had) {
  let longest = 0
  let run = 0
  let seen = had
  for (const { after } of github.trace) {
    if (after[feed]?.assets.includes(LATEST)) {
      seen = true
      run = 0
    } else if (seen) {
      run += 1
      longest = Math.max(longest, run)
    }
  }
  return longest
}

describe('publishing the bridge', () => {
  it('makes the versioned release whole and public, then moves the new feed and the old one', async () => {
    await within(BRIDGE, {}, async ({ github, files, publish, check, log }) => {
      const done = await publish()
      assert.deepEqual(done, {
        version: BRIDGE,
        release: 'created',
        feeds: { 'feed-alpha': 'created', 'update-alpha': 'replaced' },
      })
      const tag = `v${BRIDGE}`
      assert.equal(github.isDraft(tag), false, 'published')
      assert.equal(github.isPrerelease(tag), true)
      assert.equal(github.title(tag), `ConsensFlow ${BRIDGE}`)
      assert.deepEqual(
        github.names(tag),
        [
          ...Object.keys(publishedAssets(files)).filter((name) => name !== 'SHA256SUMS'),
          'SHA256SUMS',
        ].sort(),
      )
      assert.deepEqual(github.names('feed-alpha'), [LATEST])
      assert.deepEqual(github.names('update-alpha'), [LATEST])
      for (const feed of ['feed-alpha', 'update-alpha']) {
        assert.equal(github.asset(feed, LATEST).toString(), files[LATEST], feed)
      }
      assert.deepEqual(await check(), [], 'the feeds serve what the rule says')
      assert.ok(log.some((line) => line.startsWith('gh release create')))
    })
  })

  it('makes the release a draft, and publishes it only once every file is there', async () => {
    await within(BRIDGE, {}, async ({ github, publish }) => {
      await publish()
      const calls = github.calls
      const create = calls.findIndex(([, verb]) => verb === 'create')
      assert.deepEqual(calls[create].slice(0, 5), [
        'release',
        'create',
        `v${BRIDGE}`,
        '--draft',
        '--verify-tag',
      ])
      assert.ok(calls[create].includes('--prerelease'))
      const publishing = calls.findIndex(
        ([, verb, , flag]) => verb === 'edit' && flag === '--draft=false',
      )
      const upload = calls.findIndex(([, verb, tag]) => verb === 'upload' && tag === `v${BRIDGE}`)
      assert.ok(create < upload && upload < publishing, 'created, filled, then published')
      const filled = github.trace[publishing - 1].after[`v${BRIDGE}`]
      assert.equal(filled.draft, true, 'the release was a draft until then')
      assert.equal(filled.assets.length, 7, 'with all its files')
    })
  })

  it('writes the sums and the release page as the workflow always has', async () => {
    await within(BRIDGE, {}, async ({ github, files, dir, publish }) => {
      await publish()
      const sums = readFileSync(join(dir, 'SHA256SUMS'), 'utf8')
      assert.equal(
        sums,
        publishedAssets(files).SHA256SUMS,
        'one line per file, as sha256sum writes it',
      )
      assert.equal(github.asset(`v${BRIDGE}`, 'SHA256SUMS').toString(), sums)
      const body = github.notes(`v${BRIDGE}`)
      assert.ok(body.startsWith(`ConsensFlow ${BRIDGE}, the notes\n\n**Downloads.**`), body)
      assert.match(body, /%LOCALAPPDATA%\\dev\.ngvoicu\.consensflow\\portable-runtime/)
      assert.match(body, /Check for Updates on the Alpha channel\.$/m)
    })
  })

  it('marks the old feed as pinned to the bridge, and the new one is only the feed', async () => {
    await within(BRIDGE, {}, async ({ github, publish }) => {
      await publish()
      assert.equal(github.title('feed-alpha'), 'Alpha update feed')
      assert.equal(github.isPrerelease('feed-alpha'), true)
      assert.equal(github.title('update-alpha'), 'Alpha update feed, pinned')
      assert.match(
        github.notes('update-alpha'),
        /^Pinned to ConsensFlow 3\.0\.0-alpha\.81, the first release to read feed-alpha: .*from it on read feed-alpha\./,
      )
    })
  })

  it('moves the old feed with no moment at which it lacks its latest.json beyond one call', async () => {
    await within(BRIDGE, {}, async ({ github, publish }) => {
      await publish()
      assert.equal(gap(github, 'update-alpha', true), 1, 'between the two renames, and no longer')
      assert.deepEqual(
        mentioning(github, 'delete-asset').map((args) => args.slice(0, 4)),
        [['release', 'delete-asset', 'update-alpha', PREVIOUS]],
        'only the previous file is deleted, after the new one is in place',
      )
      assert.ok(!JSON.stringify(github.calls).includes('--clobber'))
      assert.equal(github.names('update-alpha').includes(LATEST), true)
    })
  })

  it('is refused for an archive the old apps would refuse, and nothing is moved', async () => {
    await within(BRIDGE, { members: () => [`${ROOT}MacOS/app`] }, async ({ github, publish }) => {
      await assert.rejects(publish(), /the bridge must keep the layout.*MacOS\/node/)
      assert.deepEqual(github.calls, [], 'not so much as a look at GitHub')
    })
  })

  it('is refused for a missing file, or a missing set of notes, before anything is asked', async () => {
    for (const path of [`nsis/ConsensFlow_${BRIDGE}_x64-setup.exe`, 'notes.txt', 'latest.json']) {
      await within(BRIDGE, {}, async ({ github, dir, publish }) => {
        rmSync(join(dir, ...path.split('/')))
        await assert.rejects(
          publish(),
          new RegExp(`^Error: missing ${path.replace(/[.]/g, '\\.')}$`),
        )
        assert.deepEqual(github.calls, [])
      })
    }
  })
})

describe('publishing a later release', () => {
  it('moves the new feed only, and does not so much as touch the old one', async () => {
    await within(LATER, {}, async ({ github, publish, check }) => {
      const before = github.asset('update-alpha', LATEST).toString()
      const done = await publish()
      assert.deepEqual(done.feeds, { 'feed-alpha': 'created' })
      assert.deepEqual(mentioning(github, 'update-alpha'), [])
      assert.equal(github.asset('update-alpha', LATEST).toString(), before, 'pinned to the bridge')
      assert.deepEqual(await check(), [])
    })
  })

  it('moves both new feeds for a stable release, and the old feeds neither', async () => {
    await within('3.0.1', {}, async ({ github, publish }) => {
      const done = await publish()
      assert.deepEqual(done.feeds, { 'feed-alpha': 'created', 'feed-stable': 'created' })
      assert.equal(github.isPrerelease('v3.0.1'), false, 'a stable release is not a prerelease')
      assert.match(github.notes('v3.0.1'), /on the Stable channel\.$/m)
      assert.deepEqual(
        mentioning(github, 'update-alpha').concat(mentioning(github, 'update-stable')),
        [],
      )
    })
  })

  it('is refused while the old feed still serves the release before the bridge, and nothing is moved', async () => {
    // The bridge's tag holds the manifest; its release failed before it moved update-alpha.
    await within(LATER, { serving: OLDER }, async ({ github, publish }) => {
      await assert.rejects(
        publish(),
        /3\.0\.0-alpha\.82 may not move a feed, and nothing was moved:\n- update-alpha serves 3\.0\.0-alpha\.80, not the bridge 3\.0\.0-alpha\.81/,
      )
      assert.deepEqual(github.calls, [], 'no release, no feed, not so much as a look')
      assert.equal(github.has(`v${LATER}`), false)
      assert.equal(github.has('feed-alpha'), false)
    })
  })

  it('is refused for every way the old feed or the bridge can fail to be there', async () => {
    for (const change of [
      (github) => github.override('/update-alpha/latest.json', 404),
      (github) => github.override('/update-alpha/latest.json', 503),
      (github) => github.override(`/v${BRIDGE}/ConsensFlow_${BRIDGE}_aarch64.app.tar.gz`, 'junk'),
      (github) => github.override(`/v${BRIDGE}/ConsensFlow_${BRIDGE}_x64-portable.exe`, 404),
      (github) => github.override(`/v${BRIDGE}/SHA256SUMS`, 404),
    ]) {
      await within(LATER, {}, async ({ github, publish }) => {
        change(github)
        await assert.rejects(publish(), /nothing was moved/)
        assert.deepEqual(github.calls, [])
      })
    }
  })

  it('is refused for a release before the bridge', async () => {
    await within(OLDER, {}, async ({ github, publish }) => {
      await assert.rejects(publish(), /3\.0\.0-alpha\.80 comes before the bridge/)
      assert.deepEqual(github.calls, [])
    })
  })
})

describe('the bridge that failed before it moved update-alpha, and the release after it', () => {
  it('is finished by running it again; the release after it waits until then', async () => {
    const github = await githubSim()
    const bridge = builtFiles(github.base, BRIDGE)
    const later = builtFiles(github.base, LATER)
    github.release('update-alpha', {
      prerelease: true,
      assets: { [LATEST]: latestJson(github.base, OLDER) },
    })
    const dirs = [folderOf(bridge), folderOf(later)]
    const run = (version, dir, extra = {}) =>
      publishRelease({
        dir,
        tag: `v${version}`,
        base: github.base,
        repo: github.repo,
        gh: github.gh,
        manifest: RULE,
        members: () => OLD_LAYOUT,
        patience: QUICK,
        ...extra,
      })
    try {
      // alpha.81, the bridge: the release is made and the new feed moved, and the run dies at update-alpha.
      github.fail((args) => args.includes('update-alpha') && 'the network went away')
      await assert.rejects(run(BRIDGE, dirs[0]), /the network went away/)
      assert.equal(github.isDraft(`v${BRIDGE}`), false, 'its release is public')
      assert.equal(github.asset('feed-alpha', LATEST).toString(), bridge[LATEST])
      assert.equal(github.asset('update-alpha', LATEST).toString(), latestJson(github.base, OLDER))

      // alpha.82 is made from a tree that holds the tag of alpha.81 and its manifest: it is refused.
      github.fail(() => undefined)
      const calls = github.calls.length
      await assert.rejects(
        run(LATER, dirs[1]),
        /update-alpha serves 3\.0\.0-alpha\.80, not the bridge/,
      )
      assert.equal(github.calls.length, calls, 'nothing was asked of gh for it')
      assert.equal(github.has(`v${LATER}`), false)

      // Running alpha.81 again finishes it: no "release exists" stop, no second release.
      const finished = await run(BRIDGE, dirs[0])
      assert.deepEqual(finished.feeds, { 'feed-alpha': 'kept', 'update-alpha': 'replaced' })
      assert.equal(finished.release, 'kept')
      assert.equal(
        mentioning(github, `v${BRIDGE}`).filter((args) => args[1] === 'create').length,
        1,
      )
      assert.equal(github.asset('update-alpha', LATEST).toString(), bridge[LATEST])

      // Now alpha.82 may move its feed.
      const next = await run(LATER, dirs[1])
      assert.equal(next.release, 'created')
      assert.equal(github.asset('feed-alpha', LATEST).toString(), later[LATEST])
      assert.equal(
        github.asset('update-alpha', LATEST).toString(),
        bridge[LATEST],
        'still the bridge',
      )
    } finally {
      for (const dir of dirs) rmSync(dir, { recursive: true, force: true })
      await github.close()
    }
  })
})

describe('a publish cut short while it makes the versioned release', () => {
  it('leaves a draft that is no release, and running it again fills it and publishes it', async () => {
    await within(BRIDGE, {}, async ({ github, publish, check }) => {
      // The upload of the files dies after the draft is made.
      github.fail(
        (args) => args[1] === 'upload' && args[2] === `v${BRIDGE}` && 'upload interrupted',
      )
      await assert.rejects(publish(), /upload interrupted/)
      assert.equal(github.isDraft(`v${BRIDGE}`), true, 'not public')
      assert.equal(github.has('feed-alpha'), false, 'no feed moved')
      github.fail(() => undefined)
      const done = await publish()
      assert.equal(done.release, 'remade')
      assert.equal(github.isDraft(`v${BRIDGE}`), false)
      assert.deepEqual(await check(), [])
    })
  })

  it('empties a draft that holds some of the files, a short one among them, and fills it again', async () => {
    await within(BRIDGE, {}, async ({ github, files, publish, check }) => {
      github.release(`v${BRIDGE}`, {
        draft: true,
        prerelease: true,
        assets: {
          [`ConsensFlow_${BRIDGE}_aarch64.dmg`]: 'a short upload',
          'stray.txt': 'left over',
        },
      })
      const done = await publish()
      assert.equal(done.release, 'remade')
      assert.equal(github.names(`v${BRIDGE}`).includes('stray.txt'), false, 'the draft was emptied')
      assert.equal(
        github.asset(`v${BRIDGE}`, `ConsensFlow_${BRIDGE}_aarch64.dmg`).toString(),
        files[`ConsensFlow_${BRIDGE}_aarch64.dmg`],
      )
      assert.deepEqual(await check(), [])
    })
  })

  it('does not publish a draft whose file is short, and says so', async () => {
    await within(BRIDGE, {}, async ({ github, publish }) => {
      const dmg = `ConsensFlow_${BRIDGE}_aarch64.dmg`
      // The second look at the release is the one after its files are uploaded: one of them is found short.
      let views = 0
      github.fail((args) => {
        if (args[1] === 'view' && args[2] === `v${BRIDGE}`) {
          views += 1
          if (views === 2) github.put(`v${BRIDGE}`, dmg, 'cut')
        }
      })
      await assert.rejects(
        publish(),
        new RegExp(`${dmg} as 3 bytes, uploaded, not as the \\d+ built`),
      )
      assert.equal(github.isDraft(`v${BRIDGE}`), true, 'still not public')
      assert.deepEqual(mentioning(github, 'feed-alpha'), [])
    })
  })

  it('publishes the draft when the run died at the very publishing, and the rest follows', async () => {
    await within(BRIDGE, {}, async ({ github, publish, check }) => {
      github.fail((args) => args[1] === 'edit' && args[2] === `v${BRIDGE}` && 'died')
      await assert.rejects(publish(), /died/)
      assert.equal(github.isDraft(`v${BRIDGE}`), true)
      github.fail(() => undefined)
      await publish()
      assert.equal(github.isDraft(`v${BRIDGE}`), false)
      assert.deepEqual(await check(), [])
    })
  })

  it('adds what a published release lacks, and holds what it has to the files built', async () => {
    await within(BRIDGE, {}, async ({ github, files, publish, check }) => {
      const portable = `ConsensFlow_${BRIDGE}_x64-portable.exe`
      const held = Object.entries(publishedAssets(files)).filter(([name]) => name !== portable)
      github.release(`v${BRIDGE}`, { prerelease: true, assets: Object.fromEntries(held) })
      const done = await publish()
      assert.equal(done.release, 'completed')
      assert.deepEqual(
        github.calls.filter(([, verb]) => verb === 'upload'),
        [
          ['release', 'upload', `v${BRIDGE}`, `portable/${portable}`],
          ['release', 'upload', 'feed-alpha', LATEST],
          ['release', 'upload', 'update-alpha', NEXT],
        ],
        'only the missing file',
      )
      assert.deepEqual(await check(), [])
    })
  })

  it('refuses a published release whose file is not the one built, and moves no feed', async () => {
    await within(BRIDGE, {}, async ({ github, files, publish }) => {
      const published = publishedAssets(files)
      published[`ConsensFlow_${BRIDGE}_aarch64.app.tar.gz`] = 'an archive from another build'
      github.release(`v${BRIDGE}`, { prerelease: true, assets: published })
      await assert.rejects(
        publish(),
        new RegExp(
          `the release v${BRIDGE} \\(kept\\) does not hold the files built here, and no feed was moved: a published release's files are not replaced.*\\n- ConsensFlow_${BRIDGE}_aarch64\\.app\\.tar\\.gz does not download from v${BRIDGE} as the one built`,
        ),
      )
      assert.deepEqual(changes(github), [], 'nothing changed at all')
      assert.equal(github.has('feed-alpha'), false)
    })
  })
})

describe('a publish cut short while it moves the feeds', () => {
  it('moves the feeds that are left, and leaves what is done as it is', async () => {
    await within(BRIDGE, {}, async ({ github, publish, check }) => {
      github.fail((args) => args.includes('update-alpha') && 'died at the old feed')
      await assert.rejects(publish(), /died at the old feed/)
      github.fail(() => undefined)
      const asked = github.calls.length
      const done = await publish()
      assert.deepEqual(done.feeds, { 'feed-alpha': 'kept', 'update-alpha': 'replaced' })
      assert.equal(done.release, 'kept')
      assert.equal(
        github.calls.slice(asked).filter((args) => args[1] === 'create').length,
        0,
        'no release is made again',
      )
      assert.deepEqual(await check(), [])
    })
  })

  it('does nothing at all to a release and its feeds that are done, which pinned notes aside', async () => {
    await within(BRIDGE, {}, async ({ github, publish }) => {
      await publish()
      const asked = github.calls.length
      const again = await publish()
      assert.deepEqual(again, {
        version: BRIDGE,
        release: 'kept',
        feeds: { 'feed-alpha': 'kept', 'update-alpha': 'kept' },
      })
      assert.deepEqual(
        changes({ calls: github.calls.slice(asked) }),
        [
          [
            ...['release', 'edit', 'update-alpha', '--title', 'Alpha update feed, pinned'],
            ...['--notes', github.notes('update-alpha')],
          ],
        ],
        'only the pin, which says the same again',
      )
    })
  })

  it('stops where gh cannot tell whether a release is there, and makes nothing', async () => {
    await within(BRIDGE, {}, async ({ github, publish }) => {
      github.fail((args) => args[1] === 'view' && 'HTTP 401: Bad credentials')
      await assert.rejects(publish(), /could not look at the release v3\.0\.0-alpha\.81: HTTP 401/)
      assert.deepEqual(changes(github), [])
    })
  })
})

describe("replacing a feed's latest.json", () => {
  const OLD = latestJson('http://example.test', OLDER)

  /** The bridge, about to be published, against an update-alpha that holds `assets`. */
  async function replacing(assets) {
    const world = await before(BRIDGE)
    world.github.release('update-alpha', { prerelease: true, assets })
    return world
  }
  const holds = (github) => github.names('update-alpha')
  const serves = (github) => github.asset('update-alpha', LATEST)?.toString()
  /** A call in a few words: what it did, to what. */
  const brief = (args) =>
    args[0] === 'api'
      ? `rename to ${args.at(-1).slice('name='.length)}`
      : [args[1], args[2], args.find((arg) => arg.startsWith('latest'))].filter(Boolean).join(' ')

  it('uploads the new one beside the old and swaps them by rename, deleting nothing live', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      await world.publish()
      const { github } = world
      const start = github.calls.findIndex(
        (args) => args[1] === 'view' && args[2] === 'update-alpha',
      )
      assert.deepEqual(github.calls.slice(start).map(brief), [
        'view update-alpha',
        `upload update-alpha ${NEXT}`,
        'view update-alpha',
        `rename to ${PREVIOUS}`,
        `rename to ${LATEST}`,
        `delete-asset update-alpha ${PREVIOUS}`,
        'edit update-alpha',
      ])
      assert.deepEqual(holds(github), [LATEST])
      assert.equal(serves(github), world.files[LATEST])
    } finally {
      await world.done()
    }
  })

  it('leaves the old latest.json where it was when the upload of the new one fails', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      world.github.fail((args) => args[1] === 'upload' && args.includes(NEXT) && 'the upload broke')
      await assert.rejects(
        world.publish(),
        /uploading the new latest\.json to update-alpha: the upload broke/,
      )
      assert.equal(serves(world.github), OLD)
      assert.equal(gap(world.github, 'update-alpha', true), 0, 'never without it')
      assert.deepEqual(holds(world.github), [LATEST])
    } finally {
      await world.done()
    }
  })

  it('leaves it where it was when the first rename fails, and clears what it uploaded', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      world.github.fail(
        (args) => args[0] === 'api' && args.includes(`name=${PREVIOUS}`) && 'the rename broke',
      )
      await assert.rejects(
        world.publish(),
        /renaming latest\.json to latest\.previous\.json: the rename broke/,
      )
      assert.equal(serves(world.github), OLD)
      assert.equal(gap(world.github, 'update-alpha', true), 0)
      assert.deepEqual(holds(world.github), [LATEST], 'the new file is cleared')
    } finally {
      await world.done()
    }
  })

  it('says what is missing when the live latest.json is gone before it can be set aside', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      // Somebody deletes it between the upload of the new one and the first rename.
      let views = 0
      world.github.fail((args) => {
        if (args[1] === 'view' && args[2] === 'update-alpha') {
          views += 1
          if (views === 2) world.github.remove('update-alpha', LATEST)
        }
      })
      await assert.rejects(world.publish(), /there is no file to rename to latest\.previous\.json/)
      assert.deepEqual(
        holds(world.github),
        [],
        'the new file is cleared, and nothing else was touched',
      )
    } finally {
      await world.done()
    }
  })

  it('puts the old latest.json back when the second rename fails', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      let failed = false
      world.github.fail((args) => {
        if (args[0] === 'api' && args.includes(`name=${LATEST}`) && !failed) {
          failed = true
          return 'the rename broke'
        }
      })
      await assert.rejects(
        world.publish(),
        /renaming latest\.next\.json to latest\.json: the rename broke/,
      )
      assert.equal(serves(world.github), OLD, 'the feed serves what it did')
      assert.equal(
        gap(world.github, 'update-alpha', true),
        2,
        'lacking it for the failed call, and until the one that puts it back',
      )
      assert.deepEqual(holds(world.github), [LATEST, NEXT], 'the new one waits beside it')
    } finally {
      await world.done()
    }
  })

  it('says the feed lacks its latest.json when the old one cannot be put back either, and a rerun mends it', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      world.github.fail(
        (args) => args[0] === 'api' && args.includes(`name=${LATEST}`) && 'the network is gone',
      )
      await assert.rejects(
        world.publish(),
        /the network is gone; and update-alpha lacks its latest\.json until this is run again, for putting latest\.previous\.json back failed: .*the network is gone/,
      )
      assert.deepEqual(holds(world.github), [NEXT, PREVIOUS])
      world.github.fail(() => undefined)
      const done = await world.publish()
      assert.equal(done.feeds['update-alpha'], 'replaced')
      assert.deepEqual(holds(world.github), [LATEST])
      assert.equal(serves(world.github), world.files[LATEST])
      assert.deepEqual(await world.check(), [])
    } finally {
      await world.done()
    }
  })

  it('does not fail the release for a previous latest.json it could not delete, and the next run clears it', async () => {
    const world = await replacing({ [LATEST]: OLD })
    try {
      world.github.fail(
        (args) => args[1] === 'delete-asset' && args.includes(PREVIOUS) && 'could not delete',
      )
      await world.publish()
      assert.deepEqual(holds(world.github), [LATEST, PREVIOUS])
      assert.ok(
        world.log.some((line) => line.includes(`${PREVIOUS} of update-alpha was not cleared`)),
      )
      world.github.fail(() => undefined)
      await world.publish()
      assert.deepEqual(holds(world.github), [LATEST])
    } finally {
      await world.done()
    }
  })

  // Every state a swap cut short can be left in, and what running it again does.
  for (const [name, assets] of [
    ['an upload of the new one that was cut short', { [LATEST]: OLD, [NEXT]: 'half' }],
    ['the old one set aside and the new one not yet in place', { [PREVIOUS]: OLD, [NEXT]: 'new' }],
    ['the new one in place and the old one not yet deleted', { [PREVIOUS]: OLD, [LATEST]: 'new' }],
    ['the old one set aside and nothing else', { [PREVIOUS]: OLD }],
    ['the new one beside nothing: the feed lost its latest.json', { [NEXT]: 'new' }],
    ['a feed with no files at all', {}],
  ]) {
    it(`settles ${name}, and moves the feed`, async () => {
      const world = await replacing(assets)
      try {
        const done = await world.publish()
        assert.match(done.feeds['update-alpha'], /^(replaced|uploaded)$/)
        assert.deepEqual(holds(world.github), [LATEST], 'nothing is left beside it')
        assert.equal(serves(world.github), world.files[LATEST])
        assert.deepEqual(await world.check(), [])
      } finally {
        await world.done()
      }
    })
  }

  it('puts the previous latest.json back first when the feed has none, so it is served again at once', async () => {
    const world = await replacing({ [PREVIOUS]: OLD, [NEXT]: 'new' })
    try {
      await world.publish()
      const first = world.github.calls.find((args) => args[0] === 'api')
      assert.equal(
        first.at(-1),
        `name=${LATEST}`,
        'the first change to the feed puts the old one back',
      )
    } finally {
      await world.done()
    }
  })

  it('leaves a feed that already serves this latest.json untouched', async () => {
    const world = await before(BRIDGE)
    try {
      world.github.release('update-alpha', {
        prerelease: true,
        assets: { [LATEST]: world.files[LATEST] },
      })
      const done = await world.publish()
      assert.equal(done.feeds['update-alpha'], 'kept')
      const touched = world.github.calls.filter(
        (args) => args.includes('update-alpha') && args[1] === 'upload',
      )
      assert.deepEqual(touched, [])
    } finally {
      await world.done()
    }
  })
})

describe('only the push of a version tag publishes, from the command line', {
  skip: process.platform === 'win32' && 'the publish job runs on Linux',
}, () => {
  /** The script, run with a `gh` first on its PATH that records being run, and the environment given. */
  function run(args, env) {
    const root = mkdtempSync(join(tmpdir(), 'cf-publish-cli-'))
    const bin = join(root, 'bin')
    const log = join(root, 'gh.log')
    mkdirSync(bin)
    writeFileSync(join(bin, 'gh'), `#!/bin/sh\necho "$@" >> '${log}'\nexit 1\n`)
    chmodSync(join(bin, 'gh'), 0o755)
    return new Promise((done) => {
      const child = spawn(process.execPath, [SCRIPT, ...args], {
        env: { PATH: `${bin}:${process.env.PATH}`, ...env },
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
        const asked = existsSync(log) ? readFileSync(log, 'utf8') : ''
        rmSync(root, { recursive: true, force: true })
        done({ status, stdout, stderr, asked })
      })
    })
  }
  const args = ['--dir', '.', '--tag', `v${BRIDGE}`, '--base', 'http://127.0.0.1:1']
  const tagPush = {
    GITHUB_EVENT_NAME: 'push',
    GITHUB_REF_TYPE: 'tag',
    GITHUB_REF_NAME: `v${BRIDGE}`,
    GH_REPO: 'ngvoicu/consensflow',
  }

  it('refuses a hand run, though it is on a tag, before it asks gh anything', async () => {
    const ran = await run(args, { ...tagPush, GITHUB_EVENT_NAME: 'workflow_dispatch' })
    assert.equal(ran.status, 1)
    assert.match(
      ran.stderr,
      /^publish: only the push of a version tag publishes; this is workflow_dispatch on tag$/m,
    )
    assert.equal(ran.asked, '', 'gh was not run')
  })

  it('refuses a push of a branch, a run outside a workflow, and a tag that is not the one pushed', async () => {
    for (const [env, says] of [
      [{ ...tagPush, GITHUB_REF_TYPE: 'branch' }, /this is push on branch/],
      [{}, /this is not a workflow on nothing/],
      [
        { ...tagPush, GITHUB_REF_NAME: `v${LATER}` },
        /this run is for v3\.0\.0-alpha\.82, and it was asked to publish v3\.0\.0-alpha\.81/,
      ],
    ]) {
      const ran = await run(args, { GH_REPO: 'ngvoicu/consensflow', ...env })
      assert.equal(ran.status, 1, JSON.stringify(env))
      assert.match(ran.stderr, says)
      assert.equal(ran.asked, '')
    }
  })

  it('says what it lacks', async () => {
    const ran = await run([], tagPush)
    assert.equal(ran.status, 1)
    assert.match(
      ran.stderr,
      /publish needs --dir, --tag and --base, and the repository \(GH_REPO\)/,
    )
  })
})
