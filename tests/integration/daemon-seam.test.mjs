import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { startIntegration } from './harness.mjs'

/**
 * The rig starts the daemon `CONSENSFLOW_TEST_DAEMON` names, Node's when it
 * names none, and connects it to the real headless bridge (`npm run
 * test:daemons` runs this against both). What else the daemon is asked is the
 * other suites' to show.
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
})
