import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { NATIVE_CF } from '../choice.mjs'
import { startIntegration } from './harness.mjs'

/**
 * A result the chief decides on before it is given it, end to end through the
 * real pane host (`npm run test:integration`). The chief's turn of 2026-10-08:
 * T-9 finished while the chief was in a turn of its own, so its result
 * stayed queued; in that turn the chief read it with `cf task get T-9`,
 * accepted the task and put T-10 after it, and ConsensFlow still pasted the
 * result into its window, "Decide with: cf task accept T-9 …", when the turn
 * ended. The decision withdraws the result: the chief is given the next
 * task's result, and never the one it had decided on.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

test('a result the chief read and decided on in its own turn is withdrawn, and the next result reaches it', async () => {
  const app = await startIntegration({
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
    const chiefFrame = await app.openFrame(`p${project}-chief`)
    // `cf` as the chief runs it in its window: its own token, the native binary.
    const cf = (...words) =>
      execFileSync(NATIVE_CF, words, {
        env: {
          ...app.env,
          CONSENSFLOW_URL: chiefFrame.env.CONSENSFLOW_URL,
          CONSENSFLOW_TOKEN: chiefFrame.env.CONSENSFLOW_TOKEN,
        },
        encoding: 'utf8',
      })
    const results = async () =>
      (await app.requestNode('inbox.get', { project, participant: 'chief' })).messages
        .filter((message) => message.kind === 'result')
        .sort((a, b) => a.id - b.id)
    const chiefActivity = async () =>
      (await app.requestNode('board.get', { project })).board.lanes.find(
        (lane) => lane.participant.handle === 'chief',
      ).activity.state

    // T-1 takes its window a few seconds; the chief's next turn takes much longer.
    await app.tell(
      project,
      `DISPATCH --tier ${added.member.tier} SLEEP 4 Reply with exactly: ONE_DONE`,
    )
    await app.tell(project, 'SLEEP 40 Reply with exactly: CHIEF_BUSY')

    // The window finishes T-1 while the chief is busy: its result waits behind the turn.
    await app.waitFor(async () => (await results()).length === 1, 30_000)
    const [waiting] = await results()
    assert.deepEqual([waiting.body, waiting.state], ['ONE_DONE', 'queued'])
    assert.equal(await chiefActivity(), 'working', 'the chief is in its turn, not interrupted')

    // In that turn the chief reads it, whole, and a read receives nothing.
    assert.match(
      cf('task', 'get', 'T-1'),
      new RegExp(`m-${waiting.id} \\[queued\\] result T-1 from @worker-[a-z]+-[a-z]+\\nONE_DONE`),
    )
    assert.equal((await results())[0].state, 'queued')
    // It accepts the task and puts the next one after it.
    cf('task', 'accept', 'T-1')
    cf('task', 'add', '--after', 'T-1', 'Reply with exactly: TWO_DONE')
    const [withdrawn] = await results()
    assert.deepEqual(
      [withdrawn.id, withdrawn.state, withdrawn.reason],
      [waiting.id, 'cancelled', 'T-1 was accepted'],
    )

    // Its turn ends, T-2 finishes, and the chief is given T-2's result: the dispatcher
    // pastes into the idle window, oldest first, so T-1's would have come before it.
    await app.waitFor(
      async () => (await results()).some((m) => m.body === 'TWO_DONE' && m.state === 'delivered'),
      90_000,
    )
    const [first, second] = await results()
    assert.deepEqual(
      [first.id, first.state, first.reason],
      [waiting.id, 'cancelled', 'T-1 was accepted'],
      'still withdrawn, not pasted',
    )
    const chiefSession = chiefFrame.argv[chiefFrame.argv.indexOf('--session-id') + 1]
    const transcript = app.transcript(chiefSession)
    assert.ok(
      !transcript.includes(`[ConsensFlow m-${first.id} `),
      'the chief was never given the result it had decided on',
    )
    assert.ok(!transcript.includes('Decide with: cf task accept T-1'), 'nor its footer')
    assert.match(
      transcript,
      new RegExp(`\\[ConsensFlow m-${second.id} · T-2 · result from @worker-[a-z]+-[a-z]+\\]`),
    )

    // A result the chief was given stays given when it decides on its task after.
    cf('task', 'accept', 'T-2')
    assert.deepEqual(
      (await results()).map((m) => [m.id, m.state]),
      [
        [first.id, 'cancelled'],
        [second.id, 'delivered'],
      ],
    )
  } finally {
    await app.close()
  }
})
