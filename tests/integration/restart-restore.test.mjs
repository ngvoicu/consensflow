import assert from 'node:assert/strict'
import test from 'node:test'
import { startIntegration } from './harness.mjs'

test('a session open when the app quits comes back on its own lead session', async () => {
  let app = await startIntegration()
  const root = app.root
  try {
    const opened = await app.openTab()
    const session = opened.tab.lead.nativeSession
    await app.waitFor(() => app.processes().some((process) => process.sessionId === session))

    // The app's own quit order: the daemon dies first, then the pane host
    // reaps the panes, so no one is left to record the sessions as ended.
    app.killEditor()
    await app.waitFor(() => app.uiExited())
    await app.close({ preserveRoot: true })

    app = await startIntegration({ existingRoot: root })
    // The lead is relaunched on the session it had before the quit.
    await app.waitFor(() => app.openFrames.some((frame) => frame.argv?.includes(session)), 20_000)
    const state = await app.requestNode('state.list', {})
    assert.ok(state.tabs.some((tab) => tab.id === opened.tab.id && tab.closed !== true))
  } finally {
    await app.close()
  }
})
