import assert from 'node:assert/strict'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * Tiered dispatch, reviews and quota, end to end through the real pane host
 * (VERIFY-TD-13): fake Claude agents in real PTYs, the daemon picking the
 * member, a review the chief puts on the board going to a reviewer like any
 * task, and a worker whose provider refuses it mid-task losing the task to
 * another.
 */

const DAEMON = fileURLToPath(new URL('./core-daemon.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/** Four fake agents on the fake `claude`: the chief, two workers on one model, a reviewer on another. */
function staff(app) {
  writeFileSync(
    join(app.env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify({
      schemaVersion: 1,
      agents: [
        { id: 'chief', kind: 'claude-code', model: 'fake-chief' },
        { id: 'worker', kind: 'claude-code', model: 'fake' },
        { id: 'worker2', kind: 'claude-code', model: 'fake' },
        { id: 'checker', kind: 'claude-code', model: 'fake-2' },
      ],
    })}\n`,
  )
}

/** A project opened as the New project dialog opens one: the staff and the approval setting together. */
async function project(app, { gate, members }) {
  const opened = await app.requestNode('project.open', {
    directory: app.workspace,
    agent: 'chief',
    ...(gate === undefined ? {} : { gate }),
    staff: members.map(([agent, role]) => ({ agent, roles: [role] })),
  })
  assert.equal(opened.ok, true, JSON.stringify(opened))
  const id = opened.project.id
  const board = async () => (await app.requestNode('board.get', { project: id })).board
  const tiers = Object.fromEntries(
    (await board()).lanes
      .filter((lane) => lane.participant.agent !== null)
      .map((lane) => [lane.participant.agent, lane.participant.tier]),
  )
  const task = async (number) =>
    (await app.requestNode('task.get', { project: id, task: number })).task
  // A member's work runs in a session of its own: its lane is the session's,
  // the newest one when it has had several.
  const lane = async (handle) =>
    (await board()).lanes.findLast(
      (candidate) =>
        candidate.participant.member === handle || candidate.participant.handle === handle,
    )
  const inbox = async (participant) =>
    (await app.requestNode('inbox.get', { project: id, participant })).messages
  return { id, tiers, board, task, lane, inbox }
}

test('a review is a task the chief puts on the board: a reviewer of its tier takes it and its findings come back as the result', async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    staff(app)
    const p = await project(app, {
      members: [
        ['worker', 'worker'],
        ['checker', 'reviewer'],
      ],
    })
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} Reply with exactly: WORKER_OK`)
    await app.waitFor(async () => {
      const messages = await p.inbox('chief')
      return messages.some(
        (m) => m.kind === 'result' && m.taskNumber === 1 && m.state === 'delivered',
      )
    }, 60_000)
    assert.equal((await p.task(1)).state, 'done', 'nothing is reviewed on its own')
    assert.deepEqual(
      (await p.board()).lanes.flatMap((lane) => lane.tasks).map((t) => t.number),
      [1],
      'no review task appears by itself',
    )

    await app.tell(
      p.id,
      `DISPATCH --review --tier ${p.tiers.checker} Review T-1. Reply with exactly: FINDINGS_OK`,
    )
    await app.waitFor(async () => (await p.task(2))?.state === 'done', 90_000)
    const review = await p.task(2)
    assert.deepEqual([review.pool, review.tier], ['reviewer', p.tiers.checker])
    assert.match(review.assignee, /^checker-/)
    const findings = review.messages.find((m) => m.kind === 'result')
    assert.deepEqual([findings.recipient, findings.body], ['chief', 'FINDINGS_OK'])
    assert.equal((await p.task(1)).state, 'done', 'the chief decides the work')
  } finally {
    await app.close()
  }
})

test('each task runs in its own worker session: the window closes with the task, the next opens a new one', async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    staff(app)
    const p = await project(app, { members: [['worker', 'worker']] })
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
      await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} Reply with exactly: ${word}`)
      await app.waitFor(
        async () =>
          (await p.lane('worker'))?.tasks.some((t) => t.state === 'done' && t.title.includes(word)),
        60_000,
      )
      await app.waitFor(async () => (await p.lane('worker'))?.pane === null, 30_000)
      assert.equal((await p.lane('worker')).activity.state, 'closed')
      assert.equal(sessions().size, expected, 'one native session per task, plus the chief')
    }
    const workerPids = app.processes().filter((entry) => entry.pid !== app.processes()[0].pid)
    await app.waitFor(async () => workerPids.every((entry) => !alive(entry.pid)), 30_000)
  } finally {
    await app.close()
  }
})

test('a worker refused by its provider mid-task loses the task to the other worker of its tier', async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CF_TEST_QUOTA_OUT: 'worker' },
  })
  try {
    staff(app)
    const p = await project(app, {
      members: [
        ['worker', 'worker'],
        ['worker2', 'worker'],
      ],
    })
    await app.tell(
      p.id,
      `DISPATCH --tier ${p.tiers.worker} QUOTA-OUT Reply with exactly: WORKER_OK`,
    )
    await app.waitFor(async () => (await p.task(1))?.state === 'done', 90_000)
    const done = await p.task(1)
    assert.match(done.assignee, /^worker2-/, 'a session of the other worker')
    assert.match(
      done.body,
      /Reassigned from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\)/,
    )
    // Quota is the member's: the member row says it is out, not a session.
    const out = (await p.board()).lanes.find((l) => l.participant.handle === 'worker').participant
    assert.ok(
      out.outUntil !== null && Date.parse(out.outUntil) > Date.now(),
      'the first worker is out',
    )
    assert.ok(
      (await p.inbox('chief')).some(
        (m) =>
          m.kind === 'note' &&
          /^T-1 was taken back from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\)/.test(
            m.body,
          ),
      ),
      'the requester was told',
    )
    const second = (await p.lane('worker2')).tasks[0]
    assert.equal(second.number, 1)
    await app.waitFor(async () => {
      const results = (await p.inbox('chief')).filter((m) => m.kind === 'result')
      return results.some((m) => m.body === 'WORKER_OK' && m.state === 'delivered')
    }, 60_000)
  } finally {
    await app.close()
  }
})

test('with human approval required, the brief and the result each wait for the human before they move', async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    staff(app)
    const p = await project(app, { gate: true, members: [['worker', 'worker']] })
    assert.equal((await p.board()).project.gate, true)
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} Reply with exactly: WORKER_OK`)

    // The chief's brief is assigned, then held: no worker window opens for it.
    await app.waitFor(async () => (await p.board()).gated.length === 1, 60_000)
    const [brief] = (await p.board()).gated
    assert.deepEqual([brief.kind, brief.sender, brief.taskNumber], ['task', 'chief', 1])
    assert.equal((await p.task(1)).state, 'queued')
    await new Promise((resolve) => setTimeout(resolve, 1500))
    assert.equal(
      app.openFrames.some((frame) => frame.id === `p${p.id}-${brief.recipient}`),
      false,
      'nothing opened while the human had not approved',
    )
    const approved = await app.requestNode('message.approve', { message: brief.id })
    assert.equal(approved.ok, true, JSON.stringify(approved))
    await app.waitFor(async () => (await p.task(1))?.state === 'done', 60_000)

    // The result waits the same way; the chief's window gets nothing until it is passed on.
    await app.waitFor(async () => (await p.board()).gated.length === 1, 60_000)
    const [result] = (await p.board()).gated
    assert.deepEqual([result.kind, result.recipient, result.body], ['result', 'chief', 'WORKER_OK'])
    assert.deepEqual(
      (await p.inbox('chief')).filter((m) => m.kind === 'result'),
      [],
      "not in the chief's inbox yet",
    )
    await app.requestNode('message.approve', { message: result.id })
    await app.waitFor(async () => {
      const messages = await p.inbox('chief')
      return messages.some((m) => m.id === result.id && m.state === 'delivered')
    }, 60_000)
  } finally {
    await app.close()
  }
})

test("the human opens a finished session's window on its own conversation, and closing it ends the process", async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    staff(app)
    const p = await project(app, { members: [['worker', 'worker']] })
    const alive = (pid) => {
      try {
        process.kill(pid, 0)
        return true
      } catch {
        return false
      }
    }
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} Reply with exactly: ONE`)
    await app.waitFor(async () => (await p.task(1))?.state === 'done', 60_000)
    const handle = (await p.task(1)).assignee
    const session = async () =>
      (await p.board()).lanes.find((lane) => lane.participant.handle === handle)
    await app.waitFor(async () => (await session())?.pane === null, 30_000)
    // The fake agent records its pid and native session: the chief's comes first.
    const chief = app.processes()[0].sessionId
    const first = app.processes().find((entry) => entry.sessionId !== chief)
    await app.waitFor(async () => !alive(first.pid), 30_000)

    const opened = await app.requestNode('session.open', { project: p.id, handle })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    await app.waitFor(async () => (await session())?.pane !== null, 30_000)
    // A fresh process on the same conversation.
    await app.waitFor(
      async () =>
        app
          .processes()
          .some((entry) => entry.sessionId === first.sessionId && entry.pid !== first.pid),
      30_000,
    )
    const again = app
      .processes()
      .filter((entry) => entry.sessionId === first.sessionId)
      .at(-1)
    await new Promise((resolve) => setTimeout(resolve, 2_000))
    assert.notEqual((await session()).pane, null, 'it stays open with nothing to do')
    assert.equal(alive(again.pid), true)

    const closed = await app.requestNode('session.close', { project: p.id, handle })
    assert.equal(closed.ok, true, JSON.stringify(closed))
    await app.waitFor(async () => (await session())?.pane === null, 30_000)
    await app.waitFor(async () => !alive(again.pid), 30_000)
    assert.ok(await session(), 'the session stays for the human')
  } finally {
    await app.close()
  }
})
