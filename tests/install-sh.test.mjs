// The Mac installer (install/install.sh) against a scripted feed: what it
// reads, every check it makes before it installs, and how it installs. Its
// test copies trust a test key; the apps are fake ones, signed ad hoc, in real
// DMGs, so the copies take an ad-hoc signature with ConsensFlow's identifier
// where the script asks for ConsensFlow's Developer ID, and ask a stand-in
// Gatekeeper, except in the cases that hold those two checks. The run against a
// real release, signed and notarized, is install/real-install.mjs.

import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
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
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import {
  feedOf,
  INSTALL_SH,
  makeKey,
  scriptedRelease,
  serve,
  signedName,
  sumsOf,
  testCopy,
  VERSION_ORDER,
} from '../install/kit.mjs'

const mac = process.platform === 'darwin'
const REQUIREMENT =
  'requirement="anchor apple generic and identifier \\"dev.ngvoicu.consensflow\\" and certificate leaf[subject.OU] = \\"$team\\""'
const AD_HOC = `requirement='identifier "dev.ngvoicu.consensflow"'`
const SPCTL = 'spctl=/usr/sbin/spctl'

// A launcher: a program that runs its arguments as its child and answers its status, which a
// test puts inside an app, as a ConsensFlow window's shell runs under the app's own programs.
const LAUNCHER = `#include <spawn.h>
#include <sys/wait.h>
extern char **environ;
int main(int argc, char **argv) {
  pid_t pid;
  int status;
  if (argc < 2 || posix_spawn(&pid, argv[1], 0, 0, argv + 1, environ) != 0) return 127;
  waitpid(pid, &status, 0);
  return WIFEXITED(status) ? WEXITSTATUS(status) : 1;
}
`

let work
let key
let server
const files = {}
const dmgs = {}

/**
 * A fake ConsensFlow.app of `version` in `dir`, signed ad hoc as `identifier`, with `extra`
 * among its resources where it is given (another build of the same version).
 */
function fakeApp(dir, version, { identifier = 'dev.ngvoicu.consensflow', extra } = {}) {
  const app = path.join(dir, 'ConsensFlow.app')
  mkdirSync(path.join(app, 'Contents/MacOS'), { recursive: true })
  writeFileSync(
    path.join(app, 'Contents/Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>app</string>
<key>CFBundleIdentifier</key><string>${identifier}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>${version}</string>
</dict></plist>
`,
  )
  writeFileSync(path.join(app, 'Contents/MacOS/app'), '#!/bin/sh\nexit 0\n')
  chmodSync(path.join(app, 'Contents/MacOS/app'), 0o755)
  if (extra) {
    mkdirSync(path.join(app, 'Contents/Resources'))
    writeFileSync(path.join(app, 'Contents/Resources/extra'), extra)
  }
  run('codesign', ['-s', '-', '--force', '-i', identifier, app])
  return app
}

/** A DMG holding `app`, as bytes. */
function dmgOf(app, name) {
  const folder = mkdtempSync(path.join(work, 'dmg-'))
  run('ditto', [app, path.join(folder, 'ConsensFlow.app')])
  const out = path.join(work, name)
  run('hdiutil', [
    'create',
    '-quiet',
    '-fs',
    'HFS+',
    '-volname',
    'ConsensFlow',
    '-srcfolder',
    folder,
    '-format',
    'UDZO',
    out,
  ])
  return readFileSync(out)
}

function run(program, args) {
  const ran = spawnSync(program, args, { encoding: 'utf8' })
  assert.equal(ran.status, 0, `${program} ${args.join(' ')}: ${ran.stderr}`)
  return ran.stdout
}

function dmgName(version) {
  return `ConsensFlow_${version}_aarch64.dmg`
}

/** Puts the scripted release `version` on the server, with `options` for scriptedRelease. */
function publish(version, options = {}, artifacts = { [dmgName(version)]: dmgs[version] }) {
  for (const at of Object.keys(files)) {
    if (at.includes(`/v${version}/`)) delete files[at]
  }
  Object.assign(files, scriptedRelease('/releases', version, artifacts, key, options))
}

/** A fresh case: its own folders, and a test copy made with `replacements`. */
function freshCase(
  replacements = [
    [REQUIREMENT, AD_HOC],
    [SPCTL, `spctl=${path.join(work, 'spctl')}`],
    ['quit_wait=120', 'quit_wait=4'],
  ],
) {
  const dir = mkdtempSync(path.join(work, 'case-'))
  const script = testCopy(INSTALL_SH, path.join(dir, 'install.sh'), key.keyLine, replacements)
  mkdirSync(path.join(dir, 'tmp'))
  mkdirSync(path.join(dir, 'Applications'))
  return { dir, script, apps: path.join(dir, 'Applications'), tmp: path.join(dir, 'tmp') }
}

/** Runs a case's script with `args` (and `env`) under `shell`, started by `launcher` where one is given. */
function install(at, args = [], { env = {}, shell = '/bin/sh', launcher } = {}) {
  return new Promise((resolve) => {
    const [program, words] = launcher
      ? [launcher, [shell, at.script, ...args]]
      : [shell, [at.script, ...args]]
    const child = spawn(program, words, {
      env: {
        PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
        HOME: at.dir,
        TMPDIR: at.tmp,
        CONSENSFLOW_RELEASES_URL: `${server.url}/releases`,
        CONSENSFLOW_INSTALL_DIR: at.apps,
        ...env,
      },
    })
    let out = ''
    child.stdout.on('data', (chunk) => {
      out += chunk
    })
    child.stderr.on('data', (chunk) => {
      out += chunk
    })
    child.on('close', (status) => resolve({ status, out }))
  })
}

function installedVersion(at) {
  const plist = path.join(at.apps, 'ConsensFlow.app/Contents/Info.plist')
  if (!existsSync(plist)) return null
  return spawnSync('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', plist], {
    encoding: 'utf8',
  }).stdout.trim()
}

/** What a refused case must leave: nothing installed, nothing in its folder, and its temporary folder gone. */
function assertRefused(at, ran, words) {
  assert.equal(ran.status, 1, ran.out)
  assert.match(ran.out, words)
  assert.match(ran.out, /nothing was installed/)
  assert.deepEqual(readdirSync(at.apps), [], 'nothing is left where the app goes')
  assert.deepEqual(readdirSync(at.tmp), [], "the run's own folder is gone")
}

function assertInstalled(at, ran, version) {
  assert.equal(ran.status, 0, ran.out)
  assert.equal(installedVersion(at), version)
  assert.deepEqual(
    readdirSync(at.apps),
    ['ConsensFlow.app'],
    'no copy or old app is left beside it',
  )
  assert.deepEqual(readdirSync(at.tmp), [], "the run's own folder is gone")
  run('codesign', ['--verify', '--deep', '--strict', path.join(at.apps, 'ConsensFlow.app')])
}

/** A stay-open AppleScript applet as `dir`/ConsensFlow.app, which quits when asked unless `refuses`. */
function applet(dir, refuses) {
  const source = refuses
    ? 'on idle\nreturn 1\nend idle\non quit\nend quit'
    : 'on idle\nreturn 1\nend idle'
  const app = path.join(dir, 'ConsensFlow.app')
  run('osacompile', ['-s', '-o', app, '-e', source])
  return app
}

function processesOf(app) {
  return spawnSync('ps', ['-axo', 'pid=,comm='], { encoding: 'utf8' })
    .stdout.split('\n')
    .filter((line) => line.includes(`${app}/Contents/`))
    .map((line) => Number(line.trim().split(/\s+/)[0]))
}

/** A folder of stand-ins for the programs `scripts` names (`{ name: shell text }`). */
function standIns(at, scripts) {
  const bin = path.join(at.dir, 'bin')
  mkdirSync(bin, { recursive: true })
  for (const [name, text] of Object.entries(scripts)) {
    writeFileSync(path.join(bin, name), `#!/bin/sh\n${text}\n`, { mode: 0o755 })
  }
  return `${bin}:/usr/bin:/bin:/usr/sbin:/sbin`
}

/** What a refusal after an installed ConsensFlow was found must leave: that one, as it was. */
function assertKept(at, ran, words, version) {
  assert.equal(ran.status, 1, ran.out)
  assert.match(ran.out, words)
  assert.match(ran.out, /nothing was installed/)
  assert.equal(installedVersion(at), version, 'the installed app is the one that was there')
  assert.deepEqual(readdirSync(at.apps), ['ConsensFlow.app'], 'no copy is left beside it')
  assert.deepEqual(readdirSync(at.tmp), [], "the run's own folder is gone")
}

async function waitFor(check, what) {
  for (let i = 0; i < 100; i++) {
    if (check()) return
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  assert.fail(`${what} did not happen in 10 s`)
}

test('install.sh orders versions as SemVer does', () => {
  const lines = readFileSync(INSTALL_SH, 'utf8').split('\n')
  const from = lines.indexOf('# version order: begin')
  const to = lines.indexOf('# version order: end')
  assert.ok(from >= 0 && to > from, 'install.sh marks its order of versions')
  const order = lines.slice(from + 1, to).join('\n')
  for (const [a, b, newer] of VERSION_ORDER) {
    const ran = spawnSync('/bin/sh', ['-c', `${order}\nnewer_than "$1" "$2"`, 'sh', a, b])
    assert.equal(ran.status === 0, newer, `${a} after ${b}`)
  }
})

test('the official download follows a redirect to https only', () => {
  const fetch = readFileSync(INSTALL_SH, 'utf8')
    .split('\n')
    .filter((line) => line.includes('curl '))
  assert.ok(
    fetch.some((line) => line.includes("--proto '=https' --proto-redir '=https' --tlsv1.2")),
    fetch.join('\n'),
  )
})

test('on Linux, install.sh refuses in plain words and installs nothing', {
  skip: process.platform !== 'linux' && 'this is not Linux',
}, () => {
  const ran = spawnSync('sh', [INSTALL_SH], {
    encoding: 'utf8',
    env: { ...process.env, CONSENSFLOW_RELEASES_URL: 'http://127.0.0.1:9/none' },
  })
  assert.equal(ran.status, 1, ran.stdout + ran.stderr)
  assert.equal(
    ran.stderr,
    'consensflow: ConsensFlow is made for macOS and Windows only; there is no Linux build.\nconsensflow: nothing was installed.\n',
  )
})

describe('install.sh', { skip: !mac && "the Mac's installer runs on macOS" }, () => {
  before(async () => {
    work = mkdtempSync(path.join(os.tmpdir(), 'cf-install-sh-'))
    key = makeKey()
    for (const version of ['3.0.7-alpha.1', '3.0.7-alpha.2', '3.0.7']) {
      const folder = mkdtempSync(path.join(work, 'app-'))
      dmgs[version] = dmgOf(fakeApp(folder, version), dmgName(version))
    }
    // Another build of 3.0.7, which no SHA256SUMS here lists.
    dmgs.other = dmgOf(
      fakeApp(mkdtempSync(path.join(work, 'app-')), '3.0.7', { extra: 'another build' }),
      'other.dmg',
    )
    const notarized = path.join(work, 'spctl')
    writeFileSync(
      notarized,
      '#!/bin/sh\necho "$5: accepted"\necho "source=Notarized Developer ID"\n',
    )
    chmodSync(notarized, 0o755)
    Object.assign(
      files,
      feedOf('/releases', 'stable', '3.0.7'),
      feedOf('/releases', 'alpha', '3.0.7-alpha.2'),
    )
    for (const version of Object.keys(dmgs)) publish(version)
    server = await serve(files)
  })

  after(async () => {
    await server?.close()
    if (work) rmSync(work, { recursive: true, force: true })
  })

  describe('what it installs', () => {
    test("the stable feed's release, by default", async () => {
      const at = freshCase()
      const ran = await install(at)
      assertInstalled(at, ran, '3.0.7')
      assert.match(ran.out, /SHA256SUMS is signed by ConsensFlow's update key, for 3\.0\.7/)
      assert.match(ran.out, /has the SHA-256 the signed SHA256SUMS lists/)
    })

    test("the alpha feed's release with --alpha, and with CONSENSFLOW_CHANNEL=alpha", async () => {
      const flag = freshCase()
      assertInstalled(flag, await install(flag, ['--alpha']), '3.0.7-alpha.2')
      const env = freshCase()
      assertInstalled(
        env,
        await install(env, [], { env: { CONSENSFLOW_CHANNEL: 'alpha' } }),
        '3.0.7-alpha.2',
      )
    })

    test('the release --version names, without reading a feed, and v before it is taken', async () => {
      const at = freshCase()
      const from = server.asked.length
      assertInstalled(at, await install(at, ['--version', 'v3.0.7-alpha.1']), '3.0.7-alpha.1')
      assert.ok(
        !server.asked.slice(from).some((asked) => asked.includes('/feed-')),
        'no feed was read',
      )
      const env = freshCase()
      assertInstalled(
        env,
        await install(env, [], { env: { CONSENSFLOW_VERSION: '3.0.7-alpha.1' } }),
        '3.0.7-alpha.1',
      )
      const equals = freshCase()
      assertInstalled(equals, await install(equals, ['--version=3.0.7-alpha.1']), '3.0.7-alpha.1')
    })

    test('it downloads only the files the release names: SHA256SUMS, its signature and the DMG', async () => {
      const at = freshCase()
      const from = server.asked.length
      assertInstalled(at, await install(at, ['--version', '3.0.7']), '3.0.7')
      assert.deepEqual(server.asked.slice(from), [
        '/releases/download/v3.0.7/SHA256SUMS',
        '/releases/download/v3.0.7/SHA256SUMS.sig',
        `/releases/download/v3.0.7/${dmgName('3.0.7')}`,
      ])
    })

    test('it installs when the script comes on its input, as curl | sh gives it', async () => {
      const at = freshCase()
      const piped = await new Promise((resolve) => {
        const child = spawn('/bin/sh', ['-s', '--', '--version', '3.0.7'], {
          env: {
            PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
            HOME: at.dir,
            TMPDIR: at.tmp,
            CONSENSFLOW_RELEASES_URL: `${server.url}/releases`,
            CONSENSFLOW_INSTALL_DIR: at.apps,
          },
        })
        let out = ''
        child.stdout.on('data', (chunk) => {
          out += chunk
        })
        child.stderr.on('data', (chunk) => {
          out += chunk
        })
        child.on('close', (status) => resolve({ status, out }))
        child.stdin.end(readFileSync(at.script))
      })
      assertInstalled(at, piped, '3.0.7')
    })

    test('an installed ConsensFlow is upgraded where it is', async () => {
      const at = freshCase()
      assertInstalled(at, await install(at, ['--version', '3.0.7-alpha.1']), '3.0.7-alpha.1')
      assertInstalled(at, await install(at, ['--version', '3.0.7']), '3.0.7')
    })

    test("it runs under dash as under the Mac's sh", {
      skip: !existsSync('/bin/dash') && 'no dash here',
    }, async () => {
      const at = freshCase()
      assertInstalled(at, await install(at, [], { shell: '/bin/dash' }), '3.0.7')
    })
  })

  describe('what it refuses before it downloads the app', () => {
    test('Linux, in plain words', async () => {
      const at = freshCase()
      const bin = path.join(at.dir, 'bin')
      mkdirSync(bin)
      writeFileSync(path.join(bin, 'uname'), '#!/bin/sh\necho Linux\n')
      chmodSync(path.join(bin, 'uname'), 0o755)
      const from = server.asked.length
      const ran = await install(at, [], { env: { PATH: `${bin}:/usr/bin:/bin:/usr/sbin:/sbin` } })
      assertRefused(
        at,
        ran,
        /ConsensFlow is made for macOS and Windows only; there is no Linux build\./,
      )
      assert.equal(server.asked.length, from, 'nothing was downloaded')
    })

    test('an Intel Mac', async () => {
      const at = freshCase()
      const bin = path.join(at.dir, 'bin')
      mkdirSync(bin)
      writeFileSync(path.join(bin, 'sysctl'), '#!/bin/sh\necho 0\n')
      chmodSync(path.join(bin, 'sysctl'), 0o755)
      const ran = await install(at, [], { env: { PATH: `${bin}:/usr/bin:/bin:/usr/sbin:/sbin` } })
      assertRefused(at, ran, /made for Apple silicon/)
    })

    test('an option it does not know, and a version that is no release number', async () => {
      const at = freshCase()
      const unknown = await install(at, ['--beta'])
      assert.equal(unknown.status, 2)
      assert.match(unknown.out, /unknown option '--beta'/)
      const bad = await install(at, ['--version', 'latest; rm -rf /'])
      assert.equal(bad.status, 2)
      assert.match(bad.out, /a version is a release number/)
      const channel = await install(at, [], { env: { CONSENSFLOW_CHANNEL: 'nightly' } })
      assert.equal(channel.status, 2)
    })

    test('--help says how to install and uninstall, and installs nothing', async () => {
      const at = freshCase()
      const ran = await install(at, ['--help'])
      assert.equal(ran.status, 0)
      assert.match(ran.out, /--alpha/)
      assert.match(ran.out, /Uninstall: quit ConsensFlow and move ConsensFlow\.app/)
      assert.deepEqual(readdirSync(at.apps), [])
    })

    test('a release with no signed SHA256SUMS: those before 3.0.7-alpha.1', async () => {
      publish('3.0.6', { signature: null }, { [dmgName('3.0.6')]: dmgs['3.0.7'] })
      const at = freshCase()
      const ran = await install(at, ['--version', '3.0.6'])
      assertRefused(
        at,
        ran,
        /ConsensFlow 3\.0\.6 has no signed SHA256SUMS[\s\S]*before 3\.0\.7-alpha\.1/,
      )
    })

    test('a feed that names no version', async () => {
      Object.assign(files, { '/releases/download/feed-alpha/latest.json': '{"notes":"none"}' })
      try {
        const at = freshCase()
        assertRefused(
          at,
          await install(at, ['--alpha']),
          /the alpha update feed at .* names no version/,
        )
      } finally {
        Object.assign(files, feedOf('/releases', 'alpha', '3.0.7-alpha.2'))
      }
    })
  })

  describe('the signature of SHA256SUMS', () => {
    const flip = (base64, at) => {
      const bytes = Buffer.from(base64, 'base64')
      bytes[at] ^= 1
      return bytes.toString('base64')
    }
    /** The .sig with line `line` (0-3) of its text changed by `change`. */
    const changeLine = (sig, line, change) => {
      const lines = Buffer.from(sig, 'base64').toString('latin1').split('\n')
      lines[line] = change(lines[line])
      return Buffer.from(lines.join('\n'), 'latin1').toString('base64')
    }
    const cases = [
      [
        'a flipped bit in the signature',
        { signature: (_, sig) => changeLine(sig, 1, (line) => flip(line, 40)) },
        /the signature does not match the file/,
      ],
      [
        'a flipped bit in SHA256SUMS',
        { sums: undefined, flipSums: true },
        /the signature does not match the file/,
      ],
      [
        'a flipped bit in the trusted comment',
        {
          signature: (_, sig) =>
            changeLine(sig, 2, (line) => line.replace('timestamp:1', 'timestamp:3')),
        },
        /the trusted comment's signature does not match/,
      ],
      [
        'a flipped bit in the global signature',
        { signature: (_, sig) => changeLine(sig, 3, (line) => flip(line, 10)) },
        /the trusted comment's signature does not match/,
      ],
      [
        'a signature by another key',
        { signature: (text) => makeKey().sign(Buffer.from(text, 'latin1'), signedName('3.0.7')) },
        /made by another key/,
      ],
      [
        "a signature of another key with this key's id",
        {
          signature: (text, sig) => {
            const other = makeKey().sign(Buffer.from(text, 'latin1'), signedName('3.0.7'))
            const id = Buffer.from(
              Buffer.from(sig, 'base64').toString('latin1').split('\n')[1],
              'base64',
            ).subarray(2, 10)
            return changeLine(other, 1, (line) => {
              const bytes = Buffer.from(line, 'base64')
              id.copy(bytes, 2)
              return bytes.toString('base64')
            })
          },
        },
        /the signature does not match the file/,
      ],
      [
        'a SHA256SUMS signed for another release',
        { signature: (text) => key.sign(Buffer.from(text, 'latin1'), signedName('3.0.7-alpha.1')) },
        /the signature is for ConsensFlow_3\.0\.7-alpha\.1_SHA256SUMS, not ConsensFlow_3\.0\.7_SHA256SUMS/,
      ],
      [
        'a signature of an update archive',
        { signature: (text) => key.sign(Buffer.from(text, 'latin1'), 'ConsensFlow.app.tar.gz') },
        /the signature is for ConsensFlow\.app\.tar\.gz, not/,
      ],
      [
        'a signature whose trusted comment says more',
        {
          signature: (text) =>
            key.sign(Buffer.from(text, 'latin1'), '', {
              comment: `timestamp:1\tfile:${signedName('3.0.7')}\textra`,
            }),
        },
        /the signature is for/,
      ],
      [
        "minisign's legacy signature of the file itself (Ed)",
        {
          signature: (_, sig) =>
            changeLine(sig, 1, (line) => {
              const bytes = Buffer.from(line, 'base64')
              bytes[1] = 0x64
              return bytes.toString('base64')
            }),
        },
        /not of minisign's prehashed kind/,
      ],
      [
        'a signature file that is not one',
        { signature: () => Buffer.from('hello\n').toString('base64') },
        /is not a minisign signature/,
      ],
    ]
    for (const [name, options, words] of cases) {
      test(`${name} is refused`, async () => {
        const dmg = { [dmgName('3.0.7')]: dmgs['3.0.7'] }
        if (options.flipSums) {
          const good = sumsOf(dmg)
          const signature = key.sign(Buffer.from(good, 'latin1'), signedName('3.0.7'))
          const changed = Buffer.from(good, 'latin1')
          changed[3] ^= 1
          publish('3.0.7', { sums: changed.toString('latin1'), signature })
        } else {
          publish('3.0.7', options)
        }
        try {
          const at = freshCase()
          assertRefused(at, await install(at, ['--version', '3.0.7']), words)
        } finally {
          publish('3.0.7')
        }
      })
    }

    test('the script trusts the key it names, and no other: a copy of it trusting no test key refuses', async () => {
      const at = freshCase([])
      const real = path.join(at.dir, 'real.sh')
      copyFileSync(INSTALL_SH, real)
      at.script = real
      assertRefused(at, await install(at, ['--version', '3.0.7']), /made by another key/)
    })
  })

  describe('the DMG and the app in it', () => {
    test('a DMG with one byte changed is refused by its SHA-256', async () => {
      const tampered = Buffer.from(dmgs['3.0.7'])
      tampered[tampered.length - 600] ^= 1
      publish(
        '3.0.7',
        { sums: sumsOf({ [dmgName('3.0.7')]: dmgs['3.0.7'] }) },
        { [dmgName('3.0.7')]: tampered },
      )
      try {
        const at = freshCase()
        assertRefused(
          at,
          await install(at, ['--version', '3.0.7']),
          /ConsensFlow_3\.0\.7_aarch64\.dmg was changed: its SHA-256 is [0-9a-f]{64}, and the signed SHA256SUMS says/,
        )
      } finally {
        publish('3.0.7')
      }
    })

    test('a SHA256SUMS that lists no DMG for the release is refused', async () => {
      publish('3.0.7', { sums: sumsOf({ 'ConsensFlow_3.0.7_x64-setup.exe': 'x' }) })
      try {
        const at = freshCase()
        assertRefused(
          at,
          await install(at, ['--version', '3.0.7']),
          /3\.0\.7's SHA256SUMS lists no ConsensFlow_3\.0\.7_aarch64\.dmg/,
        )
      } finally {
        publish('3.0.7')
      }
    })

    test('an app of another version than the release is refused', async () => {
      publish('3.0.7', {}, { [dmgName('3.0.7')]: dmgs['3.0.7-alpha.1'] })
      try {
        const at = freshCase()
        assertRefused(
          at,
          await install(at, ['--version', '3.0.7']),
          /the app in ConsensFlow_3\.0\.7_aarch64\.dmg is version '3\.0\.7-alpha\.1', not 3\.0\.7/,
        )
      } finally {
        publish('3.0.7')
      }
    })

    test('an app whose code signature does not hold is refused (codesign --verify --deep --strict)', async () => {
      const folder = mkdtempSync(path.join(work, 'app-'))
      const app = fakeApp(folder, '3.0.7')
      writeFileSync(path.join(app, 'Contents/MacOS/app'), '#!/bin/sh\necho changed after signing\n')
      publish('3.0.7', {}, { [dmgName('3.0.7')]: dmgOf(app, 'broken.dmg') })
      try {
        const at = freshCase()
        assertRefused(
          at,
          await install(at, ['--version', '3.0.7']),
          /the code signature of the app in ConsensFlow_3\.0\.7_aarch64\.dmg does not hold/,
        )
      } finally {
        publish('3.0.7')
      }
    })

    test("an app not signed by ConsensFlow's Developer ID is refused (team 3K53U2H749)", async () => {
      const at = freshCase([[SPCTL, `spctl=${path.join(work, 'spctl')}`]])
      assertRefused(
        at,
        await install(at, ['--version', '3.0.7']),
        /the app in ConsensFlow_3\.0\.7_aarch64\.dmg is not signed by ConsensFlow's Developer ID \(team 3K53U2H749\)/,
      )
    })

    test('an app Gatekeeper does not accept as notarized is refused (spctl --assess)', async () => {
      const at = freshCase([[REQUIREMENT, AD_HOC]])
      assertRefused(
        at,
        await install(at, ['--version', '3.0.7']),
        /Gatekeeper (refuses|does not take) the app in ConsensFlow_3\.0\.7_aarch64\.dmg/,
      )
    })

    test("Gatekeeper's yes without notarization is refused", async () => {
      const accepted = path.join(work, 'spctl-developer-id')
      writeFileSync(accepted, '#!/bin/sh\necho "$5: accepted"\necho "source=Developer ID"\n')
      chmodSync(accepted, 0o755)
      const at = freshCase([
        [REQUIREMENT, AD_HOC],
        [SPCTL, `spctl=${accepted}`],
      ])
      assertRefused(
        at,
        await install(at, ['--version', '3.0.7']),
        /Gatekeeper does not take the app in ConsensFlow_3\.0\.7_aarch64\.dmg as notarized/,
      )
    })
  })

  describe('what is checked is what goes in', () => {
    test("the check's word is taken only from a clean exit", async () => {
      const at = freshCase()
      const PATH = standIns(at, {
        osascript: 'case "$3" in *verify.js) echo ok; exit 1 ;; esac\nexec /usr/bin/osascript "$@"',
      })
      assertRefused(
        at,
        await install(at, ['--version', '3.0.7'], { env: { PATH } }),
        /the check of 3\.0\.7's SHA256SUMS ended with status 1/,
      )
    })

    test('the hash of the DMG comes from the bytes of SHA256SUMS whose signature was checked', async () => {
      // The DMG served is another build; the list on disk is rewritten to its hash once the
      // signature of the real one was checked.
      publish(
        '3.0.7',
        { sums: sumsOf({ [dmgName('3.0.7')]: dmgs['3.0.7'] }) },
        { [dmgName('3.0.7')]: dmgs.other },
      )
      try {
        const at = freshCase()
        const other = sumsOf({ [dmgName('3.0.7')]: dmgs.other })
        writeFileSync(path.join(at.dir, 'other-sums'), other)
        const PATH = standIns(at, {
          osascript: `case "$3" in *verify.js)\n/usr/bin/osascript "$@"; s=$?\ncp "${path.join(at.dir, 'other-sums')}" "$(dirname "$3")/SHA256SUMS"\nexit $s ;;\nesac\nexec /usr/bin/osascript "$@"`,
        })
        assertRefused(
          at,
          await install(at, ['--version', '3.0.7'], { env: { PATH } }),
          /ConsensFlow_3\.0\.7_aarch64\.dmg was changed/,
        )
      } finally {
        publish('3.0.7')
      }
    })

    test('the copy that goes in is checked again: one swapped once the DMG was checked is refused', async () => {
      const at = freshCase()
      fakeApp(at.apps, '3.0.7-alpha.1')
      const swapped = fakeApp(mkdtempSync(path.join(work, 'app-')), '3.0.7', {
        identifier: 'dev.someone.else',
      })
      const PATH = standIns(at, {
        ditto: `/usr/bin/ditto "$@" || exit $?\ncase "$2" in *.installing.*) rm -rf "$2"; /usr/bin/ditto "${swapped}" "$2" ;; esac`,
      })
      assertKept(
        at,
        await install(at, ['--version', '3.0.7'], { env: { PATH } }),
        /the copy of the app made in .* is not signed by ConsensFlow's Developer ID/,
        '3.0.7-alpha.1',
      )
    })

    test('an open ConsensFlow is asked to quit only once the copy that goes in has passed its checks', async () => {
      const at = freshCase()
      const app = applet(at.apps, false)
      const swapped = fakeApp(mkdtempSync(path.join(work, 'app-')), '3.0.7', {
        identifier: 'dev.someone.else',
      })
      const PATH = standIns(at, {
        ditto: `/usr/bin/ditto "$@" || exit $?\ncase "$2" in *.installing.*) rm -rf "$2"; /usr/bin/ditto "${swapped}" "$2" ;; esac`,
      })
      run('open', ['-g', '-j', app])
      await waitFor(() => processesOf(app).length > 0, 'the applet starting')
      try {
        const ran = await install(at, ['--version', '3.0.7'], { env: { PATH } })
        assert.equal(ran.status, 1, ran.out)
        assert.match(ran.out, /the copy of the app made in .* is not signed/)
        assert.doesNotMatch(ran.out, /asking it to quit/)
        assert.ok(processesOf(app).length > 0, 'it still runs, never asked')
      } finally {
        for (const pid of processesOf(app)) process.kill(pid, 'SIGKILL')
      }
    })

    test('an app opened again between the quit and the swap is refused, and the installed one kept', async () => {
      const at = freshCase()
      const installed = fakeApp(at.apps, '3.0.7-alpha.1')
      const count = path.join(at.dir, 'ps-count')
      const PATH = standIns(at, {
        ps: `if [ "$1" = "-axo" ]; then\nn=$(cat "${count}" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "${count}"\n/bin/ps "$@"\n[ "$n" -ge 2 ] && echo "99999 ${installed}/Contents/MacOS/app"\nexit 0\nfi\nexec /bin/ps "$@"`,
      })
      assertKept(
        at,
        await install(at, ['--version', '3.0.7'], { env: { PATH } }),
        /was opened again while the install waited/,
        '3.0.7-alpha.1',
      )
    })

    test('the old app is deleted only once the new one says its version, and is put back when it does not', async () => {
      const wrong = path.join(work, 'plistbuddy-wrong')
      writeFileSync(
        wrong,
        '#!/bin/sh\ncase "$3" in "$WRONG_PLIST") echo 9.9.9; exit 0 ;; esac\nexec /usr/libexec/PlistBuddy "$@"\n',
        { mode: 0o755 },
      )
      const replacements = [
        [REQUIREMENT, AD_HOC],
        [SPCTL, `spctl=${path.join(work, 'spctl')}`],
        ['plistbuddy=/usr/libexec/PlistBuddy', `plistbuddy=${wrong}`],
      ]
      const upgrade = freshCase(replacements)
      fakeApp(upgrade.apps, '3.0.7-alpha.1')
      const env = (at) => ({
        WRONG_PLIST: path.join(at.apps, 'ConsensFlow.app/Contents/Info.plist'),
      })
      assertKept(
        upgrade,
        await install(upgrade, ['--version', '3.0.7'], { env: env(upgrade) }),
        /says version '9\.9\.9' after the install, not 3\.0\.7; the app that was there is put back/,
        '3.0.7-alpha.1',
      )
      const fresh = freshCase(replacements)
      assertRefused(
        fresh,
        await install(fresh, ['--version', '3.0.7'], { env: env(fresh) }),
        /says version '9\.9\.9' after the install/,
      )
    })
  })

  describe('an installed ConsensFlow newer than the release', () => {
    test('is kept, and nothing is downloaded but the feed', async () => {
      const at = freshCase()
      fakeApp(at.apps, '3.0.8')
      const from = server.asked.length
      assertKept(
        at,
        await install(at),
        /ConsensFlow 3\.0\.8 is installed in .*, newer than 3\.0\.7, which the stable channel names\. To go back to 3\.0\.7, run this with --version 3\.0\.7\./,
        '3.0.8',
      )
      assert.deepEqual(server.asked.slice(from), ['/releases/download/feed-stable/latest.json'])
    })

    test('a release is newer than its own alphas', async () => {
      const at = freshCase()
      fakeApp(at.apps, '3.0.7')
      assertKept(
        at,
        await install(at, ['--alpha']),
        /ConsensFlow 3\.0\.7 is installed in .*, newer than 3\.0\.7-alpha\.2/,
        '3.0.7',
      )
    })

    test('goes back to the release --version names', async () => {
      const at = freshCase()
      fakeApp(at.apps, '3.0.8')
      assertInstalled(at, await install(at, ['--version', '3.0.7']), '3.0.7')
    })

    test('an older alpha is upgraded by the alpha feed', async () => {
      const at = freshCase()
      fakeApp(at.apps, '3.0.7-alpha.1')
      assertInstalled(at, await install(at, ['--alpha']), '3.0.7-alpha.2')
    })
  })

  describe('an open ConsensFlow', () => {
    test('is asked to quit, and the install waits for it and goes on', async () => {
      const at = freshCase()
      const app = applet(at.apps, false)
      run('open', ['-g', '-j', app])
      await waitFor(() => processesOf(app).length > 0, 'the applet starting')
      const ran = await install(at, ['--version', '3.0.7'])
      assertInstalled(at, ran, '3.0.7')
      assert.match(ran.out, /ConsensFlow is open: asking it to quit/)
    })

    test('that stays open is never stopped, and nothing is installed', async () => {
      const at = freshCase()
      const app = applet(at.apps, true)
      run('open', ['-g', '-j', app])
      await waitFor(() => processesOf(app).length > 0, 'the applet starting')
      try {
        const ran = await install(at, ['--version', '3.0.7'])
        assert.equal(ran.status, 1, ran.out)
        assert.match(ran.out, /is still open after 4s\. Quit it/)
        assert.ok(processesOf(app).length > 0, 'the app still runs')
        assert.deepEqual(readdirSync(at.apps), ['ConsensFlow.app'])
        assert.notEqual(installedVersion(at), '3.0.7')
        assert.deepEqual(readdirSync(at.tmp), [])
      } finally {
        for (const pid of processesOf(app)) process.kill(pid, 'SIGKILL')
      }
    })

    test('that the install runs inside is not asked to quit: the install is refused', async () => {
      const at = freshCase()
      const app = fakeApp(at.apps, '3.0.7-alpha.1')
      const launcher = path.join(app, 'Contents/MacOS/launcher')
      writeFileSync(path.join(work, 'launcher.c'), LAUNCHER)
      run('cc', ['-o', launcher, path.join(work, 'launcher.c')])
      const ran = await install(at, ['--version', '3.0.7'], { launcher })
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, /this shell runs inside the ConsensFlow at .*Run this from Terminal/)
      assert.equal(installedVersion(at), '3.0.7-alpha.1')
      assert.deepEqual(readdirSync(at.tmp), [])
    })
  })

  describe('where it installs', () => {
    test('a folder it cannot write to is refused, and sudo is never used', async () => {
      const at = freshCase()
      chmodSync(at.apps, 0o555)
      try {
        const ran = await install(at, ['--version', '3.0.7'])
        assert.equal(ran.status, 1, ran.out)
        assert.match(ran.out, /is not yours to write to[\s\S]*never uses sudo itself/)
        assert.deepEqual(readdirSync(at.tmp), [])
      } finally {
        chmodSync(at.apps, 0o755)
      }
      assert.ok(
        !readFileSync(INSTALL_SH, 'utf8')
          .split('\n')
          .some((line) => /^\s*sudo\b|[;&|]\s*sudo\b/.test(line)),
        'the script runs no sudo',
      )
    })
  })
})
