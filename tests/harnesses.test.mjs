import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  harnessPath,
  missingHarnesses,
  offerable,
  paneArgv,
  probeExecutable,
  runnable,
  terminate,
} from '../src/harnesses.js'
import { fakeNodeExecutable } from './helpers.mjs'

/** The last line of the shim npm writes for a global package, with npm's variables. */
const NPM_SHIM =
  '@ECHO off\r\nSETLOCAL\r\nCALL :find_dp0\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\\node_modules\\@x\\cli\\bin\\cli.js" %*\r\n'

/** How a harness CLI is run: a program as it is, a Windows .cmd through cmd.exe with its quoting. */
describe('runnable', () => {
  it('runs a program directly, with its arguments untouched', () => {
    const run = runnable('/usr/local/bin/codex', ['app-server', 'a b'])
    assert.deepEqual(run, {
      file: '/usr/local/bin/codex',
      args: ['app-server', 'a b'],
      options: {},
    })
  })

  it('runs a .cmd through cmd.exe, each argument quoted the way cmd.exe reads it', () => {
    const run = runnable(
      'C:\\Program Files\\nodejs\\codex.cmd',
      ['-c', 'developer_instructions="hi" & more', 'trailing\\'],
      { ComSpec: 'C:\\Windows\\System32\\cmd.exe' },
    )
    assert.equal(run.file, 'C:\\Windows\\System32\\cmd.exe')
    assert.deepEqual(run.options, { windowsVerbatimArguments: true })
    assert.deepEqual(run.args.slice(0, 3), ['/d', '/s', '/c'])
    // The exact line, as cross-spawn (npm's own runner) would write it: every
    // special character carries one caret for cmd.exe's reading of the line
    // and two more for the script's own reading; an inner quote is backslashed
    // for the program and a trailing backslash doubled.
    assert.equal(
      run.args[3],
      '"C:\\Program^ Files\\nodejs\\codex.cmd ^^^"-c^^^" ^^^"developer_instructions=\\^^^"hi\\^^^"^^^ ^^^&^^^ more^^^" ^^^"trailing\\\\^^^""',
    )
  })

  it('ends a child with the signal asked for, where signals exist', {
    skip: process.platform === 'win32',
  }, () => {
    const signals = []
    const child = () => ({ pid: 1, exitCode: null, signalCode: null, kill: (s) => signals.push(s) })
    terminate(child())
    terminate(child(), 'SIGKILL')
    assert.deepEqual(signals, ['SIGTERM', 'SIGKILL'])
  })

  describe('an npm-style shim runs its script with node directly, so arguments may hold newlines', () => {
    const root = mkdtempSync(join(tmpdir(), 'cf-shim-'))
    after(() => rmSync(root, { recursive: true, force: true }))
    const bin = join(root, 'bin')
    mkdirSync(join(bin, 'node_modules', '@x', 'cli', 'bin'), { recursive: true })
    const script = join(bin, 'node_modules', '@x', 'cli', 'bin', 'cli.js')
    writeFileSync(script, '')
    const shim = join(bin, 'cli.cmd')
    writeFileSync(shim, NPM_SHIM)

    it('with the node on PATH when none sits beside the shim', () => {
      const elsewhere = join(root, 'elsewhere')
      mkdirSync(elsewhere)
      writeFileSync(join(elsewhere, 'node.exe'), '', { mode: 0o755 })
      const env = { OS: 'Windows_NT', PATH: elsewhere, PATHEXT: '.EXE' }
      assert.deepEqual(runnable(shim, ['queue', 'a\nb'], env), {
        file: join(elsewhere, 'node.exe'),
        args: [script, 'queue', 'a\nb'],
        options: {},
      })
    })

    it('with the node beside the shim first, as npm itself would', () => {
      writeFileSync(join(bin, 'node.exe'), '')
      assert.equal(runnable(shim, [], { OS: 'Windows_NT', PATH: '' }).file, join(bin, 'node.exe'))
    })

    it('and a shim naming its program and script outright, as the tests write them', () => {
      const own = join(root, 'own.cmd')
      writeFileSync(own, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`)
      assert.deepEqual(runnable(own, ['x'], {}), {
        file: process.execPath,
        args: [script, 'x'],
        options: {},
      })
    })

    it('but a shim it cannot read goes through cmd.exe', () => {
      const opaque = join(root, 'opaque.cmd')
      writeFileSync(opaque, '@echo off\r\n"%NODE_EXE%" "%NPM_CLI_JS%" %*\r\n')
      assert.match(runnable(opaque, [], {}).file, /\\cmd\.exe$/i)
    })

    it('and a window opens on the shim as its node and script, or not at all', () => {
      const env = { OS: 'Windows_NT', PATH: '' }
      assert.deepEqual(paneArgv([shim, '--model', 'a b'], env), [
        join(bin, 'node.exe'),
        script,
        '--model',
        'a b',
      ])
      const opaque = join(root, 'opaque.cmd')
      assert.throws(() => paneArgv([opaque], env), /is not an npm shim/)
      assert.deepEqual(paneArgv(['/usr/local/bin/pi', 'x'], env), ['/usr/local/bin/pi', 'x'])
    })
  })

  it('treats .bat the same and everything else as a program', () => {
    assert.match(runnable('C:\\x\\tool.BAT', [], {}).file, /\\cmd\.exe$/i)
    assert.equal(runnable('C:\\x\\claude.exe', []).file, 'C:\\x\\claude.exe')
  })
})

/** Where a harness CLI is found on Windows, where npm writes three files for each. */
describe('finding a harness on Windows', () => {
  const PATHEXT = '.COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL'

  it("opens npm's .cmd shim, never the sh script or the PowerShell one beside it", () => {
    const root = mkdtempSync(join(tmpdir(), 'cf-win-npm-'))
    try {
      const bin = join(root, 'npm')
      const script = join(bin, 'node_modules', '@openai', 'codex', 'bin', 'codex.js')
      mkdirSync(dirname(script), { recursive: true })
      writeFileSync(script, '')
      writeFileSync(join(bin, 'node.exe'), '', { mode: 0o755 })
      // What npm's cmd-shim writes for a global package: all three run something.
      writeFileSync(
        join(bin, 'codex'),
        '#!/bin/sh\nexec node "$basedir/node_modules/@openai/codex/bin/codex.js" "$@"\n',
        { mode: 0o755 },
      )
      writeFileSync(
        join(bin, 'codex.cmd'),
        NPM_SHIM.replace('@x\\cli\\bin\\cli.js', '@openai\\codex\\bin\\codex.js'),
        { mode: 0o755 },
      )
      writeFileSync(join(bin, 'codex.ps1'), '#!/usr/bin/env pwsh\n', { mode: 0o755 })
      const env = { OS: 'Windows_NT', PATH: bin, PATHEXT, HOME: root, USERPROFILE: root }
      const found = harnessPath('codex', env)
      assert.equal(found, join(bin, 'codex.cmd'))
      assert.deepEqual(paneArgv([found, '--model', 'm'], env), [
        join(bin, 'node.exe'),
        script,
        '--model',
        'm',
      ])
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  it('takes the first extension PATHEXT names that Windows can start, and no bare name', () => {
    const root = mkdtempSync(join(tmpdir(), 'cf-win-pathext-'))
    try {
      const bin = join(root, 'bin')
      mkdirSync(bin)
      for (const name of ['pi', 'pi.js', 'pi.cmd', 'pi.exe']) {
        writeFileSync(join(bin, name), '', { mode: 0o755 })
      }
      const env = (pathext) => ({ OS: 'Windows_NT', PATH: bin, PATHEXT: pathext, HOME: root })
      assert.equal(harnessPath('pi', env('.JS;.CMD;.EXE')), join(bin, 'pi.cmd'))
      assert.equal(harnessPath('pi', env(PATHEXT)), join(bin, 'pi.exe'))
      rmSync(join(bin, 'pi.cmd'))
      rmSync(join(bin, 'pi.exe'))
      assert.equal(harnessPath('pi', env(PATHEXT)), null, 'a bare name or a script is no program')
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})

/** What a CLI says about itself is asked once per executable as it is on disk. */
describe('probing a CLI', () => {
  it('asks an unchanged CLI once, an updated one again, and one that did not answer again', async () => {
    const root = mkdtempSync(join(tmpdir(), 'cf-probe-'))
    try {
      const calls = join(root, 'calls')
      const slow = join(root, 'slow')
      const cli = (version) =>
        fakeNodeExecutable(
          join(root, 'tool'),
          `#!${process.execPath}
import { appendFileSync, existsSync, rmSync } from 'node:fs'
appendFileSync(${JSON.stringify(calls)}, 'x')
if (existsSync(${JSON.stringify(slow)})) {
  rmSync(${JSON.stringify(slow)})
  setTimeout(() => {}, 5_000)
} else console.log('tool ${version}')
`,
        )
      const count = () => readFileSync(calls, 'utf8').length
      let tool = cli('1.0.0')
      assert.deepEqual(await probeExecutable(tool, ['--version'], process.env), {
        stdout: 'tool 1.0.0\n',
        code: 0,
      })
      await probeExecutable(tool, ['--version'], process.env)
      assert.equal(count(), 1, 'an unchanged CLI is asked once')
      tool = cli('1.1.0')
      assert.match((await probeExecutable(tool, ['--version'], process.env)).stdout, /1\.1\.0/)
      assert.equal(count(), 2, 'an updated CLI is asked again')
      tool = cli('1.2.0')
      writeFileSync(slow, '')
      await assert.rejects(probeExecutable(tool, ['--version'], process.env, { timeoutMs: 300 }))
      assert.match((await probeExecutable(tool, ['--version'], process.env)).stdout, /1\.2\.0/)
      assert.equal(count(), 4, 'a CLI that did not answer in time is asked again')
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})

/** A harness not installed here shows only on the Harnesses page, not in the pickers. */
describe('what the pickers offer', () => {
  it('names every harness missing from a machine with none installed', () => {
    const empty = mkdtempSync(join(tmpdir(), 'cf-none-'))
    try {
      const env = { HOME: empty, USERPROFILE: empty, PATH: empty }
      assert.deepEqual(missingHarnesses(env).sort(), ['claude', 'codex', 'devin', 'opencode', 'pi'])
    } finally {
      rmSync(empty, { recursive: true, force: true })
    }
  })

  it('hides the agents on a missing harness and leaves the rest as they are', () => {
    const agents = [
      { name: 'zeus', harness: 'claude' },
      { name: 'ares', harness: 'devin' },
      { name: 'iris', harness: 'image' },
    ]
    assert.deepEqual(offerable(agents, ['devin']), [
      { name: 'zeus', harness: 'claude' },
      { name: 'ares', harness: 'devin', hidden: true, notInstalled: true },
      { name: 'iris', harness: 'image' },
    ])
    // An image agent runs through Codex: without Codex it is not offered.
    assert.deepEqual(offerable([{ name: 'pygmalion', harness: 'image' }], ['codex']), [
      { name: 'pygmalion', harness: 'image', hidden: true, notInstalled: true },
    ])
    // One saved for a harness this build does not run (Kimi) has no window to open.
    assert.deepEqual(offerable([{ name: 'old', harness: 'kimi', unsupported: true }], []), [
      { name: 'old', harness: 'kimi', unsupported: true, hidden: true, notInstalled: true },
    ])
  })
})
