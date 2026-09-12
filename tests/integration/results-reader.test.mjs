import assert from 'node:assert/strict'
import test from 'node:test'
import { startIntegration } from './harness.mjs'

test('TEST-PANE-75: real CLI reads complete results through the scoped daemon without a terminal write', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const leadEnv = opened.leadEnv
    const tab = opened.tab.id
    const lead = opened.tab.panes.find((pane) => pane.kind === 'lead')
    await app.requestNode('notify.set', { scope: 'tab', id: tab, mode: 'manual' })
    const consult = await app.runCli(['run', '@worker', 'CF_LARGE', '--new'], leadEnv)
    assert.equal(consult.code, 0, consult.stderr)
    await app.waitFor(() => Object.values(app.threads()).some((row) => row.sessionId))
    const conversation = Object.keys(app.threads())[0]
    await app.waitFor(() =>
      app.transcript(app.threads()[conversation].sessionId).includes('stop_hook_summary'),
    )

    const index = await app.runCli(['results', '--json'], leadEnv)
    assert.equal(index.code, 0, index.stderr)
    const workers = JSON.parse(index.stdout).workers
    assert.equal(workers.length, 1)
    assert.equal(workers[0].conversation, conversation)
    assert.equal(workers[0].results.length, 1)
    assert.equal(workers[0].results[0].bytes, 60_000)
    assert.equal(workers[0].results[0].status, 'unread')
    const forbidden = await app.http('/api/panes/results.list', { method: 'POST', body: { tab } })
    assert.equal(forbidden.status, 403, 'the roster UI token cannot read lead results')
    const other = await app.openTab()
    const foreign = await app.runCli(['read', conversation], other.leadEnv)
    assert.equal(foreign.code, 1)

    await app.requestRust('pane.input', { id: lead.id, generation: lead.generation, bytes: [13] })
    assert.equal(
      (await app.requestRust('pane.snapshot', { id: lead.id, generation: lead.generation }))
        .draftLatched,
      true,
    )
    const read = await app.runCli(['read', conversation], leadEnv)
    assert.equal(read.code, 0, read.stderr)
    const record = app.deliveries().find((entry) => entry.claims.some((claim) => claim.manual))
    assert.ok(record)
    assert.ok(record.parts.length > 1)
    assert.ok(read.stdout.includes(record.parts[0].text))
    assert.equal(record.state, 'collecting', 'stdout alone is not a native receipt')
    assert.equal(
      app.nodeFrames.some((frame) => frame.op === 'pane.write_paste'),
      false,
    )
    assert.match(read.stderr, new RegExp(`cf read ${record.id}`))

    // The native fixture's harness process itself invokes the CLI for every
    // part and records complete tool outputs, exactly as for daemon delivery.
    await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from(`@worker answered in ${conversation} — run: cf read ${record.id}\r`)],
    })
    await app.waitFor(
      () => app.deliveries().find((entry) => entry.id === record.id)?.state === 'received',
      15_000,
    )
    const final = await app.runCli(['results', '--json'], leadEnv)
    assert.equal(JSON.parse(final.stdout).workers[0].results[0].status, 'read')
    assert.equal(
      app.nodeFrames.some((frame) => frame.op === 'pane.write_paste'),
      false,
    )
    assert.deepEqual(
      app.threads()[conversation].seen?.[leadEnv.CONSENSFLOW_LEAD_ID] ?? [],
      [],
      'omitted worker discussion remains unread',
    )
  } finally {
    await app.close()
  }
})
