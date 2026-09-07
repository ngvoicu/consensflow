import assert from 'node:assert/strict'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
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
          (delivery) =>
            delivery.conversation === answer.conversation &&
            delivery.channel === 'pty-inline' &&
            delivery.state === 'accepted',
        ),
    )
    const answers = await app.requestNode('answers.list', {
      tab: opened.tab.id,
      pane: opened.tab.panes.find((pane) => pane.kind === 'lead')?.id,
      conversation: answer.conversation,
    })
    const listed = answers.answers.find((item) => item.id === app.deliveries()[0].answerId)
    assert.equal(listed.ready, true)
    assert.equal(listed.delivered, true)
    const unread = await app.runCli(['results', answer.conversation, '--json'], opened.leadEnv)
    assert.equal(unread.code, 0, unread.stderr)
    assert.equal(
      JSON.parse(unread.stdout).workers[0].results[0].status,
      'read',
      'an automatically delivered answer has a verified native receipt',
    )
    const state = await app.requestNode('state.list', {})
    assert.equal(state.ok, true, JSON.stringify(state))
    assert.equal(
      state.tabs.some((tab) => tab.id === opened.tab.id),
      true,
      'the page still draws the lead tab after the worker exits',
    )
  } finally {
    await app.close()
  }
})

async function assertReadFaultVisible(fault) {
  const app = await startIntegration({ fakeEnv: { CF_INTEGRATION_READ_FAULT: fault } })
  try {
    const opened = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'CF_LARGE answer for a read fault', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
    const workerSession = app.threads()[answer.conversation].sessionId
    await app.waitFor(() => app.transcript(workerSession).includes('L'.repeat(60_000)))

    let delivery
    await app.waitFor(() => {
      delivery = app
        .deliveries()
        .find(
          (candidate) =>
            candidate.conversation === answer.conversation && candidate.channel === 'cf-read',
        )
      return delivery !== undefined
    })
    const leadSession = opened.tab.lead.nativeSession
    assert.equal(typeof leadSession, 'string')
    if (fault === 'early-close') {
      const faultLog = join(app.env.CONSENSFLOW_HOME, 'cf-read-fault.jsonl')
      await app.waitFor(() => existsSync(faultLog))
      const result = JSON.parse(readFileSync(faultLog, 'utf8').trim().split('\n')[0])
      assert.equal(result.code, 0, JSON.stringify(result))
      assert.equal(result.stdout, '')
      assert.equal(result.receiverClosedBeforeOutput, true)
      await app.waitFor(() => app.transcript(leadSession).includes('cf read failed'))
    } else {
      await app.waitFor(() => app.transcript(leadSession).includes('lead receipt'))
      await app.waitFor(() => {
        delivery = app.deliveries().find((candidate) => candidate.id === delivery.id)
        return Array.isArray(delivery?.partCoverage)
      })
      assert.equal(
        delivery.partCoverage.every((ids) => ids.length === 0),
        true,
      )
      for (const part of delivery.parts) {
        assert.equal(
          app.transcript(leadSession).includes(part.close),
          true,
          `${fault} must preserve end marker ${part.close}`,
        )
      }
    }

    const state = await app.requestNode('state.list', {})
    const visible = state.deliveries.find((candidate) => candidate.id === delivery.id)
    assert.ok(visible, JSON.stringify(state))
    assert.notEqual(visible.state, 'accepted')
    const answers = await app.requestNode('answers.list', {
      tab: opened.tab.id,
      pane: opened.tab.panes.find((pane) => pane.conversation === answer.conversation)?.id,
      conversation: answer.conversation,
    })
    const item = answers.answers.find((candidate) => candidate.id === delivery.answerId)
    assert.ok(item, JSON.stringify(answers))
    assert.equal(item.ready, true)
    assert.equal(item.delivered, false)
    assert.deepEqual(item.partProgress, {
      delivery: delivery.id,
      total: delivery.parts.length,
      uncovered: delivery.parts.map((part) => part.k),
    })
  } finally {
    await app.close()
  }
}

test('real cf read stdout early close is visible and never accepted', async () => {
  await assertReadFaultVisible('early-close')
})

test('a receiver that keeps only cf-read tails cannot complete delivery', async () => {
  await assertReadFaultVisible('tail-only')
})

test('cf-read end markers without bodies leave uncovered ranges visible', async () => {
  await assertReadFaultVisible('body-loss')
})

test('killing the real bridge after paste admission leaves delivery uncertain without replay', async () => {
  const app = await startIntegration({
    fakeEnv: { CF_INTEGRATION_BRIDGE_FAULT: 'after-paste-before-cr' },
  })
  try {
    const opened = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'bridge dies after paste admission', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    const observed = join(app.env.CONSENSFLOW_HOME, 'paste-observed')
    await app.waitFor(() => existsSync(observed), 10_000, 1)
    const observation = JSON.parse(readFileSync(observed, 'utf8'))
    assert.equal(observation.rawMode, true, JSON.stringify(observation))
    assert.equal(observation.crSeenBeforeKill, false, JSON.stringify(observation))
    assert.equal(app.signalRust('SIGSTOP'), true)
    await new Promise((resolve) => setTimeout(resolve, 20))
    const stoppedObservation = JSON.parse(readFileSync(observed, 'utf8'))
    assert.equal(stoppedObservation.rawMode, true, JSON.stringify(stoppedObservation))
    assert.equal(stoppedObservation.crSeenBeforeKill, false, JSON.stringify(stoppedObservation))
    assert.equal(app.killRust('SIGKILL'), true)
    await app.waitFor(() => app.rustExited(), 2_000, 1)
    await app.waitFor(() => app.uiExited(), 2_000, 1)
    const durableRoot = app.root
    const afterCrash = app
      .deliveries()
      .find((delivery) => delivery.conversation === answer.conversation)
    assert.ok(afterCrash, 'crash delivery was not persisted')
    assert.equal(afterCrash.state, 'uncertain', JSON.stringify(afterCrash))
    const count = app.deliveries().length
    await app.close({ preserveRoot: true })

    const restarted = await startIntegration({ existingRoot: durableRoot })
    try {
      await restarted.waitFor(
        () => restarted.deliveries().some((delivery) => delivery.state === 'uncertain'),
        65_000,
        250,
      )
      const recovered = restarted.deliveries().find((delivery) => delivery.id === afterCrash.id)
      assert.equal(restarted.deliveries().length, count, 'restart did not create a new delivery')
      assert.deepEqual(recovered, afterCrash)
      const resumed = await restarted.requestNode('tab.resume', { tab: opened.tab.id })
      assert.equal(resumed.ok, true, JSON.stringify(resumed))
      await restarted.waitFor(() => restarted.openFrames.length >= 1, 10_000, 25)
      await restarted.waitFor(
        async () => {
          const state = await restarted.requestNode('state.list', {})
          const tab = state.tabs.find((candidate) => candidate.id === opened.tab.id)
          return tab?.closed === false && tab.lead?.bound === true
        },
        10_000,
        25,
      )
      await new Promise((resolve) => setTimeout(resolve, 2_100))
      const stillUncertain = restarted
        .deliveries()
        .find((delivery) => delivery.id === afterCrash.id)
      assert.deepEqual(stillUncertain, afterCrash)
      assert.equal(
        restarted.deliveries().length,
        count,
        'resuming the live lead created a new delivery',
      )
      assert.equal(
        restarted.nodeFrames.filter(
          (frame) => frame.kind === 'req' && frame.op === 'pane.write_paste',
        ).length,
        0,
        'resuming the live lead replayed a paste',
      )
    } finally {
      await restarted.close()
    }
  } finally {
    await app.close()
  }
})

test('a large worker answer uses lossless cf-read parts and real CLI reads', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const run = await app.runCli(
      ['run', '@worker', 'CF_LARGE answer with complete framing', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
    const workerSession = app.threads()[answer.conversation].sessionId
    await app.waitFor(() => app.transcript(workerSession).includes('L'.repeat(60_000)))

    let delivery
    await app.waitFor(() => {
      delivery = app
        .deliveries()
        .find(
          (candidate) =>
            candidate.conversation === answer.conversation && candidate.channel === 'cf-read',
        )
      return (
        delivery?.state === 'accepted' &&
        Array.isArray(delivery.parts) &&
        delivery.parts.length > 1 &&
        typeof delivery.file === 'string' &&
        existsSync(delivery.file)
      )
    })
    assert.equal(readFileSync(delivery.file, 'utf8'), 'L'.repeat(60_000))
    for (const part of delivery.parts) {
      const read = await app.runCli(['read', delivery.id, '--part', String(part.k)], opened.leadEnv)
      assert.equal(read.code, 0, read.stderr)
      assert.equal(read.stdout, part.text)
    }
  } finally {
    await app.close()
  }
})

test('a native human draft blocks delivery until the current draft is cleared', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const lead = opened.tab.panes.find((pane) => pane.kind === 'lead')
    assert.ok(lead)
    const typed = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from('human draft\r')],
    })
    assert.equal(typed.ok, true, JSON.stringify(typed))
    await app.waitFor(() =>
      app.rustFrames.some(
        (frame) =>
          frame.kind === 'evt' &&
          frame.op === 'pane.enter' &&
          frame.body?.id === lead.id &&
          frame.body?.generation === lead.generation,
      ),
    )
    const entered = app.rustFrames.find(
      (frame) =>
        frame.kind === 'evt' &&
        frame.op === 'pane.enter' &&
        frame.body?.id === lead.id &&
        frame.body?.generation === lead.generation,
    )
    const run = await app.runCli(
      ['run', '@worker', 'worker waits behind a human draft', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    await app.waitFor(() =>
      app
        .deliveries()
        .some(
          (delivery) =>
            delivery.conversation === answer.conversation &&
            delivery.state === 'pending' &&
            /draft open/i.test(delivery.reason ?? ''),
        ),
    )

    const newerTyped = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from('newer human draft\r')],
    })
    assert.equal(newerTyped.ok, true, JSON.stringify(newerTyped))
    await app.waitFor(() =>
      app.rustFrames.some(
        (frame) =>
          frame.kind === 'evt' &&
          frame.op === 'pane.enter' &&
          frame.body?.id === lead.id &&
          frame.body?.generation === lead.generation &&
          frame.body.epoch > entered.body.epoch,
      ),
    )
    const newerEntered = [...app.rustFrames]
      .reverse()
      .find(
        (frame) =>
          frame.kind === 'evt' &&
          frame.op === 'pane.enter' &&
          frame.body?.id === lead.id &&
          frame.body?.generation === lead.generation &&
          frame.body.epoch > entered.body.epoch,
      )
    assert.ok(newerEntered)

    const staleClear = await app.requestRust('draft.clear', {
      id: lead.id,
      generation: lead.generation,
      submittedEpoch: entered.body.epoch,
      submissionId: 'draft-cover-one',
    })
    assert.notEqual(staleClear.ok, true, 'no legacy clear authority on the bridge')
    const snapshot = await app.requestRust('pane.snapshot', {
      id: lead.id,
      generation: lead.generation,
    })
    assert.equal(snapshot.draftLatched, true, JSON.stringify(snapshot))

    const cleared = await app.requestRust('draft.clear', {
      id: lead.id,
      generation: lead.generation,
      submittedEpoch: newerEntered.body.epoch,
      submissionId: 'draft-cover-two',
    })
    assert.notEqual(cleared.ok, true, 'even the current epoch cannot authorize a Node clear')
    assert.equal(
      app.deliveries().find((delivery) => delivery.conversation === answer.conversation).state,
      'pending',
    )
  } finally {
    await app.close()
  }
})
