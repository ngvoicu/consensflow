import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { daemonCommand } from '../helpers.mjs'
import { startIntegration } from './harness.mjs'

/**
 * A native question, end to end through the real pane host (TEST-CF1-15): a
 * fake Claude worker asks through its question tool, the hook in its settings
 * file puts the question on the board, the chief answers it with `cf answer`
 * as its inbox told it to, and the hook hands the answer back into the tool
 * call, so the worker goes on and finishes its task. Nothing is pasted into
 * the worker's window for that.
 */

const DAEMON = fileURLToPath(new URL('./core-daemon.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))
/** Whether the daemon under test is the native one (`npm run test:daemons` runs both). */
const NATIVE = daemonCommand([DAEMON]).native

test("a worker's question with options goes to the chief's inbox and its answer returns through the hook", async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      agent: 'chief',
    })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const project = opened.project.id
    const added = await app.requestNode('member.add', { project, agent: 'worker' })
    assert.equal(added.ok, true, JSON.stringify(added))
    const questions = [
      {
        question: 'Which colour?',
        header: 'Colour',
        options: [{ label: 'red' }, { label: 'blue', description: 'REPLY blue' }],
        multiSelect: false,
      },
    ]
    await app.tell(project, `DISPATCH --tier ${added.member.tier} ASK ${JSON.stringify(questions)}`)

    const board = async () => (await app.requestNode('board.get', { project })).board
    // A member's work runs in a session of its own: its lane is the session's.
    const lane = async (handle) =>
      (await board()).lanes.findLast(
        (candidate) =>
          candidate.participant.member === handle || candidate.participant.handle === handle,
      )
    const inbox = async (participant) =>
      (await app.requestNode('inbox.get', { project, participant })).messages

    // The task waits only until the chief answers, which the fake chief does at
    // once, so the proof is the thread the ledger kept, not a glimpse of the state.
    await app.waitFor(
      async () =>
        (await inbox('chief')).some((m) => m.kind === 'question' && m.state === 'delivered'),
      30_000,
    )
    const question = (await inbox('chief')).find((m) => m.kind === 'question')
    assert.match(question.sender, /^worker-/, "the worker's session asked")
    assert.deepEqual([question.taskNumber, question.questions[0].options[1].label], [1, 'blue'])
    assert.equal(question.body, 'Colour: Which colour?\n- red\n- blue: REPLY blue')

    await app.waitFor(async () => (await lane('worker'))?.tasks[0]?.state === 'done', 30_000)
    const session = (await lane('worker')).participant.handle
    const answer = (await inbox(session)).find((m) => m.kind === 'answer')
    assert.deepEqual(
      [answer.state, answer.choices, answer.sender, answer.body],
      ['read', [['blue']], 'chief', 'Colour: blue'],
      'the answer is collected by the hook, never delivered as text',
    )
    // What read it: the native daemon, the hook's own receipt once it handed the
    // answer over (a hook that says so before its worker's turn goes on); Node's
    // reads a choice answer as it is written, and knows no receipt.
    assert.deepEqual(answer.receipt, NATIVE ? { door: true } : null)
    const { task } = await app.requestNode('task.get', { project, task: 1 })
    assert.equal(task.messages.find((m) => m.kind === 'result').body, 'answered: blue')
    await app.waitFor(
      async () =>
        (await inbox('chief')).some((m) => m.kind === 'result' && m.state === 'delivered'),
      30_000,
    )
  } catch (cause) {
    // What the windows showed and what the ledger held, for the failure report.
    const board = (await app.requestNode('board.get', { project: 1 })).board
    cause.message += `\nboard=${JSON.stringify(board.lanes.map((l) => [l.participant.handle, l.tasks.map((t) => [t.number, t.state])]))}`
    for (const id of app.openFrames.map((frame) => frame.id)) {
      cause.message += `\n--- ${id}: ${app.output(id).slice(-1500)}`
    }
    const worker = await app.requestNode('task.get', { project: 1, task: 1 })
    cause.message += `\ntask 1=${JSON.stringify(worker.task?.messages?.map((m) => [m.kind, m.state, m.body.slice(0, 400)]))}`
    cause.message += `\nexits=${JSON.stringify(app.exits)}`
    throw cause
  } finally {
    await app.close()
  }
})
