import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * The board after a re-plan, end to end through the real pane host
 * (`npm run test:daemons`). The chief eval `six-decisions`
 * (2026-10-07, on the native daemon) cancelled tasks that were still open for
 * a tier, and its check "the board showed every task" counted fewer than it
 * had made: both daemons left such a task off the board, as a task paused in
 * the backlog and a removed member's. The owner decided (2026-10-07) that none
 * leaves it. The board lists a task by the lane of whoever has it, or among the
 * open ones when no lane has it (waiting for a member, paused or called off
 * before any member had it, or a removed member's), and `cf task list` lists
 * both: the page draws the open ones on their requester's row.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/** Where each task the board draws is: `open T-n state`, or the lane's handle (a session's name masked) and the same. */
const listed = (board) =>
  [
    ...board.open.map((task) => `open T-${task.number} ${task.state}`),
    ...board.lanes.flatMap((lane) =>
      lane.tasks.map(
        (task) =>
          `${lane.participant.handle.replace(/^(\w+)-[a-z]+-[a-z]+$/, '$1-*')} T-${task.number} ${task.state}`,
      ),
    ),
  ].sort()

/** What the chief's `cf` printed each time it ran one for the board, in order: its window's record of the turns. */
const printed = (record) =>
  record
    .split('\n')
    .filter((line) => line.trim() !== '')
    .map((line) => JSON.parse(line))
    .filter((entry) => entry.type === 'assistant')
    .map((entry) => entry.message.content.map((part) => part.text ?? '').join(''))
    .filter((text) => text.startsWith('ran cf: '))
    .map((text) => text.slice('ran cf: '.length).replace(/@worker-[a-z]+-[a-z]+/g, '@worker-*'))

test('a task no lane has stays on the board and in cf task list: called off, paused, or a removed member’s', async () => {
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
    const chiefSession = chiefFrame.argv[chiefFrame.argv.indexOf('--session-id') + 1]
    const board = async () => (await app.requestNode('board.get', { project })).board
    const task = async (number) =>
      (await app.requestNode('task.get', { project, task: number })).task

    // T-1 is done and not accepted, so the three tasks that need it wait, open for a tier.
    await app.tell(project, `DISPATCH --tier ${added.member.tier} Reply with exactly: ONE`)
    await app.waitFor(async () => (await task(1))?.state === 'done', 30_000)
    for (const word of ['TWO', 'THREE', 'FOUR']) {
      await app.tell(
        project,
        `DISPATCH --tier ${added.member.tier} --needs T-1 Reply with exactly: ${word}`,
      )
    }
    await app.waitFor(async () => (await board()).open.length === 3, 30_000)
    assert.deepEqual(listed(await board()), [
      'open T-2 open',
      'open T-3 open',
      'open T-4 open',
      'worker-* T-1 done',
    ])

    // The chief calls T-2 off with its own `cf`: the next board it reads lists it apart
    // from what waits for a member.
    await app.tell(project, 'CF task list')
    await app.tell(project, 'CF task cancel T-2')
    await app.waitFor(async () => (await task(2)).state === 'cancelled', 30_000)
    await app.tell(project, 'CF task list')
    await app.waitFor(async () => printed(app.transcript(chiefSession)).length === 3, 30_000)
    const [before, , after] = printed(app.transcript(chiefSession))
    assert.match(before, /^Waiting for a member\nT-2 \[open\] blocked by T-1 .*\nT-3 /)
    assert.match(
      after,
      /^Waiting for a member\nT-3 \[open\] .*\nT-4 \[open\] .*\nWith no member\nT-2 \[cancelled\] blocked by T-1 /,
      'the chief reads the task it called off, no longer among those waiting',
    )
    assert.deepEqual(listed(await board()), [
      'open T-2 cancelled',
      'open T-3 open',
      'open T-4 open',
      'worker-* T-1 done',
    ])

    // The human cancels T-3 and pauses T-4, as the page's buttons do: both stay.
    assert.equal((await app.requestNode('task.cancel', { project, task: 3 })).ok, true)
    assert.equal((await app.requestNode('task.pause', { project, task: 4 })).ok, true)
    assert.deepEqual(
      listed(await board()),
      ['open T-2 cancelled', 'open T-3 cancelled', 'open T-4 paused', 'worker-* T-1 done'],
      'all four are drawn: the paused one in the backlog, the two called off to be deleted',
    )
    assert.deepEqual(
      await Promise.all([2, 3, 4].map(async (number) => [number, (await task(number)).state])),
      [
        [2, 'cancelled'],
        [3, 'cancelled'],
        [4, 'paused'],
      ],
    )
    for (const number of [2, 3, 4]) {
      assert.equal((await task(number)).assignee, null, `no member ever had T-${number}`)
    }
    assert.equal(
      (await app.requestNode('task.get', { project, task: 5 })).ok,
      false,
      'and there are four, not five',
    )

    // The human resumes T-4 from the backlog and deletes the two called off, as the page does.
    assert.equal((await app.requestNode('task.resume', { project, task: 4 })).ok, true)
    assert.equal((await app.requestNode('tasks.delete', { project, tasks: [2, 3] })).ok, true)
    assert.deepEqual(listed(await board()), ['open T-4 open', 'worker-* T-1 done'])

    // The human removes the worker: T-1, which it did and the chief has not accepted, stays on
    // the board, no lane's, and the chief reads it so.
    assert.equal((await app.requestNode('member.remove', { project, agent: 'worker' })).ok, true)
    assert.deepEqual(listed(await board()), ['open T-1 done', 'open T-4 open'])
    await app.tell(project, 'CF task list')
    await app.waitFor(async () => printed(app.transcript(chiefSession)).length === 4, 30_000)
    assert.match(
      printed(app.transcript(chiefSession)).at(-1),
      /^Waiting for a member\nT-4 \[open\] blocked by T-1 .*\nWith no member\nT-1 \[done\] @worker-\* ← @chief: .*$/,
    )
  } finally {
    await app.close()
  }
})
