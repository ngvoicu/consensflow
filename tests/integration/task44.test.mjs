import assert from 'node:assert/strict'
import { appendFileSync } from 'node:fs'
import test from 'node:test'
import { answers } from '../../hosts/lib/completion.js'
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

test('a closed lead keeps a pending answer held until resume and explicit transfer', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const lead = opened.tab.panes.find((pane) => pane.kind === 'lead')
    assert.ok(lead)
    const typed = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from('hold this draft\r')],
    })
    assert.equal(typed.ok, true, JSON.stringify(typed))
    const run = await app.runCli(
      ['run', '@worker', 'answer waits for the closed lead', '--new', '--json'],
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
    const old = app.deliveries().find((delivery) => delivery.conversation === answer.conversation)
    const killed = await app.requestRust('pane.kill', {
      id: lead.id,
      generation: lead.generation,
    })
    assert.equal(killed.ok, true, JSON.stringify(killed))
    await app.waitFor(async () => (await app.requestNode('state.list', {})).tabs[0].closed === true)

    const closed = await app.requestNode('state.list', {})
    assert.equal(
      closed.held.some((held) => held.id === old.id),
      true,
      'closed tabs expose pending answers as held',
    )
    const resumed = await app.requestNode('tab.resume', { tab: opened.tab.id })
    assert.equal(resumed.ok, true, JSON.stringify(resumed))
    await app.waitFor(
      async () => (await app.requestNode('state.list', {})).tabs[0].closed === false,
    )

    const transferred = await app.requestNode('held.send', { tab: opened.tab.id })
    assert.equal(transferred.ok, true, JSON.stringify(transferred))
    assert.equal(transferred.sent.length, 1)
    await app.waitFor(() =>
      app
        .deliveries()
        .some((delivery) => delivery.heldOf === old.id && delivery.state === 'accepted'),
    )
  } finally {
    await app.close()
  }
})

test('manual policy keeps a queued worker answer until Deliver now', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const policy = await app.requestNode('notify.set', {
      scope: 'tab',
      id: opened.tab.id,
      mode: 'manual',
    })
    assert.equal(policy.ok, true, JSON.stringify(policy))
    const run = await app.runCli(
      ['run', '@worker', 'manual answer waits for a human', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
    await new Promise((resolve) => setTimeout(resolve, 1_200))
    assert.deepEqual(app.deliveries(), [])
    const row = app.threads()[answer.conversation]
    const completion = await answers(row.kind, row.sessionId, app.env)
    const item = completion.items.find((candidate) => candidate.role === 'assistant')
    const sent = await app.requestNode('deliver.now', {
      tab: opened.tab.id,
      conversation: answer.conversation,
      answerId: item.id,
    })
    assert.equal(sent.ok, true, JSON.stringify(sent))
    await app.waitFor(() =>
      app
        .deliveries()
        .some(
          (delivery) =>
            delivery.conversation === answer.conversation && delivery.state === 'accepted',
        ),
    )
  } finally {
    await app.close()
  }
})

test('a replaced native worker session suspends its pending delivery', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const lead = opened.tab.panes.find((pane) => pane.kind === 'lead')
    assert.ok(lead)
    const typed = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from('keep delivery pending\r')],
    })
    assert.equal(typed.ok, true, JSON.stringify(typed))
    const run = await app.runCli(
      ['run', '@worker', 'CF_HOLD replacement is invalidating this', '--new', '--json'],
      opened.leadEnv,
    )
    assert.equal(run.code, 0, run.stderr)
    const answer = JSON.parse(run.stdout)
    await app.waitFor(() => typeof app.threads()[answer.conversation]?.sessionId === 'string')
    const session = app.threads()[answer.conversation].sessionId
    await app.waitFor(() =>
      app
        .deliveries()
        .some(
          (delivery) =>
            delivery.conversation === answer.conversation && delivery.state === 'pending',
        ),
    )
    appendFileSync(
      `${app.env.CLAUDE_CONFIG_DIR}/projects/integration/${session}.jsonl`,
      `${JSON.stringify({
        sessionId: 'replacement-session',
        version: '2.1.247',
        timestamp: new Date().toISOString(),
        type: 'system',
        subtype: 'replacement-observed',
        uuid: 'replacement-observed',
      })}\n`,
    )
    await app.waitFor(() =>
      app
        .deliveries()
        .some(
          (delivery) =>
            delivery.conversation === answer.conversation &&
            delivery.suspended === true &&
            /replaced/i.test(delivery.reason ?? ''),
        ),
    )
    assert.equal(app.threads()[answer.conversation].sessionId, null)
  } finally {
    await app.close()
  }
})

test('a replaced native lead session suspends its pending delivery', async () => {
  const app = await startIntegration()
  try {
    const opened = await app.openTab()
    const lead = opened.tab.panes.find((pane) => pane.kind === 'lead')
    assert.ok(lead)
    const typed = await app.requestRust('pane.input', {
      id: lead.id,
      generation: lead.generation,
      bytes: [...Buffer.from('keep the lead delivery pending\r')],
    })
    assert.equal(typed.ok, true, JSON.stringify(typed))
    const run = await app.runCli(
      ['run', '@worker', 'lead replacement must suspend this', '--new', '--json'],
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
    const leadSession = opened.tab.lead.nativeSession
    appendFileSync(
      `${app.env.CLAUDE_CONFIG_DIR}/projects/integration/${leadSession}.jsonl`,
      `${JSON.stringify({
        sessionId: 'replacement-lead-session',
        version: '2.1.247',
        timestamp: new Date().toISOString(),
        type: 'system',
        subtype: 'replacement-observed',
        uuid: 'replacement-lead-observed',
      })}\n`,
    )
    await app.waitFor(() =>
      app
        .deliveries()
        .some(
          (delivery) =>
            delivery.conversation === answer.conversation &&
            delivery.suspended === true &&
            /replaced/i.test(delivery.reason ?? ''),
        ),
    )
    // Suspension is persisted before the watcher invalidates the lead binding.
    let state
    let tab
    await app.waitFor(async () => {
      state = await app.requestNode('state.list', {})
      tab = state.tabs.find((candidate) => candidate.id === opened.tab.id)
      return tab.lead.bound === false
    })
    assert.equal(tab.lead.nativeSession, null, JSON.stringify(state))
    assert.notEqual(
      state.deliveries.find((delivery) => delivery.conversation === answer.conversation).state,
      'accepted',
    )
  } finally {
    await app.close()
  }
})
