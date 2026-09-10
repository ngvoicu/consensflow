import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const SCRIPT = fileURLToPath(new URL('../app/scripts/prepare-update.mjs', import.meta.url))
const SIGNER = fileURLToPath(new URL('../app/node_modules/.bin/tauri', import.meta.url))
const VERSION = '3.0.0-alpha.99'
const STABLE = '3.0.0'
const COMPAT = {
  'claude-code': ['2.1.265'],
  codex: ['0.153.4'],
  opencode: ['1.18.29'],
  pi: ['0.85.1'],
}
const DATE = '2026-09-09T12:00:00Z'

function writeRepo(dir, version) {
  mkdirSync(join(dir, 'app', 'src-tauri'), { recursive: true })
  mkdirSync(join(dir, 'src'), { recursive: true })
  writeFileSync(join(dir, 'package.json'), JSON.stringify({ version }))
  writeFileSync(join(dir, 'app', 'src-tauri', 'Cargo.toml'), `[package]\nversion = "${version}"\n`)
  writeFileSync(join(dir, 'app', 'src-tauri', 'tauri.conf.json'), JSON.stringify({ version }))
  writeFileSync(join(dir, 'src', 'verified-harnesses.json'), JSON.stringify(COMPAT))
}

function writeApp(parent, appName, version, cliVersion = version) {
  const app = join(parent, appName)
  const cli = join(app, 'Contents', 'Resources', 'cli')
  mkdirSync(join(cli, 'bin'), { recursive: true })
  mkdirSync(join(cli, 'hosts'), { recursive: true })
  mkdirSync(join(cli, 'src'), { recursive: true })
  mkdirSync(join(app, 'Contents', 'MacOS'), { recursive: true })
  writeFileSync(
    join(app, 'Contents', 'Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.ngvoicu.consensflow</string><key>CFBundleExecutable</key><string>ConsensFlow</string><key>CFBundleShortVersionString</key><string>${version}</string><key>CFBundleVersion</key><string>${version}</string></dict></plist>\n`,
  )
  writeFileSync(join(cli, 'package.json'), JSON.stringify({ version: cliVersion }))
  writeFileSync(join(cli, 'bin', 'cf.mjs'), '#!/usr/bin/env node\n')
  writeFileSync(join(cli, 'hosts', 'probe.txt'), 'hosts\n')
  writeFileSync(join(cli, 'src', 'verified-harnesses.json'), JSON.stringify(COMPAT))
  writeFileSync(join(app, 'Contents', 'MacOS', 'ConsensFlow'), 'binary\n')
  chmodSync(join(cli, 'bin', 'cf.mjs'), 0o755)
  chmodSync(join(app, 'Contents', 'MacOS', 'ConsensFlow'), 0o755)
  return app
}

function writeTarEntries(path, entries) {
  const source = `import io, json, sys, tarfile
with tarfile.open(sys.argv[1], 'w:gz') as archive:
    for name, data in json.loads(sys.argv[2]):
        info = tarfile.TarInfo(name)
        body = data.encode()
        info.size = len(body)
        archive.addfile(info, io.BytesIO(body))
`
  execFileSync('python3', ['-c', source, path, JSON.stringify(entries)], { stdio: 'ignore' })
}

function fixture({
  version = VERSION,
  cliVersion,
  repoVersion,
  archiveVersion,
  tamper = false,
} = {}) {
  const root = mkdtempSync(join(tmpdir(), 'cf-update-'))
  const repo = join(root, 'repo')
  writeRepo(repo, repoVersion ?? version)
  const staged = join(root, 'staged')
  mkdirSync(staged, { recursive: true })
  const appName = 'ConsensFlow.app'
  const bundle = writeApp(staged, appName, version, cliVersion ?? version)
  const archived = join(root, 'archived')
  mkdirSync(archived, { recursive: true })
  const archivedApp = writeApp(
    archived,
    appName,
    archiveVersion ?? version,
    archiveVersion ?? version,
  )
  if (tamper) {
    writeFileSync(
      join(archivedApp, 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs'),
      '#!/usr/bin/env node\n// tampered\n',
    )
  }
  const archiveName = `ConsensFlow-${version}_aarch64.app.tar.gz`
  const archive = join(root, archiveName)
  execFileSync('tar', ['-czf', archive, '-C', archived, appName], {
    env: { ...process.env, COPYFILE_DISABLE: '1' },
  })
  const key = join(root, 'test-signing-key')
  const signerEnv = { ...process.env }
  delete signerEnv.TAURI_SIGNING_PRIVATE_KEY
  delete signerEnv.TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  execFileSync(SIGNER, ['signer', 'generate', '--ci', '--password', '', '--write-keys', key], {
    env: signerEnv,
    stdio: 'ignore',
  })
  execFileSync(SIGNER, ['signer', 'sign', '--password', '', '--private-key-path', key, archive], {
    env: signerEnv,
    stdio: 'ignore',
  })
  const signature = `${archive}.sig`
  const notes = join(root, 'notes.txt')
  writeFileSync(notes, 'Alpha 99 fixes delivery races.\n')
  const compatibility = join(root, 'compat.json')
  writeFileSync(compatibility, JSON.stringify(COMPAT))
  const output = join(root, 'latest.json')
  return { root, repo, bundle, archive, signature, notes, compatibility, output }
}

function run(args) {
  const child = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' })
  return { code: child.status, stdout: child.stdout ?? '', stderr: child.stderr ?? '' }
}

function baseArgs(fx, overrides = {}) {
  const values = {
    bundle: fx.bundle,
    archive: fx.archive,
    signature: fx.signature,
    notes: fx.notes,
    output: fx.output,
    channel: 'alpha',
    date: DATE,
    repo: fx.repo,
    ...overrides,
  }
  return Object.entries(values).flatMap(([key, value]) => [`--${key}`, value])
}

describe('TEST-PANE-150 prepare-update metadata', () => {
  it('builds correct deterministic Tauri metadata on the happy path', () => {
    const fx = fixture()
    try {
      const first = run(baseArgs(fx))
      assert.equal(first.code, 0, first.stderr)
      const metadata = JSON.parse(readFileSync(fx.output, 'utf8'))
      assert.equal(metadata.version, VERSION)
      assert.equal(metadata.notes, 'Alpha 99 fixes delivery races.')
      assert.equal(metadata.pub_date, DATE)
      const archiveName = basename(fx.archive)
      assert.equal(
        metadata.platforms['darwin-aarch64'].url,
        `https://github.com/ngvoicu/consensflow/releases/download/v${VERSION}/${archiveName}`,
      )
      assert.equal(
        metadata.platforms['darwin-aarch64'].signature,
        readFileSync(fx.signature, 'utf8').trim(),
      )
      assert.equal(metadata.consensflow, undefined)
      const second = run(baseArgs(fx, { output: join(fx.root, 'latest2.json') }))
      assert.equal(second.code, 0, second.stderr)
      assert.equal(
        readFileSync(join(fx.root, 'latest2.json'), 'utf8'),
        readFileSync(fx.output, 'utf8'),
      )
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('alpha channel accepts a stable graduation version', () => {
    const fx = fixture({ version: STABLE })
    try {
      const result = run(baseArgs(fx))
      assert.equal(result.code, 0, result.stderr)
      assert.equal(JSON.parse(readFileSync(fx.output, 'utf8')).version, STABLE)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('stable channel accepts stable and rejects prereleases', () => {
    const stableFx = fixture({ version: STABLE })
    const preFx = fixture()
    try {
      const ok = run(baseArgs(stableFx, { channel: 'stable' }))
      assert.equal(ok.code, 0, ok.stderr)
      const bad = run(baseArgs(preFx, { channel: 'stable' }))
      assert.notEqual(bad.code, 0)
    } finally {
      rmSync(stableFx.root, { recursive: true, force: true })
      rmSync(preFx.root, { recursive: true, force: true })
    }
  })

  it('rejects a bundle whose plist and bundled cli versions disagree', () => {
    const fx = fixture({ cliVersion: '3.0.0-alpha.98' })
    try {
      assert.notEqual(run(baseArgs(fx)).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects a bundle that disagrees with source repo versions', () => {
    const fx = fixture({ repoVersion: '3.0.0-alpha.98' })
    try {
      const result = run(baseArgs(fx))
      assert.notEqual(result.code, 0)
      assert.match(result.stderr, /version/i)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects an archive whose packaged version differs', () => {
    const fx = fixture({ archiveVersion: '3.0.0-alpha.98' })
    try {
      assert.notEqual(run(baseArgs(fx)).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects an archive whose file bytes differ from the supplied bundle', () => {
    const fx = fixture({ tamper: true })
    try {
      const result = run(baseArgs(fx))
      assert.notEqual(result.code, 0)
      assert.match(result.stderr, /manifest|content|match/i)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects blank and malformed signatures', () => {
    const fx = fixture()
    try {
      writeFileSync(fx.signature, '   \n')
      assert.notEqual(run(baseArgs(fx)).code, 0)
      writeFileSync(fx.signature, 'not a signature!!!\n')
      assert.notEqual(run(baseArgs(fx)).code, 0)
      writeFileSync(fx.signature, 'short\n')
      assert.notEqual(run(baseArgs(fx)).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects missing and empty archives', () => {
    const fx = fixture()
    try {
      const missing = run(baseArgs(fx, { archive: join(fx.root, 'nope.tar.gz') }))
      assert.notEqual(missing.code, 0)
      writeFileSync(fx.archive, '')
      assert.notEqual(run(baseArgs(fx)).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects archives with traversal and absolute entries', () => {
    const fx = fixture()
    try {
      const evil = join(fx.root, `ConsensFlow-${VERSION}_aarch64-evil.app.tar.gz`)
      writeTarEntries(evil, [['../evil.txt', 'evil']])
      assert.notEqual(run(baseArgs(fx, { archive: evil })).code, 0)
      const absolute = join(fx.root, `ConsensFlow-${VERSION}_aarch64-abs.app.tar.gz`)
      writeTarEntries(absolute, [['/tmp/cf-evil.txt', 'evil']])
      assert.notEqual(run(baseArgs(fx, { archive: absolute })).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects invalid channels and dates', () => {
    const fx = fixture()
    try {
      assert.notEqual(run(baseArgs(fx, { channel: 'beta' })).code, 0)
      assert.notEqual(run(baseArgs(fx, { channel: 'Alpha' })).code, 0)
      assert.notEqual(run(baseArgs(fx, { channel: 'alpha', date: 'next friday' })).code, 0)
      assert.notEqual(run(baseArgs(fx, { channel: 'alpha', date: '2026-13-99T99:99:99Z' })).code, 0)
      assert.notEqual(run(baseArgs(fx, { channel: 'alpha', date: '2026-09-09' })).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects non-alpha prereleases on alpha and build metadata anywhere', () => {
    const betaFx = fixture({ version: '3.0.0-beta.1' })
    const buildFx = fixture({ version: '3.0.0-alpha.99+build.1' })
    try {
      assert.notEqual(run(baseArgs(betaFx)).code, 0)
      assert.notEqual(run(baseArgs(buildFx)).code, 0)
    } finally {
      rmSync(betaFx.root, { recursive: true, force: true })
      rmSync(buildFx.root, { recursive: true, force: true })
    }
  })

  it('rejects archive filenames without the version and unknown CLI flags', () => {
    const fx = fixture()
    try {
      const renamed = join(fx.root, 'ConsensFlow-latest.app.tar.gz')
      execFileSync('cp', [fx.archive, renamed])
      assert.notEqual(run(baseArgs(fx, { archive: renamed })).code, 0)
      const injected = run([...baseArgs(fx), '--url', 'https://evil.example/x.tar.gz'])
      assert.notEqual(injected.code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('preserves notes URLs while constructing the platform URL itself', () => {
    const fx = fixture()
    try {
      writeFileSync(fx.notes, 'See https://evil.example/notes for details.\n')
      writeFileSync(fx.compatibility, JSON.stringify(COMPAT))
      const result = run(baseArgs(fx))
      assert.equal(result.code, 0, result.stderr)
      const raw = readFileSync(fx.output, 'utf8')
      assert.match(raw, /evil\.example\/notes/)
      assert.match(raw, /github\.com\/ngvoicu\/consensflow\/releases\/download/)
      const metadata = JSON.parse(raw)
      assert.equal(metadata.notes, 'See https://evil.example/notes for details.')
      assert.doesNotMatch(metadata.platforms['darwin-aarch64'].url, /evil\.example/)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })

  it('rejects missing required arguments', () => {
    const fx = fixture()
    try {
      assert.notEqual(run(['--bundle', fx.bundle]).code, 0)
      assert.notEqual(run(baseArgs(fx).filter((arg) => arg !== fx.bundle)).code, 0)
    } finally {
      rmSync(fx.root, { recursive: true, force: true })
    }
  })
})
