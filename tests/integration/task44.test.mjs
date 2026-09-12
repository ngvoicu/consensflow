import assert from 'node:assert/strict'
import { mkdirSync, renameSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { startIntegration } from './harness.mjs'

test('worker harness starts when the desktop has only the Finder PATH (TEST-PANE-59)', async () => {
  const app = await startIntegration({ bridgeEnv: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin' } })
  try {
    const opened = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'greet the lead', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const { conversation, pane } = JSON.parse(run.stdout)
    const output = () =>
      app.rustFrames
        .filter((frame) => frame.op === 'pane.output' && frame.body.id === pane.id)
        .map((frame) => Buffer.from(frame.body.bytes).toString('utf8'))
        .join('')
    await app.waitFor(() => {
      const session = app.threads()[conversation]?.sessionId
      return (
        output().includes('could not be started') ||
        (typeof session === 'string' &&
          app.transcript(session).includes('worker completed from a real PTY child'))
      )
    })
    assert.ok(
      app
        .transcript(app.threads()[conversation]?.sessionId)
        .includes('worker completed from a real PTY child'),
      output(),
    )
  } finally {
    await app.close()
  }
})

test('two same-directory leads keep concurrent consults on distinct bindings', async () => {
  const app = await startIntegration()
  try {
    const [first, second] = await Promise.all([app.openTab(), app.openTab()])
    const runs = await Promise.all([
      app.runCli(['run', '@worker', 'same task from both leads', '--new', '--json'], first.leadEnv),
      app.runCli(
        ['run', '@worker', 'same task from both leads', '--new', '--json'],
        second.leadEnv,
      ),
    ])
    for (const run of runs) assert.equal(run.code, 0, run.stderr)

    const answers = runs.map((run) => JSON.parse(run.stdout))
    assert.notEqual(answers[0].conversation, answers[1].conversation)
    assert.equal(
      app.openFrames.filter((frame) => frame.argv.includes('--in-pane')).length,
      2,
      'one real worker PTY per concurrent consult',
    )

    const rows = app.threads()
    assert.equal(rows[answers[0].conversation].lead, `tab:${first.tab.id}:1`)
    assert.equal(rows[answers[1].conversation].lead, `tab:${second.tab.id}:1`)
    for (const answer of answers) {
      await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
      const session = app.threads()[answer.conversation].sessionId
      await app.waitFor(() =>
        app.transcript(session).includes('worker completed from a real PTY child'),
      )
    }
  } finally {
    await app.close()
  }
})

test('two concurrent --new runs under one lead keep separate worker bindings', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const runs = await Promise.all([
      app.runCli(['run', '@worker', 'same lead first task', '--new', '--json'], opened.leadEnv),
      app.runCli(['run', '@worker', 'same lead second task', '--new', '--json'], opened.leadEnv),
    ])
    for (const run of runs) assert.equal(run.code, 0, run.stderr)
    const answers = runs.map((run) => JSON.parse(run.stdout))
    assert.notEqual(answers[0].conversation, answers[1].conversation)
    assert.equal(
      app.openFrames.filter((frame) => frame.argv.includes('--in-pane')).length,
      2,
      'two --new runs under one lead still launch two real worker PTYs',
    )
    const rows = app.threads()
    assert.equal(rows[answers[0].conversation].lead, `tab:${opened.tab.id}:1`)
    assert.equal(rows[answers[1].conversation].lead, `tab:${opened.tab.id}:1`)
    for (const answer of answers) {
      await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
      const session = app.threads()[answer.conversation].sessionId
      await app.waitFor(() =>
        app.transcript(session).includes('worker completed from a real PTY child'),
      )
    }
  } finally {
    await app.close()
  }
})

test('a live worker continues in its existing PTY without a second launch', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'CF_HOLD live worker', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    const opensBeforeSay = app.openFrames.filter((frame) => frame.argv.includes('--in-pane')).length

    const said = await app.runCli(
      ['say', answer.conversation, 'follow up on the same live pane'],
      opened.leadEnv,
    )
    assert.equal(said.code, 0, said.stderr)
    assert.match(said.stdout, new RegExp(answer.conversation))
    assert.equal(
      app.openFrames.filter((frame) => frame.argv.includes('--in-pane')).length,
      opensBeforeSay,
      'continuation pastes into the existing worker PTY',
    )

    await app.waitFor(() => {
      const session = app.threads()[answer.conversation]?.sessionId
      return typeof session === 'string' && app.transcript(session).includes('worker follow-up')
    })
  } finally {
    await app.close()
  }
})

test('TEST-PANE-83: detected user-local harness starts in a worker without being on PATH', async () => {
  const app = await startIntegration({ fakeEnv: { PATH: '/usr/bin:/bin' } })
  try {
    const local = join(app.env.HOME, '.local', 'bin')
    mkdirSync(local, { recursive: true })
    renameSync(join(app.root, 'fake-bin', 'claude'), join(local, 'claude'))
    const opened = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'local harness launch', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const { conversation, pane } = JSON.parse(run.stdout)
    const output = () =>
      app.rustFrames
        .filter((f) => f.op === 'pane.output' && f.body.id === pane.id)
        .map((f) => Buffer.from(f.body.bytes).toString('utf8'))
        .join('')
    await app.waitFor(
      () =>
        output().includes('could not be started') ||
        app
          .transcript(app.threads()[conversation]?.sessionId)
          .includes('worker completed from a real PTY child'),
    )
    assert.ok(
      app
        .transcript(app.threads()[conversation]?.sessionId)
        .includes('worker completed from a real PTY child'),
      output(),
    )
  } finally {
    await app.close()
  }
})

test('TEST-PANE-83: failed worker startup remains visible with its error after the controller exits', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    await app.waitFor(() =>
      app.processes().some((row) => row.sessionId === opened.tab.lead.nativeSession),
    )
    writeFileSync(
      join(app.root, 'fake-bin', 'claude'),
      '#!/bin/sh\necho native-startup-error >&2\nexit 7\n',
    )
    const run = await app.runCli(
      ['run', '@worker', 'missing harness', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const { pane, conversation } = JSON.parse(run.stdout)
    await app.waitFor(() =>
      app.rustFrames.some((f) => f.op === 'pane.exit' && f.body.id === pane.id),
    )
    await app.waitFor(() => app.threads()[conversation]?.reserved === undefined)
    const state = await app.requestNode('state.list', {})
    const failed = state.tabs[0].panes.find((p) => p.id === pane.id)
    assert.ok(failed, 'a failed worker must remain inspectable')
    assert.equal(failed.alive, false)
    assert.match(failed.failure.message, /claude.*exited with code 7/)
    assert.equal(app.threads()[conversation].reserved, undefined)
    assert.equal(
      (await app.requestNode('tab.delete', { tab: opened.tab.id, generation: 1 })).outcome,
      'deleted',
    )
  } finally {
    await app.close()
  }
})

test('TEST-PANE-83: a missing worker harness refuses before any reservation or pane is created', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    await app.waitFor(() =>
      app.processes().some((row) => row.sessionId === opened.tab.lead.nativeSession),
    )
    unlinkSync(join(app.root, 'fake-bin', 'claude'))
    const run = await app.runCli(
      ['run', '@worker', 'never launch', '--new', '--json'],
      opened.leadEnv,
    )
    assert.notEqual(run.code, 0, run.stdout)
    assert.match(run.stderr, /not installed/)
    assert.deepEqual(app.threads(), {})
    assert.equal(app.openFrames.length, 1)
  } finally {
    await app.close()
  }
})

test('TEST-PANE-83: continuing a live worker needs no second executable lookup', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const first = await app.runCli(
      ['run', '@worker', 'CF_HOLD existing worker', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(first.code, 0, first.stderr)
    const { conversation } = JSON.parse(first.stdout)
    await app.waitFor(() => app.processes().length === 2)
    unlinkSync(join(app.root, 'fake-bin', 'claude'))
    const next = await app.runCli(
      ['run', '@worker', 'continue the existing worker', '--session', conversation, '--json'],
      opened.leadEnv,
    )
    assert.equal(next.code, 0, next.stderr)
    assert.equal(JSON.parse(next.stdout).outcome, 'said')
    assert.equal(app.openFrames.length, 2)
  } finally {
    await app.close()
  }
})
