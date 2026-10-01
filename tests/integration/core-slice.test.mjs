import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { startIntegration } from './harness.mjs'

/**
 * The new core end to end (TEST-BDC-09 through the real pane host): its daemon,
 * the real Rust headless bridge and PTYs, and fake Claude agents in them. The
 * human gives the chief a task on the board; the chief hands part of it to a
 * worker's tier with `cf task add`; the core picks the worker and opens its window with the task,
 * collects the worker's answer as the result and delivers it into the chief's
 * window, where the chief's own transcript shows it arrived.
 */

const CORE_EDITOR = fileURLToPath(new URL('./core-editor.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

test('a chief hands a task to a worker through the board and the result lands in its window', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      harness: 'claude-code',
    })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const project = opened.project.id
    const added = await app.requestNode('member.add', { project, agent: 'worker' })
    assert.equal(added.ok, true, JSON.stringify(added))
    const leadFrame = app.openFrames.find((frame) => frame.id === `p${project}-chief`)
    assert.ok(leadFrame, 'the chief window opened')

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
    const leadSession = leadFrame.argv[leadFrame.argv.indexOf('--session-id') + 1]
    assert.match(
      app.transcript(leadSession),
      new RegExp(`\\[ConsensFlow m-${result.id} · T-1 · result from @worker-[a-z]+-[a-z]+\\]`),
    )
  } finally {
    await app.close()
  }
})

test('a chief with unsent text in its terminal still gets its result, behind that text', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      harness: 'claude-code',
    })
    const project = opened.project.id
    const { member } = await app.requestNode('member.add', { project, agent: 'worker' })
    const chief = app.openFrames.find((frame) => frame.id === `p${project}-chief`)

    // The human dispatches from the chief's own terminal, then types more and
    // leaves it unsent. Nothing waits for that text (the owner's choice,
    // 2026-10-01): the result is pasted in behind it and both go in together.
    await app.waitFor(async () => {
      const { board } = await app.requestNode('board.get', { project })
      return board.lanes.find((l) => l.participant.handle === 'chief').activity.state === 'idle'
    })
    for (const text of [
      `DISPATCH --tier ${member.tier} Reply with exactly: TYPED_OK\r`,
      'half a thought',
    ]) {
      const typed = await app.requestRust('pane.input', {
        id: chief.id,
        generation: chief.generation,
        bytes: [...Buffer.from(text)],
      })
      assert.equal(typed.ok, true, JSON.stringify(typed))
    }

    await app.waitFor(async () => {
      const { messages } = await app.requestNode('inbox.get', { project, participant: 'chief' })
      return messages.some((m) => m.kind === 'result' && m.state === 'delivered')
    }, 30_000)
    const { messages } = await app.requestNode('inbox.get', { project, participant: 'chief' })
    const result = messages.find((m) => m.kind === 'result')
    assert.equal(result.body, 'TYPED_OK')
    const session = chief.argv[chief.argv.indexOf('--session-id') + 1]
    assert.match(
      app.transcript(session),
      new RegExp(`half a thought\\[ConsensFlow m-${result.id} · T-1 · result from @worker-`),
    )
  } finally {
    await app.close()
  }
})

test('one member runs two tasks at once, each in a session and window of its own', async () => {
  const app = await startIntegration({
    editor: CORE_EDITOR,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      harness: 'claude-code',
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
