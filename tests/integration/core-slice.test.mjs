import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * The new core end to end (TEST-BDC-09 through the real pane host): its daemon,
 * the real Rust headless bridge and PTYs, and fake Claude agents in them. The
 * human gives the lead a task on the board; the lead hands part of it to a
 * worker with `cf task add`; the core opens the worker's window with the task,
 * collects the worker's answer as the result and delivers it into the lead's
 * window, where the lead's own transcript shows it arrived.
 */

const CORE_EDITOR = fileURLToPath(new URL('./core-editor.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

test('a lead hands a task to a worker through the board and the result lands in its window', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('session.open', {
      directory: app.workspace,
      harness: 'claude-code',
    })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const session = opened.session.id
    const added = await app.requestNode('member.add', { session, agent: 'worker' })
    assert.equal(added.ok, true, JSON.stringify(added))
    const leadFrame = app.openFrames.find((frame) => frame.id === `s${session}-lead`)
    assert.ok(leadFrame, 'the lead window opened')

    const given = await app.requestNode('task.add', {
      session,
      to: 'lead',
      body: 'DISPATCH @worker Reply with exactly: WORKER_OK',
    })
    assert.equal(given.ok, true, JSON.stringify(given))

    const board = async () => (await app.requestNode('board.get', { session })).board
    const lane = async (handle) =>
      (await board()).lanes.find((candidate) => candidate.participant.handle === handle)
    await app.waitFor(async () => (await lane('worker'))?.tasks[0]?.state === 'done', 30_000)
    const workerTask = (await lane('worker')).tasks[0]
    assert.deepEqual([workerTask.requester, workerTask.number], ['lead', 2])

    const workerFrame = app.openFrames.find((frame) => frame.id === `s${session}-worker`)
    assert.match(
      workerFrame.argv.at(-1),
      /^\[ConsensFlow m-\d+ · T-2 · task from @lead\]\nReply with exactly: WORKER_OK$/,
    )
    assert.ok(workerFrame.argv.includes('bypassPermissions'))

    await app.waitFor(async () => {
      const { messages } = await app.requestNode('inbox.get', { session, participant: 'lead' })
      return messages.some((m) => m.kind === 'result' && m.state === 'delivered')
    }, 30_000)
    const { messages } = await app.requestNode('inbox.get', { session, participant: 'lead' })
    const result = messages.find((m) => m.kind === 'result')
    assert.equal(result.body, 'WORKER_OK')
    // The lead's native session is the `--session-id` its window was launched with.
    const leadSession = leadFrame.argv[leadFrame.argv.indexOf('--session-id') + 1]
    assert.match(
      app.transcript(leadSession),
      new RegExp(`\\[ConsensFlow m-${result.id} · T-2 · result from @worker\\]`),
    )
  } finally {
    await app.close()
  }
})
