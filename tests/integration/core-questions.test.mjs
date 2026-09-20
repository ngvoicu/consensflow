import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * A native question, end to end through the real pane host (TEST-CF1-15): a
 * fake Claude worker asks through its question tool, the hook in its settings
 * file puts the question on the board, the lead answers it with `cf answer`
 * as its inbox told it to, and the hook hands the answer back into the tool
 * call, so the worker goes on and finishes its task. Nothing is pasted into
 * the worker's window for that.
 */

const CORE_EDITOR = fileURLToPath(new URL('./core-editor.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

test("a worker's question with options goes to the lead's inbox and its answer returns through the hook", async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      harness: 'claude-code',
      review: 'none',
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
    const given = await app.requestNode('task.add', {
      project,
      to: 'lead',
      body: `DISPATCH --tier ${added.member.tier} ASK ${JSON.stringify(questions)}`,
    })
    assert.equal(given.ok, true, JSON.stringify(given))

    const board = async () => (await app.requestNode('board.get', { project })).board
    // A member's work runs in a session of its own: its lane is the session's.
    const lane = async (handle) =>
      (await board()).lanes.findLast(
        (candidate) =>
          candidate.participant.member === handle || candidate.participant.handle === handle,
      )
    const inbox = async (participant) =>
      (await app.requestNode('inbox.get', { project, participant })).messages

    // The task waits only until the lead answers, which the fake lead does at
    // once, so the proof is the thread the ledger kept, not a glimpse of the state.
    await app.waitFor(
      async () =>
        (await inbox('lead')).some((m) => m.kind === 'question' && m.state === 'delivered'),
      30_000,
    )
    const question = (await inbox('lead')).find((m) => m.kind === 'question')
    assert.match(question.sender, /^worker-/, "the worker's session asked")
    assert.deepEqual([question.taskNumber, question.questions[0].options[1].label], [2, 'blue'])
    assert.equal(question.body, 'Colour: Which colour?\n- red\n- blue: REPLY blue')

    await app.waitFor(async () => (await lane('worker'))?.tasks[0]?.state === 'done', 30_000)
    const session = (await lane('worker')).participant.handle
    const answer = (await inbox(session)).find((m) => m.kind === 'answer')
    assert.deepEqual(
      [answer.state, answer.choices, answer.sender, answer.body],
      ['read', [['blue']], 'lead', 'Colour: blue'],
      'the answer is collected by the hook, never delivered as text',
    )
    const { task } = await app.requestNode('task.get', { project, task: 2 })
    assert.equal(task.messages.find((m) => m.kind === 'result').body, 'answered: blue')
    await app.waitFor(
      async () => (await inbox('lead')).some((m) => m.kind === 'result' && m.state === 'delivered'),
      30_000,
    )
  } catch (cause) {
    // What the windows showed and what the ledger held, for the failure report.
    const board = (await app.requestNode('board.get', { project: 1 })).board
    cause.message += `\nboard=${JSON.stringify(board.lanes.map((l) => [l.participant.handle, l.tasks.map((t) => [t.number, t.state])]))}`
    for (const id of app.openFrames.map((frame) => frame.id)) {
      cause.message += `\n--- ${id}: ${app.output(id).slice(-1500)}`
    }
    const worker = await app.requestNode('task.get', { project: 1, task: 2 })
    cause.message += `\ntask 2=${JSON.stringify(worker.task?.messages?.map((m) => [m.kind, m.state, m.body.slice(0, 400)]))}`
    cause.message += `\nexits=${JSON.stringify(app.exits)}`
    throw cause
  } finally {
    await app.close()
  }
})
