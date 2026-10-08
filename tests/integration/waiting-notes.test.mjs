import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'
import { project, staff } from './staff.mjs'

/**
 * What the chief is told of a task that waits, end to end through the real pane
 * host (`npm run test:integration`), as the chief's report of 2026-10-08 had it
 * on a daemon of Node's: fake agents in real PTYs, a worker whose provider
 * refuses it, and a chief in the middle of a turn, so that what ConsensFlow
 * writes it waits behind the turn.
 * - m-1668: "T-357 was taken back from @ullr … waits for another standard
 *   worker" reached the chief after another worker had taken T-357 and started.
 * - m-1670 and its kin: "T-166 waits with @artemis … until 2026-10-13T08:00Z; it
 *   goes on by itself then" reached the chief after the human had logged the
 *   harness into another account and the daemon had resumed the task; and the
 *   chief it had reached believed the work stopped for five days.
 * The note a task's wait left is withdrawn if the chief has not been given it
 * when the task moves on, and the chief that was given the hold note is told the
 * task goes on.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/** A brief the fake worker's provider refuses (`QUOTA-OUT`), and answers when it does not. */
const REFUSED = 'QUOTA-OUT Reply with exactly: WORKER_OK'

/**
 * The account the fake worker's provider refuses while its file is there
 * (`CF_TEST_QUOTA_FILE`): `switched` removes it, the human logging the harness
 * into another account, after which the same brief is not refused.
 */
function refusingAccount() {
  const folder = mkdtempSync(join(tmpdir(), 'cf-account-'))
  const file = join(folder, 'refused')
  writeFileSync(file, '')
  return {
    file,
    switched: () => rmSync(file, { force: true }),
    gone: () => rmSync(folder, { recursive: true, force: true }),
  }
}

/** The messages of the chief's inbox that are notes of ConsensFlow's saying `pattern`, oldest first. */
async function notes(p, pattern) {
  return (await p.inbox('chief'))
    .filter((m) => m.kind === 'note' && m.sender === null && pattern.test(m.body))
    .sort((a, b) => a.id - b.id)
}

/** The chief's window, and the transcript of its conversation. */
async function chiefOf(app, p) {
  const frame = await app.openFrame(`p${p.id}-chief`)
  const session = frame.argv[frame.argv.indexOf('--session-id') + 1]
  return { transcript: () => app.transcript(session) }
}

/** Waits until the chief's turn ends. */
async function untilTheChiefIsIdle(app, p, timeoutMs = 90_000) {
  await app.waitFor(
    async () =>
      (await p.board()).lanes.find((lane) => lane.participant.handle === 'chief').activity.state ===
      'idle',
    timeoutMs,
  )
}

test('the note that a task was taken back is withdrawn when another worker takes it before the chief is given the note', async () => {
  const app = await startIntegration({
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
    const chief = await chiefOf(app, p)
    // The first worker sleeps six seconds on its brief and is then refused; the
    // chief's next turn is far longer than that.
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} SLEEP 6 ${REFUSED}`)
    await app.tell(p.id, 'SLEEP 40 Reply with exactly: CHIEF_BUSY')

    await app.waitFor(
      async () => (await notes(p, /^T-1 was taken back from /)).length === 1,
      60_000,
    )
    const [taken] = await notes(p, /^T-1 was taken back from /)
    assert.match(
      taken.body,
      /^T-1 was taken back from @worker-[a-z]+-[a-z]+ \(ran out of quota after starting\) and waits for another \w+ worker\.$/,
    )
    // The second worker takes the task while the chief is still in its turn.
    await app.waitFor(async () => /^worker2-/.test((await p.task(1)).assignee ?? ''), 30_000)
    const withdrawn = (await notes(p, /^T-1 was taken back from /))[0]
    assert.deepEqual(
      [withdrawn.id, withdrawn.state, withdrawn.reason],
      [taken.id, 'cancelled', 'T-1 was taken by @worker2'],
    )

    // The chief's turn ends and the worker's result reaches it; the note never does.
    await app.waitFor(async () => {
      const results = (await p.inbox('chief')).filter((m) => m.kind === 'result')
      return results.some((m) => m.body === 'WORKER_OK' && m.state === 'delivered')
    }, 120_000)
    const transcript = chief.transcript()
    assert.ok(!transcript.includes('was taken back'), 'the chief was never told the task waits')
    assert.ok(transcript.includes('WORKER_OK'), 'but it was given the result')
  } finally {
    await app.close()
  }
})

test('a chief given the note that a task is held is told it goes on when the account is switched', async () => {
  const account = refusingAccount()
  const app = await startIntegration({
    fakeEnv: {
      CF_TEST_HARNESS: FAKE_AGENT,
      CF_TEST_QUOTA_OUT: 'worker',
      CF_TEST_QUOTA_FILE: account.file,
    },
  })
  try {
    staff(app)
    const p = await project(app, { members: [['worker', 'worker']] })
    const chief = await chiefOf(app, p)
    // The only worker is refused with a reset hours away: its task is held with its window.
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} ${REFUSED}`)
    await app.waitFor(async () => (await notes(p, /^T-1 waits with /)).length === 1, 60_000)
    const [hold] = await notes(p, /^T-1 waits with /)
    assert.match(
      hold.body,
      /^T-1 waits with @worker-[a-z]+-[a-z]+: out of quota until \S+, or sooner if its account has quota again; it goes on by itself\.$/,
    )
    await app.waitFor(
      async () => (await notes(p, /^T-1 waits with /))[0].state === 'delivered',
      60_000,
    )
    assert.equal((await p.task(1)).state, 'paused')

    // The human logs the harness into another account and says so.
    account.switched()
    const back = await app.requestNode('member.back', { project: p.id, participant: 'worker' })
    assert.equal(back.ok, true, JSON.stringify(back))
    await app.waitFor(async () => (await notes(p, /^T-1 goes on:/)).length === 1, 30_000)
    const [goes] = await notes(p, /^T-1 goes on:/)
    assert.equal(goes.body, 'T-1 goes on: its account has quota again.')
    assert.equal(goes.taskNumber, 1)
    await untilTheChiefIsIdle(app, p)
    await app.waitFor(
      async () => (await notes(p, /^T-1 goes on:/))[0].state === 'delivered',
      30_000,
    )
    assert.ok(chief.transcript().includes('T-1 goes on: its account has quota again.'))
    const held = await notes(p, /^T-1 waits with /)
    assert.deepEqual(
      held.map((note) => note.state),
      ['delivered'],
      'what the chief was given stays, and the task was held once',
    )
  } finally {
    await app.close()
    account.gone()
  }
})

test('the note that a task is held is withdrawn when the account is switched before the chief is given it, and nothing is said after', async () => {
  const account = refusingAccount()
  const app = await startIntegration({
    fakeEnv: {
      CF_TEST_HARNESS: FAKE_AGENT,
      CF_TEST_QUOTA_OUT: 'worker',
      CF_TEST_QUOTA_FILE: account.file,
    },
  })
  try {
    staff(app)
    const p = await project(app, { members: [['worker', 'worker']] })
    const chief = await chiefOf(app, p)
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} ${REFUSED}`)
    await app.tell(p.id, 'SLEEP 40 Reply with exactly: CHIEF_BUSY')
    await app.waitFor(async () => (await notes(p, /^T-1 waits with /)).length === 1, 60_000)
    const [hold] = await notes(p, /^T-1 waits with /)
    assert.equal(hold.state, 'queued', 'the chief is in its turn')

    account.switched()
    const back = await app.requestNode('member.back', { project: p.id, participant: 'worker' })
    assert.equal(back.ok, true, JSON.stringify(back))
    await app.waitFor(async () => (await p.task(1)).state !== 'paused', 30_000)
    const [withdrawn] = await notes(p, /^T-1 waits with /)
    assert.deepEqual(
      [withdrawn.id, withdrawn.state, withdrawn.reason],
      [hold.id, 'cancelled', 'T-1 resumed'],
    )
    assert.deepEqual(await notes(p, /^T-1 goes on:/), [], 'nobody was told it waits')

    await untilTheChiefIsIdle(app, p)
    await new Promise((resolve) => setTimeout(resolve, 3_000))
    const transcript = chief.transcript()
    const inbox = (await p.inbox('chief')).map((m) => [m.id, m.kind, m.state, m.reason, m.body])
    assert.ok(
      !transcript.includes('waits with'),
      `the chief never believed it waited five days: ${JSON.stringify(inbox)}`,
    )
    assert.ok(!transcript.includes('goes on:'))
    assert.equal((await notes(p, /^T-1 waits with /)).length, 1, 'the task was held once')
  } finally {
    await app.close()
    account.gone()
  }
})
