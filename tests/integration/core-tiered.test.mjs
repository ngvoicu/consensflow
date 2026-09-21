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

/** A project opened as the New project dialog opens one: the team and the policy together. */
async function project(app, { review, gate, members }) {
  const opened = await app.requestNode('project.open', {
    directory: app.workspace,
    harness: 'claude-code',
    review,
    ...(gate === undefined ? {} : { gate }),
    team: members.map(([agent, role]) => ({ agent, roles: [role] })),
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
      return messages.some(
        (m) => m.kind === 'result' && m.taskNumber === 2 && m.state === 'delivered',
      )
    }, 60_000)
    const results = (await p.inbox('lead')).filter(
      (m) => m.kind === 'result' && m.state === 'delivered',
    )
    assert.deepEqual(
      results.map((m) => [m.taskNumber, m.body]),
      [[2, 'WORKER_OK']],
      'one delivery: the result; the findings stay on the review task',
    )
    const findings = (await p.task(review.number)).messages.find((m) => m.kind === 'result')
    assert.deepEqual([findings.state, findings.body], ['read', 'VERDICT: pass'])
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
    assert.match(back.assignee, /^worker-/, 'the work goes back to its author, the same session')
    assert.ok(
      back.messages.some(
        (m) =>
          m.kind === 'task' &&
          /^Review round 1 by @checker-[a-z]+-[a-z]+ asks for changes:/.test(m.body),
      ),
      'the findings reach the worker as a follow-up',
    )
    await app.waitFor(
      async () => (await p.task(2))?.round === 2 && (await p.task(2))?.state === 'done',
      120_000,
    )
    // One delivery to the lead: the result, with both verdicts written under
    // it as it goes; no note. The findings stay on the review tasks.
    await app.waitFor(async () => {
      const messages = await p.inbox('lead')
      return messages.some(
        (m) => m.kind === 'result' && m.taskNumber === 2 && m.state === 'delivered',
      )
    }, 60_000)
    assert.ok(
      !(await p.inbox('lead')).some((m) => m.kind === 'note' && /changes twice/.test(m.body)),
      'no note about the rounds',
    )
    // Each round ran in a session of its own, and both sessions stay for the human.
    const reviews = (await p.board()).lanes
      .filter((lane) => lane.participant.member === 'checker')
      .flatMap((lane) => lane.tasks)
    assert.deepEqual(
      reviews.map((t) => [t.kind, t.state, t.verdict]),
      [
        ['review', 'done', 'changes'],
        ['review', 'done', 'changes'],
      ],
    )
    for (const review of reviews) {
      const findings = (await p.task(review.number)).messages.find((m) => m.kind === 'result')
      assert.deepEqual([findings.state, findings.body], ['read', 'VERDICT: changes'])
    }
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
    assert.match(done.assignee, /^worker2-/, 'a session of the other worker')
    assert.match(
      done.body,
      /Reassigned from @worker-[a-z]+-[a-z]+, which ran out of quota after starting/,
    )
    // Quota is the member's: the member row says it is out, not a session.
    const out = (await p.board()).lanes.find((l) => l.participant.handle === 'worker').participant
    assert.ok(
      out.outUntil !== null && Date.parse(out.outUntil) > Date.now(),
      'the first worker is out',
    )
    assert.ok(
      (await p.inbox('lead')).some(
        (m) =>
          m.kind === 'note' &&
          /^T-2 was taken back from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\)/.test(
            m.body,
          ),
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

test('with human approval required, the brief and the result each wait for the human before they move', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    team(app)
    const p = await project(app, { review: 'none', gate: true, members: [['worker', 'worker']] })
    assert.equal((await p.board()).project.gate, true)
    const given = await app.requestNode('task.add', {
      project: p.id,
      to: 'lead',
      body: `DISPATCH --tier ${p.tiers.worker} Reply with exactly: WORKER_OK`,
    })
    assert.equal(given.ok, true, JSON.stringify(given))

    // The lead's brief is assigned, then held: no worker window opens for it.
    await app.waitFor(async () => (await p.board()).gated.length === 1, 60_000)
    const [brief] = (await p.board()).gated
    assert.deepEqual([brief.kind, brief.sender, brief.taskNumber], ['task', 'lead', 2])
    assert.equal((await p.task(2)).state, 'queued')
    await new Promise((resolve) => setTimeout(resolve, 1500))
    assert.equal(
      app.openFrames.some((frame) => frame.id === `p${p.id}-${brief.recipient}`),
      false,
      'nothing opened while the human had not approved',
    )
    const approved = await app.requestNode('message.approve', { message: brief.id })
    assert.equal(approved.ok, true, JSON.stringify(approved))
    await app.waitFor(async () => (await p.task(2))?.state === 'done', 60_000)

    // The result waits the same way; the lead's window gets nothing until it is passed on.
    await app.waitFor(async () => (await p.board()).gated.length === 1, 60_000)
    const [result] = (await p.board()).gated
    assert.deepEqual([result.kind, result.recipient, result.body], ['result', 'lead', 'WORKER_OK'])
    assert.deepEqual(
      (await p.inbox('lead')).filter((m) => m.kind === 'result'),
      [],
      "not in the lead's inbox yet",
    )
    await app.requestNode('message.approve', { message: result.id })
    await app.waitFor(async () => {
      const messages = await p.inbox('lead')
      return messages.some((m) => m.id === result.id && m.state === 'delivered')
    }, 60_000)
  } finally {
    await app.close()
  }
})
