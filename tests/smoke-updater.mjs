/**
 * The update path, proven end to end on the packaged app: an installed app
 * (the bridge, the release that took the relaxed update check) is given this
 * checkout's app as an update, by its own updater, from a feed this run serves.
 * Every case of `tests/updater-smoke.test.mjs` runs: the update itself, the
 * two updates that are refused, and an app replaced by hand.
 *
 *   npm run smoke:updater                     # builds both apps, runs every case
 *   npm run smoke:updater -- --from-app A --to-app B   # takes built apps
 *   npm run smoke:updater -- --only refused   # the cases whose names hold a word
 *
 * What it builds (macOS, offline): the bridge exported from its release tag, and
 * this checkout as the update, which the build gives the version after the
 * bridge's. Both are built with this run's updater public key and signed ad hoc;
 * the key pair is made for the run (`tauri signer generate`) and goes with the
 * run's folder, and the product's key and every Apple certificate stay where
 * they are, unread (`tests/updater-smoke/signing.mjs`). The apps taken as built
 * paths verify against the run's key all the same: the packaged self-test hands
 * the app the public key it is to verify with, so a build kept from another run
 * takes this run's signatures.
 *
 * Options:
 *   --from-app <path>   the installed app, a built ConsensFlow.app
 *   --to-app <path>     the update, a built ConsensFlow.app
 *   --bridge <dir>      a checkout of the bridge to build from (else its tag is exported)
 *   --cache <dir>       where the builds are kept (app/src-tauri/target/updater-smoke)
 *   --reuse             take the apps a run kept in the cache, and build only what is not there
 *   --build-only        build, say where, and run nothing
 *   --export-bridge     export the bridge's source from its tag into the cache, say where, and stop
 *   --only <words>      run the cases whose names hold a word (comma apart)
 *   --machines <dir>    make each case's machine in this folder, not the system's
 *   --timeout <ms>      how long each wait of a case is given (180000)
 *   --keep              keep the run's folder, and each case's machine
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseArgs } from 'node:util'
import { buildApp, exportBridge, keepBuilt, REPO } from './updater-smoke/build.mjs'
import { plistValue } from './updater-smoke/bundle.mjs'
import { generateKey } from './updater-smoke/signing.mjs'
import { updateVersion } from './updater-smoke/versions.mjs'

const { values } = parseArgs({
  options: {
    'from-app': { type: 'string' },
    'to-app': { type: 'string' },
    bridge: { type: 'string' },
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

if (process.platform !== 'darwin') {
  process.stderr.write('smoke:updater: the update path is macOS bundles and codesign\n')
  process.exit(1)
}

const cache = resolve(values.cache ?? join(REPO, 'app', 'src-tauri', 'target', 'updater-smoke'))
mkdirSync(cache, { recursive: true })
const say = (text) => process.stdout.write(`smoke:updater: ${text}\n`)
if (values['export-bridge']) {
  say(`the bridge's source is in ${exportBridge({ into: join(cache, 'bridge-source') })}`)
  process.exit(0)
}
const folder = mkdtempSync(join(realpathSync(cache), 'run-'))

try {
  const key = generateKey(REPO, join(folder, 'keys'))
  say(`this run's updater key is ${key.publicKeyFile}`)

  const kept = (name) => join(cache, name, 'ConsensFlow.app')
  const takeKept = (name) => values.reuse && existsSync(kept(name)) && kept(name)
  let installed = (values['from-app'] && resolve(values['from-app'])) || takeKept('installed')
  if (!installed) {
    const checkout =
      values.bridge === undefined
        ? exportBridge({ into: join(cache, 'bridge-source') })
        : resolve(values.bridge)
    say(`building the installed app from ${checkout}`)
    const built = buildApp({ checkout, work: folder, publicKey: key.publicKey })
    installed = keepBuilt(built, kept('installed'))
  }
  say(`installed app: ${installed}`)

  let update = (values['to-app'] && resolve(values['to-app'])) || takeKept('update')
  if (!update) {
    const version = updateVersion(
      JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8')).version,
      plistValue(installed, 'CFBundleShortVersionString'),
    )
    say(`building the update from ${REPO} as ${version}`)
    const built = buildApp({ checkout: REPO, work: folder, publicKey: key.publicKey, version })
    update = keepBuilt(built, kept('update'))
  }
  say(`update: ${update}`)

  if (!values['build-only']) {
    const env = {
      ...process.env,
      CONSENSFLOW_UPDATER_SMOKE: '1',
      CONSENSFLOW_UPDATER_FROM_APP: installed,
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
    process.exitCode = ran.status ?? 1
  }
} finally {
  if (!values.keep) rmSync(folder, { recursive: true, force: true })
}
