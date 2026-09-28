import assert from 'node:assert/strict'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { agentsUi } from '../src/core/agents-server.js'
import { Credentials, startApi } from '../src/core/api.js'
import { HarnessAdmin } from '../src/harness-admin.js'
import { openLedger } from '../src/ledger/index.js'
import { fakeExecutable, tempEnv } from './helpers.mjs'

test('administration lists missing harnesses without probing or installing them', async () => {
  const t = tempEnv()
  try {
    const admin = new HarnessAdmin(t.env, {
      latest: async () => {
        throw new Error('must not call')
      },
    })
    const rows = await admin.check()
    assert.equal(rows.length, 6)
    assert.ok(rows.every((r) => r.installed === false && r.version.state === 'not-installed'))
    assert.equal(rows.find((r) => r.id === 'kimi').chief, false)
  } finally {
    t.cleanup()
  }
})

test('version and update checks cache results and refresh on request', async () => {
  const t = tempEnv()
  try {
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    fakeExecutable(join(t.env.PATH, 'codex'), { output: 'codex-cli 99.1.0' })
    let calls = 0
    const admin = new HarnessAdmin(t.env, {
      latest: async () => {
        calls++
        return '99.2.0'
      },
    })
    const [row] = await admin.check('codex')
    assert.equal(row.version.value, '99.1.0')
    assert.equal(row.update.state, 'available')
    assert.equal(Object.hasOwn(row, 'integration'), false)
    await admin.check('codex')
    assert.equal(calls, 1)
    await admin.check('codex', { refresh: true })
    assert.equal(calls, 2)
  } finally {
    t.cleanup()
  }
})

test('offline and invalid version output are explicit failures, not latest or incompatible', async () => {
  const t = tempEnv()
  try {
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    fakeExecutable(join(t.env.PATH, 'claude'), { output: 'unusual' })
    const admin = new HarnessAdmin(t.env, {
      latest: async () => {
        throw new Error('offline')
      },
    })
    const [row] = await admin.check('claude')
    assert.equal(row.version.state, 'unknown')
    assert.equal(row.update.state, 'error')
    assert.equal(Object.hasOwn(row, 'integration'), false)
    await assert.rejects(admin.check('../invalid'), /Unknown harness/)
  } finally {
    t.cleanup()
  }
})

test('each install method names itself and the command that updates it the same way', async () => {
  const { releaseSource } = await import('../src/harness-admin.js')
  const t = tempEnv()
  try {
    // These layouts are POSIX; the paths they answer read the same on every platform here.
    const posix = (source) => ({
      ...source,
      update: source.update?.map((x) => x.replaceAll('\\', '/')),
    })
    const codex = posix(releaseSource('codex', '/opt/homebrew/Caskroom/codex/0.1/bin/codex', t.env))
    assert.deepEqual(
      [codex.url, codex.distribution, codex.update],
      [
        'https://formulae.brew.sh/api/cask/codex.json',
        'Homebrew',
        ['/opt/homebrew/bin/brew', 'upgrade', '--cask', 'codex'],
      ],
    )
    const formula = posix(
      releaseSource('opencode', '/opt/homebrew/Cellar/opencode/1/bin/opencode', t.env),
    )
    assert.deepEqual(
      [formula.url, formula.update],
      [
        'https://formulae.brew.sh/api/formula/opencode.json',
        ['/opt/homebrew/bin/brew', 'upgrade', 'opencode'],
      ],
    )
    assert.equal(
      releaseSource('claude', '/opt/homebrew/Caskroom/claude-code@latest/2/bin/claude', t.env).url,
      'https://formulae.brew.sh/api/cask/claude-code@latest.json',
    )
    const claude = releaseSource('claude', '/Users/me/.local/share/claude/versions/2.1.280', t.env)
    assert.deepEqual(
      [claude.distribution, claude.update],
      [
        "Claude's installer, latest channel",
        ['/Users/me/.local/share/claude/versions/2.1.280', 'update'],
      ],
    )
    const pi = posix(
      releaseSource(
        'pi',
        '/usr/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js',
        t.env,
      ),
    )
    assert.deepEqual(
      [pi.distribution, pi.update],
      ['npm', ['/usr/bin/npm', 'install', '-g', '@earendil-works/pi-coding-agent@latest']],
    )
    const opencode = releaseSource('opencode', '/Users/me/.opencode/bin/opencode', t.env)
    assert.deepEqual(
      [opencode.distribution, opencode.update],
      ["OpenCode's installer", ['/Users/me/.opencode/bin/opencode', 'upgrade']],
    )
    const devin = releaseSource(
      'devin',
      '/Users/me/.local/share/devin/cli/_versions/current/bin/devin',
      t.env,
    )
    assert.deepEqual(
      [devin.distribution, devin.update],
      [
        "Devin's installer",
        ['/Users/me/.local/share/devin/cli/_versions/current/bin/devin', 'update'],
      ],
    )
    // On Windows the installer copies the current version to ~/.local/bin, no link.
    const home = t.env.HOME
    const copy = join(home, '.local', 'bin', 'claude.exe')
    mkdirSync(dirname(copy), { recursive: true })
    writeFileSync(copy, '')
    assert.equal(releaseSource('claude', copy, t.env).distribution, null, 'no versions beside it')
    mkdirSync(join(home, '.local', 'share', 'claude', 'versions', '2.1.274'), { recursive: true })
    const copied = releaseSource('claude', copy, t.env)
    assert.deepEqual(
      [copied.distribution, copied.update],
      ["Claude's installer, latest channel", [copy, 'update']],
    )
    const unknown = releaseSource('codex', '/somewhere/else/codex', t.env)
    assert.deepEqual(
      [unknown.distribution, unknown.update, unknown.url],
      [null, null, 'https://registry.npmjs.org/@openai/codex/latest'],
    )
  } finally {
    t.cleanup()
  }
})

test('updates a harness with its own tool, checks it again, and says what happened', async () => {
  const t = tempEnv()
  try {
    // Codex as its own installer puts it: the version it prints lives in a file the updater rewrites.
    const bin = join(t.env.HOME, '.codex', 'bin')
    mkdirSync(bin, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    const versionFile = join(t.env.HOME, 'codex-version')
    writeFileSync(versionFile, '1.0.0\n')
    const codexPath = fakeExecutable(join(bin, 'codex'), { outputFile: versionFile })
    const runs = []
    let fail = false
    const admin = new HarnessAdmin(t.env, {
      latest: async () => '1.0.1',
      run: async (file, args, options) => {
        runs.push([file, args, options.cwd])
        if (fail) {
          const error = new Error('Command failed: codex update')
          error.stderr = 'no network\n'
          throw error
        }
        writeFileSync(versionFile, '1.0.1\n')
        return { stdout: 'Updated to 1.0.1\n', stderr: '' }
      },
    })
    const [checked] = await admin.check('codex')
    assert.deepEqual(
      [checked.distribution, checked.update.state, checked.update.command],
      ["Codex's installer", 'available', `${codexPath} update`],
    )
    const done = await admin.update('codex')
    // The command the admin ran: the fake itself on POSIX; on Windows a .cmd
    // runs through what `runnable` chose, so the shape is looser there.
    if (process.platform === 'win32') {
      assert.equal(runs.length, 1)
      assert.equal(runs[0][2], t.env.HOME)
      assert.match(JSON.stringify(runs[0]), /codex[^"]*update|update/)
    } else {
      assert.deepEqual(runs, [[codexPath, ['update'], t.env.HOME]])
    }
    assert.deepEqual(
      [done.state, done.before, done.after, done.command, done.output, done.harness.version.value],
      ['updated', '1.0.0', '1.0.1', `${codexPath} update`, 'Updated to 1.0.1', '1.0.1'],
    )
    assert.equal(done.harness.update.state, 'current', 'checked again after the update')

    fail = true
    const failed = await admin.update('codex')
    assert.deepEqual(
      [failed.state, failed.reason, failed.output, failed.after],
      ['failed', 'Command failed: codex update', 'no network', '1.0.1'],
    )

    // A CLI found somewhere ConsensFlow does not recognize is the human's to update.
    fakeExecutable(join(t.env.PATH, 'pi'), { output: '0.1.0' })
    const unsupported = await admin.update('pi')
    assert.equal(unsupported.state, 'unsupported')
    assert.match(unsupported.reason, /update it the way you installed it/)
    await assert.rejects(admin.update('devin'), /Devin is not installed/)
  } finally {
    t.cleanup()
  }
})

test('harness checks do not require session storage or expose receipt diagnostics', async () => {
  const t = tempEnv()
  mkdirSync(t.env.HOME, { recursive: true })
  mkdirSync(t.env.PATH, { recursive: true })
  mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
  fakeExecutable(join(t.env.PATH, 'codex'), { output: '1.2.3' })
  const ledger = openLedger(join(t.env.CONSENSFLOW_HOME, 'consensflow.db'))
  const token = 'ui-token'
  const server = await startApi({
    ledger,
    credentials: new Credentials(),
    ui: agentsUi(t.env, { token, harnessLatest: async () => '1.2.4' }),
  })
  try {
    const response = await fetch(`${server.url}/api/harnesses/check`, {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: '{}',
    })
    assert.equal(response.status, 200)
    const { harnesses } = await response.json()
    assert.equal(harnesses.find((row) => row.id === 'codex').update.state, 'available')
    assert.ok(
      harnesses.every(
        (row) => !Object.hasOwn(row, 'integration') && !Object.hasOwn(row, 'instructions'),
      ),
    )
  } finally {
    await server.close()
    ledger.close()
    t.cleanup()
  }
})

test('Devin diagnostics report the minimum native version and idle collection limit', async () => {
  const t = tempEnv()
  try {
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    fakeExecutable(join(t.env.PATH, 'devin'), { output: '3000.6.14' })
    const admin = new HarnessAdmin(t.env, {
      latest: async (_id, source) => {
        assert.equal(source.url, 'https://static.devin.ai/cli/current/manifest.json')
        return '3000.10.21'
      },
    })
    const [row] = await admin.check('devin')
    assert.equal(row.setup.state, 'update-required')
    assert.match(row.setup.reason, /3000.10.21/)
    assert.equal(Object.hasOwn(row, 'receiveNote'), false)
    assert.deepEqual([row.distribution, row.update.command], [null, null])
  } finally {
    t.cleanup()
  }
})
