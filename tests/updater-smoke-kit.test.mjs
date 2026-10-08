import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { request } from 'node:https'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import {
  assertBuiltWith,
  BRIDGE_TAG,
  earlierReleases,
  flipRelease,
  overrideConfig,
  RELEASES,
  REPO,
} from './updater-smoke/build.mjs'
import {
  archiveOf,
  assertAdHoc,
  copyBundle,
  copyOver,
  digestManifest,
  IDENTITY,
  inspectBundle,
  REFUSED,
  refusedBundle,
  verifySeal,
} from './updater-smoke/bundle.mjs'
import { archiveName, feedDocument, makeTls, serveUpdates } from './updater-smoke/feed.mjs'
import {
  alive,
  descendants,
  killTree,
  parseTable,
  processTable,
  until,
} from './updater-smoke/processes.mjs'
import { cleanEnv, generateKey, signFile, tauriCli } from './updater-smoke/signing.mjs'
import {
  compareVersions,
  flipTag,
  newest,
  nextVersion,
  parseVersion,
  updateVersion,
} from './updater-smoke/versions.mjs'

/**
 * What `npm run smoke:updater` is made of apart from the apps it runs: the
 * versions it gives the update, the key it signs with and what a build may
 * inherit, the feed it serves, the bundles it inspects and the refused ones it
 * makes. The evidence it reads is in tests/updater-smoke-evidence.test.mjs.
 */

const MACOS = process.platform === 'darwin' ? false : 'the bundles are macOS apps'
const WINDOWS = process.platform === 'win32' && 'the tree under test is made by sh'

/** A folder of this test's own, made and removed round `body`. */
function inAFolder(body) {
  const folder = mkdtempSync(join(tmpdir(), 'cf-updater-kit-'))
  try {
    return body(folder)
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
}

describe('the version the update is given', () => {
  it('reads semver’s order: pre-releases by number, a release after its pre-releases', () => {
    const order = [
      '3.0.0-alpha.9',
      '3.0.0-alpha.10',
      '3.0.0-alpha.81',
      '3.0.0-beta.1',
      '3.0.0',
      '3.0.1',
    ]
    for (let at = 1; at < order.length; at += 1) {
      assert.equal(compareVersions(order[at - 1], order[at]), -1, `${order[at - 1]} < ${order[at]}`)
      assert.equal(compareVersions(order[at], order[at - 1]), 1)
    }
    assert.equal(compareVersions('3.0.0-alpha.81', '3.0.0-alpha.81'), 0)
    assert.deepEqual(parseVersion('3.0.0-alpha.81'), { core: [3, 0, 0], pre: ['alpha', 81] })
    assert.throws(() => parseVersion('3.0'), /not a semantic version/)
  })

  it('is the next one after the newest installed app’s, unless a release has moved past it', () => {
    assert.equal(nextVersion('3.0.0-alpha.81'), '3.0.0-alpha.82')
    assert.equal(nextVersion('3.0.0'), '3.0.1')
    assert.equal(updateVersion('3.0.0-alpha.81', ['3.0.0-alpha.81']), '3.0.0-alpha.82')
    assert.equal(updateVersion('3.0.0-alpha.80', ['3.0.0-alpha.81']), '3.0.0-alpha.82')
    assert.equal(updateVersion('3.0.0-alpha.90', ['3.0.0-alpha.81']), '3.0.0-alpha.90')
    // Two apps are installed from (the bridge and the flip), and the update is newer than both.
    assert.equal(
      updateVersion('3.0.0-alpha.82', ['3.0.0-alpha.81', '3.0.0-alpha.82']),
      '3.0.0-alpha.83',
    )
    assert.equal(
      updateVersion('3.0.0-alpha.83', ['3.0.0-alpha.82', '3.0.0-alpha.81']),
      '3.0.0-alpha.83',
    )
    assert.equal(newest(['3.0.0-alpha.9', '3.0.0-alpha.10', '3.0.0-alpha.2']), '3.0.0-alpha.10')
  })

  it('has the flip release the newest tag after the bridge’s, and none where there is none', () => {
    const bridge = 'v3.0.0-alpha.81'
    assert.equal(
      flipTag(['v3.0.0-alpha.80', bridge, 'v3.0.0-alpha.82', 'v3.0.0-alpha.9'], bridge),
      'v3.0.0-alpha.82',
    )
    // Release numbers are numbers, and what is no release tag is none.
    assert.equal(
      flipTag([bridge, 'v3.0.0-alpha.100', 'v3.0.0-alpha.99', 'not-a-tag', 'v3.0'], bridge),
      'v3.0.0-alpha.100',
    )
    assert.throws(() => flipTag([], bridge), /no release newer than the bridge/)
    assert.throws(
      () => flipTag(['v3.0.0-alpha.80', bridge], bridge),
      /name the flip with --flip-ref/,
    )
  })
})

describe('the releases the update goes to', () => {
  it('are the bridge, whose daemon and `cf setup` are Node’s, and the flip, whose are the native cf’s', () => {
    assert.deepEqual(RELEASES, {
      bridge: { daemon: 'node', setup: 'node' },
      flip: { daemon: 'native', setup: 'native' },
    })
    assert.equal(BRIDGE_TAG, 'v3.0.0-alpha.81')
  })

  it('have the flip among the tags of this checkout’s history, but for the one under test', {
    skip: process.platform === 'win32' && 'git is run as sh runs it',
  }, () => {
    inAFolder((folder) => {
      const git = (...args) =>
        execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.com', ...args], {
          cwd: folder,
          encoding: 'utf8',
          stdio: ['ignore', 'pipe', 'pipe'],
        })
      const commit = (what, tag) => {
        writeFileSync(join(folder, 'f'), what)
        git('add', 'f')
        git('commit', '-q', '-m', what)
        if (tag !== undefined) git('tag', tag)
      }
      git('init', '-q')
      commit('the release before the bridge', 'v3.0.0-alpha.80')
      commit('the bridge', 'v3.0.0-alpha.81')
      commit('the flip', 'v3.0.0-alpha.82')
      // A tag that is no release of ours is no release.
      git('tag', 'deploy-1')
      commit('the release under test, tagged', 'v3.0.0-alpha.83')
      // On its commit, that tag is the release under test and not one it follows.
      assert.deepEqual(earlierReleases(folder).sort(), [
        'v3.0.0-alpha.80',
        'v3.0.0-alpha.81',
        'v3.0.0-alpha.82',
      ])
      assert.equal(flipRelease(folder), 'v3.0.0-alpha.82')
      // A commit after it, untagged: the tag is behind it, and is the newest release there is.
      commit('after it')
      assert.equal(earlierReleases(folder).length, 4)
      assert.equal(flipRelease(folder), 'v3.0.0-alpha.83')
    })
  })
})

describe('what a build is given', () => {
  it('is the override of the run’s public key, and of the version where the build is the update', () => {
    assert.deepEqual(overrideConfig({ publicKey: 'KEY' }), {
      plugins: { updater: { pubkey: 'KEY' } },
    })
    assert.deepEqual(overrideConfig({ publicKey: 'KEY', version: '3.0.0-alpha.82' }), {
      plugins: { updater: { pubkey: 'KEY' } },
      version: '3.0.0-alpha.82',
    })
  })

  it('inherits no updater key, Apple certificate or identity, and is offline', () => {
    const base = {
      PATH: '/usr/bin',
      HOME: '/home/someone',
      TAURI_SIGNING_PRIVATE_KEY: 'production',
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'password',
      TAURI_SIGNING_PRIVATE_KEY_PATH: '/home/someone/.tauri/consensflow.key',
      TAURI_PRIVATE_KEY: 'older production',
      APPLE_CERTIFICATE: 'certificate',
      APPLE_SIGNING_IDENTITY: 'Developer ID Application: Someone',
      APPLE_ID: 'someone@example.com',
      APPLE_API_KEY: 'key',
      CSC_LINK: 'certificate',
    }
    const env = cleanEnv(base)
    assert.deepEqual(Object.keys(env).sort(), ['CARGO_NET_OFFLINE', 'HOME', 'PATH'])
    assert.equal(env.CARGO_NET_OFFLINE, 'true')
    assert.equal(base.TAURI_SIGNING_PRIVATE_KEY, 'production', 'the caller’s own is not touched')
  })

  it('names no key of the product’s and no home folder to look for one in', () => {
    const files = [
      join(REPO, 'tests', 'smoke-updater.mjs'),
      join(REPO, 'tests', 'updater-smoke.test.mjs'),
      ...readdirSync(join(REPO, 'tests', 'updater-smoke')).map((name) =>
        join(REPO, 'tests', 'updater-smoke', name),
      ),
    ]
    for (const file of files) {
      const text = readFileSync(file, 'utf8')
      assert.ok(!/homedir|\.tauri\/|\.tauri'/.test(text), `${file} looks where a key lives`)
      assert.ok(!/TAURI_SIGNING_PRIVATE_KEY/.test(text), `${file} names the product’s key variable`)
    }
  })

  it('is held to its key: the executable has the run’s and not the product’s', () => {
    inAFolder((folder) => {
      const app = join(folder, 'ConsensFlow.app')
      mkdirSync(join(app, 'Contents', 'MacOS'), { recursive: true })
      const executable = join(app, 'Contents', 'MacOS', 'app')
      writeFileSync(executable, 'binary RUNKEY binary')
      assertBuiltWith(app, 'RUNKEY', 'PRODUCTKEY', 'the app')
      writeFileSync(executable, 'binary PRODUCTKEY binary')
      assert.throws(() => assertBuiltWith(app, 'RUNKEY', 'PRODUCTKEY', 'the app'), /not built with/)
      writeFileSync(executable, 'binary RUNKEY PRODUCTKEY binary')
      assert.throws(
        () => assertBuiltWith(app, 'RUNKEY', 'PRODUCTKEY', 'the app'),
        /carries the product/,
      )
    })
  })
})

describe('the table of processes', () => {
  it('reads ps’s rows: pid, parent, state and the whole command', () => {
    const rows = parseTable(
      '  101     1 Ss   /a/b app --flag\n 202   101 S+   /x y z\nnot a row\n  303   202 Z    (defunct)\n',
    )
    assert.deepEqual(rows, [
      { pid: 101, ppid: 1, state: 'Ss', command: '/a/b app --flag' },
      { pid: 202, ppid: 101, state: 'S+', command: '/x y z' },
      { pid: 303, ppid: 202, state: 'Z', command: '(defunct)' },
    ])
  })

  it('finds everything under a process: its children, theirs, a chain of them', () => {
    const at = (pid, ppid) => ({ pid, ppid, state: 'S', command: 'x' })
    const table = [at(1, 0), at(2, 1), at(3, 2), at(4, 3), at(5, 1), at(9, 8)]
    assert.deepEqual(descendants(table, 1).sort(), [2, 3, 4, 5])
    assert.deepEqual(descendants(table, 3), [4])
    assert.deepEqual(descendants(table, 7), [])
  })

  it('ends a process and all that is under it, which a group does not hold', {
    skip: WINDOWS,
  }, async () => {
    // A shell with two sleeps under it, and a shell under that with another: three levels.
    const root = spawn(
      '/bin/sh',
      ['-c', '/bin/sleep 60 & /bin/sh -c "/bin/sleep 60 & wait" & wait'],
      {
        stdio: 'ignore',
      },
    )
    let tree = [root.pid]
    try {
      await until('the tree is up', () => descendants(processTable(), root.pid).length >= 3, 10_000)
      tree = [root.pid, ...descendants(processTable(), root.pid)]
      killTree(root.pid)
      await until('the tree is gone', () => tree.every((pid) => !alive(pid)), 10_000)
    } finally {
      // Whatever the test found, nothing of it is left running to hold the suite up.
      for (const pid of tree) {
        try {
          process.kill(pid, 'SIGKILL')
        } catch {
          // Already gone, which is what the test is about.
        }
      }
    }
  })
})

describe('the run’s updater key', () => {
  const cli = existsSync(tauriCli(REPO)) ? false : 'the Tauri CLI is not installed'

  it('is made for the run, signs what it is asked to and stays in the folder it was given', {
    skip: cli,
  }, () => {
    inAFolder((folder) => {
      const first = generateKey(REPO, join(folder, 'first'))
      const second = generateKey(REPO, join(folder, 'second'))
      assert.ok(first.privateKey.startsWith(folder), 'the key is made where it was told')
      assert.match(first.publicKey, /^[A-Za-z0-9+/]+=*$/, 'a public key is one line of base64')
      assert.notEqual(first.publicKey, second.publicKey, 'a new pair each time')
      const archive = join(folder, 'archive.tar.gz')
      writeFileSync(archive, 'bytes')
      const signature = signFile(REPO, first.privateKey, archive)
      assert.match(signature, /^[A-Za-z0-9+/]+=*$/, 'a signature is one line of base64')
      assert.equal(signature, readFileSync(`${archive}.sig`, 'utf8').trim())
      assert.notEqual(
        signFile(REPO, second.privateKey, archive),
        signature,
        'another key, another signature',
      )
    })
  })
})

describe('the feed the apps ask', () => {
  it('offers a release the way a release’s feed does: a versioned GitHub asset, signed', () => {
    const document = feedDocument('3.0.0-alpha.82', 'SIGNATURE')
    const platform = document.platforms['darwin-aarch64']
    assert.equal(document.version, '3.0.0-alpha.82')
    assert.equal(platform.signature, 'SIGNATURE')
    assert.equal(
      platform.url,
      `https://github.com/ngvoicu/consensflow/releases/download/v3.0.0-alpha.82/${archiveName('3.0.0-alpha.82')}`,
    )
    assert.match(archiveName('3.0.0-alpha.82'), /^[A-Za-z0-9._-]+\.app\.tar\.gz$/)
    assert.ok(archiveName('3.0.0-alpha.82').includes('3.0.0-alpha.82'))
    assert.ok(document.pub_date && document.notes)
  })

  it('serves the feed and the archive when offered, and not found when not', {
    skip: MACOS,
  }, async () => {
    const folder = mkdtempSync(join(tmpdir(), 'cf-updater-feed-'))
    const tls = makeTls(folder)
    const served = await serveUpdates(tls)
    const get = (path) =>
      new Promise((resolve, reject) => {
        const url = new URL(path, served.url)
        request(url, { ca: readFileSync(tls.caCert) }, (response) => {
          const chunks = []
          response.on('data', (chunk) => chunks.push(chunk))
          response.on('end', () =>
            resolve({ status: response.statusCode, body: Buffer.concat(chunks) }),
          )
        })
          .on('error', reject)
          .end()
      })
    try {
      assert.equal((await get('/feed')).status, 404, 'nothing offered yet')
      served.offer('3.0.0-alpha.82', 'SIGNATURE', Buffer.from('archive bytes'))
      const feed = await get('/feed')
      assert.equal(feed.status, 200)
      assert.deepEqual(JSON.parse(feed.body), feedDocument('3.0.0-alpha.82', 'SIGNATURE'))
      assert.equal((await get('/archive')).body.toString(), 'archive bytes')
      assert.equal((await get('/elsewhere')).status, 404)
      served.withdraw()
      assert.equal((await get('/feed')).status, 404, 'withdrawn')
      assert.equal((await get('/archive')).status, 404)
    } finally {
      await served.close()
      rmSync(folder, { recursive: true, force: true })
    }
  })
})

/** A signed bundle as the Rust tests of the installed app’s check build one: macOS programs under real names. */
function fakeBundle(
  parent,
  {
    version = '3.0.0-alpha.82',
    identity = IDENTITY,
    node = true,
    cf = true,
    buildVersion = version,
  } = {},
) {
  const app = join(parent, 'ConsensFlow.app')
  for (const dir of ['Contents/MacOS', 'Contents/Resources/cli/bin']) {
    mkdirSync(join(app, dir), { recursive: true })
  }
  copyFileSync('/bin/echo', join(app, 'Contents', 'MacOS', 'app'))
  writeFileSync(
    join(app, 'Contents', 'Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${identity}</string><key>CFBundleExecutable</key><string>app</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>${buildVersion}</string><key>CFBundleShortVersionString</key><string>${version}</string></dict></plist>`,
  )
  if (cf) {
    const file = join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf')
    copyFileSync('/bin/echo', file)
    execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', file], { stdio: 'pipe' })
  }
  if (node) {
    copyFileSync('/bin/echo', join(app, 'Contents', 'MacOS', 'node'))
    for (const dir of ['hosts', 'src']) {
      mkdirSync(join(app, 'Contents', 'Resources', 'cli', dir), { recursive: true })
    }
    writeFileSync(join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs'), 'test Node-era file')
    writeFileSync(
      join(app, 'Contents', 'Resources', 'cli', 'package.json'),
      `{"name":"consensflow","version":"${version}"}`,
    )
  }
  chmodSync(join(app, 'Contents', 'MacOS', 'app'), 0o755)
  execFileSync('/usr/bin/codesign', ['--force', '--deep', '--sign', '-', app], { stdio: 'pipe' })
  return app
}

describe('the bundles the smoke inspects', { skip: MACOS }, () => {
  it('takes what the installed app’s check takes: the flip’s bundle with Node, and the one without', () => {
    inAFolder((folder) => {
      const withNode = inspectBundle(fakeBundle(join(folder, 'a')), 'with Node')
      assert.deepEqual(
        [withNode.version, withNode.node, withNode.cliVersion],
        ['3.0.0-alpha.82', true, '3.0.0-alpha.82'],
      )
      const without = inspectBundle(fakeBundle(join(folder, 'b'), { node: false }), 'without Node')
      assert.deepEqual(
        [without.version, without.node, without.cliVersion],
        ['3.0.0-alpha.82', false, null],
      )
      verifySeal(withNode.app)
      verifySeal(without.app)
      assertAdHoc(without.app, 'without Node')
    })
  })

  it('refuses another app, two versions in one plist, a bundle with no cf, and half of Node', () => {
    inAFolder((folder) => {
      const refuse = (options, words) => {
        const parent = mkdtempSync(join(folder, 'case-'))
        assert.throws(() => inspectBundle(fakeBundle(parent, options), 'the bundle'), words)
      }
      refuse({ identity: 'dev.example.other' }, /not dev\.ngvoicu\.consensflow/)
      refuse({ buildVersion: '3.0.0-alpha.99' }, /two versions differ/)
      refuse({ cf: false }, /no window's cf/)
      const half = fakeBundle(mkdtempSync(join(folder, 'half-')))
      rmSync(join(half, 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs'))
      assert.throws(() => inspectBundle(half, 'the bundle'), /some of Node's files and not all/)
    })
  })

  it('makes the refused bundles the way the smoke needs them: one the seal passes, and one it does not', () => {
    inAFolder((folder) => {
      const update = fakeBundle(join(folder, 'update'))
      const without = refusedBundle('without-cf', update, join(folder, 'refused'))
      verifySeal(without)
      assert.ok(!existsSync(join(without, 'Contents', 'Resources', 'cli', 'bin', 'cf')))
      const tampered = refusedBundle('tampered', update, join(folder, 'refused'))
      assert.throws(() => verifySeal(tampered), /code|seal|invalid|modified/i)
      assert.ok(existsSync(join(tampered, 'Contents', 'Resources', 'cli', 'bin', 'cf')))
      verifySeal(update)
      assert.deepEqual(Object.keys(REFUSED), ['without-cf', 'tampered'])
    })
  })

  it('copies a bundle whole, replacing what was there, or over it with the old files left', () => {
    inAFolder((folder) => {
      const update = fakeBundle(join(folder, 'update'))
      const place = join(folder, 'Applications', 'ConsensFlow.app')
      copyBundle(update, place)
      assert.deepEqual(digestManifest(place), digestManifest(update))
      writeFileSync(join(place, 'Contents', 'stale'), 'an old file')
      copyOver(update, place)
      assert.ok(
        existsSync(join(place, 'Contents', 'stale')),
        'a copy over keeps what the new one lacks',
      )
      copyBundle(update, place)
      assert.deepEqual(
        digestManifest(place),
        digestManifest(update),
        'a replacement leaves none of it',
      )
    })
  })

  it('archives the app as a release does, with ConsensFlow.app at the root', () => {
    inAFolder((folder) => {
      const update = fakeBundle(join(folder, 'update'))
      const archive = archiveOf(update, join(folder, 'out', 'a.tar.gz'))
      const listed = execFileSync('/usr/bin/tar', ['-tzf', archive], { encoding: 'utf8' })
        .split('\n')
        .filter(Boolean)
      assert.ok(
        listed.every((name) => name.startsWith('ConsensFlow.app')),
        listed.join('\n'),
      )
      assert.throws(
        () => archiveOf(copyBundle(update, join(folder, 'Other.app')), join(folder, 'b.tar.gz')),
        /holds ConsensFlow\.app/,
      )
    })
  })
})
