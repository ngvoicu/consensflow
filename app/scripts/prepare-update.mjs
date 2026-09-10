#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { readFileSync, statSync, writeFileSync } from 'node:fs'
import { basename, join, resolve } from 'node:path'

const REQUIRED = ['bundle', 'archive', 'signature', 'notes', 'output', 'channel', 'date']
const CHANNELS = new Set(['alpha', 'stable'])
const SIGNATURE_LINE = /^[A-Za-z0-9+/]+={0,2}$/

const ARCHIVE_PROBE = String.raw`
import hashlib, json, os, plistlib, stat, sys, tarfile
from pathlib import PurePosixPath

def fail(message):
    raise RuntimeError(message)

def manifest(root):
    result = []
    def visit(path, name):
        info = os.lstat(path)
        mode = stat.S_IMODE(info.st_mode)
        if stat.S_ISLNK(info.st_mode):
            fail('the app bundle contains a symlink: ' + name)
        if stat.S_ISDIR(info.st_mode):
            result.append([name, 'dir', mode])
            for child in sorted(os.listdir(path)):
                visit(os.path.join(path, child), name + '/' + child)
        elif stat.S_ISREG(info.st_mode):
            digest = hashlib.sha256()
            with open(path, 'rb') as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(chunk)
            result.append([name, 'file', mode, digest.hexdigest()])
        else:
            fail('the app bundle contains a special file: ' + name)
    visit(root, 'ConsensFlow.app')
    return result

def member_bytes(archive, name):
    member = archive.getmember(name)
    stream = archive.extractfile(member)
    if stream is None:
        fail('archive member is not readable: ' + name)
    return stream.read()

try:
    bundle, archive_path = sys.argv[1], sys.argv[2]
    expected = manifest(bundle)
    actual = []
    seen = set()
    with tarfile.open(archive_path, 'r:gz') as archive:
        for member in archive.getmembers():
            path = PurePosixPath(member.name.rstrip('/'))
            if path.is_absolute() or '..' in path.parts or path.parts[:1] != ('ConsensFlow.app',):
                fail('the update archive contains an unsafe path: ' + member.name)
            name = str(path)
            if name in seen:
                fail('the update archive contains a duplicate path: ' + name)
            seen.add(name)
            mode = member.mode & 0o777
            if member.issym() or member.islnk():
                fail('the update archive contains a symlink or hard link: ' + name)
            if member.isdir():
                actual.append([name, 'dir', mode])
            elif member.isfile():
                stream = archive.extractfile(member)
                if stream is None:
                    fail('archive file is not readable: ' + name)
                digest = hashlib.sha256()
                for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(chunk)
                actual.append([name, 'file', mode, digest.hexdigest()])
            else:
                fail('the update archive contains a special entry: ' + name)
        if 'ConsensFlow.app' not in seen:
            fail('the update archive has no ConsensFlow.app root')
        archived_plist = plistlib.loads(member_bytes(archive, 'ConsensFlow.app/Contents/Info.plist'))
        archived_package = json.loads(member_bytes(archive, 'ConsensFlow.app/Contents/Resources/cli/package.json'))
    if sorted(actual) != sorted(expected):
        fail('the archive content manifest does not match the supplied bundle')
    version = archived_plist.get('CFBundleShortVersionString')
    if not isinstance(version, str) or not isinstance(archived_package.get('version'), str):
        fail('the archive has no readable packaged versions')
    print(json.dumps({'version': version, 'cliVersion': archived_package['version']}))
except Exception as cause:
    print(str(cause), file=sys.stderr)
    sys.exit(1)
`

function fail(message) {
  throw new Error(message)
}

function parseArgs(argv) {
  const values = {}
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index]
    if (!flag.startsWith('--') || !REQUIRED.includes(flag.slice(2)) && flag !== '--repo') {
      fail(`unknown argument: ${flag}`)
    }
    const name = flag.slice(2)
    const value = argv[index + 1]
    if (value === undefined || value.startsWith('--')) fail(`${flag} needs a value`)
    if (Object.hasOwn(values, name)) fail(`duplicate argument: ${flag}`)
    values[name] = value
    index += 1
  }
  for (const name of REQUIRED) {
    if (!Object.hasOwn(values, name)) fail(`missing required argument: --${name}`)
  }
  return { ...values, repo: values.repo === undefined ? process.cwd() : resolve(values.repo) }
}

function readJson(path, label) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch (cause) {
    fail(`could not read ${label}: ${cause.message}`)
  }
}

function semver(text, label) {
  if (typeof text !== 'string') fail(`${label} must be a semantic version`)
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(text)
  if (match === null) fail(`${label} is not a canonical semantic version: ${text}`)
  if (match[4]?.split('.').some((part) => /^\d+$/.test(part) && part.length > 1 && part.startsWith('0'))) {
    fail(`${label} has a leading-zero prerelease identifier`)
  }
  return { text, prerelease: match[4] === undefined ? [] : match[4].split('.') }
}

function readPlist(app, field) {
  const result = spawnSync('/usr/bin/plutil', [
    '-extract', field, 'raw', '-o', '-', join(app, 'Contents', 'Info.plist'),
  ], { encoding: 'utf8' })
  if (result.status !== 0) fail(`bundle has no readable ${field} in Info.plist`)
  return result.stdout.trim()
}

function bundleInfo(app) {
  const path = resolve(app)
  if (!path.endsWith('.app') || !statSync(path).isDirectory()) fail(`bundle is not a .app directory: ${path}`)
  const executable = readPlist(path, 'CFBundleExecutable')
  if (executable.includes('/') || executable.includes('\\') || executable.includes('\0')) {
    fail('bundle executable name is unsafe')
  }
  const binary = join(path, 'Contents', 'MacOS', executable)
  const cli = join(path, 'Contents', 'Resources', 'cli')
  const packagePath = join(cli, 'package.json')
  for (const [label, requiredPath] of [
    ['native executable', binary],
    ['bundled CLI package', packagePath],
    ['bundled CLI entrypoint', join(cli, 'bin', 'cf.mjs')],
    ['bundled CLI hosts', join(cli, 'hosts')],
    ['bundled CLI source', join(cli, 'src')],
  ]) {
    try {
      statSync(requiredPath)
    } catch {
      fail(`bundle is missing ${label}: ${requiredPath}`)
    }
  }
  const version = readPlist(path, 'CFBundleShortVersionString')
  const cliVersion = readJson(packagePath, 'bundled CLI package').version
  semver(version, 'bundle version')
  semver(cliVersion, 'bundled CLI version')
  if (version !== cliVersion) fail(`bundle and bundled CLI versions differ: ${version} != ${cliVersion}`)
  return {
    path,
    version,
    cliVersion,
  }
}

function sourceVersions(repo) {
  const packageVersion = readJson(join(repo, 'package.json'), 'source package.json').version
  const cargo = readFileSync(join(repo, 'app', 'src-tauri', 'Cargo.toml'), 'utf8')
  const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1]
  if (cargoVersion === undefined) fail('source Cargo.toml has no package version')
  const tauriVersion = readJson(join(repo, 'app', 'src-tauri', 'tauri.conf.json'), 'Tauri config').version
  for (const [label, version] of [['package.json', packageVersion], ['Cargo.toml', cargoVersion], ['tauri.conf.json', tauriVersion]]) {
    semver(version, `source ${label} version`)
  }
  if (![packageVersion, cargoVersion, tauriVersion].every((version) => version === packageVersion)) {
    fail('source package, Cargo and Tauri versions do not match')
  }
  return packageVersion
}

function checkSignature(path) {
  let value
  try {
    value = readFileSync(path, 'utf8').trim()
  } catch (cause) {
    fail(`could not read signature: ${cause.message}`)
  }
  if (value.length === 0 || !SIGNATURE_LINE.test(value)) fail('signature is not outer-base64 minisign text')
  let decoded
  try {
    const bytes = Buffer.from(value, 'base64')
    if (bytes.length < 80 || bytes.toString('base64') !== value) fail('signature is not canonical base64')
    decoded = bytes.toString('utf8')
    if (!Buffer.from(decoded).equals(bytes)) fail('signature is not valid UTF-8 minisign text')
  } catch {
    fail('signature is not outer-base64 minisign text')
  }
  const lines = decoded.replace(/\n$/, '').split('\n')
  if (
    lines.length !== 4 ||
    lines[0] !== 'untrusted comment: signature from tauri secret key' ||
    !SIGNATURE_LINE.test(lines[1]) ||
    !/^trusted comment: timestamp:\d+\s+file:[A-Za-z0-9._-]+$/.test(lines[2]) ||
    !SIGNATURE_LINE.test(lines[3])
  ) {
    fail('signature is not a Tauri minisign envelope')
  }
  return value
}

function checkDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value) || !Number.isFinite(Date.parse(value))) {
    fail('publication date must be RFC3339 with an explicit timezone')
  }
}

function inspectArchive(archive, bundle, version) {
  let info
  try {
    if (statSync(archive).size === 0) fail('archive is empty')
  } catch (cause) {
    fail(`could not read archive: ${cause.message}`)
  }
  const filename = basename(archive)
  if (!filename.endsWith('.app.tar.gz') || !filename.includes(version) || !/^[A-Za-z0-9._-]+$/.test(filename)) {
    fail('archive filename must be a safe, versioned .app.tar.gz asset')
  }
  const result = spawnSync('python3', ['-c', ARCHIVE_PROBE, bundle, archive], { encoding: 'utf8' })
  if (result.status !== 0) fail(`archive safety/content check failed: ${result.stderr.trim() || 'python3 failed'}`)
  try {
    info = JSON.parse(result.stdout)
  } catch {
    fail('archive probe returned invalid metadata')
  }
  semver(info.version, 'archived bundle version')
  semver(info.cliVersion, 'archived CLI version')
  if (info.version !== version || info.cliVersion !== version) fail('archive packaged versions do not match the bundle')
}

function main(argv) {
  const options = parseArgs(argv)
  const bundle = bundleInfo(options.bundle)
  const source = sourceVersions(options.repo)
  if (source !== bundle.version) fail(`source version ${source} does not match bundle version ${bundle.version}`)
  const version = semver(bundle.version, 'release version')
  if (!CHANNELS.has(options.channel)) fail('channel must be alpha or stable')
  if (options.channel === 'stable' && version.prerelease.length > 0) fail('stable channel requires a stable release')
  if (options.channel === 'alpha' && version.prerelease.length > 0 && version.prerelease[0] !== 'alpha') fail('alpha channel accepts only alpha prereleases or stable releases')
  checkDate(options.date)
  const notes = readFileSync(options.notes, 'utf8').trim()
  if (Buffer.byteLength(notes, 'utf8') > 64 * 1024) fail('release notes exceed 64 KiB')
  const signature = checkSignature(options.signature)
  inspectArchive(options.archive, bundle.path, bundle.version)
  const filename = basename(options.archive)
  const metadata = {
    version: bundle.version,
    notes,
    pub_date: options.date,
    platforms: {
      'darwin-aarch64': {
        url: `https://github.com/ngvoicu/consensflow/releases/download/v${bundle.version}/${filename}`,
        signature,
      },
    },
  }
  writeFileSync(options.output, `${JSON.stringify(metadata)}\n`, 'utf8')
}

try {
  main(process.argv.slice(2))
} catch (cause) {
  console.error(`prepare-update: ${cause instanceof Error ? cause.message : String(cause)}`)
  process.exitCode = 1
}
