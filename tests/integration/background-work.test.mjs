import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'
import { project, staff } from './staff.mjs'

/**
 * A worker's background work outlives its turn (the report of 2026-10-09): T-183's
 * worker started a recording run under a lock tool in the background, armed its
 * harness's monitor and ended its turn. ConsensFlow took the turn's end for the
 * result, marked the task done and closed the window, and the job died with it,
 * killed outright (no RELEASED), and the monitor never woke the worker. End to end
 * through the real pane host (`npm run test:integration`): fake agents in real
 * PTYs, and a job of the fake agent's (`background-job.mjs`) that writes RELEASED
 * when it is asked to end and FINISHED when it ends by itself.
 *
 * - A turn that ends while the job it started runs is not the task's result: the
 *   board says the window works, and the job runs on until it ends; the turn the
 *   job's end wakes is the one whose end is.
 * - Work that was there before the turn (a server the harness started with its
 *   window) is not the turn's.
 * - A window that closes with the job running (a cancel, a pause) asks it to end
 *   first, and whoever gave the task is told the job was running.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/** How long the daemon waits, once the job has ended, for the agent to go on. */
const SETTLE_MS = '1500'

/** The files of the jobs the fake windows started under `label`: `<label>-<window's pid>-<n>`. */
function jobs(app, label) {
  const folder = join(app.root, 'jobs')
  if (!existsSync(folder)) return []
  return readdirSync(folder)
    .filter((name) => name.startsWith(`${label}-`) && name.endsWith('.pid'))
    .map((name) => join(folder, name.slice(0, -'.pid'.length)))
}

const pidOf = (job) => Number(readFileSync(`${job}.pid`, 'utf8'))

/** A project with one worker, given the brief `words` through the chief; resolves once its job runs. */
async function aWorkerWithAJob(app, words) {
  staff(app)
  const p = await project(app, { members: [['worker', 'worker']] })
  await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} ${words}`)
  await app.waitFor(() => jobs(app, 'job').length === 1, 60_000)
  const [job] = jobs(app, 'job')
  return { p, job }
}

/**
 * Waits until the worker's turn is over with its job started: the task has its
 * result, or the window says why it has not.
 */
async function untilTheTurnEnded(app, p) {
  await app.waitFor(async () => {
    const task = await p.task(1)
    return (
      ['done', 'failed', 'cancelled'].includes(task?.state) ||
      (await p.lane('worker'))?.activity?.reason !== undefined
    )
  }, 60_000)
}

test('a turn that ends with the job it started running is not the result: the job ends, and the turn it wakes is', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CONSENSFLOW_BACKGROUND_SETTLE_MS: SETTLE_MS },
  })
  try {
    const { p, job } = await aWorkerWithAJob(app, 'BACKGROUND --wake 12 started the job')
    await untilTheTurnEnded(app, p)

    assert.equal(
      (await p.task(1)).state,
      'working',
      'the turn ended with the job running: the task has no result yet',
    )
    assert.deepEqual((await p.lane('worker')).activity, {
      state: 'working',
      reason: 'background work running (1 process)',
    })
    assert.equal(app.pidAlive(pidOf(job)), true, 'the job runs on')
    assert.deepEqual(
      (await p.inbox('chief')).filter((message) => message.kind === 'result'),
      [],
      'the chief is given no result',
    )

    // The job ends by itself, the agent is woken, and its turn's end is the result.
    await app.waitFor(async () => (await p.task(1)).state === 'done', 60_000)
    assert.equal(existsSync(`${job}.finished`), true, 'the job ran to its end')
    assert.equal(existsSync(`${job}.released`), false, 'and was never asked to end')
    await app.waitFor(
      async () => (await p.inbox('chief')).some((message) => message.kind === 'result'),
      30_000,
    )
    const result = (await p.inbox('chief')).find((message) => message.kind === 'result')
    assert.equal(result.body, 'started the job\n\nwoken: started the job')
  } finally {
    await app.close()
  }
})

test('a job that ends with no turn to wake leaves the last turn’s end as the result, once the agent has had a moment', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CONSENSFLOW_BACKGROUND_SETTLE_MS: SETTLE_MS },
  })
  try {
    const { p, job } = await aWorkerWithAJob(app, 'BACKGROUND 6 started the job')
    await untilTheTurnEnded(app, p)
    assert.equal((await p.task(1)).state, 'working', 'the job runs, and the task has no result yet')

    await app.waitFor(async () => (await p.task(1)).state === 'done', 60_000)
    assert.equal(existsSync(`${job}.finished`), true, 'the job ran to its end')
    assert.equal(existsSync(`${job}.released`), false)
    await app.waitFor(
      async () => (await p.inbox('chief')).some((message) => message.kind === 'result'),
      30_000,
    )
    assert.equal(
      (await p.inbox('chief')).find((message) => message.kind === 'result').body,
      'started the job',
    )
  } finally {
    await app.close()
  }
})

test('work the window had before its turn is not the turn’s: a server the harness started with it holds nothing', async () => {
  const app = await startIntegration({
    fakeEnv: {
      CF_TEST_HARNESS: FAKE_AGENT,
      CF_TEST_MCP_SERVER: '1',
      CONSENSFLOW_BACKGROUND_SETTLE_MS: SETTLE_MS,
    },
  })
  try {
    staff(app)
    const p = await project(app, { members: [['worker', 'worker']] })
    // A turn of a few seconds: the daemon has marked where it began when it ends.
    await app.tell(p.id, `DISPATCH --tier ${p.tiers.worker} SLEEP 3 Reply with exactly: WORKER_OK`)
    await app.waitFor(async () => (await p.task(1))?.state === 'done', 60_000)
    assert.ok(jobs(app, 'server').length >= 2, 'the chief and the worker each had a server')
    await app.waitFor(
      async () => (await p.inbox('chief')).some((message) => message.kind === 'result'),
      30_000,
    )
    assert.equal(
      (await p.inbox('chief')).find((message) => message.kind === 'result').body,
      'WORKER_OK',
    )
  } finally {
    await app.close()
  }
})

test('cancelling a task whose job still runs asks the job to end first, and tells whoever gave the task', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CONSENSFLOW_BACKGROUND_SETTLE_MS: SETTLE_MS },
  })
  try {
    const { p, job } = await aWorkerWithAJob(app, 'BACKGROUND 120 started the job')
    await untilTheTurnEnded(app, p)
    assert.equal((await p.task(1)).state, 'working')

    const stopped = await app.requestNode('task.cancel', { project: p.id, task: 1 })
    assert.equal(stopped.ok, true, JSON.stringify(stopped))

    // The window closed with the job asked to end, not killed: it wrote RELEASED.
    await app.waitFor(() => existsSync(`${job}.released`), 30_000)
    assert.equal(existsSync(`${job}.finished`), false)
    await app.waitFor(() => !app.pidAlive(pidOf(job)), 30_000)

    const told = async () =>
      (await p.inbox('chief')).filter(
        (message) =>
          message.kind === 'note' &&
          /^T-1's window closed with background work still running: /.test(message.body),
      )
    await app.waitFor(async () => (await told()).length === 1, 30_000)
    assert.match((await told())[0].body, /background-job\.mjs/)
  } finally {
    await app.close()
  }
})

test('pausing a task whose job still runs leaves its window and the job as they are', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CONSENSFLOW_BACKGROUND_SETTLE_MS: SETTLE_MS },
  })
  try {
    const { p, job } = await aWorkerWithAJob(app, 'BACKGROUND 120 started the job')
    await untilTheTurnEnded(app, p)

    const paused = await app.requestNode('task.pause', { project: p.id, task: 1 })
    assert.equal(paused.ok, true, JSON.stringify(paused))
    await app.waitFor(async () => (await p.task(1)).state === 'paused', 30_000)

    // A paused task goes on in its window: nothing was closed, nothing asked to end.
    await new Promise((resolve) => setTimeout(resolve, 3_000))
    assert.notEqual((await p.lane('worker')).pane, null, 'the window is open')
    assert.equal(app.pidAlive(pidOf(job)), true, 'the job runs on')
    assert.equal(existsSync(`${job}.released`), false, 'and was not asked to end')
  } finally {
    await app.close()
  }
})

test('a job that ignores the signals that ask it to end is killed once the grace of ten seconds is over', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT, CONSENSFLOW_BACKGROUND_SETTLE_MS: SETTLE_MS },
  })
  try {
    const { p, job } = await aWorkerWithAJob(app, 'BACKGROUND --deaf 120 started the job')
    await untilTheTurnEnded(app, p)
    const closing = Date.now()
    assert.equal((await app.requestNode('task.cancel', { project: p.id, task: 1 })).ok, true)
    await app.waitFor(() => !app.pidAlive(pidOf(job)), 30_000)
    assert.ok(Date.now() - closing >= 9_000, 'the job was given its grace')
    assert.equal(existsSync(`${job}.released`), false, 'and was killed, not asked')
  } finally {
    await app.close()
  }
})
