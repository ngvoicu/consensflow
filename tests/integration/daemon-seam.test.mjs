import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { startLine } from '../choice.mjs'
import { daemonCommand } from '../helpers.mjs'
import { startIntegration } from './harness.mjs'

const LIAR = fileURLToPath(new URL('./liar-daemon.mjs', import.meta.url))

/**
 * The rig starts the daemon `CONSENSFLOW_TEST_DAEMON` names (`node`, `native`,
 * or Node's when it names none) and connects it to the real headless bridge
 * (`npm run test:daemons` runs this against both, each leg naming its own and
 * saying which leg it is with `CONSENSFLOW_TEST_LEG`). What else the daemon is
 * asked is the other suites' to show.
 */
describe('the rig starts the daemon it is told to', () => {
  it('answers the page over the headless bridge', async () => {
    const rig = await startIntegration()
    try {
      assert.deepEqual(await rig.requestNode('ping', {}), { ok: true })
    } finally {
      await rig.close()
    }
  })

  // The leg's label is its own word for which daemon this run is, not the
  // selection's: a selection that came to the other daemon agrees with itself.
  it("starts the daemon its leg names, and says so in its log's start line", async () => {
    const leg = process.env.CONSENSFLOW_TEST_LEG ?? ''
    const rig = await startIntegration()
    try {
      const log = readFileSync(join(rig.env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8')
      const start = startLine(log, rig.daemonPid())
      assert.notEqual(start, null, `no start line of pid ${rig.daemonPid()} in ${log}`)
      // A run no runner labelled is held to the daemon it selected.
      assert.equal(start.kind, leg === '' ? daemonCommand([]).kind : leg, start.line)
      assert.equal(rig.daemon.kind, start.kind)
      assert.equal(rig.daemon.line, start.line)
    } finally {
      await rig.close()
    }
  })

  it('refuses a daemon whose start line says it is the other one, and ends it', async () => {
    // Asked for Node's, in its own words whatever this run's leg is: the stand-in
    // writes the native daemon's line.
    const rig = startIntegration({ daemon: LIAR, select: 'node' })
    try {
      await assert.rejects(
        rig,
        /Node's daemon was asked for, but the start line in its log says rust 0\.0\.0/,
      )
    } finally {
      // A rig that took it holds the stand-in and a pane host: ended here, so that
      // the failure is the test's and not a run that never ends.
      await rig.then(
        (app) => app.close(),
        () => {},
      )
    }
  })
})
