import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * ConsensFlow end to end (TEST-BDC-09 through the real pane host): the daemon,
 * the real Rust headless bridge and PTYs, and fake Claude agents in them. The
 * human gives the chief a task on the board; the chief hands part of it to a
 * worker's tier with `cf task add`; the daemon picks the worker and opens its window with the task,
 * collects the worker's answer as the result and delivers it into the chief's
 * window, where the chief's own transcript shows it arrived.
 */

const DAEMON = fileURLToPath(new URL('./core-daemon.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))
const CF = fileURLToPath(new URL('../../bin/cf.mjs', import.meta.url))

test('a chief hands a task to a worker through the board and the result lands in its window', async () => {
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
    const chiefFrame = await app.openFrame(`p${project}-chief`)

    await app.tell(project, `DISPATCH --tier ${added.member.tier} Reply with exactly: WORKER_OK`)

    const board = async () => (await app.requestNode('board.get', { project })).board
    // A member's work runs in a session of its own: its lane is the session's.
    const lane = async (handle) =>
      (await board()).lanes.findLast(
        (candidate) =>
          candidate.participant.member === handle || candidate.participant.handle === handle,
      )
    await app.waitFor(async () => (await lane('worker'))?.tasks[0]?.state === 'done', 30_000)
    const workerTask = (await lane('worker')).tasks[0]
    assert.deepEqual([workerTask.requester, workerTask.number], ['chief', 1])

    const workerFrame = app.openFrames.find(
      (frame) => frame.id === `p${project}-${workerTask.assignee}`,
    )
    assert.match(
      workerFrame.argv.at(-1),
      /^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nReply with exactly: WORKER_OK$/,
    )
    assert.ok(workerFrame.argv.includes('bypassPermissions'))

    await app.waitFor(async () => {
      const { messages } = await app.requestNode('inbox.get', { project, participant: 'chief' })
      return messages.some((m) => m.kind === 'result' && m.state === 'delivered')
    }, 30_000)
    const { messages } = await app.requestNode('inbox.get', { project, participant: 'chief' })
    const result = messages.find((m) => m.kind === 'result')
    assert.equal(result.body, 'WORKER_OK')
    // Every move also went to the home's event file as it happened, for
    // whoever watches the daemon from outside.
    const events = (await readFile(join(app.env.CONSENSFLOW_HOME, 'events.jsonl'), 'utf8'))
      .trim()
      .split('\n')
      .map((line) => JSON.parse(line))
    assert.ok(events.some((e) => e.kind === 'task.opened' && e.data.task === 1))
    assert.ok(events.some((e) => e.kind === 'task.state' && e.data.to === 'done'))
    assert.ok(events.some((e) => e.kind === 'window.activity' && e.participant === 'chief'))
    // The chief's native session is the `--session-id` its window was launched with.
    const chiefSession = chiefFrame.argv[chiefFrame.argv.indexOf('--session-id') + 1]
    assert.match(
      app.transcript(chiefSession),
      new RegExp(`\\[ConsensFlow m-${result.id} · T-1 · result from @worker-[a-z]+-[a-z]+\\]`),
    )
  } finally {
    await app.close()
  }
})

test('one member runs two tasks at once, each in a session and window of its own', async () => {
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
    for (const word of ['ONE', 'TWO']) {
      await app.tell(project, `DISPATCH --tier ${added.member.tier} Reply with exactly: ${word}`)
    }
    const board = async () => (await app.requestNode('board.get', { project })).board
    const dispatched = async () =>
      (await board()).lanes
        .filter((lane) => lane.participant.member === 'worker')
        .flatMap((lane) => lane.tasks)
    await app.waitFor(
      async () => (await dispatched()).filter((t) => t.state === 'done').length === 2,
      60_000,
    )
    const sessions = (await dispatched()).map((t) => t.assignee)
    assert.equal(new Set(sessions).size, 2, `two sessions: ${sessions}`)
    for (const handle of sessions) assert.match(handle, /^worker-[a-z]+-[a-z]+$/)
    const windows = app.openFrames.filter((frame) => frame.id.startsWith(`p${project}-worker-`))
    assert.deepEqual(
      windows.map((frame) => frame.id).sort(),
      sessions.map((handle) => `p${project}-${handle}`).sort(),
      'each session had a window of its own',
    )
  } finally {
    await app.close()
  }
})

test('switching the chief opens a fresh window that gets the handoff and reads what the old one was told', async () => {
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      agent: 'chief',
    })
    const project = opened.project.id
    const chiefs = () => app.openFrames.filter((frame) => frame.id === `p${project}-chief`)
    const sessionOf = (frame) => frame.argv[frame.argv.indexOf('--session-id') + 1]
    const first = await app.openFrame(`p${project}-chief`)
    const modelOf = (frame) => frame.argv[frame.argv.indexOf('--model') + 1]
    assert.equal(modelOf(first), 'fake-chief', "the first chief runs on its agent's model")
    await app.waitFor(async () => {
      const { board } = await app.requestNode('board.get', { project })
      return board.lanes.find((l) => l.participant.handle === 'chief').activity.state === 'idle'
    })
    // The human tells the first chief something only it knows.
    const typed = await app.requestRust('pane.input', {
      id: first.id,
      generation: first.generation,
      bytes: [...Buffer.from('Reply with exactly: TERN-7314\r')],
    })
    assert.equal(typed.ok, true, JSON.stringify(typed))
    await app.waitFor(() => app.transcript(sessionOf(first)).includes('"text":"TERN-7314"'))

    const switched = await app.requestNode('chief.switch', { project, agent: 'worker' })
    assert.equal(switched.ok, true, JSON.stringify(switched))
    await app.waitFor(() => chiefs().length === 2)
    const second = chiefs()[1]
    assert.notEqual(sessionOf(second), sessionOf(first), 'every switch starts fresh')
    assert.equal(modelOf(second), 'fake', "the agent's model")
    await app.waitFor(
      () => app.transcript(sessionOf(second)).includes('You are the chief now'),
      30_000,
    )
    const { board } = await app.requestNode('board.get', { project })
    assert.equal(board.project.state, 'open', 'a switch is not a close')

    // The new chief reads, with its own token, what the human told the old one.
    const history = execFileSync(process.execPath, [CF, 'history'], {
      env: {
        ...app.env,
        CONSENSFLOW_URL: second.env.CONSENSFLOW_URL,
        CONSENSFLOW_TOKEN: second.env.CONSENSFLOW_TOKEN,
      },
      encoding: 'utf8',
    })
    assert.match(history, /Human: Reply with exactly: TERN-7314/)
    assert.match(history, /Claude Code chief: TERN-7314/)
    assert.ok(!history.includes('[ConsensFlow m-'), 'no page can prove a delivery arrived')
  } finally {
    await app.close()
  }
})
