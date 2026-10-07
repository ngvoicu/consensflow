import { execFileSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cleanEnv, tauriCli } from './signing.mjs'

/**
 * What the updater smoke builds: the installed app (the bridge, exported from
 * its release tag) and the update (this checkout), each with the run's
 * public key, and the update with the next version. Both are given by the
 * build's own override (`tauri build --config`), a file in the run's folder:
 * the product's configuration (`tauri.conf.json`) is never written, and a
 * build with no override is the product's own.
 */

export const REPO = dirname(dirname(dirname(fileURLToPath(import.meta.url))))

/**
 * The release the installed app is: the first whose installed check is the
 * relaxed one (identity, version, `cli/bin/cf`, the seal), which is what lets
 * it take an update that ships no Node.
 */
export const BRIDGE_TAG = 'v3.0.0-alpha.81'

/** Where a build leaves its bundle: the workspace's one build folder. */
export function builtApp(checkout) {
  return join(
    checkout,
    'app',
    'src-tauri',
    'target',
    'release',
    'bundle',
    'macos',
    'ConsensFlow.app',
  )
}

/** The override a build is given: the run's public key, and the version where the build is the update. */
export function overrideConfig({ publicKey, version }) {
  const config = { plugins: { updater: { pubkey: publicKey } } }
  if (version !== undefined) config.version = version
  return config
}

/**
 * The bundle's `cli/package.json` (what `prepare-sidecar` stages from the
 * checkout's) says the version the override gives, as a release's would: the
 * staged copy is the build's own and the checkout's is not touched. A bundle
 * that has none (the one without Node) has nothing to say.
 */
function stampStagedVersion(app, version) {
  const file = join(app, 'src-tauri', 'resources', 'cli', 'package.json')
  if (!existsSync(file)) return
  const manifest = JSON.parse(readFileSync(file, 'utf8'))
  manifest.version = version
  writeFileSync(file, `${JSON.stringify(manifest, null, 2)}\n`)
}

/**
 * The app's executable holds the run's public key and not the product's: the
 * override reached the build, and an update signed by the run's key is one that
 * the app, as built, could take. (The product's public key is in its
 * configuration, which is the one thing of its key this reads.)
 */
export function assertBuiltWith(app, publicKey, productKey, label) {
  const executable = readFileSync(join(app, 'Contents', 'MacOS', 'app'))
  const holds = (key) => executable.includes(Buffer.from(key))
  if (!holds(publicKey)) throw new Error(`${label} was not built with this run's updater key`)
  if (holds(productKey)) throw new Error(`${label} carries the product's updater key`)
}

/** The updater key the product's configuration carries: public, and the one a build must not keep. */
export function productKeyOf(checkout) {
  const config = JSON.parse(
    readFileSync(join(checkout, 'app', 'src-tauri', 'tauri.conf.json'), 'utf8'),
  )
  return config.plugins.updater.pubkey
}

/**
 * Builds the app of `checkout` as `npm --prefix app run build` does (the page's
 * bundle, the sidecar and the CLI staged, then Tauri), with the override in
 * `work`, and returns the bundle it left. The signing is ad hoc, as the product's
 * configuration has it (`signingIdentity: "-"`), and nothing in the environment
 * can say otherwise.
 */
export function buildApp({ checkout, work, publicKey, version }) {
  const app = join(checkout, 'app')
  const env = cleanEnv()
  const run = (program, args) => execFileSync(program, args, { cwd: app, env, stdio: 'inherit' })
  run('npm', ['run', 'bundle:ui'])
  run('npm', ['run', 'prepare-sidecar'])
  if (version !== undefined) stampStagedVersion(app, version)
  mkdirSync(work, { recursive: true })
  const override = join(work, `tauri-${version ?? 'as-is'}.json`)
  writeFileSync(override, `${JSON.stringify(overrideConfig({ publicKey, version }), null, 2)}\n`)
  run(process.execPath, [tauriCli(checkout), 'build', '--bundles', 'app', '--config', override])
  const built = builtApp(checkout)
  assertBuiltWith(built, publicKey, productKeyOf(checkout), `the app built from ${checkout}`)
  return built
}

/** A copy of a built bundle that the next build does not replace, with its attributes kept. */
export function keepBuilt(built, kept) {
  rmSync(kept, { recursive: true, force: true })
  mkdirSync(dirname(kept), { recursive: true })
  execFileSync('/usr/bin/ditto', [built, kept])
  return kept
}

/**
 * The checkout of the bridge's release, exported from its tag into `into`
 * (once: a tag does not change) and given the caches of `repo` to build from:
 * the node modules, and the Node and console-host downloads, which a build
 * offline cannot fetch.
 */
export function exportBridge({ repo = REPO, into, tag = BRIDGE_TAG }) {
  const mark = join(into, '.exported-from')
  if (existsSync(mark) && readFileSync(mark, 'utf8').trim() === tag) return into
  rmSync(into, { recursive: true, force: true })
  mkdirSync(into, { recursive: true })
  const archive = `${into}.tar`
  try {
    execFileSync('git', ['archive', '--format=tar', '--output', archive, tag], {
      cwd: repo,
      stdio: ['ignore', 'ignore', 'pipe'],
    })
  } catch (cause) {
    const said = cause?.stderr?.toString?.().trim() || cause.message
    throw new Error(
      `the bridge's release ${tag} is not in this repository (git fetch --tags): ${said}`,
    )
  }
  execFileSync('/usr/bin/tar', ['-xf', archive, '-C', into])
  rmSync(archive, { force: true })
  // A second root configuration inside the checkout would stop its linter from running.
  rmSync(join(into, 'biome.json'), { force: true })
  for (const shared of ['node_modules', join('app', 'node_modules'), join('app', '.cache')]) {
    // What is not there to give is left out: a build says so, and the export does not.
    if (existsSync(join(repo, shared))) {
      symlinkSync(realpathSync(join(repo, shared)), join(into, shared))
    }
  }
  writeFileSync(mark, `${tag}\n`)
  return into
}
