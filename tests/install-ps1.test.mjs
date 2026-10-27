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
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import net from 'node:net'
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
  VERSION_ORDER,
} from '../install/kit.mjs'

const windows = process.platform === 'win32'
const UNINSTALL_ENTRY =
  "'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ConsensFlow'"
const MACHINE_ENTRIES =
  "$machineEntries = @('HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ConsensFlow', 'HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ConsensFlow')"
// Where the cases keep the uninstall entries they make, each under a name of its own.
const TEST_KEYS = 'HKCU\\Software\\ConsensFlow-installer-tests'

function onPath(program) {
  return spawnSync(windows ? 'where' : 'which', [program], { encoding: 'utf8' }).status === 0
}

const shells = [
  ['Windows PowerShell', 'powershell', windows],
  ['PowerShell 7', 'pwsh', onPath('pwsh')],
].filter(([, , available]) => available)

const setupName = (version) => `ConsensFlow_${version}_x64-setup.exe`

for (const [label, shell] of shells) {
  test(`install.ps1 orders versions as SemVer does, in ${label}`, () => {
    const lines = readFileSync(INSTALL_PS1, 'utf8').split('\n')
    const from = lines.findIndex((line) => line.trim() === '# version order: begin')
    const to = lines.findIndex((line) => line.trim() === '# version order: end')
    assert.ok(from >= 0 && to > from, 'install.ps1 marks its order of versions')
    const work = mkdtempSync(path.join(os.tmpdir(), 'cf-version-order-'))
    try {
      const script = path.join(work, 'order.ps1')
      writeFileSync(
        script,
        `${lines.slice(from + 1, to).join('\n')}\n${VERSION_ORDER.map(([a, b]) => `[bool](Test-CfNewer '${a}' '${b}')`).join('\n')}\n`,
      )
      const ran = spawnSync(
        shell,
        ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script],
        {
          encoding: 'utf8',
        },
      )
      assert.equal(ran.status, 0, ran.stderr)
      const said = ran.stdout.trim().split(/\r?\n/)
      VERSION_ORDER.forEach(([a, b, newer], at) => {
        assert.equal(said[at], newer ? 'True' : 'False', `${a} after ${b}`)
      })
    } finally {
      rmSync(work, { recursive: true, force: true })
    }
  })

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

    /**
     * A fresh case: its own folders, and a test copy that reads uninstall entries of its own
     * (made by `entry`), with `replacements` made too.
     */
    function freshCase(replacements = []) {
      const dir = mkdtempSync(path.join(work, 'case-'))
      const name = path.basename(dir)
      const user = `${TEST_KEYS}\\${name}-user`
      const machine = `${TEST_KEYS}\\${name}-machine`
      const script = testCopy(INSTALL_PS1, path.join(dir, 'install.ps1'), key.keyLine, [
        [UNINSTALL_ENTRY, `'${user.replace('HKCU', 'HKCU:')}'`],
        [MACHINE_ENTRIES, `$machineEntries = @('${machine.replace('HKCU', 'HKCU:')}')`],
        ['$quitWait = 120', '$quitWait = 3'],
        ...replacements,
      ])
      for (const folder of ['tmp', 'local']) mkdirSync(path.join(dir, folder))
      return {
        dir,
        script,
        tmp: path.join(dir, 'tmp'),
        local: path.join(dir, 'local'),
        user,
        machine,
      }
    }

    /** Makes the uninstall entry `key` say ConsensFlow `version` is installed in `location`. */
    function entry(key, version, location) {
      for (const [name, value] of [
        ['DisplayName', 'ConsensFlow'],
        ['DisplayVersion', version],
        ['InstallLocation', `"${location}"`],
      ]) {
        const made = spawnSync('reg', ['add', key, '/v', name, '/d', value, '/f'], {
          encoding: 'utf8',
        })
        assert.equal(made.status, 0, made.stderr)
      }
    }

    function install(at, args = [], { env = {}, launcher, onStart } = {}) {
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
        onStart?.(child)
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
      if (windows) spawnSync('reg', ['delete', TEST_KEYS, '/f'])
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

      describe('what is checked is what goes in', () => {
        test('the hash of the installer comes from the bytes of SHA256SUMS whose signature was checked', async () => {
          const other = Buffer.from('MZ another installer')
          publish(
            '3.0.7',
            { sums: sumsOf({ [setupName('3.0.7')]: setup }) },
            { [setupName('3.0.7')]: other },
          )
          try {
            const swapped = sumsOf({ [setupName('3.0.7')]: other }).trim()
            const at = freshCase([
              [
                `Write-Host "consensflow: SHA256SUMS is signed by ConsensFlow's update key, for $version"`,
                `Write-Host "consensflow: SHA256SUMS is signed by ConsensFlow's update key, for $version"; [IO.File]::WriteAllText($sums, "${swapped}\`n")`,
              ],
            ])
            assertRefused(
              at,
              await install(at, ['--version', '3.0.7']),
              /ConsensFlow_3\.0\.7_x64-setup\.exe was changed/,
            )
          } finally {
            publish('3.0.7')
          }
        })

        test('the installer is held from its hash until it has run: nothing writes or deletes it', async () => {
          const at = freshCase([
            [
              "$run = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru",
              "try { [IO.File]::WriteAllText($setup, 'swapped'); Write-Host 'consensflow-test: WRITTEN' } catch { Write-Host 'consensflow-test: HELD' }; try { Remove-Item -LiteralPath $setup -Force; Write-Host 'consensflow-test: DELETED' } catch { Write-Host 'consensflow-test: KEPT' }; $run = [pscustomobject]@{ ExitCode = 0 }",
            ],
          ])
          const ran = await install(at, ['--version', '3.0.7'])
          assert.match(ran.out, /consensflow-test: HELD/)
          assert.match(ran.out, /consensflow-test: KEPT/)
          assert.match(ran.out, /Windows lists ConsensFlow as version 'nothing'/)
          assert.deepEqual(readdirSync(at.tmp), [], "the run's own folder is gone")
        })
      })

      describe('what is installed already', () => {
        test('a ConsensFlow newer than the release is kept, unless --version names the release', async () => {
          const at = freshCase()
          entry(at.user, '3.0.8', path.join(at.local, 'ConsensFlow'))
          const from = server.asked.length
          assertRefused(
            at,
            await install(at),
            /ConsensFlow 3\.0\.8 is installed, newer than 3\.0\.7, which the stable channel names\. To go back to 3\.0\.7, run this with --version 3\.0\.7/,
          )
          assert.deepEqual(server.asked.slice(from), ['/releases/download/feed-stable/latest.json'])
          stopAtHash('3.0.7')
          try {
            assertRefused(
              at,
              await install(at, ['--version', '3.0.7']),
              /ConsensFlow_3\.0\.7_x64-setup\.exe was changed/,
            )
          } finally {
            publish('3.0.7')
          }
        })

        test('a release is newer than its own alphas', async () => {
          const at = freshCase()
          entry(at.user, '3.0.7', path.join(at.local, 'ConsensFlow'))
          assertRefused(
            at,
            await install(at, ['--alpha']),
            /ConsensFlow 3\.0\.7 is installed, newer than 3\.0\.7-alpha\.2/,
          )
        })

        test('a ConsensFlow installed for every user is not joined by a per-user one', async () => {
          const at = freshCase()
          entry(at.machine, '3.0.6', 'C:\\Program Files\\ConsensFlow')
          const from = server.asked.length
          assertRefused(
            at,
            await install(at, ['--version', '3.0.7']),
            /ConsensFlow 3\.0\.6 is installed for every user of this PC, in C:\\Program Files\\ConsensFlow\. This installer installs for your user only/,
          )
          assert.equal(server.asked.length, from, 'nothing was downloaded')
        })
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

        test('opened again right before the installer starts, it is refused', async () => {
          const at = freshCase()
          const ping = program(at, 'ConsensFlow.exe')
          const opened = freshCase([
            [
              'Close-CfOpen $dir\n',
              `Close-CfOpen $dir; $null = Start-Process -FilePath '${ping}' -ArgumentList '-n', '30', '127.0.0.1' -WindowStyle Hidden; Start-Sleep -Milliseconds 500\n`,
            ],
          ])
          opened.local = at.local
          try {
            assertRefused(
              opened,
              await install(opened, ['--version', '3.0.7'], { env: { LOCALAPPDATA: at.local } }),
              /ConsensFlow in .* was opened again while the install waited/,
            )
          } finally {
            // The stand-in alone, by its path: no other ConsensFlow on the machine is touched.
            spawnSync('powershell', [
              '-NoProfile',
              '-Command',
              `Get-Process | Where-Object { $_.Path -eq '${ping}' } | Stop-Process -Force`,
            ])
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

      // A run that is killed (TerminateProcess: no `finally` runs) leaves its own folder in the
      // user's temporary folder: consensflow-install.<pid>.<12 hex>. The next run takes away what
      // is unmistakably an earlier run's: by exactly such a name, of a pid that no install runs as
      // (a PowerShell is one that may be, whatever it is doing; so is a process whose command line
      // names install.ps1), never a link, said in full.
      describe("what an earlier install left in the user's temporary folder", () => {
        const children = []
        after(() => {
          for (const child of children) child.kill()
        })
        const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
        async function waitFor(check, what) {
          for (let i = 0; i < 300; i++) {
            if (check()) return
            await wait(100)
          }
          assert.fail(`${what} did not happen in 30 s`)
        }
        /** The pid of a process that ran and ended. */
        async function deadPid() {
          const child = spawn(process.execPath, ['-e', ''])
          await new Promise((resolve) => child.on('exit', resolve))
          return child.pid
        }
        /** A process kept running: a PowerShell, or a program that is none. */
        function running(program, args) {
          const child = spawn(program, args, { stdio: 'ignore' })
          children.push(child)
          return child.pid
        }
        const folderIn = (at, name, files = { 'verify.js': 'x' }) => {
          const folder = path.join(at.tmp, name)
          mkdirSync(folder)
          for (const [file, text] of Object.entries(files))
            writeFileSync(path.join(folder, file), text)
          return folder
        }
        const exact = (words) => words.replaceAll('\\', '\\\\').replaceAll('.', '\\.')

        test("a dead run's folder is removed, a PowerShell's and what only looks similar kept, and a link is never followed", async () => {
          publish('3.0.6', { signature: null })
          const at = freshCase()
          const hex = '0123456789ab'
          const dead = await deadPid()
          const live = running(shell, [
            '-NoProfile',
            '-NonInteractive',
            '-Command',
            'Start-Sleep -Seconds 120',
          ])
          const other = running(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'])
          const gone = folderIn(at, `consensflow-install.${dead}.${hex}`)
          const unrelated = folderIn(at, `consensflow-install.${other}.${hex}`)
          const kept = folderIn(at, `consensflow-install.${live}.${hex}`)
          // A junction inside a dead run's folder, to a folder that is someone's: only the junction goes.
          const elsewhere = mkdtempSync(path.join(work, 'elsewhere-'))
          writeFileSync(path.join(elsewhere, 'keep'), 'mine')
          symlinkSync(elsewhere, path.join(gone, 'link'), 'junction')
          // And a symbolic link, where this user may make one (it takes a privilege Windows gives some).
          const aside = mkdtempSync(path.join(work, 'aside-'))
          writeFileSync(path.join(aside, 'keep'), 'mine')
          try {
            symlinkSync(aside, path.join(gone, 'symbolic'), 'dir')
          } catch {
            // not allowed here: the junction alone is tried
          }
          const similar = [
            `consensflow-install.${dead}.${hex.slice(0, 11)}`,
            `consensflow-install.${dead}.${hex}c`,
            `consensflow-install.${dead}.${hex.slice(0, 10)}ag`,
            // Windows takes upper case for lower, so this one has other digits than the real ones.
            `consensflow-install.${dead}.${'fedcba987654'.toUpperCase()}`,
            `consensflow-install.0${dead}.${hex}`,
            `consensflow-install.${dead}x.${hex}`,
            `consensflow-install.99999999999.${hex}`,
            `consensflow-install.${dead}.${hex}.bak`,
            `consensflow-install.${dead}`,
            `consensflow-install.${hex}`,
            `xconsensflow-install.${dead}.${hex}`,
          ]
          for (const name of similar) folderIn(at, name)
          // A link by the very shape of a dead run's folder, to a folder somewhere else; and a file.
          symlinkSync(
            elsewhere,
            path.join(at.tmp, `consensflow-install.${dead + 1}.${hex}`),
            'junction',
          )
          writeFileSync(path.join(at.tmp, `consensflow-install.${dead + 2}.${hex}`), 'file')
          const ran = await install(at, ['--version', '3.0.6'])
          assert.notEqual(ran.status, 0, ran.out)
          assert.match(ran.out, /nothing was installed/)
          assert.deepEqual(
            readdirSync(at.tmp).sort(),
            [
              path.basename(kept),
              ...similar,
              `consensflow-install.${dead + 1}.${hex}`,
              `consensflow-install.${dead + 2}.${hex}`,
            ].sort(),
            `only a dead run's folder and the pid that is another program's are gone, and this run's own: ${ran.out}`,
          )
          assert.equal(readFileSync(path.join(elsewhere, 'keep'), 'utf8'), 'mine')
          assert.equal(readFileSync(path.join(aside, 'keep'), 'utf8'), 'mine')
          assert.ok(!existsSync(gone) && !existsSync(unrelated))
          // The refusal is the release's own: this run's folder was there to download into.
          assert.match(ran.out, /ConsensFlow 3\.0\.6 has no signed SHA256SUMS/)
          assert.match(ran.out, new RegExp(`removed ${exact(gone)}`))
          assert.match(ran.out, new RegExp(`removed ${exact(unrelated)}`))
          assert.match(
            ran.out,
            new RegExp(
              `left ${exact(kept)} alone: process ${live} may be an install that is still going on`,
            ),
          )
          assert.match(
            ran.out,
            new RegExp(`left .*consensflow-install\\.${dead + 1}\\.${hex} alone`),
          )
        })

        test("this run's folder carries its pid, and one killed hard leaves it for the next run to take away", async () => {
          publish('3.0.6', { signature: null })
          const at = freshCase()
          // A server that takes the connection and never answers: the run hangs on its first download.
          const sockets = new Set()
          const hung = net.createServer((socket) => {
            sockets.add(socket)
            socket.on('error', () => {})
          })
          await new Promise((resolve) => hung.listen(0, '127.0.0.1', resolve))
          let child
          const first = install(at, ['--version', '3.0.7'], {
            env: { CONSENSFLOW_RELEASES_URL: `http://127.0.0.1:${hung.address().port}/releases` },
            onStart: (started) => {
              child = started
            },
          })
          try {
            await waitFor(() => readdirSync(at.tmp).length === 1, 'the run making its folder')
            const [folder] = readdirSync(at.tmp)
            assert.match(folder, new RegExp(`^consensflow-install\\.${child.pid}\\.[0-9a-f]{12}$`))
            child.kill()
            await first
            assert.deepEqual(readdirSync(at.tmp), [folder], 'the kill left it')
            const ran = await install(at, ['--version', '3.0.6'])
            assert.match(ran.out, new RegExp(`removed ${exact(path.join(at.tmp, folder))}`))
            assert.deepEqual(readdirSync(at.tmp), [], 'the next run took it away, and its own')
          } finally {
            child?.kill()
            for (const socket of sockets) socket.destroy()
            await new Promise((resolve) => hung.close(resolve))
          }
        })
      })
    })
  })
}
