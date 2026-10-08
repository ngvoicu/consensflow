import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * A tell and a cancel, end to end through the real pane host
 * (`npm run test:daemons`). The chief eval `six-decisions`
 * (2026-10-07, on the native daemon) counted "every tell the chief sent was
 * answered (1/2)": the chief had told the window of T-2, was in a turn of its
 * own for minutes, and called T-2 off. Nothing is delivered to a window that is
 * at work, so an answer to the tell waited in the chief's queue, and a task
 * called off takes back whatever of it is still on its way: the answer the
 * window gave was cancelled before the chief read it, and the eval's count, which
 * skips a cancelled answer, took the tell for unanswered.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/**
 * A project with a worker whose task T-1 is working when the chief tells it:
 * its turn takes fifteen seconds. Once the chief's tell is in, T-1 is paused.
 * The task's thread, by kind of message.
 */
async function toldWhileWorking(app) {
  const opened = await app.requestNode('project.open', {
    directory: app.workspace,
    agent: 'chief',
  })
  assert.equal(opened.ok, true, JSON.stringify(opened))
  const project = opened.project.id
  const added = await app.requestNode('member.add', { project, agent: 'worker' })
  assert.equal(added.ok, true, JSON.stringify(added))
  const task = async () => (await app.requestNode('task.get', { project, task: 1 })).task
  const thread = async (kind) => (await task()).messages.filter((m) => m.kind === kind)
  await app.tell(project, `DISPATCH --tier ${added.member.tier} SLEEP 15 Reply with exactly: ONE`)
  await app.waitFor(async () => (await task())?.state === 'working', 30_000)
  await app.tell(project, 'CF tell T-1 :: Stop now. REPLY stopped')
  await app.waitFor(async () => (await task()).state === 'paused', 30_000)
  return { project, thread }
}

test("the answer a window gave the chief's tell reaches the chief once its turn is over, and stays an answer when the task is called off after", async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const { project, thread } = await toldWhileWorking(app)

    // The window comes to rest, takes the tell in and answers it; the chief is idle, and has it.
    await app.waitFor(
      async () => (await thread('answer')).some((m) => m.state === 'delivered'),
      60_000,
    )
    const [tell] = await thread('question')
    const [answered] = await thread('answer')
    assert.deepEqual(
      [tell.urgent, tell.state, answered.replyTo, answered.recipient, answered.body],
      [true, 'delivered', tell.id, 'chief', 'stopped'],
    )

    assert.equal((await app.requestNode('task.cancel', { project, task: 1 })).ok, true)
    assert.deepEqual(
      (await thread('answer')).map((m) => m.state),
      ['delivered'],
      'what the chief has is not taken back',
    )
  } finally {
    await app.close()
  }
})

test("the answer a window gave the chief's tell is withdrawn when the chief's task is called off before the chief read it", async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const { project, thread } = await toldWhileWorking(app)
    // The chief goes into a turn of its own: nothing is delivered to it until that is over.
    await app.tell(project, 'SLEEP 90 Reply with exactly: BUSY')

    // The window comes to rest, takes the tell in and answers it with cf: the answer waits for the chief.
    await app.waitFor(async () => (await thread('answer')).length === 1, 60_000)
    // Whether the daemon has read the window's record of the tell yet is its own look's to say.
    await app.waitFor(async () => (await thread('question'))[0].state === 'delivered', 30_000)
    const [tell] = await thread('question')
    assert.deepEqual(
      [tell.urgent, tell.sender, tell.body, tell.state],
      [true, 'chief', 'Stop now. REPLY stopped', 'delivered'],
      'the tell reached the window',
    )
    const [answered] = await thread('answer')
    assert.deepEqual(
      [answered.replyTo, answered.recipient, answered.body, answered.state],
      [tell.id, 'chief', 'stopped', 'queued'],
      'the window answered it, and the chief has not read the answer',
    )

    // The task is called off before the chief reads it: by the human here, as the chief's own
    // window is busy; the ledger's cancel is the same, and its reason names who made it.
    assert.equal((await app.requestNode('task.cancel', { project, task: 1 })).ok, true)
    const [kept] = await thread('question')
    const [withdrawn] = await thread('answer')
    assert.deepEqual(
      [kept.state, withdrawn.state, withdrawn.reason],
      ['delivered', 'cancelled', 'cancelled by @human'],
      'the tell stays as it was delivered, and its answer is taken back',
    )
    // What the chief eval counts as an answer to a tell is one that is not cancelled: none.
    assert.deepEqual(
      (await thread('answer')).filter((m) => m.state !== 'cancelled'),
      [],
    )
  } finally {
    await app.close()
  }
})
