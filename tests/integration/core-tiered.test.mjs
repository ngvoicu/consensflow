import assert from 'node:assert/strict'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * Tiered dispatch, the review gate and quota, end to end through the real
 * pane host (VERIFY-TD-13): fake Claude agents in real PTYs, the daemon
 * picking the member, a reviewer on another model judging the work, and a
 * worker whose provider refuses it mid-task losing the task to another.
 */

const CORE_EDITOR = fileURLToPath(new URL('./core-editor.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/** Three fake agents on the fake `claude`: two workers on one model, a reviewer on another. */
function team(app) {
  writeFileSync(
    join(app.env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify({
      schemaVersion: 1,
      agents: [
        { id: 'worker', kind: 'claude-code', model: 'fake' },
        { id: 'worker2', kind: 'claude-code', model: 'fake' },
        { id: 'checker', kind: 'claude-code', model: 'fake-2' },
      ],
    })}\n`,
  )
}

async function project(app, { review, members }) {
  const opened = await app.requestNode('project.open', {
    directory: app.workspace,
    harness: 'claude-code',
    review,
  })
  assert.equal(opened.ok, true, JSON.stringify(opened))
  const id = opened.project.id
  const tiers = {}
  for (const [agent, role] of members) {
    const added = await app.requestNode('member.add', { project: id, agent, role })
    assert.equal(added.ok, true, JSON.stringify(added))
    tiers[agent] = added.member.tier
  }
  const board = async () => (await app.requestNode('board.get', { project: id })).board
  const task = async (number) =>
    (await app.requestNode('task.get', { project: id, task: number })).task
  const lane = async (handle) =>
    (await board()).lanes.find((candidate) => candidate.participant.handle === handle)
  const inbox = async (participant) =>
    (await app.requestNode('inbox.get', { project: id, participant })).messages
  return { id, tiers, board, task, lane, inbox }
}

test('a reviewer on another model passes the work, and the requester gets the result with the review', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    team(app)
    const p = await project(app, {
      review: 'members',
      members: [
        ['worker', 'worker'],
        ['checker', 'reviewer'],
      ],
    })
    const given = await app.requestNode('task.add', {
      project: p.id,
      to: 'lead',
      body: `DISPATCH --tier ${p.tiers.worker} Reply with exactly: WORKER_OK\\nREVIEWER: VERDICT: pass`,
    })
    assert.equal(given.ok, true, JSON.stringify(given))

    await app.waitFor(async () => (await p.task(2))?.state === 'review', 60_000)
    await app.waitFor(async () => (await p.lane('checker'))?.tasks.length === 1, 60_000)
    const review = (await p.lane('checker')).tasks[0]
    assert.deepEqual([review.kind, review.reviewOf, review.title], ['review', 2, 'Review T-2'])
    await app.waitFor(async () => (await p.task(2))?.state === 'done', 60_000)
    const reviewed = await p.task(2)
    assert.equal(reviewed.round, 1)
    assert.equal((await p.task(review.number)).verdict, 'pass')
    await app.waitFor(async () => {
      const messages = await p.inbox('lead')
      return messages.filter((m) => m.kind === 'result' && m.state === 'delivered').length === 2
    }, 60_000)
    const results = (await p.inbox('lead')).filter((m) => m.kind === 'result').reverse()
    assert.deepEqual(
      results.map((m) => [m.taskNumber, m.body]),
      [
        [2, 'WORKER_OK'],
        [review.number, 'VERDICT: pass'],
      ],
    )
  } finally {
    await app.close()
  }
})

test('each task runs in its own worker session: the window closes with the task, the next opens a new one', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    team(app)
    const p = await project(app, { review: 'none', members: [['worker', 'worker']] })
    const alive = (pid) => {
      try {
        process.kill(pid, 0)
        return true
      } catch {
        return false
      }
    }
    const sessions = () => new Set(app.processes().map((entry) => entry.sessionId))
    for (const [word, expected] of [
      ['ONE', 2],
      ['TWO', 3],
    ]) {
      await app.requestNode('task.add', {
        project: p.id,
        to: 'lead',
        body: `DISPATCH --tier ${p.tiers.worker} Reply with exactly: ${word}`,
      })
      await app.waitFor(
        async () =>
          (await p.lane('worker'))?.tasks.some((t) => t.state === 'done' && t.title.includes(word)),
        60_000,
      )
      await app.waitFor(async () => (await p.lane('worker'))?.pane === null, 30_000)
      assert.equal((await p.lane('worker')).activity.state, 'closed')
      assert.equal(sessions().size, expected, 'one native session per task, plus the lead')
    }
    const workerPids = app.processes().filter((entry) => entry.pid !== app.processes()[0].pid)
    await app.waitFor(async () => workerPids.every((entry) => !alive(entry.pid)), 30_000)
  } finally {
    await app.close()
  }
})

test('a reviewer asking for changes twice sends the work back once, then the requester decides', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    team(app)
    const p = await project(app, {
      review: 'members',
      members: [
        ['worker', 'worker'],
        ['checker', 'reviewer'],
      ],
    })
    await app.requestNode('task.add', {
      project: p.id,
      to: 'lead',
      body: `DISPATCH --tier ${p.tiers.worker} Reply with exactly: WORKER_OK\\nREVIEWER: VERDICT: changes`,
    })
    await app.waitFor(async () => (await p.task(2))?.round === 1, 90_000)
    const back = await p.task(2)
    assert.equal(back.assignee, 'worker', 'the work goes back to its author')
    assert.ok(
      back.messages.some(
        (m) =>
          m.kind === 'task' && m.body.startsWith('Review round 1 by @checker asks for changes:'),
      ),
      'the findings reach the worker as a follow-up',
    )
    await app.waitFor(
      async () => (await p.task(2))?.round === 2 && (await p.task(2))?.state === 'done',
      120_000,
    )
    await app.waitFor(async () => {
      const notes = (await p.inbox('lead')).filter((m) => m.kind === 'note')
      return notes.some((m) => m.body.includes('asked for changes twice in review'))
    }, 60_000)
    const reviews = (await p.lane('checker')).tasks
    assert.deepEqual(
      reviews.map((t) => [t.kind, t.state, t.verdict]),
      [
        ['review', 'done', 'changes'],
        ['review', 'done', 'changes'],
      ],
    )
  } finally {
    await app.close()
  }
})

test('a worker refused by its provider mid-task loses the task to the other worker of its tier', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CF_TEST_QUOTA_OUT: 'worker' },
  })
  try {
    team(app)
    const p = await project(app, {
      review: 'none',
      members: [
        ['worker', 'worker'],
        ['worker2', 'worker'],
      ],
    })
    await app.requestNode('task.add', {
      project: p.id,
      to: 'lead',
      body: `DISPATCH --tier ${p.tiers.worker} QUOTA-OUT Reply with exactly: WORKER_OK`,
    })
    await app.waitFor(async () => (await p.task(2))?.state === 'done', 90_000)
    const done = await p.task(2)
    assert.equal(done.assignee, 'worker2')
    assert.match(done.body, /Reassigned from @worker, which ran out of quota after starting/)
    const out = (await p.lane('worker')).participant
    assert.ok(
      out.outUntil !== null && Date.parse(out.outUntil) > Date.now(),
      'the first worker is out',
    )
    assert.ok(
      (await p.inbox('lead')).some(
        (m) =>
          m.kind === 'note' &&
          m.body.startsWith('T-2 was taken back from @worker (ran out of quota after starting)'),
      ),
      'the requester was told',
    )
    const second = (await p.lane('worker2')).tasks[0]
    assert.equal(second.number, 2)
    await app.waitFor(async () => {
      const results = (await p.inbox('lead')).filter((m) => m.kind === 'result')
      return results.some((m) => m.body === 'WORKER_OK' && m.state === 'delivered')
    }, 60_000)
  } finally {
    await app.close()
  }
})
