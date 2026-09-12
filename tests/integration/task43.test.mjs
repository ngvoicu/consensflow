import assert from 'node:assert/strict'
import { existsSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { startIntegration } from './harness.mjs'

test('page requests enter Node while native requests enter Rust with separate ids', async () => {
  const app = await startIntegration()
  try {
    const page = await app.requestNode('state.list', {})
    assert.equal(page.ok, true, JSON.stringify(page))
    assert.equal(page.available, true)
    const native = await app.requestRust('pane.list', {})
    assert.deepEqual(native, { ok: true, panes: [] })
    assert.equal(
      app.nodeFrames.some((frame) => frame.kind === 'res' && frame.id.startsWith('r-test-')),
      true,
    )
    assert.equal(
      app.rustFrames.some((frame) => frame.kind === 'res' && frame.id.startsWith('n-test-')),
      true,
    )
  } finally {
    await app.close()
  }
})

test('real cf ui and the headless bridge open a PTY through the production path', async () => {
  const app = await startIntegration()
  try {
    // The real UI endpoint is the page request origin. It invokes the same
    // Node tab.open handler the desktop command sends over the bridge, while
    // Rust remains the real PTY host and returns the pane.open response.
    const opened = await app.openTab()
    assert.equal(opened.tab.id, 't-1')
    assert.equal(opened.tab.closed, false)
    assert.equal(app.openFrames.length, 1)
    assert.equal(app.openFrames[0].cwd, app.workspace)
    assert.ok(app.openFrames[0].id)
    assert.ok(app.openFrames[0].generation >= 1)
    assert.equal(app.openFrames[0].launch.length > 0, true)

    const runPromise = app.runCli(
      ['run', '@worker', 'CF_DELAYED complete from a real PTY', '--new', '--json'],
      opened.leadEnv,
    )
    const readyFile = join(app.env.CONSENSFLOW_HOME, 'worker-ready')
    await app.waitFor(() => existsSync(readyFile))
    let conversation
    await app.waitFor(() => {
      conversation = Object.entries(app.threads()).find(
        ([, row]) => row.agent === 'worker' && typeof row.sessionId === 'string',
      )?.[0]
      return typeof conversation === 'string'
    })
    const initialUnread = await app.runCli(['results', conversation, '--json'], opened.leadEnv)
    assert.equal(initialUnread.code, 0, initialUnread.stderr)
    assert.deepEqual(JSON.parse(initialUnread.stdout).workers[0].results, [])
    writeFileSync(join(app.env.CONSENSFLOW_HOME, 'worker-release'), 'release\n')
    const run = await runPromise
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    assert.equal(answer.outcome, 'opened', run.stdout)
    assert.equal(typeof answer.conversation, 'string')

    await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
    const workerSession = app.threads()[answer.conversation].sessionId
    await app.waitFor(() =>
      app
        .processes()
        .some(({ pid, sessionId }) => sessionId === workerSession && !app.pidAlive(pid)),
    )
    await app.waitFor(() =>
      app.transcript(workerSession).includes('worker completed from a real PTY child'),
    )
    await app.waitFor(() =>
      app
        .deliveries()
        .some(
          (result) => result.conversation === answer.conversation && result.state === 'received',
        ),
    )
    const result = app.deliveries().find((result) => result.conversation === answer.conversation)
    const nativeLead = app.openFrames[0].argv[app.openFrames[0].argv.indexOf('--session-id') + 1]
    assert.ok(app.transcript(nativeLead).includes(result.answer))
    assert.equal(
      app.nodeFrames.some((frame) => frame.op === 'pane.write_paste'),
      false,
    )
  } finally {
    await app.close()
  }
})

for (const fault of ['tail-only', 'body-loss', 'no-receipt']) {
  test(`native incomplete receipt stays unconfirmed without replay: ${fault}`, async () => {
    const app = await startIntegration({ fakeEnv: { CF_INTEGRATION_READ_FAULT: fault } })
    try {
      const opened = await app.openTab()
      assert.equal(
        (await app.runCli(['run', '@worker', 'answer', '--new'], opened.leadEnv)).code,
        0,
      )
      await app.waitFor(() =>
        app.deliveries().some((r) => r.claims.some((c) => c.state === 'submitting')),
      )
      const result = app.deliveries()[0]
      assert.notEqual(result.state, 'received')
      await new Promise((resolve) => setTimeout(resolve, 1700))
      const later = app.deliveries().find((r) => r.id === result.id)
      assert.equal(later.claims.length, 1)
      assert.notEqual(later.state, 'received')
      assert.equal(
        app.nodeFrames.some((frame) => frame.op === 'pane.write_paste'),
        false,
      )
    } finally {
      await app.close()
    }
  })
}

test('all worker replies and multipart answers pass through the durable receiver inbox', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const started = await app.runCli(
      ['run', '@worker', 'CF_HOLD first', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(started.code, 0, started.stderr)
    const conversation = JSON.parse(started.stdout).conversation
    const row = () => app.threads()[conversation]
    await app.waitFor(() => row()?.sessionId)
    const worker = JSON.parse(started.stdout).pane
    for (let n = 0; n < 4; n++) {
      await app.requestRust('pane.input', {
        id: worker.id,
        generation: worker.generation,
        bytes: [...Buffer.from(`follow up ${n}\r`)],
      })
    }
    await app.waitFor(
      () =>
        app.deliveries().filter((r) => r.conversation === conversation && r.state === 'received')
          .length === 5,
      15000,
    )
    const big = await app.runCli(['run', '@worker', 'CF_LARGE', '--new', '--json'], opened.leadEnv)
    assert.equal(big.code, 0, big.stderr)
    const largeName = JSON.parse(big.stdout).conversation
    await app.waitFor(
      () => app.deliveries().some((r) => r.conversation === largeName && r.state === 'received'),
      20000,
    )
    const result = app.deliveries().find((r) => r.conversation === largeName)
    assert.equal(result.answer.length, 60000)
    assert.ok(result.parts.length > 1)
    assert.equal(
      new Set(result.claims.filter((c) => c.state === 'received').map((c) => c.part)).size,
      result.parts.length,
    )
    assert.equal(
      app.nodeFrames.some((frame) => frame.op === 'pane.write_paste'),
      false,
    )
  } finally {
    await app.close()
  }
})
