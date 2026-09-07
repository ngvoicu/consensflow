import assert from 'node:assert/strict'
import test from 'node:test'
import { startIntegration } from './harness.mjs'

test('TEST-PANE-73: two real lead sessions stay alive and accept input concurrently', async () => {
  const app = await startIntegration()
  try {
    const [first, second] = await Promise.all([app.openTab(), app.openTab()])
    const firstLead = first.tab.panes.find((pane) => pane.kind === 'lead')
    const secondLead = second.tab.panes.find((pane) => pane.kind === 'lead')
    assert.ok(firstLead)
    assert.ok(secondLead)
    assert.notEqual(first.tab.id, second.tab.id)

    await app.waitFor(() => app.processes().length >= 2)
    const processes = app.processes()
    const firstProcess = processes.find(
      (process) => process.sessionId === first.tab.lead.nativeSession,
    )
    const secondProcess = processes.find(
      (process) => process.sessionId === second.tab.lead.nativeSession,
    )
    assert.ok(firstProcess)
    assert.ok(secondProcess)
    assert.notEqual(firstProcess.pid, secondProcess.pid)
    assert.equal(app.pidAlive(firstProcess.pid), true)
    assert.equal(app.pidAlive(secondProcess.pid), true)

    const [firstInput, secondInput] = await Promise.all([
      app.requestRust('pane.input', {
        id: firstLead.id,
        generation: firstLead.generation,
        bytes: [...Buffer.from('first live input\r')],
      }),
      app.requestRust('pane.input', {
        id: secondLead.id,
        generation: secondLead.generation,
        bytes: [...Buffer.from('second live input\r')],
      }),
    ])
    assert.equal(firstInput.ok, true, JSON.stringify(firstInput))
    assert.equal(secondInput.ok, true, JSON.stringify(secondInput))
    assert.equal(
      (await app.requestNode('state.list', {})).tabs.every((tab) => tab.closed !== true),
      true,
    )
    assert.equal(
      app.nodeFrames.some((frame) => frame.op === 'tab.suspend'),
      false,
    )
    assert.equal(
      app.nodeFrames.some((frame) => frame.op === 'pane.close'),
      false,
    )

    assert.equal(app.pidAlive(firstProcess.pid), true)
    assert.equal(app.pidAlive(secondProcess.pid), true)
  } finally {
    await app.close()
  }
})
