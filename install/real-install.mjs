// The installers against a real release, as the CI's installers job runs them
// on macOS and Windows (npm run install:real):
//
//   node install/real-install.mjs [--channel alpha|stable] [--dir DIR] [--shell powershell|pwsh] [--upgrade] [--no-open]
//
// It reads the channel's feed on GitHub, and installs the release it names:
//
// - with the script as it is published (install/install.sh on a Mac,
//   install.ps1 on Windows), from GitHub, where the release has a signed
//   SHA256SUMS (3.0.7-alpha.1 and later);
// - else from a mirror on this machine: the release's own files from GitHub (its
//   SHA256SUMS and its DMG or installer, checked against each other), with a
//   signature of that SHA256SUMS by a key made for the run, installed by a test
//   copy of the script that trusts that key and differs from it in nothing else.
//
// First, from the mirror, a copy of the DMG or installer with one byte changed
// must be refused, with nothing installed. With --upgrade, the release before it
// is installed first (from the mirror), opened (unless --no-open), and the
// install of the new one must ask it to quit and upgrade it where it is. Each
// install is then held to the version: the app's own (Info.plist, Windows'
// uninstall entry) and the `cf` inside it.
//
// On a Mac the app goes to DIR (--dir), which a run outside CI must name: the
// ConsensFlow in /Applications is never touched by it. On Windows the per-user
// installer puts it where it puts it.

import { Buffer } from 'node:buffer'
import { spawn, spawnSync } from 'node:child_process'
import crypto from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { parseArgs } from 'node:util'

import {
  feedOf,
  INSTALL_PS1,
  INSTALL_SH,
  makeKey,
  scriptedRelease,
  serve,
  testCopy,
} from './kit.mjs'

const GITHUB = 'https://github.com/ngvoicu/consensflow/releases'
const API = 'https://api.github.com/repos/ngvoicu/consensflow/releases?per_page=30'
const mac = process.platform === 'darwin'

const { values: options } = parseArgs({
  options: {
    channel: { type: 'string', default: 'alpha' },
    dir: { type: 'string' },
    shell: { type: 'string', default: 'powershell' },
    upgrade: { type: 'boolean', default: false },
    'no-open': { type: 'boolean', default: false },
  },
})

function say(words) {
  console.log(`real-install: ${words}`)
}

function fail(words) {
  console.error(`real-install: ${words}`)
  process.exit(1)
}

async function download(url) {
  const headers =
    url.startsWith('https://api.github.com/') && process.env.GITHUB_TOKEN
      ? { authorization: `Bearer ${process.env.GITHUB_TOKEN}` }
      : {}
  const response = await fetch(url, { headers })
  if (!response.ok) return null
  return Buffer.from(await response.arrayBuffer())
}

/** The file of a release this platform installs. */
function artifactOf(version) {
  return mac ? `ConsensFlow_${version}_aarch64.dmg` : `ConsensFlow_${version}_x64-setup.exe`
}

const fetched = new Map()

/** The release `version`'s SHA256SUMS and file, from GitHub, held to each other (downloaded once a run). */
async function releaseFiles(version) {
  if (!fetched.has(version)) fetched.set(version, await fetchRelease(version))
  return fetched.get(version)
}

async function fetchRelease(version) {
  const name = artifactOf(version)
  const sums = await download(`${GITHUB}/download/v${version}/SHA256SUMS`)
  const file = await download(`${GITHUB}/download/v${version}/${name}`)
  if (!sums || !file) fail(`could not download ${version}'s SHA256SUMS and ${name} from GitHub`)
  const listed = sums
    .toString('latin1')
    .split('\n')
    .find((line) => line.endsWith(`  ${name}`))
  const hash = crypto.createHash('sha256').update(file).digest('hex')
  if (!listed?.startsWith(hash))
    fail(`${name} from GitHub does not hash to what ${version}'s SHA256SUMS lists`)
  return { sums: sums.toString('latin1'), name, file }
}

/** The release before `version` that has this platform's file, by GitHub's list of releases. */
async function previousOf(version) {
  const list = JSON.parse((await download(API))?.toString('utf8') ?? '[]')
  const names = list.map((release) => release.tag_name.replace(/^v/, ''))
  const at = names.indexOf(version)
  const before = list
    .slice(at + 1)
    .find((release) =>
      release.assets.some((asset) => asset.name === artifactOf(release.tag_name.replace(/^v/, ''))),
    )
  if (at < 0 || !before)
    fail(`GitHub lists no release before ${version} with ${artifactOf('<version>')}`)
  return before.tag_name.replace(/^v/, '')
}

/** Runs the script at `script` with `env`, saying what it says; resolves to its status and words. */
function runScript(script, env, args = []) {
  const [program, words] = mac
    ? ['/bin/sh', [script, ...args]]
    : [
        options.shell,
        [
          '-NoProfile',
          '-ExecutionPolicy',
          'Bypass',
          '-Command',
          `Get-Content -Raw -LiteralPath '${script}' | Invoke-Expression`,
        ],
      ]
  return new Promise((resolve) => {
    const child = spawn(program, words, { env: { ...process.env, ...env } })
    let out = ''
    for (const stream of [child.stdout, child.stderr]) {
      stream.on('data', (chunk) => {
        out += chunk
        process.stdout.write(chunk)
      })
    }
    child.on('close', (status) => resolve({ status, out }))
  })
}

// The `cf` asked for its version runs as a person's terminal runs it: without the variables a
// ConsensFlow window gives its member, under which it is that member's `cf` (as when this runs
// in one).
const cfEnv = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.toUpperCase().startsWith('CONSENSFLOW_')),
)

/** The version installed, as the app says it, and as the `cf` inside it says it. */
function installed(dir) {
  if (mac) {
    const app = path.join(dir, 'ConsensFlow.app')
    if (!existsSync(app)) return null
    const plist = spawnSync(
      '/usr/libexec/PlistBuddy',
      ['-c', 'Print :CFBundleShortVersionString', path.join(app, 'Contents/Info.plist')],
      { encoding: 'utf8' },
    )
    const cf = spawnSync(path.join(app, 'Contents/Resources/cli/bin/cf'), ['--version'], {
      encoding: 'utf8',
      env: cfEnv,
    })
    return { app: plist.stdout.trim(), cf: cf.stdout.trim(), where: app }
  }
  const entry = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-Command',
      "$e = Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ConsensFlow' -ErrorAction SilentlyContinue; if ($e) { $e.DisplayVersion; $e.InstallLocation.Trim('\"') }",
    ],
    { encoding: 'utf8' },
  )
  const [version, where] = entry.stdout.trim().split(/\r?\n/)
  if (!version) return null
  const cf = spawnSync(path.join(where, 'cli', 'bin', 'cf.exe'), ['--version'], {
    encoding: 'utf8',
    env: cfEnv,
  })
  return { app: version, cf: cf.stdout.trim(), where }
}

function expectInstalled(dir, version) {
  const found = installed(dir)
  if (!found || found.app !== version || found.cf !== version)
    fail(
      `after the install, the app says ${found?.app ?? 'nothing'} and its cf ${found?.cf ?? 'nothing'}, not ${version}`,
    )
  say(`installed ${version} in ${found.where}: the app and its cf say ${version}`)
  if (mac) {
    for (const words of [
      ['codesign', ['--verify', '--deep', '--strict', found.where]],
      ['/usr/sbin/spctl', ['--assess', '--type', 'execute', found.where]],
    ]) {
      const ran = spawnSync(words[0], words[1], { encoding: 'utf8' })
      if (ran.status !== 0) fail(`${words[0]} refuses the installed app: ${ran.stderr}`)
    }
  }
}

/** The processes of the installed app. */
function running(dir) {
  const where = installed(dir)?.where
  if (!where) return []
  if (mac) {
    return spawnSync('ps', ['-axo', 'pid=,comm='], { encoding: 'utf8' })
      .stdout.split('\n')
      .filter((line) => line.includes(`${where}/Contents/`))
  }
  const ran = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-Command',
      `@(Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith('${where}\\', 'OrdinalIgnoreCase') }).Count`,
    ],
    { encoding: 'utf8' },
  )
  return Array.from({ length: Number(ran.stdout.trim()) || 0 })
}

async function openApp(dir) {
  const where = installed(dir).where
  if (mac) spawnSync('open', ['-g', where])
  else spawn(path.join(where, 'ConsensFlow.exe'), [], { detached: true, stdio: 'ignore' }).unref()
  for (let i = 0; i < 120; i++) {
    if (running(dir).length > 0) {
      await new Promise((resolve) => setTimeout(resolve, 15000))
      say(`the installed app is open (${running(dir).length} processes)`)
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 1000))
  }
  fail('the installed app did not open within two minutes')
}

/** A mirror on this machine of the releases `versions`, signed by `key`, with the feed naming the last. */
async function mirror(versions, key, tamper) {
  const files = {}
  for (const version of versions) {
    const { sums, name, file } = await releaseFiles(version)
    const served = Buffer.from(file)
    if (tamper && version === versions.at(-1)) served[Math.floor(served.length / 2)] ^= 1
    Object.assign(files, scriptedRelease('/releases', version, { [name]: served }, key, { sums }))
  }
  Object.assign(files, feedOf('/releases', options.channel, versions.at(-1)))
  return serve(files)
}

if (!['alpha', 'stable'].includes(options.channel)) fail('--channel is alpha or stable')
if (mac && !process.env.CI) {
  if (!options.dir)
    fail(
      'on a Mac outside CI, name the folder to install into with --dir (the ConsensFlow in /Applications is never touched)',
    )
  // A second ConsensFlow opened on a Mac in use would start a second daemon on its home.
  if (options.upgrade && !options['no-open'])
    fail(
      'on a Mac outside CI, --upgrade runs with --no-open: the ConsensFlow in use is never joined by another',
    )
}
const dir = mac ? path.resolve(options.dir ?? '/Applications') : null
if (dir) mkdirSync(dir, { recursive: true })
const installEnv = mac ? { CONSENSFLOW_INSTALL_DIR: dir } : {}

const feed = JSON.parse(
  (await download(`${GITHUB}/download/feed-${options.channel}/latest.json`))?.toString('utf8') ??
    'null',
)
if (!feed?.version) fail(`the ${options.channel} feed on GitHub names no version`)
const version = feed.version
const signed = (await download(`${GITHUB}/download/v${version}/SHA256SUMS.sig`)) !== null
say(
  `the ${options.channel} feed names ${version}; its SHA256SUMS is ${signed ? 'signed: it is installed from GitHub' : 'not signed (before 3.0.7-alpha.1): it is installed from a mirror'}`,
)

const work = mkdtempSync(path.join(os.tmpdir(), 'cf-real-install-'))
try {
  const key = makeKey()
  const copy = testCopy(
    mac ? INSTALL_SH : INSTALL_PS1,
    path.join(work, mac ? 'install.sh' : 'install.ps1'),
    key.keyLine,
  )
  const before = JSON.stringify(installed(dir))

  // A copy of the file with one byte changed is refused, and nothing is installed.
  const tampered = await mirror([version], key, true)
  try {
    const ran = await runScript(copy, {
      ...installEnv,
      CONSENSFLOW_RELEASES_URL: `${tampered.url}/releases`,
      CONSENSFLOW_CHANNEL: options.channel,
      CONSENSFLOW_VERSION: '',
    })
    if (
      ran.status === 0 ||
      !/was changed: its SHA-256 is/.test(ran.out) ||
      !/nothing was installed/.test(ran.out)
    )
      fail('the changed file was not refused')
    if (JSON.stringify(installed(dir)) !== before)
      fail('the refused install changed what is installed')
    say(`${artifactOf(version)} with one byte changed was refused, and nothing was installed`)
  } finally {
    await tampered.close()
  }

  if (options.upgrade) {
    const previous = await previousOf(version)
    const local = await mirror([previous, version], key, false)
    try {
      const first = await runScript(copy, {
        ...installEnv,
        CONSENSFLOW_RELEASES_URL: `${local.url}/releases`,
        CONSENSFLOW_VERSION: previous,
      })
      if (first.status !== 0) fail(`the install of ${previous} failed`)
      expectInstalled(dir, previous)
      if (!options['no-open']) await openApp(dir)
      const second = await runScript(copy, {
        ...installEnv,
        CONSENSFLOW_RELEASES_URL: `${local.url}/releases`,
        CONSENSFLOW_CHANNEL: options.channel,
        CONSENSFLOW_VERSION: '',
      })
      if (second.status !== 0) fail(`the upgrade from ${previous} to ${version} failed`)
      if (!options['no-open'] && !/is open: asking it to (quit|close)/.test(second.out))
        fail('the upgrade did not ask the open app to quit')
      if (running(dir).length > 0) fail('the app still runs after the upgrade')
      expectInstalled(dir, version)
      say(
        `${previous} was upgraded to ${version} where it was${options['no-open'] ? '' : ', once the open app had quit when asked'}`,
      )
    } finally {
      await local.close()
    }
  }

  if (signed) {
    const ran = await runScript(
      mac ? INSTALL_SH : INSTALL_PS1,
      {
        ...installEnv,
        CONSENSFLOW_CHANNEL: options.channel,
        CONSENSFLOW_VERSION: '',
        CONSENSFLOW_RELEASES_URL: '',
      },
      [],
    )
    if (ran.status !== 0) fail(`the published script did not install ${version} from GitHub`)
  } else {
    const local = await mirror([version], key, false)
    try {
      const ran = await runScript(copy, {
        ...installEnv,
        CONSENSFLOW_RELEASES_URL: `${local.url}/releases`,
        CONSENSFLOW_CHANNEL: options.channel,
        CONSENSFLOW_VERSION: '',
      })
      if (ran.status !== 0) fail(`the install of ${version} from the mirror failed`)
    } finally {
      await local.close()
    }
  }
  expectInstalled(dir, version)
  say(
    `ConsensFlow ${version} installed ${signed ? 'from GitHub with the published script' : 'from the mirror (its release has no signed SHA256SUMS)'}`,
  )
} finally {
  rmSync(work, { recursive: true, force: true })
}
