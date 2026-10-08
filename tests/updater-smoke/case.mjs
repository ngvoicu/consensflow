import assert from 'node:assert/strict'
import { existsSync, mkdtempSync, readdirSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative, resolve } from 'node:path'
import { launchApp } from './app.mjs'
import { appEnv, recordedPids, sandbox } from './box.mjs'
import { RELEASES, REPO } from './build.mjs'
import { assertAdHoc, copyBundle, digestManifest, inspectBundle, verifySeal } from './bundle.mjs'
import { appLog, assertApp, daemonLog, daemonOf, gone, ledgerHeld } from './evidence.mjs'
import { makeTls, serveUpdates } from './feed.mjs'
import { alive, processTable, TIMEOUT_MS, until } from './processes.mjs'
import { generateKey } from './signing.mjs'
import { compareVersions } from './versions.mjs'

/**
 * What every case of the updater smoke is made of: the two built apps, held to
 * what the installed app's check takes of a bundle; one machine per case, with
 * the installed app copied into it and a feed served for it; the app started on
 * it; and the steps the update's cases share, each proved by what the machine
 * shows (processes.mjs, evidence.mjs).
 */

/** The path an environment variable names: a built app, which is never the installed one in /Applications. */
function builtPath(name) {
  const value = process.env[name]
  assert.ok(
    typeof value === 'string' && value.length > 0,
    `${name} is required: \`npm run smoke:updater\` builds the two apps and gives their paths`,
  )
  const path = realpathSync(resolve(value))
  assert.ok(path.endsWith('.app'), `${name} must point to a .app bundle: ${path}`)
  const outside = relative('/Applications', path)
  assert.ok(
    outside === '..' || outside.startsWith('../') || outside.startsWith('/'),
    `${name} may not point into /Applications: ${path}`,
  )
  return path
}

/**
 * The release the installed app is, as the driver names it: one of `RELEASES`,
 * which says what its app starts and what its `cf setup` wrote.
 */
function releaseOf(name) {
  const release = process.env[name]
  assert.ok(
    Object.hasOwn(RELEASES, release ?? ''),
    `${name} is ${JSON.stringify(release)}: it names the release the installed app is, one of ${Object.keys(RELEASES).join(', ')}`,
  )
  return release
}

/**
 * The inputs of the run: both apps, as built, checked once (identity, versions,
 * the code signature, ad hoc), the release the installed one is, and the updater
 * key the run signs with (the driver's, or one made here).
 */
export function loadInputs() {
  const fromApp = builtPath('CONSENSFLOW_UPDATER_FROM_APP')
  const release = releaseOf('CONSENSFLOW_UPDATER_FROM_RELEASE')
  const toApp = builtPath('CONSENSFLOW_UPDATER_TO_APP')
  assert.notEqual(fromApp, toApp, 'FROM_APP and TO_APP must be distinct source bundles')
  const from = inspectBundle(fromApp, 'FROM_APP')
  const to = inspectBundle(toApp, 'TO_APP')
  assert.ok(
    compareVersions(to.version, from.version) > 0,
    `the update (${to.version}) must be newer than the installed app (${from.version})`,
  )
  for (const info of [from, to]) {
    // The check reads the plist's version; a CLI manifest, where the bundle has one, says the same.
    if (info.cliVersion !== null) {
      assert.equal(info.cliVersion, info.version, `${info.label}: its CLI's version is not its own`)
    }
    verifySeal(info.app)
    assertAdHoc(info.app, info.label)
  }
  let key = process.env.CONSENSFLOW_UPDATER_KEY
  let keyFolder = null
  if (key === undefined || key === '') {
    keyFolder = mkdtempSync(join(realpathSync(tmpdir()), 'cf-updater-key-'))
    key = generateKey(REPO, keyFolder).privateKey
  }
  return {
    from,
    release,
    to,
    fromManifest: digestManifest(fromApp),
    toManifest: digestManifest(toApp),
    key: { privateKey: key, publicKeyFile: `${key}.pub` },
    keyFolder,
    keep: process.env.CONSENSFLOW_UPDATER_SMOKE_KEEP === '1',
  }
}

/**
 * What an install leaves behind that it should not: in the staging folder of the
 * home it extracts into, empty or gone once an install is over, and beside the app
 * it replaces, where the folder holds the app and nothing else.
 */
export function stagingLeft(box) {
  const folder = join(box.state, 'app', 'updates')
  return [
    ...(existsSync(folder) ? readdirSync(folder).map((name) => `updates/${name}`) : []),
    ...readdirSync(box.apps).filter((name) => name !== 'ConsensFlow.app'),
  ]
}

/**
 * One case's machine: the installed app copied in, its feed served, and the
 * app started on it when asked. A case that passed (`finished`) takes its machine
 * away; one that did not leaves it where it is, for whoever has to read it.
 */
export async function startCase(inputs) {
  const box = sandbox(process.env.CONSENSFLOW_UPDATER_SMOKE_DIR || tmpdir())
  const tls = makeTls(box.tls)
  const feed = await serveUpdates(tls)
  copyBundle(inputs.from.app, box.copy)
  assert.deepEqual(
    digestManifest(box.copy),
    inputs.fromManifest,
    'the installed copy differs from FROM_APP',
  )
  verifySeal(box.copy)
  const installed = inspectBundle(box.copy, 'the installed copy')
  const kase = {
    box,
    feed,
    installed,
    app: null,
    /** The pids of the second ConsensFlows the case started to see the ledger refuse them. */
    probes: new Set(),
    finished: false,
    /** Starts the app of the copy now in place, told to find `expected` once it has been updated. */
    start(expected, options = {}) {
      const bundle = inspectBundle(box.copy, 'the app in place')
      kase.app = launchApp(
        bundle.binary,
        appEnv(box, {
          feed: feed.url,
          certificate: tls.caCert,
          publicKeyFile: inputs.key.publicKeyFile,
          expected,
          deadlineMs: TIMEOUT_MS + 10_000,
        }),
        box.root,
        options,
      )
      return kase.app
    },
    async cleanup() {
      kase.app?.killRecorded()
      for (const pid of new Set(recordedPids(box))) {
        try {
          process.kill(pid, 'SIGKILL')
        } catch {
          // The stand-in already exited.
        }
      }
      await feed.close()
      if (!kase.finished || inputs.keep) {
        process.stdout.write(`updater smoke: the case's machine is kept at ${box.root}\n`)
        return
      }
      rmSync(box.root, { recursive: true, force: true })
    },
    /** What a failure should say of the machine it happened on. */
    report: () =>
      `\nthe machine is kept at ${box.root}\napp.log:\n${appLog(box).slice(-3000)}\ndaemon.log:\n${daemonLog(box).slice(-3000)}`,
  }
  return kase
}

/**
 * The app up and its daemon started, from the machine's own words: the page said
 * it booted at the version asked, the app's process is the copy's executable,
 * and the daemon the app chose (Node's or the native one) logged its start, is
 * the app's child, runs this bundle's `cf ui`, and is the only one that runs.
 */
export async function bootEvidence(kase, app, { version, pid = null }) {
  const boot = await app.waitFor(
    `an update boot at ${version}`,
    (event) => event.event === 'update-boot' && event.data.currentVersion === version,
  )
  assert.ok(pid === null || boot.pid !== pid, 'the app that booted is the one that was replaced')
  assert.ok(alive(boot.pid), `the app (pid ${boot.pid}) was not alive at update boot`)
  const bundle = inspectBundle(kase.box.copy, 'the app in place')
  assertApp(processTable(), boot.pid, bundle.binary)
  const daemon = await daemonOf(kase.box, {
    app: boot.pid,
    bundle: kase.box.copy,
    probes: kase.probes,
  })
  return { app: boot.pid, daemon }
}

/**
 * The daemon holds the ledger: a second ConsensFlow on the home is refused. Asked
 * once the page has had an answer of the daemon (it has the ledger by then, and a
 * probe before that could be the one to take it).
 */
export async function heldEvidence(kase) {
  await ledgerHeld(inspectBundle(kase.box.copy, 'the app in place'), kase.box, kase.probes)
}

/**
 * The page has two windows open and the install refused for them: the stand-ins
 * are two live processes, and nothing the page started has changed (the app, its
 * daemon). Returns the two stand-ins' pids.
 */
export async function blockedEvidence(kase, app, booted) {
  const blocked = await app.waitFor(
    'blocked update install report',
    (event) => event.event === 'update-blocked',
  )
  assert.equal(blocked.data.phase, 'ready')
  assert.equal(blocked.data.blockers.length, 2, 'the ready snapshot did not expose both open panes')
  const chiefs = await until('two stand-in chief processes', () => {
    const pids = [...new Set(recordedPids(kase.box))]
    return pids.length === 2 ? pids : null
  })
  assert.ok(chiefs.every(alive), `the chiefs were not alive before the blocked install: ${chiefs}`)
  assert.ok(alive(booted.app), 'a blocked install changed the app process')
  assert.ok(alive(booted.daemon.pid), 'a blocked install changed the daemon')
  return chiefs
}

/** Tells the page to go on, and waits for the two windows to close, which is what the install waited for. */
export async function releasePanes(app, chiefs) {
  app.continueUpdate()
  await until('two stand-in chiefs close', () => chiefs.every(gone))
}
