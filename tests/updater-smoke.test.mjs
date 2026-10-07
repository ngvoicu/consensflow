import assert from 'node:assert/strict'
import { existsSync, readFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import test, { after } from 'node:test'
import { recordedPids } from './updater-smoke/box.mjs'
import { REPO } from './updater-smoke/build.mjs'
import {
  copyOver,
  digestManifest,
  REFUSED,
  refusedBundle,
  verifySeal,
} from './updater-smoke/bundle.mjs'
import {
  blockedEvidence,
  bootEvidence,
  heldEvidence,
  loadInputs,
  releasePanes,
  stagingLeft,
  startCase,
} from './updater-smoke/case.mjs'
import {
  appLog,
  assertChoseNative,
  assertOnlyProbesRefused,
  daemonLog,
  daemonOf,
  gone,
} from './updater-smoke/evidence.mjs'
import { signedUpdate } from './updater-smoke/feed.mjs'
import { assertRepaired, plantCommands } from './updater-smoke/launchers.mjs'
import {
  assertKept,
  assertProjects,
  assertSound,
  assertTraced,
  readLedger,
  tracedEvents,
} from './updater-smoke/ledger.mjs'
import { alive, TIMEOUT_MS, until } from './updater-smoke/processes.mjs'
import { generateKey } from './updater-smoke/signing.mjs'

/**
 * The packaged update path, end to end: an installed app (the bridge) is given
 * this checkout's app as an update by its own updater, from a feed this run serves,
 * on a machine of the case's own. `npm run smoke:updater` builds the two apps and
 * runs every case here; without that opt-in each is one skipped test, so the
 * ordinary suite stays cheap, and once it is asked for a missing or unsafe input
 * fails with the command that provides it.
 *
 * The cases:
 *
 * - the update: install and restart, the native daemon by default on the same
 *   home, the ledger whole, the terminal command of the home repaired and no
 *   other's;
 * - two updates the app refuses (a signature that is not the key's, and bundles its
 *   check refuses): the installed bundle is intact and the app still runs;
 * - an app replaced by hand, as a disk image's copy does, with the app quit:
 *   its first start repairs the command and keeps the ledger.
 */

const REQUESTED = process.env.CONSENSFLOW_UPDATER_SMOKE === '1'
const ONLY = (process.env.CONSENSFLOW_UPDATER_ONLY ?? '').split(',').filter(Boolean)
/** The schema the bridge's ledger is at: a ledger the update touches is at it or past it. */
const BRIDGE_SCHEMA = 10
/** The daemon the installed app starts: the bridge's is Node's. */
const INSTALLED_DAEMON = process.env.CONSENSFLOW_UPDATER_FROM_DAEMON ?? 'node'

let inputs = null
after(() => {
  if (inputs?.keyFolder) rmSync(inputs.keyFolder, { recursive: true, force: true })
})

/** A case: skipped unless asked for, and on a machine of its own that is kept if it fails. */
function updaterCase(name, body) {
  test(name, { timeout: TIMEOUT_MS * 2 }, async (t) => {
    if (!REQUESTED) {
      t.skip(
        'the updater smoke runs under `npm run smoke:updater` (sets CONSENSFLOW_UPDATER_SMOKE=1)',
      )
      return
    }
    if (ONLY.length > 0 && !ONLY.some((word) => name.includes(word))) {
      t.skip(`not among the cases asked for (${ONLY})`)
      return
    }
    assert.equal(process.platform, 'darwin', 'the updater smoke needs macOS bundles and codesign')
    inputs ??= loadInputs()
    const kase = await startCase(inputs)
    t.after(() => kase.cleanup())
    try {
      await body(kase, t)
      kase.finished = true
    } catch (cause) {
      cause.message += kase.report()
      throw cause
    }
  })
}

/**
 * The app's quit, and everything it had gone: the app, its daemons, the windows'
 * stand-ins. The ledger had one holder to the end: no daemon of the app was refused it.
 */
async function quit(kase, daemons) {
  const { app, box } = kase
  app.closeInput()
  await until('every app process exits', () => !app.anyAlive())
  const ended = await app.exited
  for (const pid of daemons) assert.ok(gone(pid), `the daemon (pid ${pid}) outlived the app`)
  assert.deepEqual(recordedPids(box).filter(alive), [], 'a stand-in chief survived app shutdown')
  assertOnlyProbesRefused(daemonLog(box), kase.probes)
  return ended
}

/** The ledger of the case's home, read once no daemon holds it. */
const ledgerOf = (kase) => readLedger(join(kase.box.state, 'consensflow.db'))

/** The two folders the page opens its projects in. */
const projectsOf = (kase) => [
  kase.box.workspace,
  join(kase.box.workspace, '.consensflow-updater-second'),
]

/** Offers the run's update, signed by the run's key. */
function offerUpdate(kase, app = inputs.to.app) {
  const update = signedUpdate({
    tauri: REPO,
    privateKey: inputs.key.privateKey,
    app,
    directory: kase.box.probe,
  })
  kase.feed.offer(update.version, update.signature, update.bytes)
  return update
}

updaterCase(
  'the update installs and restarts on the native daemon, keeps the ledger and repairs its own terminal command',
  async (kase, t) => {
    const { box } = kase
    // Both names of this home's, one of which serves another home, and the other home's own.
    const planted = plantCommands(kase.installed, box, { elsewhere: true })
    offerUpdate(kase)
    const app = kase.start(inputs.to.version)

    // The installed app: its process, its daemon (the bridge's is Node's), ready, and holding the ledger.
    const first = await bootEvidence(kase, app, { version: inputs.from.version })
    assert.equal(
      first.daemon.kind,
      INSTALLED_DAEMON,
      `the installed app started ${first.daemon.runtime}`,
    )
    const chiefs = await blockedEvidence(kase, app, first)
    // The page has had its answers (two projects, two windows): the daemon has the ledger, and refuses a second.
    await heldEvidence(kase)
    t.diagnostic(
      `installed ${inputs.from.version}: app pid ${first.app}, daemon pid ${first.daemon.pid} (${first.daemon.runtime}), ledger held`,
    )
    const traced = tracedEvents(readFileSync(join(box.state, 'events.jsonl'), 'utf8'))
    assert.ok(traced.length > 0, 'the daemon traced no ledger event before the update')

    // Both windows close, the install goes through, and the app starts again as the update.
    await releasePanes(app, chiefs)
    const second = await bootEvidence(kase, app, { version: inputs.to.version, pid: first.app })
    const restarted = await app.waitFor(
      'the update restart',
      (event) => event.event === 'update-restarted',
    )
    assert.equal(
      restarted.data.currentVersion,
      inputs.to.version,
      'the restart did not report the update',
    )
    assert.equal(restarted.data.blockers, 0, 'the restart reported open panes')
    assert.equal(restarted.pid, second.app)
    assert.notEqual(restarted.pid, first.app, 'the update restarted in the original process')
    assert.ok(alive(restarted.pid), 'the restarted app was not alive when it reported ready')
    await until('the first app exits', () => gone(first.app))
    await until('the first daemon exits and lets go of the ledger', () => gone(first.daemon.pid))
    // The page read the board of the update's daemon: it is ready, and has the ledger.
    await heldEvidence(kase)

    // The update's app: its daemon is the native one, by default, chosen as the app's log says.
    assert.equal(second.daemon.kind, 'native', `the update started ${second.daemon.runtime}`)
    assert.notEqual(second.daemon.pid, first.daemon.pid)
    assertChoseNative(appLog(box), box.state)
    assert.ok(!existsSync(join(box.state, 'use-node')), 'the way back to Node was taken')
    t.diagnostic(
      `updated ${inputs.to.version}: app pid ${second.app}, daemon pid ${second.daemon.pid} (${second.daemon.runtime}), ledger held, the app log names the choice`,
    )

    // The terminal command of this home runs the update's cf; no other command changed.
    const cf = join(box.copy, 'Contents', 'Resources', 'cli', 'bin', 'cf')
    assertRepaired({
      box,
      planted,
      cf,
      appLogText: appLog(box),
      version: second.daemon.runtime.split(' ')[1],
    })
    t.diagnostic(
      `the terminal command of ${box.state} now runs ${cf}; the commands of ${box.other} and the one pinned to it are as they were`,
    )

    // The bundle in place is the update, byte for byte, sealed, with nothing left of the install.
    verifySeal(box.copy)
    assert.deepEqual(
      digestManifest(box.copy),
      inputs.toManifest,
      'installed copy bytes do not equal TO_APP',
    )
    assert.deepEqual(
      digestManifest(inputs.to.app),
      inputs.toManifest,
      'TO_APP was mutated by the smoke',
    )
    assert.deepEqual(stagingLeft(box), [], 'the install left staging files in the home')

    // The quit takes everything with it, and the ledger is whole.
    const ended = await quit(kase, [first.daemon.pid, second.daemon.pid])
    t.diagnostic(`the original app exited ${ended.code} / ${ended.signal}`)
    assert.deepEqual(
      app.events.filter((event) => event.event === 'update-failure'),
      [],
      'the successful updater smoke reported a failure',
    )
    const ledger = ledgerOf(kase)
    assertSound(ledger, { atLeast: BRIDGE_SCHEMA })
    assertProjects(ledger, projectsOf(kase))
    assertTraced(traced, ledger)
    t.diagnostic(
      `the ledger: schema ${ledger.version}, sound, the ${traced.length} events traced before the update are in it`,
    )
  },
)

/** The steps of an update the installed app refuses: it runs to the install, or to the download. */
async function refusedFlow(kase, t, { blocked }) {
  const app = kase.start(inputs.to.version, { expected: ['update-failure'] })
  const first = await bootEvidence(kase, app, { version: inputs.from.version })
  assert.equal(first.daemon.kind, INSTALLED_DAEMON)
  if (blocked) await releasePanes(app, await blockedEvidence(kase, app, first))
  const failure = await app.waitFor('the refusal', (event) => event.event === 'update-failure', {
    unless: (event) =>
      event.event === 'update-restarted' ||
      (event.event === 'update-boot' && event.data.currentVersion === inputs.to.version)
        ? 'the app took an update it should have refused'
        : null,
  })
  return { app, first, failure: failure.data.error, t }
}

/** What a refused update leaves: the installed bundle as it was, the app and its daemon running, the ledger held. */
async function intactEvidence(kase, { app, first }, t) {
  assert.deepEqual(
    digestManifest(kase.box.copy),
    inputs.fromManifest,
    'the installed bundle changed',
  )
  verifySeal(kase.box.copy)
  assert.deepEqual(stagingLeft(kase.box), [], 'the refused install left staging files in the home')
  assert.ok(alive(first.app), 'the app stopped')
  assert.ok(alive(first.daemon.pid), 'the daemon stopped')
  const again = await daemonOf(kase.box, {
    app: first.app,
    bundle: kase.box.copy,
    probes: kase.probes,
  })
  assert.equal(again.pid, first.daemon.pid, 'the app has another daemon')
  await heldEvidence(kase)
  assert.equal(app.events.filter((event) => event.event === 'update-restarted').length, 0)
  t.diagnostic(
    `app pid ${first.app} and daemon pid ${first.daemon.pid} still run, the bundle is byte for byte as installed, the ledger is held`,
  )
}

updaterCase(
  'an update signed by another key is refused and the installed app stays intact and running',
  async (kase, t) => {
    const { box } = kase
    const stranger = generateKey(REPO, join(box.tls, 'stranger'))
    const update = signedUpdate({
      tauri: REPO,
      privateKey: stranger.privateKey,
      app: inputs.to.app,
      directory: box.probe,
    })
    kase.feed.offer(update.version, update.signature, update.bytes)
    const flow = await refusedFlow(kase, t, { blocked: false })
    assert.match(
      flow.failure,
      /signature/i,
      `the refusal does not name the signature: ${flow.failure}`,
    )
    t.diagnostic(`refused: ${flow.failure}`)
    await intactEvidence(kase, flow, t)
    const ended = await quit(kase, [flow.first.daemon.pid])
    assert.equal(ended.code, 0, `the app exited ${ended.code} / ${ended.signal}`)
    assertSound(ledgerOf(kase), { atLeast: BRIDGE_SCHEMA })
    assertProjects(ledgerOf(kase), projectsOf(kase))
  },
)

for (const [kind, refusal] of Object.entries(REFUSED)) {
  updaterCase(
    `a signed update whose bundle fails the check (${kind}) is refused and the installed app stays intact and running`,
    async (kase, t) => {
      const refused = refusedBundle(kind, inputs.to.app, join(kase.box.root, 'refused'))
      offerUpdate(kase, refused)
      const flow = await refusedFlow(kase, t, { blocked: true })
      assert.match(flow.failure, refusal.words, `the refusal is not the check's: ${flow.failure}`)
      t.diagnostic(`refused: ${flow.failure}`)
      await intactEvidence(kase, flow, t)
      const ended = await quit(kase, [flow.first.daemon.pid])
      assert.equal(ended.code, 0, `the app exited ${ended.code} / ${ended.signal}`)
      assertSound(ledgerOf(kase), { atLeast: BRIDGE_SCHEMA })
    },
  )
}

for (const how of ['replaced', 'copied over']) {
  updaterCase(
    `an app ${how} by hand, with the app quit, repairs the terminal command at its first start and keeps the ledger`,
    async (kase, t) => {
      const { box } = kase
      // Both names of this home's serve it, and the other home has its own.
      const planted = plantCommands(kase.installed, box, { elsewhere: false })
      // The installed app has its session, and no update is offered: two projects opened and closed, then it is quit.
      const old = kase.start(inputs.to.version, { expected: ['update-failure'] })
      const first = await bootEvidence(kase, old, { version: inputs.from.version })
      const none = await old.waitFor(
        'no update to take',
        (event) => event.event === 'update-failure',
      )
      assert.match(none.data.error, /update feed is unavailable/, 'the update was not unavailable')
      const left = await quit(kase, [first.daemon.pid])
      assert.equal(left.code, 0, `the app exited ${left.code} / ${left.signal}`)
      const before = ledgerOf(kase)
      assertSound(before, { atLeast: BRIDGE_SCHEMA })
      assertProjects(before, projectsOf(kase))

      // What a disk image's copy does: the update's app where the old one was.
      if (how === 'replaced') rmSync(box.copy, { recursive: true, force: true })
      copyOver(inputs.to.app, box.copy)
      // Every file of the update is there as it is. A bundle with nothing of the old app's left
      // in it is sealed as the update is; one with files the update has not (a Node the update
      // ships none of) is not a sealed bundle, and what it must do is run.
      const merged = digestManifest(box.copy)
      for (const entry of inputs.toManifest) {
        assert.deepEqual(
          merged.find((each) => each.name === entry.name),
          entry,
          `${entry.name} is not the update's`,
        )
      }
      const stale = merged.filter(
        (entry) => !inputs.toManifest.some((each) => each.name === entry.name),
      )
      t.diagnostic(`files of the old app left in the copy: ${stale.length}`)
      if (stale.length === 0) verifySeal(box.copy)

      const app = kase.start(inputs.to.version)
      const second = await bootEvidence(kase, app, { version: inputs.to.version })
      await app.waitFor('the first start', (event) => event.event === 'update-restarted')
      await heldEvidence(kase)
      assert.equal(second.daemon.kind, 'native', `the first start ran ${second.daemon.runtime}`)
      assertChoseNative(appLog(box), box.state)
      assertRepaired({
        box,
        planted,
        cf: join(box.copy, 'Contents', 'Resources', 'cli', 'bin', 'cf'),
        appLogText: appLog(box),
        version: second.daemon.runtime.split(' ')[1],
      })
      t.diagnostic(
        `first start of ${inputs.to.version}: daemon pid ${second.daemon.pid} (${second.daemon.runtime}), the terminal command repaired`,
      )
      await quit(kase, [second.daemon.pid])
      assertKept(before, ledgerOf(kase))
      t.diagnostic(
        `the ledger: schema ${before.version} then ${ledgerOf(kase).version}, every row it had is there`,
      )
    },
  )
}
