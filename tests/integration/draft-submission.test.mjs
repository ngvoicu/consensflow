import assert from 'node:assert/strict'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { startIntegration } from './harness.mjs'

const records = (source) =>
  source
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line))

const entersFor = (app, pane) =>
  app.rustFrames.filter(
    (frame) =>
      frame.kind === 'evt' &&
      frame.op === 'pane.enter' &&
      frame.body?.id === pane.id &&
      frame.body?.generation === pane.generation,
  )

const clearsFor = (app, pane) =>
  app.nodeFrames.filter(
    (frame) =>
      frame.kind === 'req' &&
      frame.op === 'draft.clear' &&
      frame.body?.id === pane.id &&
      frame.body?.generation === pane.generation,
  )

test('TEST-PANE-61 native user observation cannot clear opaque PTY input', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_INTEGRATION_NATIVE_HUMAN_SUBMISSION: '1' },
  })
  try {
    const opened = await app.openTab()
    const lead = opened.tab.panes.find((pane) => pane.kind === 'lead')
    assert.ok(lead)
    const session = opened.tab.lead.nativeSession
    assert.equal(typeof session, 'string')

    await app.waitFor(() => app.transcript(session).includes('"content":"lead seed"'))

    const submittedText = 'TEST_PANE_61 submitted native turn'
    const newerDraft = 'newer draft must remain latched'
    const submitted = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from(`${submittedText}\r`)],
    })
    assert.equal(submitted.ok, true, JSON.stringify(submitted))

    await app.waitFor(() =>
      entersFor(app, lead).some((frame) => frame.body.epoch === submitted.epoch),
    )
    const entered = entersFor(app, lead).find((frame) => frame.body.epoch === submitted.epoch)
    assert.ok(entered)

    const newer = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from(newerDraft)],
    })
    assert.equal(newer.ok, true, JSON.stringify(newer))
    assert.ok(newer.epoch > submitted.epoch)

    writeFileSync(join(app.env.CONSENSFLOW_HOME, 'native-human-release'), 'release\n')
    const nativeUserTurn = () =>
      records(app.transcript(session)).some(
        (record) =>
          record.type === 'user' &&
          record.message?.role === 'user' &&
          record.message?.content === submittedText,
      )
    const deadline = Date.now() + 2_000
    await app.waitFor(
      () => nativeUserTurn() && (clearsFor(app, lead).length > 0 || Date.now() >= deadline),
      3_000,
    )

    assert.equal(
      nativeUserTurn(),
      true,
      'the submitted human line never reached the native transcript',
    )
    assert.deepEqual(clearsFor(app, lead), [], 'native text has no causal input token')
    const refused = await app.requestRust('pane_resume_replies', {
      id: lead.id,
      generation: lead.generation,
      inputEpoch: newer.epoch,
      sequence: 0,
    })
    assert.notEqual(refused.ok, true, 'the human recovery command is not on the Node bridge')

    const snapshot = await app.requestRust('pane.snapshot', {
      id: lead.id,
      generation: lead.generation,
    })
    assert.equal(snapshot.draftLatched, true, JSON.stringify(snapshot))
    assert.equal(snapshot.inputEpoch, newer.epoch, JSON.stringify(snapshot))
  } finally {
    await app.close()
  }
})
