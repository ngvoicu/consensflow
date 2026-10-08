import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { START_WORDS } from '../choice.mjs'
import { startIntegration } from '../integration/harness.mjs'

/**
 * The daemon under load, opt-in (`npm run load`): several projects at once,
 * each chief handing out waves of tasks to fake workers in real PTYs through
 * the real pane host, while the page polls every board, task and transcript
 * the whole time. At the end the daemon is still there, every task is done
 * and its result delivered, its log holds no error and no slow pass, and the
 * process did not grow past bounds. The size scales with
 * CONSENSFLOW_LOAD_PROJECTS, CONSENSFLOW_LOAD_WAVES and CONSENSFLOW_LOAD_TASKS
 * (tasks per wave per project).
 */

const FAKE_AGENT = fileURLToPath(new URL('../integration/fake-agent.mjs', import.meta.url))
const ENABLED = process.env.CONSENSFLOW_LOAD === '1'
const PROJECTS = Number(process.env.CONSENSFLOW_LOAD_PROJECTS ?? 3)
const WAVES = Number(process.env.CONSENSFLOW_LOAD_WAVES ?? 3)
const TASKS = Number(process.env.CONSENSFLOW_LOAD_TASKS ?? 3)
const WORKERS = 3
const POLL_MS = 50
const RSS_LIMIT_MB = 512

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The daemon's resident memory in MB, as the system counts it. */
function residentMb(pid) {
  const kb = execFileSync('ps', ['-o', 'rss=', '-p', String(pid)], { encoding: 'utf8' }).trim()
  return Math.round(Number(kb) / 1024)
}

test('the daemon stays up, delivers every task and logs nothing wrong while several projects work at once', {
  skip: ENABLED ? false : 'set CONSENSFLOW_LOAD=1 (npm run load)',
}, async () => {
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const workers = Array.from({ length: WORKERS }, (_, i) => `worker${i + 1}`)
    writeFileSync(
      join(app.env.CONSENSFLOW_HOME, 'agents.json'),
      `${JSON.stringify({
        schemaVersion: 1,
        agents: ['chief', ...workers].map((id) => ({ id, kind: 'claude-code', model: 'fake' })),
      })}\n`,
    )
    const projects = []
    for (let i = 0; i < PROJECTS; i += 1) {
      const directory = join(app.workspace, `project-${i + 1}`)
      mkdirSync(directory, { recursive: true })
      const opened = await app.requestNode('project.open', {
        directory,
        agent: 'chief',
        staff: workers.map((agent) => ({ agent, roles: ['worker'] })),
      })
      assert.equal(opened.ok, true, JSON.stringify(opened))
      const board = (await app.requestNode('board.get', { project: opened.project.id })).board
      const tier = board.lanes.find((lane) => lane.participant.agent === workers[0]).participant
        .tier
      projects.push({ id: opened.project.id, tier })
    }
    const board = async (id) => (await app.requestNode('board.get', { project: id })).board
    const delivered = async (id) =>
      (await app.requestNode('inbox.get', { project: id, participant: 'chief' })).messages.filter(
        (m) => m.kind === 'result' && m.state === 'delivered',
      ).length

    // The page, reading everything all the time.
    let polling = true
    let polls = 0
    const pollFailures = []
    const poller = (async () => {
      while (polling) {
        for (const { id } of projects) {
          try {
            const lanes = (await board(id)).lanes
            const tasks = lanes.flatMap((lane) => lane.tasks)
            for (const task of tasks.slice(-2)) {
              await app.requestNode('task.get', { project: id, task: task.number })
              await app.requestNode('task.transcript', { project: id, task: task.number })
            }
            polls += 1
          } catch (cause) {
            pollFailures.push(String(cause))
          }
        }
        await sleep(POLL_MS)
      }
    })()

    const started = Date.now()
    for (let wave = 0; wave < WAVES; wave += 1) {
      await Promise.all(
        projects.map(async ({ id, tier }) => {
          for (let n = 0; n < TASKS; n += 1) {
            await app.tell(
              id,
              `DISPATCH --tier ${tier} Reply with exactly: OK-${wave + 1}-${n + 1}`,
            )
          }
        }),
      )
      const expected = (wave + 1) * TASKS
      await app.waitFor(async () => {
        for (const { id } of projects) if ((await delivered(id)) < expected) return false
        return true
      }, 300_000)
    }
    const seconds = Math.round((Date.now() - started) / 1000)
    polling = false
    await poller

    assert.equal(app.daemonExited(), false, 'the daemon is still running')
    assert.deepEqual(pollFailures, [], 'every poll was answered')
    for (const { id } of projects) {
      const tasks = (await board(id)).lanes.flatMap((lane) => lane.tasks)
      assert.equal(tasks.length, WAVES * TASKS, `project ${id} has every task`)
      assert.deepEqual(
        tasks.filter((t) => t.state !== 'done').map((t) => `T-${t.number} ${t.state}`),
        [],
        `project ${id}: every task is done`,
      )
    }
    const log = readFileSync(join(app.env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8')
      .split('\n')
      .filter(Boolean)
    // Under the daemon the run chose (CONSENSFLOW_TEST_DAEMON), whichever it is.
    assert.match(
      log[0],
      new RegExp(`^\\S+ info start pid \\d+ ${START_WORDS[app.daemon.kind]}`),
      `the ${app.daemon.kind} daemon was under load`,
    )
    assert.deepEqual(
      log.filter((line) => / (error|warn) /.test(line)),
      [],
      'no error and no slow pass under load',
    )
    const rss = residentMb(app.daemonPid())
    assert.ok(rss < RSS_LIMIT_MB, `the daemon is ${rss} MB, past ${RSS_LIMIT_MB}`)
    console.log(
      `load: ${PROJECTS} projects × ${WAVES} waves × ${TASKS} tasks = ${PROJECTS * WAVES * TASKS} tasks in ${seconds} s, ${polls} board polls, daemon at ${rss} MB`,
    )
  } finally {
    await app.close()
  }
})
