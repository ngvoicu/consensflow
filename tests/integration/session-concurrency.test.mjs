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

test('TEST-PANE-81: deleting an active session stops its own PTYs and preserves a concurrent session', async () => {
  const app = await startIntegration()
  try {
    const first = await app.openTab()
    const second = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'CF_HOLD deletion test worker', '--new', '--json'],
      first.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const worker = JSON.parse(run.stdout)
    await app.waitFor(() => app.processes().length === 3)
    const survivor = app.processes().find((p) => p.sessionId === second.tab.lead.nativeSession)
    const victims = app.processes().filter((p) => p.pid !== survivor.pid)
    const native = app.transcript(first.tab.lead.nativeSession)
    const stale = await app.requestNode('tab.delete', { tab: first.tab.id, generation: 99 })
    assert.equal(stale.ok, false)
    assert.equal(
      victims.every((p) => app.pidAlive(p.pid)),
      true,
    )
    const result = await app.requestNode('tab.delete', { tab: first.tab.id, generation: 1 })
    assert.equal(result.outcome, 'deleted', JSON.stringify(result))
    await app.waitFor(() => victims.every((p) => !app.pidAlive(p.pid)))
    const state = await app.requestNode('state.list', {})
    assert.deepEqual(
      state.tabs.map((t) => t.id),
      [second.tab.id],
    )
    assert.equal(app.pidAlive(survivor.pid), true)
    assert.equal(app.threads()[worker.conversation].reserved, undefined)
    assert.ok(app.transcript(first.tab.lead.nativeSession).startsWith(native))
    const denied = await app.runCli(['run', '@worker', 'stale caller', '--new'], first.leadEnv)
    assert.notEqual(denied.code, 0)
    const input = await app.requestRust('pane.input', {
      id: second.tab.panes[0].id,
      generation: 1,
      bytes: [...Buffer.from('still alive\r')],
    })
    assert.equal(input.ok, true)
    assert.equal(
      (
        await app.http('/api/panes/tab.delete', {
          method: 'POST',
          body: { tab: second.tab.id, generation: 1 },
          token: second.leadEnv.CONSENSFLOW_APP_TOKEN,
        })
      ).status,
      403,
    )
  } finally {
    await app.close()
  }
})

test('TEST-PANE-81: deletion fences a worker launch already admitted, and closed sessions can be removed', async () => {
  const app = await startIntegration()
  try {
    const first = await app.openTab()
    const run = app.runCli(
      ['run', '@worker', 'CF_HOLD racing deletion', '--new', '--json'],
      first.leadEnv,
    )
    await app.waitFor(() => app.openFrames.some((frame) => frame.argv.includes('--in-pane')))
    const removed = await app.requestNode('tab.delete', { tab: first.tab.id, generation: 1 })
    await run
    assert.equal(removed.outcome, 'deleted', JSON.stringify(removed))
    await app.waitFor(() => app.processes().every((p) => !app.pidAlive(p.pid)))
    assert.deepEqual((await app.requestRust('pane.list', {})).panes, [])
    assert.deepEqual((await app.requestNode('state.list', {})).tabs, [])
    const second = await app.openTab()
    assert.notEqual(second.tab.id, first.tab.id)
    const lead = second.tab.panes[0]
    assert.equal((await app.requestRust('pane.kill', { id: lead.id, generation: 1 })).ok, true)
    await app.waitFor(
      async () => (await app.requestNode('state.list', {})).tabs[0]?.closed === true,
    )
    assert.equal(
      (await app.requestNode('tab.delete', { tab: second.tab.id, generation: 1 })).outcome,
      'deleted',
    )
    assert.deepEqual((await app.requestNode('state.list', {})).tabs, [])
  } finally {
    await app.close()
  }
})
