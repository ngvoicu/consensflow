import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { windowsEnv } from './helpers.mjs'
import {
  devinFolders,
  harnessPath,
  onWindows,
  opencodeStores,
  paneArgv,
  runnable,
} from './live/harnesses.mjs'

/**
 * Where the harnesses are on a machine and how a CLI is run, as the evals and
 * the live tools (`tests/live/harnesses.mjs`) need it. Windows is simulated on
 * any system: the environment's `OS` says it is.
 */

/** The last line of the shim npm writes for a global package, with npm's variables. */
const NPM_SHIM =
  '@ECHO off\r\nSETLOCAL\r\nCALL :find_dp0\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\\node_modules\\@x\\cli\\bin\\cli.js" %*\r\n'

describe('whether a machine is Windows', () => {
  it('is said by the environment or by the platform this runs on', () => {
    assert.equal(onWindows({ OS: 'Windows_NT' }), true)
    assert.equal(onWindows({ OS: 'windows' }), true)
    assert.equal(onWindows({}), process.platform === 'win32')
    assert.equal(onWindows({ OS: 'Linux' }), process.platform === 'win32')
  })
})

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

describe("Devin's folders", () => {
  it('are the XDG places off Windows, ~/.config and ~/.local/share unless set', {
    skip: process.platform === 'win32' && 'this platform is Windows',
  }, () => {
    assert.deepEqual(devinFolders({ HOME: '/home/a' }), {
      config: join('/home/a', '.config', 'devin'),
      data: join('/home/a', '.local', 'share', 'devin'),
    })
    assert.deepEqual(
      devinFolders({ HOME: '/home/a', XDG_CONFIG_HOME: '/xdg/config', XDG_DATA_HOME: '/xdg/data' }),
      { config: join('/xdg/config', 'devin'), data: join('/xdg/data', 'devin') },
    )
  })

  it('are %APPDATA%\\devin on Windows, config and sessions alike', () => {
    const roaming = join('/Users/a', 'AppData', 'Roaming')
    assert.deepEqual(devinFolders({ OS: 'Windows_NT', HOME: '/Users/a', APPDATA: roaming }), {
      config: join(roaming, 'devin'),
      data: join(roaming, 'devin'),
    })
    // XDG variables some shells set are not where Devin looks on Windows.
    assert.deepEqual(
      devinFolders({ OS: 'Windows_NT', USERPROFILE: '/Users/a', XDG_DATA_HOME: '/xdg/data' }),
      { config: join(roaming, 'devin'), data: join(roaming, 'devin') },
    )
  })
})

describe("OpenCode's store", () => {
  it('is its XDG data place off Windows, unless OpenCode is told another', {
    skip: process.platform === 'win32' && 'this platform is Windows',
  }, () => {
    assert.deepEqual(opencodeStores({ HOME: '/home/a' }), [
      join('/home/a', '.local', 'share', 'opencode', 'opencode.db'),
    ])
    assert.deepEqual(opencodeStores({ HOME: '/home/a', XDG_DATA_HOME: '/xdg' }), [
      join('/xdg', 'opencode', 'opencode.db'),
    ])
  })

  it('is where OPENCODE_DB or OPENCODE_DATA says, on every platform', () => {
    assert.deepEqual(opencodeStores({ OS: 'Windows_NT', HOME: '/a', OPENCODE_DB: '/x/o.db' }), [
      '/x/o.db',
    ])
    assert.deepEqual(opencodeStores({ HOME: '/a', OPENCODE_DATA: '/data' }), [
      join('/data', 'opencode.db'),
    ])
  })

  it('may also be under %LOCALAPPDATA% or %APPDATA% on Windows', () => {
    assert.deepEqual(
      opencodeStores({ OS: 'Windows_NT', HOME: '/a', LOCALAPPDATA: '/local', APPDATA: '/roaming' }),
      [
        join('/a', '.local', 'share', 'opencode', 'opencode.db'),
        join('/local', 'opencode', 'opencode.db'),
        join('/roaming', 'opencode', 'opencode.db'),
      ],
    )
  })
})

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

  it('is no harness at all for a name that is none', () => {
    assert.equal(harnessPath('vim', { PATH: '/usr/bin' }), null)
  })
})

/**
 * Where npm puts the shims of a global install on Windows, %APPDATA%\npm: a
 * terminal reaches it through the shell's own setup, which an app started from
 * the Start menu never runs.
 */
describe("npm's global folder on Windows", () => {
  const PATHEXT = '.COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL'
  const PI_SHIM = NPM_SHIM.replace(
    '@x\\cli\\bin\\cli.js',
    '@earendil-works\\pi-coding-agent\\dist\\cli.js',
  )

  /** A file at `path`, startable where a mode says so. Returns the path. */
  function stub(path, text = '') {
    mkdirSync(dirname(path), { recursive: true })
    writeFileSync(path, text, { mode: 0o755 })
    return path
  }

  /**
   * A Windows machine as a test lays one out: a home, the roaming folder
   * APPDATA names, and a PATH folder that holds nothing.
   */
  function machine() {
    const root = mkdtempSync(join(tmpdir(), 'cf-win-appdata-'))
    const roaming = join(root, 'home', 'AppData', 'Roaming')
    const bin = join(root, 'bin')
    mkdirSync(bin, { recursive: true })
    return {
      root,
      home: join(root, 'home'),
      bin,
      npm: join(roaming, 'npm'),
      /** Its environment with `changes` made to it: a variable given `undefined` is taken away. */
      env(changes = {}) {
        const env = {
          OS: 'Windows_NT',
          PATHEXT,
          HOME: join(root, 'home'),
          APPDATA: roaming,
          PATH: bin,
          ...changes,
        }
        for (const [name, value] of Object.entries(changes)) {
          if (value === undefined) delete env[name]
        }
        return env
      },
      cleanup: () => rmSync(root, { recursive: true, force: true }),
    }
  }

  /** Pi as `npm install -g` leaves it in `folder`: its shim, and the script the shim runs. */
  function installPi(folder, script = '') {
    const cli = stub(
      join(folder, 'node_modules', '@earendil-works', 'pi-coding-agent', 'dist', 'cli.js'),
      script,
    )
    return { shim: stub(join(folder, 'pi.cmd'), PI_SHIM), script: cli }
  }

  it('is where a harness is found when PATH lacks it, by the .cmd Windows starts', () => {
    const win = machine()
    try {
      const env = win.env()
      assert.equal(harnessPath('pi', env), null)
      // What npm writes for a global package is three files, one of which starts.
      stub(join(win.npm, 'pi'))
      stub(join(win.npm, 'pi.ps1'))
      assert.equal(harnessPath('pi', env), null, 'a bare name and a script are no program')
      stub(join(win.npm, 'pi.cmd'))
      assert.equal(harnessPath('pi', env), join(win.npm, 'pi.cmd'))
      stub(join(win.npm, 'codex.cmd'))
      assert.equal(harnessPath('claude', env), null, "another harness's shim is not this one's")
    } finally {
      win.cleanup()
    }
  })

  it("comes after PATH, the harness's own places and the other common ones", () => {
    const win = machine()
    try {
      const env = win.env()
      stub(join(win.npm, 'pi.cmd'))
      assert.equal(harnessPath('pi', env), join(win.npm, 'pi.cmd'))
      for (const common of ['.bun', '.npm-global', '.volta']) {
        const there = stub(join(win.home, common, 'bin', 'pi.cmd'))
        assert.equal(harnessPath('pi', env), there, `${common} before npm's folder`)
        rmSync(there)
      }
      stub(join(win.home, '.volta', 'bin', 'pi.cmd'))
      const own = stub(join(win.home, '.pi', 'bin', 'pi.cmd'))
      assert.equal(harnessPath('pi', env), own, "the harness's own place before the common ones")
      const onPath = stub(join(win.bin, 'pi.cmd'))
      assert.equal(harnessPath('pi', env), onPath)
    } finally {
      win.cleanup()
    }
  })

  it('adds nothing when APPDATA is missing or empty, nor a folder named npm in the working one', () => {
    const win = machine()
    const previous = process.cwd()
    try {
      // Where Windows keeps roaming data by default: not where a missing APPDATA points.
      stub(join(win.npm, 'pi.cmd'))
      // And where an empty APPDATA, joined with `npm`, would point from the working folder.
      stub(join(win.root, 'npm', 'pi.cmd'))
      process.chdir(win.root)
      for (const appdata of [undefined, '']) {
        const env = win.env({ APPDATA: appdata })
        assert.equal(harnessPath('pi', env), null, `APPDATA ${appdata}`)
      }
      assert.equal(harnessPath('pi', win.env()), join(win.npm, 'pi.cmd'))
    } finally {
      process.chdir(previous)
      win.cleanup()
    }
  })

  it('changes nothing off Windows, whatever APPDATA says', {
    skip: process.platform === 'win32' && 'this platform is Windows',
  }, () => {
    const win = machine()
    try {
      // A bare name is the program there, and this one starts.
      stub(join(win.npm, 'pi'))
      stub(join(win.npm, 'pi.cmd'))
      for (const OS of [undefined, 'Linux', 'Darwin']) {
        assert.equal(harnessPath('pi', win.env({ OS })), null, `OS ${OS}`)
      }
      // The same files, found where the environment says it is Windows's.
      assert.equal(harnessPath('pi', win.env()), join(win.npm, 'pi.cmd'))
    } finally {
      win.cleanup()
    }
  })

  describe('holds a shim that is started as one on PATH is', () => {
    it("with the node beside it, else the one on PATH, else the app's own", () => {
      const win = machine()
      try {
        const env = win.env()
        const { shim, script } = installPi(win.npm)
        assert.equal(harnessPath('pi', env), shim)
        const started = (file) => ({ file, args: [script, '--version'], options: {} })
        // No node on PATH, none beside the shim: the one this runs, as for a shim on PATH.
        assert.deepEqual(runnable(shim, ['--version'], env), started(process.execPath))
        stub(join(win.bin, 'node.exe'))
        assert.deepEqual(runnable(shim, ['--version'], env), started(join(win.bin, 'node.exe')))
        // The same shim in a folder PATH names is started with the same node.
        const onPath = installPi(win.bin).shim
        assert.equal(runnable(onPath, [], env).file, runnable(shim, [], env).file)
        // And the node beside it first, as npm itself would.
        stub(join(win.npm, 'node.exe'))
        assert.deepEqual(runnable(shim, ['--version'], env), started(join(win.npm, 'node.exe')))
      } finally {
        win.cleanup()
      }
    })

    it('and a window opens on it as its node and script', () => {
      const win = machine()
      try {
        const env = win.env()
        const { shim, script } = installPi(win.npm)
        const node = stub(join(win.npm, 'node.exe'))
        const found = harnessPath('pi', env)
        assert.equal(found, shim)
        assert.deepEqual(paneArgv([found, '--model', 'a b'], env), [node, script, '--model', 'a b'])
      } finally {
        win.cleanup()
      }
    })

    it('and the harness runs', () => {
      const win = machine()
      try {
        // No node on PATH or beside the shim: this very Node runs the script.
        const env = win.env(windowsEnv())
        installPi(win.npm, "console.log('pi', ...process.argv.slice(2))\n")
        const found = harnessPath('pi', env)
        assert.equal(found, join(win.npm, 'pi.cmd'))
        const run = runnable(found, ['--version'], env)
        assert.equal(
          execFileSync(run.file, run.args, { ...run.options, env, encoding: 'utf8' }),
          'pi --version\n',
        )
      } finally {
        win.cleanup()
      }
    })
  })
})
