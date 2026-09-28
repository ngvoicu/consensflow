#!/usr/bin/env node
/**
 * Build this checkout as ConsensFlow Candidate and install it beside the live app.
 *
 * The candidate is the same source with another identity
 * (`src-tauri/tauri.candidate.conf.json`): its own name, bundle identifier and
 * WebKit storage, and — because a build that is not the release chooses
 * ~/.consensflow-candidate (`isolated_home` in src-tauri/src/lib.rs) — its own
 * state, even when it is opened from Finder. It replaces only
 * ~/Applications/ConsensFlow Candidate.app.
 *
 * The live app is proven untouched rather than assumed: its bundle and roster
 * are fingerprinted before the build and compared after, and its process must
 * still be running.
 */
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const REPO = dirname(APP)
const NAME = 'ConsensFlow Candidate.app'
const IDENTIFIER = 'dev.ngvoicu.consensflow.candidate'
const BUILT = join(APP, 'src-tauri', 'target', 'release', 'bundle', 'macos', NAME)
const TARGET = join(homedir(), 'Applications', NAME)
const HOME = join(homedir(), '.consensflow-candidate')
const LIVE_APP = '/Applications/ConsensFlow.app'
const LIVE_ROSTER = join(homedir(), '.consensflow', 'agents.json')

const fail = (message) => {
  process.stderr.write(`candidate: ${message}\n`)
  process.exit(1)
}
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex')

/** PIDs and commands of processes started from inside `bundle`. */
function processesOf(bundle) {
  const prefix = `${join(bundle, 'Contents', 'MacOS')}/`
  return execFileSync('/bin/ps', ['-axo', 'pid=,command='], { encoding: 'utf8' })
    .split('\n')
    .map((line) => line.trim().match(/^(\d+) (.*)$/))
    .filter((match) => match?.[2].startsWith(prefix))
    .map((match) => ({ pid: Number(match[1]), command: match[2] }))
}

/** One hash over every path and byte in a tree: any change changes it. */
function fingerprint(root) {
  const hash = createHash('sha256')
  const walk = (directory) => {
    for (const name of readdirSync(directory).sort()) {
      const path = join(directory, name)
      const stat = lstatSync(path)
      hash.update(path.slice(root.length))
      if (stat.isDirectory()) walk(path)
      else hash.update(stat.isSymbolicLink() ? readlinkSync(path) : readFileSync(path))
    }
  }
  walk(root)
  return hash.digest('hex')
}

const canaries = () => ({
  app: existsSync(LIVE_APP) ? fingerprint(LIVE_APP) : null,
  roster: existsSync(LIVE_ROSTER) ? sha(readFileSync(LIVE_ROSTER)) : null,
})

const plist = (bundle, key) =>
  execFileSync('/usr/bin/plutil', ['-extract', key, 'raw', '-o', '-', join(bundle, 'Contents', 'Info.plist')], {
    encoding: 'utf8',
  }).trim()

if (process.platform !== 'darwin') fail('the candidate build is macOS-only for now')
if (processesOf(TARGET).length > 0) fail(`quit ConsensFlow Candidate first — it is running from ${TARGET}`)

const liveApp = processesOf(LIVE_APP).filter((row) => row.command === `${LIVE_APP}/Contents/MacOS/app`)
const before = canaries()

execFileSync('npm', ['run', 'build', '--', '--config', join(APP, 'src-tauri', 'tauri.candidate.conf.json')], {
  cwd: APP,
  stdio: 'inherit',
})
// A bundle that kept the release identity would share the live app's state
// root when opened from Finder: refuse it before it is installed anywhere.
if (plist(BUILT, 'CFBundleIdentifier') !== IDENTIFIER) fail(`${BUILT} does not carry ${IDENTIFIER}`)
// The packaged smoke runs THIS bundle with a throwaway home; a candidate that
// fails it is never installed.
try {
  execFileSync(process.execPath, ['--test', join(REPO, 'tests', 'smoke.test.mjs')], {
    cwd: REPO,
    stdio: 'inherit',
    env: { ...process.env, CONSENSFLOW_SMOKE: '1', CONSENSFLOW_SMOKE_APP: BUILT },
  })
} catch {
  fail('the packaged smoke failed; the candidate was not installed')
}

mkdirSync(dirname(TARGET), { recursive: true })
const next = `${TARGET}.next`
const previous = `${TARGET}.previous`
rmSync(next, { recursive: true, force: true })
rmSync(previous, { recursive: true, force: true })
execFileSync('/usr/bin/ditto', [BUILT, next])
if (existsSync(TARGET)) renameSync(TARGET, previous)
renameSync(next, TARGET)
rmSync(previous, { recursive: true, force: true })
execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', TARGET], { stdio: 'inherit' })

// The candidate starts with your saved agents; after that its roster is its own.
mkdirSync(HOME, { recursive: true, mode: 0o700 })
const roster = join(HOME, 'agents.json')
if (!existsSync(roster) && existsSync(LIVE_ROSTER)) {
  copyFileSync(LIVE_ROSTER, roster)
  chmodSync(roster, 0o600)
}
const git = (...args) => execFileSync('git', args, { cwd: REPO, encoding: 'utf8' }).trim()
writeFileSync(
  join(HOME, 'candidate-build.json'),
  `${JSON.stringify(
    {
      version: plist(TARGET, 'CFBundleShortVersionString'),
      app: TARGET,
      source: REPO,
      head: git('rev-parse', 'HEAD'),
      uncommittedFiles: git('status', '--porcelain').split('\n').filter(Boolean).length,
      builtAt: new Date().toISOString(),
    },
    null,
    2,
  )}\n`,
)

const after = canaries()
if (after.app !== before.app) fail(`the live app at ${LIVE_APP} changed during the build — investigate`)
if (after.roster !== before.roster) {
  fail(`${LIVE_ROSTER} changed during the build — expected only if you edited agents in the live app`)
}
for (const { pid } of liveApp) {
  try {
    process.kill(pid, 0) // signal 0: an existence check that delivers nothing
  } catch {
    fail(`the live app (PID ${pid}) is no longer running`)
  }
}

process.stdout.write(
  `installed ${TARGET} (${plist(TARGET, 'CFBundleShortVersionString')})\n` +
    `state: ${HOME}\n` +
    `live app and roster unchanged${liveApp.length ? `; live PID ${liveApp.map((row) => row.pid).join(', ')} running` : ''}\n`,
)
