/**
 * The update path, proven end to end on the packaged app: an installed app is
 * given this checkout's app as an update, by its own updater, from a feed this
 * run serves. This checkout's app ships no Node, and the update goes to an app of
 * each release that did:
 *
 * - the flip release, the release before this one: its daemon is the native
 *   `cf` and its terminal command names the `cf` of its bundle; and the same
 *   from a home that took its way back to Node (a `use-node` file), whose
 *   daemon is Node's;
 * - the bridge (`v3.0.0-alpha.81`), for a user who skips the flip: its daemon is
 *   Node's and its terminal command names Node.
 *
 * Every case of `tests/updater-smoke.test.mjs` runs for each: the update
 * itself, the two updates that are refused, and an app replaced by hand.
 *
 *   npm run smoke:updater                     # builds the apps, runs every case for both releases
 *   npm run smoke:updater -- --from flip      # the flip release alone
 *   npm run smoke:updater -- --from-app A --from-release flip --to-app B   # takes built apps
 *   npm run smoke:updater -- --only refused   # the cases whose names hold a word
 *
 * What it builds (macOS, offline): each installed release exported from its tag
 * (the flip's is the newest tag after the bridge's in this checkout's history,
 * and `--flip-ref` names another tag or a commit), and this checkout as the
 * update, which the build gives the version after the newest installed one's.
 * All are built with this run's updater public key and signed ad hoc; the key
 * pair is made for the run (`tauri signer generate`) and goes with the run's
 * folder, and the product's key and every Apple certificate stay where they are,
 * unread (`tests/updater-smoke/signing.mjs`). The apps taken as built paths
 * verify against the run's key all the same: the packaged self-test hands the
 * app the public key it is to verify with, so a build kept from another run
 * takes this run's signatures.
 *
 * Options:
 *   --from <releases>      the installed releases to run, `bridge` and `flip` (comma apart; both)
 *   --from-app <path>      the installed app, a built ConsensFlow.app (one release: --from-release)
 *   --from-release <name>  which release --from-app is (flip)
 *   --to-app <path>        the update, a built ConsensFlow.app
 *   --bridge <dir>         a checkout of the bridge to build from (else its tag is exported)
 *   --flip <dir>           a checkout of the flip release to build from (else its tag is exported)
 *   --flip-ref <ref>       the tag or commit of the flip release to export (else the newest after the bridge)
 *   --cache <dir>          where the builds are kept (app/src-tauri/target/updater-smoke)
 *   --reuse                take the apps a run kept in the cache, and build only what is not there
 *   --build-only           build, say where, and run nothing
 *   --export-bridge        export the bridge's source from its tag into the cache, say where, and stop
 *   --only <words>         run the cases whose names hold a word (comma apart)
 *   --machines <dir>       make each case's machine in this folder, not the system's
 *   --timeout <ms>         how long each wait of a case is given (180000)
 *   --keep                 keep the run's folder, and each case's machine
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseArgs } from 'node:util'
import {
  BRIDGE_TAG,
  buildApp,
  exportRelease,
  flipRelease,
  keepBuilt,
  RELEASES,
  REPO,
} from './updater-smoke/build.mjs'
import { plistValue } from './updater-smoke/bundle.mjs'
import { generateKey } from './updater-smoke/signing.mjs'
import { updateVersion } from './updater-smoke/versions.mjs'

const { values } = parseArgs({
  options: {
    from: { type: 'string' },
    'from-app': { type: 'string' },
    'from-release': { type: 'string' },
    'to-app': { type: 'string' },
    bridge: { type: 'string' },
    flip: { type: 'string' },
    'flip-ref': { type: 'string' },
    cache: { type: 'string' },
    'build-only': { type: 'boolean', default: false },
    reuse: { type: 'boolean', default: false },
    'export-bridge': { type: 'boolean', default: false },
    only: { type: 'string' },
    machines: { type: 'string' },
    timeout: { type: 'string' },
    keep: { type: 'boolean', default: false },
  },
})

const say = (text) => process.stdout.write(`smoke:updater: ${text}\n`)
const stop = (text) => {
  process.stderr.write(`smoke:updater: ${text}\n`)
  process.exit(1)
}

if (process.platform !== 'darwin') stop('the update path is macOS bundles and codesign')

/** The installed releases this run is about, in the order they run. */
function releasesAsked() {
  if (values['from-app'] !== undefined) {
    const release = values['from-release'] ?? 'flip'
    if (values.from !== undefined) stop('--from-app takes one release: name it with --from-release')
    return [release]
  }
  if (values['from-release'] !== undefined) stop('--from-release says which release --from-app is')
  return values.from === undefined ? ['flip', 'bridge'] : values.from.split(',').filter(Boolean)
}
const releases = releasesAsked()
for (const release of releases) {
  if (!Object.hasOwn(RELEASES, release)) {
    stop(`${release} is no release to install from: ${Object.keys(RELEASES).join(' or ')}`)
  }
}

const cache = resolve(values.cache ?? join(REPO, 'app', 'src-tauri', 'target', 'updater-smoke'))
mkdirSync(cache, { recursive: true })
if (values['export-bridge']) {
  say(
    `the bridge's source is in ${exportRelease({ into: join(cache, 'bridge-source'), tag: BRIDGE_TAG })}`,
  )
  process.exit(0)
}
const folder = mkdtempSync(join(realpathSync(cache), 'run-'))

/** The source of `release` to build: the checkout it was named, else its tag exported into the cache. */
function sourceOf(release) {
  const named = values[release]
  if (named !== undefined) return resolve(named)
  const tag = release === 'bridge' ? BRIDGE_TAG : (values['flip-ref'] ?? flipRelease())
  say(`the ${release} release is ${tag}`)
  return exportRelease({ into: join(cache, `${release}-source`), tag })
}

try {
  const key = generateKey(REPO, join(folder, 'keys'))
  say(`this run's updater key is ${key.publicKeyFile}`)

  const kept = (name) => join(cache, name, 'ConsensFlow.app')
  const takeKept = (name) => values.reuse && existsSync(kept(name)) && kept(name)

  /** The installed app of `release`: the one named, one a run kept, or one built from its source. */
  const installedOf = (release) => {
    if (values['from-app'] !== undefined) return resolve(values['from-app'])
    const found = takeKept(release)
    if (found) return found
    const checkout = sourceOf(release)
    say(`building the installed ${release} app from ${checkout}`)
    const built = buildApp({ checkout, work: folder, publicKey: key.publicKey })
    return keepBuilt(built, kept(release))
  }
  const installed = Object.fromEntries(releases.map((release) => [release, installedOf(release)]))
  for (const [release, app] of Object.entries(installed)) say(`installed ${release} app: ${app}`)

  let update = (values['to-app'] && resolve(values['to-app'])) || takeKept('update')
  if (!update) {
    const version = updateVersion(
      JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8')).version,
      Object.values(installed).map((app) => plistValue(app, 'CFBundleShortVersionString')),
    )
    say(`building the update from ${REPO} as ${version}`)
    const built = buildApp({ checkout: REPO, work: folder, publicKey: key.publicKey, version })
    update = keepBuilt(built, kept('update'))
  }
  say(`update: ${update}`)

  if (!values['build-only']) {
    const results = []
    for (const release of releases) {
      say(`== the update from the ${release} release`)
      const env = {
        ...process.env,
        CONSENSFLOW_UPDATER_SMOKE: '1',
        CONSENSFLOW_UPDATER_FROM_APP: installed[release],
        CONSENSFLOW_UPDATER_FROM_RELEASE: release,
        CONSENSFLOW_UPDATER_TO_APP: update,
        CONSENSFLOW_UPDATER_KEY: key.privateKey,
        CONSENSFLOW_UPDATER_ONLY: values.only ?? '',
        CONSENSFLOW_UPDATER_SMOKE_KEEP: values.keep ? '1' : '',
        CONSENSFLOW_UPDATER_SMOKE_DIR: values.machines ? resolve(values.machines) : '',
        ...(values.timeout ? { CONSENSFLOW_UPDATER_SMOKE_TIMEOUT_MS: values.timeout } : {}),
      }
      // The smoke is a test runner of its own, even when a test runs this.
      delete env.NODE_TEST_CONTEXT
      const ran = spawnSync(
        process.execPath,
        ['--test', '--test-concurrency=1', 'tests/updater-smoke.test.mjs'],
        { cwd: REPO, stdio: 'inherit', env },
      )
      results.push([release, ran.status ?? 1])
    }
    say(
      results
        .map(([release, status]) => `${release}: ${status === 0 ? 'passed' : 'FAILED'}`)
        .join('; '),
    )
    process.exitCode = results.every(([, status]) => status === 0) ? 0 : 1
  }
} finally {
  if (!values.keep) rmSync(folder, { recursive: true, force: true })
}
