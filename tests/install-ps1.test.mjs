// The Windows installer (install/install.ps1) against a scripted feed, in
// Windows PowerShell 5.1 and PowerShell 7: what it reads, every check it makes
// before it runs the installer, and what it refuses. Its test copies trust a
// test key, read no uninstall entry of this machine, and find the app under a
// LOCALAPPDATA of the case's own. Running the real installer is the run against
// a real release, install/real-install.mjs. On Linux and macOS, where
// PowerShell 7 may be, only the refusal in plain words is run.

import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { spawn, spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import {
  feedOf,
  INSTALL_PS1,
  makeKey,
  scriptedRelease,
  serve,
  signedName,
  sumsOf,
  testCopy,
} from '../install/kit.mjs'

const windows = process.platform === 'win32'
const UNINSTALL_ENTRY =
  "'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ConsensFlow'"

function onPath(program) {
  return spawnSync(windows ? 'where' : 'which', [program], { encoding: 'utf8' }).status === 0
}

const shells = [
  ['Windows PowerShell', 'powershell', windows],
  ['PowerShell 7', 'pwsh', onPath('pwsh')],
].filter(([, , available]) => available)

const setupName = (version) => `ConsensFlow_${version}_x64-setup.exe`

for (const [label, shell] of shells) {
  describe(`install.ps1 in ${label}`, () => {
    let work
    let key
    let server
    const files = {}
    // Bytes that stand for an installer: the cases stop before one would run.
    const setup = Buffer.from('MZ not an installer')

    function publish(version, options = {}, artifacts = { [setupName(version)]: setup }) {
      for (const at of Object.keys(files)) {
        if (at.includes(`/v${version}/`)) delete files[at]
      }
      Object.assign(files, scriptedRelease('/releases', version, artifacts, key, options))
    }

    function freshCase() {
      const dir = mkdtempSync(path.join(work, 'case-'))
      const script = testCopy(INSTALL_PS1, path.join(dir, 'install.ps1'), key.keyLine, [
        [UNINSTALL_ENTRY, "'HKCU:\\Software\\ConsensFlow-installer-tests\\none'"],
        ['$quitWait = 120', '$quitWait = 3'],
      ])
      for (const folder of ['tmp', 'local']) mkdirSync(path.join(dir, folder))
      return { dir, script, tmp: path.join(dir, 'tmp'), local: path.join(dir, 'local') }
    }

    function install(at, args = [], { env = {}, launcher } = {}) {
      const words = [
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        at.script,
        ...args,
      ]
      const [program, all] = launcher ? [launcher, ['/d', '/c', shell, ...words]] : [shell, words]
      return new Promise((resolve) => {
        const child = spawn(program, all, {
          env: {
            ...process.env,
            TEMP: at.tmp,
            TMP: at.tmp,
            TMPDIR: at.tmp,
            LOCALAPPDATA: at.local,
            CONSENSFLOW_RELEASES_URL: `${server.url}/releases`,
            CONSENSFLOW_CHANNEL: '',
            CONSENSFLOW_VERSION: '',
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
        child.on('close', (status) => resolve({ status, out: out.replace(/\s*\r?\n\s*/g, ' ') }))
      })
    }

    function assertRefused(at, ran, words) {
      assert.notEqual(ran.status, 0, ran.out)
      assert.match(ran.out, words)
      assert.match(ran.out, /nothing was installed/)
      assert.deepEqual(readdirSync(at.tmp), [], "the run's own folder is gone")
    }

    before(async () => {
      work = mkdtempSync(path.join(os.tmpdir(), 'cf-install-ps1-'))
      key = makeKey()
      Object.assign(
        files,
        feedOf('/releases', 'stable', '3.0.7'),
        feedOf('/releases', 'alpha', '3.0.7-alpha.2'),
      )
      for (const version of ['3.0.7-alpha.1', '3.0.7-alpha.2', '3.0.7']) publish(version)
      server = await serve(files)
    })

    after(async () => {
      await server?.close()
      if (work) rmSync(work, { recursive: true, force: true })
    })

    test('Linux and macOS are refused in plain words', {
      skip: windows && 'this is Windows',
    }, async () => {
      const at = freshCase()
      const from = server.asked.length
      const ran = await install(at)
      const words =
        process.platform === 'linux'
          ? /ConsensFlow is made for macOS and Windows only; there is no Linux build\./
          : /this is the installer for Windows\. On a Mac, run in Terminal: curl/
      assertRefused(at, ran, words)
      assert.equal(server.asked.length, from, 'nothing was downloaded')
    })

    describe('on Windows', { skip: !windows && 'the rest runs on Windows' }, () => {
      /** The release a case downloaded, by what it asked the server for. */
      function downloaded(from) {
        return server.asked.slice(from).filter((asked) => !asked.includes('/feed-'))
      }

      test('--help says how to install and uninstall', async () => {
        const ran = await install(freshCase(), ['--help'])
        assert.equal(ran.status, 0, ran.out)
        assert.match(ran.out, /--alpha/)
        assert.match(
          ran.out,
          /Uninstall: Settings > Apps > Installed apps > ConsensFlow > Uninstall/,
        )
      })

      test('an option it does not know, a version that is no release number, and a channel that is none, are refused', async () => {
        assert.match((await install(freshCase(), ['--beta'])).out, /unknown option '--beta'/)
        assert.match(
          (await install(freshCase(), ['--version', 'latest; rm'])).out,
          /a version is a release number/,
        )
        assert.match(
          (await install(freshCase(), [], { env: { CONSENSFLOW_CHANNEL: 'nightly' } })).out,
          /the channel is stable or alpha/,
        )
      })

      test('a PC that is not x64 is refused', async () => {
        const at = freshCase()
        assertRefused(
          at,
          await install(at, [], { env: { PROCESSOR_ARCHITECTURE: 'ARM64' } }),
          /made for x64[\s\S]*this PC's is ARM64/,
        )
      })

      // Each of these stops at the installer's hash, before anything would run: the bytes served
      // under its name are not the ones its SHA256SUMS lists.
      const stopAtHash = (version) =>
        publish(version, { sums: sumsOf({ [setupName(version)]: 'other bytes' }) })

      test("the stable feed's release by default, the alpha feed's with --alpha or CONSENSFLOW_CHANNEL, and --version's without a feed", async () => {
        for (const version of ['3.0.7-alpha.1', '3.0.7-alpha.2', '3.0.7']) stopAtHash(version)
        try {
          const cases = [
            [[], {}, '3.0.7', true],
            [['--alpha'], {}, '3.0.7-alpha.2', true],
            [[], { CONSENSFLOW_CHANNEL: 'alpha' }, '3.0.7-alpha.2', true],
            [['--version', 'v3.0.7-alpha.1'], {}, '3.0.7-alpha.1', false],
            [[], { CONSENSFLOW_VERSION: '3.0.7-alpha.1' }, '3.0.7-alpha.1', false],
          ]
          for (const [args, env, version, feed] of cases) {
            const at = freshCase()
            const from = server.asked.length
            const ran = await install(at, args, { env })
            assertRefused(
              at,
              ran,
              new RegExp(`${setupName(version).replaceAll('.', '\\.')} was changed`),
            )
            assert.equal(
              server.asked.slice(from).some((asked) => asked.includes('/feed-')),
              feed,
              `${args} ${JSON.stringify(env)}: a feed read`,
            )
            assert.deepEqual(downloaded(from), [
              `/releases/download/v${version}/SHA256SUMS`,
              `/releases/download/v${version}/SHA256SUMS.sig`,
              `/releases/download/v${version}/${setupName(version)}`,
            ])
          }
        } finally {
          for (const version of ['3.0.7-alpha.1', '3.0.7-alpha.2', '3.0.7']) publish(version)
        }
      })

      test('a release with no signed SHA256SUMS, as before 3.0.7-alpha.1, is refused', async () => {
        publish('3.0.6', { signature: null })
        const at = freshCase()
        assertRefused(
          at,
          await install(at, ['--version', '3.0.6']),
          /ConsensFlow 3\.0\.6 has no signed SHA256SUMS[\s\S]*before 3\.0\.7-alpha\.1/,
        )
      })

      const flip = (base64, at) => {
        const bytes = Buffer.from(base64, 'base64')
        bytes[at] ^= 1
        return bytes.toString('base64')
      }
      const changeLine = (sig, line, change) => {
        const lines = Buffer.from(sig, 'base64').toString('latin1').split('\n')
        lines[line] = change(lines[line])
        return Buffer.from(lines.join('\n'), 'latin1').toString('base64')
      }
      const signatureCases = [
        [
          'a flipped bit in the signature',
          (_, sig) => changeLine(sig, 1, (line) => flip(line, 40)),
          /the signature does not match the file/,
        ],
        [
          'a flipped bit in the trusted comment',
          (_, sig) => changeLine(sig, 2, (line) => line.replace('timestamp:1', 'timestamp:3')),
          /the trusted comment's signature does not match/,
        ],
        [
          'a flipped bit in the global signature',
          (_, sig) => changeLine(sig, 3, (line) => flip(line, 10)),
          /the trusted comment's signature does not match/,
        ],
        [
          'a signature by another key',
          (text) => makeKey().sign(Buffer.from(text, 'latin1'), signedName('3.0.7')),
          /made by another key/,
        ],
        [
          'a SHA256SUMS signed for another release',
          (text) => key.sign(Buffer.from(text, 'latin1'), signedName('3.0.7-alpha.1')),
          /the signature is for ConsensFlow_3\.0\.7-alpha\.1_SHA256SUMS, not ConsensFlow_3\.0\.7_SHA256SUMS/,
        ],
        [
          'a signature of an update archive',
          (text) => key.sign(Buffer.from(text, 'latin1'), 'ConsensFlow.app.tar.gz'),
          /the signature is for ConsensFlow\.app\.tar\.gz/,
        ],
      ]
      for (const [name, signature, words] of signatureCases) {
        test(`${name} is refused`, async () => {
          publish('3.0.7', { signature })
          try {
            const at = freshCase()
            assertRefused(at, await install(at, ['--version', '3.0.7']), words)
          } finally {
            publish('3.0.7')
          }
        })
      }

      test('a flipped bit in SHA256SUMS is refused', async () => {
        const good = sumsOf({ [setupName('3.0.7')]: setup })
        const changed = Buffer.from(good, 'latin1')
        changed[3] ^= 1
        publish('3.0.7', {
          sums: changed.toString('latin1'),
          signature: key.sign(Buffer.from(good, 'latin1'), signedName('3.0.7')),
        })
        try {
          const at = freshCase()
          assertRefused(
            at,
            await install(at, ['--version', '3.0.7']),
            /the signature does not match the file/,
          )
        } finally {
          publish('3.0.7')
        }
      })

      test('the script trusts the key it names, and no other', async () => {
        const at = freshCase()
        copyFileSync(INSTALL_PS1, at.script)
        assertRefused(at, await install(at, ['--version', '3.0.7']), /made by another key/)
      })

      test('an installer with one byte changed is refused by its SHA-256', async () => {
        const tampered = Buffer.from(setup)
        tampered[4] ^= 1
        publish(
          '3.0.7',
          { sums: sumsOf({ [setupName('3.0.7')]: setup }) },
          { [setupName('3.0.7')]: tampered },
        )
        try {
          const at = freshCase()
          assertRefused(
            at,
            await install(at, ['--version', '3.0.7']),
            /ConsensFlow_3\.0\.7_x64-setup\.exe was changed: its SHA-256 is [0-9a-f]{64}, and the signed SHA256SUMS says/,
          )
        } finally {
          publish('3.0.7')
        }
      })

      test('a SHA256SUMS that lists no installer for the release is refused', async () => {
        publish('3.0.7', { sums: sumsOf({ 'ConsensFlow_3.0.7_aarch64.dmg': 'x' }) })
        try {
          const at = freshCase()
          assertRefused(
            at,
            await install(at, ['--version', '3.0.7']),
            /3\.0\.7's SHA256SUMS lists no ConsensFlow_3\.0\.7_x64-setup\.exe/,
          )
        } finally {
          publish('3.0.7')
        }
      })

      describe('an open ConsensFlow', () => {
        /** A program of the app's that runs on its own: a copy of Windows' ping, which has no window. */
        function program(at, name) {
          const dir = path.join(at.local, 'ConsensFlow')
          mkdirSync(dir, { recursive: true })
          const copy = path.join(dir, name)
          copyFileSync(
            path.join(
              process.env.SystemRoot,
              'System32',
              name === 'cmd.exe' ? 'cmd.exe' : 'PING.EXE',
            ),
            copy,
          )
          return copy
        }

        test('that stays open is never stopped, and nothing is installed', async () => {
          const at = freshCase()
          const running = spawn(program(at, 'ConsensFlow.exe'), ['-n', '60', '127.0.0.1'], {
            stdio: 'ignore',
          })
          try {
            const ran = await install(at, ['--version', '3.0.7'])
            assertRefused(
              at,
              ran,
              /ConsensFlow in .* is still open after 3s\. Close it and run this again\./,
            )
            assert.match(ran.out, /asking it to close/)
            assert.equal(running.exitCode, null, 'it still runs')
          } finally {
            running.kill()
          }
        })

        test('that the install runs inside is not asked to close: the install is refused', async () => {
          const at = freshCase()
          const ran = await install(at, ['--version', '3.0.7'], {
            launcher: program(at, 'cmd.exe'),
          })
          assertRefused(
            at,
            ran,
            /this PowerShell runs inside the ConsensFlow in .*Run this from Windows Terminal/,
          )
        })
      })
    })
  })
}
