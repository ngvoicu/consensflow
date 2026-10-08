import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { startLine } from '../choice.mjs'
import { startIntegration } from './harness.mjs'

const LIAR = fileURLToPath(new URL('./liar-daemon.mjs', import.meta.url))

/**
 * The rig starts the native daemon (or the build `CONSENSFLOW_TEST_DAEMON`
 * names, a command as a JSON array) and connects it to the real headless bridge
 * (`npm run test:daemons`). What else the daemon is asked is the other suites'
 * to show.
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

  it("starts the native daemon, and says so in its log's start line", async () => {
    const rig = await startIntegration()
    try {
      const log = readFileSync(join(rig.env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8')
      const start = startLine(log, rig.daemonPid())
      assert.notEqual(start, null, `no start line of pid ${rig.daemonPid()} in ${log}`)
      assert.equal(start.kind, 'native', start.line)
      assert.equal(rig.daemon.kind, start.kind)
      assert.equal(rig.daemon.line, start.line)
    } finally {
      await rig.close()
    }
  })

  it('refuses a daemon whose start line says it is not the native one, and ends it', async () => {
    // Asked for a command that is not the native daemon: the stand-in writes
    // the start line of a Node daemon, as the releases before the deletion ran.
    const rig = startIntegration({ select: JSON.stringify([process.execPath, LIAR]) })
    try {
      await assert.rejects(
        rig,
        /the native daemon was asked for, but the start line in its log says node v0\.0\.0/,
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
