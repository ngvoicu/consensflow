import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { paneArgv, runnable, terminate } from '../src/harnesses.js'

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
