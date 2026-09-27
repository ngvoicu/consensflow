import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { Dispatcher, deliveryText } from '../src/core/dispatcher.js'
import { openLedger } from '../src/ledger/index.js'

/**
 * The dispatcher (TEST-BDC-09): the only actor. It launches panes, delivers
 * each participant's queue one message at a time when the participant is
 * idle, proves arrival from the harness's own record, collects a worker's
 * answer as the task's result and fails what cannot finish. Every test drives
 * it with explicit passes, a fake pane host and a scriptable fake agent.
 */

let order = 0
const item = (role, text, extra = {}) => ({
  id: `i-${++order}`,
  role,
  text,
  complete: role === 'assistant',
  settled: true,
  ...extra,
})

/** A harness adapter whose agents do exactly what the test tells them. */
function fakeAdapter(harness = 'claude-code') {
  const agents = new Map()
  const adapter = {
    harness,
    prepared: [],
    async prepare(request) {
      adapter.prepared.push(request)
      const agent = {
        launchId: request.launchId,
        handle: request.participant.handle,
        native: request.resume ?? `native-${request.launchId}`,
        items: [],
        settled: true,
        waiting: null,
        quota: null,
        admit: true,
        arrive: true,
        queued: false,
      }
      if (request.message !== null) {
        agent.items.push(item('user', request.message))
        agent.settled = false
      }
      agents.set(request.launchId, agent)
      return {
        argv: ['/bin/fake-agent', request.participant.handle],
        env: { FAKE_AGENT: request.participant.handle },
        dropEnv: [],
        nativeSession: agent.native,
        firstMessage: request.message === null ? null : 'argv',
        launch: { launchId: request.launchId },
      }
    },
    async started() {
      return {}
    },
    async deliver({ launch, text }) {
      const agent = agents.get(launch.launchId)
      if (!agent.admit) return { admitted: false, reason: 'refused by the test' }
      if (agent.arrive) {
        agent.items.push(item('user', text))
        agent.settled = false
      }
      return agent.queued ? { admitted: true, queued: true } : { admitted: true }
    },
    async observe({ launch }) {
      const agent = agents.get(launch.launchId)
      return {
        items: [...agent.items],
        settled: agent.settled,
        waiting: agent.waiting,
        quota: agent.quota,
        failed: false,
        openTools: agent.openTools ?? 0,
      }
    },
  }
  // A member's latest window: its own, or its newest session's (`zeus` finds `zeus-amber-pine`).
  adapter.agent = (handle) =>
    [...agents.values()]
      .filter((a) => a.handle === handle || a.handle.startsWith(`${handle}-`))
      .at(-1)
  adapter.answer = (handle, text) => {
    const agent = adapter.agent(handle)
    agent.items.push(item('assistant', text))
    agent.settled = true
  }
  adapter.busy = (handle) => {
    adapter.agent(handle).settled = false
  }
  adapter.quota = (handle, quota) => {
    adapter.agent(handle).quota = quota
  }
  return adapter
}

/** The saved model of each fake agent, as the roster gives it at launch. */
const MODELS = {
  zeus: 'claude-opus-5',
  diana: 'gpt-5.6-luna',
  hera: 'muse-spark',
  calliope: 'claude-opus-5',
  astraeus: 'gpt-6-astra',
}

/** Whether a pane id is a window of `handle`: its own, or one of its sessions'. */
const windowOf = (id, handle) =>
  id.endsWith(`-${handle}`) ||
  (/^p\d+-/.test(id) && id.replace(/^p\d+-/, '').startsWith(`${handle}-`))

/** A pane host that opens nothing real and exits panes when told to. */
function fakeHost() {
  const exits = []
  const enters = []
  const host = {
    opened: [],
    killed: [],
    requests: [],
    refuse: false,
    hold: null,
    holdExits: false,
    async request(op, body) {
      host.requests.push([op, body])
      return { ok: true, outcome: 'cleared', ...(op === 'pane.snapshot' ? host.snapshot : {}) }
    },
    onEnter(listener) {
      enters.push(listener)
    },
    enter(handle, epoch) {
      const body = host.opened.filter((b) => windowOf(b.id, handle)).at(-1)
      for (const listener of enters) listener({ id: body.id, generation: body.generation, epoch })
    },
    async open(body) {
      if (host.refuse) {
        host.refuse = false
        return { ok: false, error: 'refused by the test' }
      }
      await host.hold
      host.opened.push(body)
      return { ok: true, id: body.id, generation: body.generation }
    },
    async kill(pane) {
      host.killed.push(pane)
      // A killed process is gone before the next pass, so its exit lands at
      // once; a test that wants the gap between the two holds it and sends it.
      if (!host.holdExits) {
        for (const listener of exits) await listener({ id: pane.id, generation: pane.generation })
      }
      return { ok: true }
    },
    onExit(listener) {
      exits.push(listener)
    },
    async exit(handle) {
      const body = host.opened.filter((b) => windowOf(b.id, handle)).at(-1)
      for (const listener of exits) await listener({ id: body.id, generation: body.generation })
    },
    last(handle) {
      return host.opened.filter((b) => windowOf(b.id, handle)).at(-1)
    },
  }
  return host
}

async function setup(fn, options = {}) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-dispatch-'))
  let at = Date.parse('2026-09-19T12:00:00.000Z')
  const clock = {
    now: () => new Date(at),
    advance: (ms) => {
      at += ms
    },
  }
  const list = [
    'amber-pine',
    'brisk-birch',
    'calm-brook',
    'coral-canyon',
    'crisp-cedar',
    'dusky-cliff',
  ]
  let named = 0
  const ledger = openLedger(path.join(dir, 'consensflow.db'), {
    now: clock.now,
    names: () => list[named++ % list.length],
  })
  const adapter = fakeAdapter()
  const host = fakeHost()
  const make = () =>
    new Dispatcher({
      ledger,
      host,
      adapters: { 'claude-code': adapter },
      clock,
      credentials: {
        issue: ({ participant }) => `token-${participant.handle}`,
        revoke: () => {},
      },
      paneEnv: (participant) => ({ CONSENSFLOW_PARTICIPANT: participant.handle }),
      roles: (participant) => `instructions for ${participant.role}`,
      roster: (name) => ({ id: name, model: MODELS[name], profile: { modelKey: MODELS[name] } }),
      arrivalTimeoutMs: 30_000,
      launchTimeoutMs: 120_000,
      maxAttempts: 3,
      ...options,
    })
  try {
    await fn({ ledger, adapter, host, clock, dispatcher: make(), make, dir })
  } finally {
    ledger.close()
    await rm(dir, { recursive: true, force: true })
  }
}

/** A project with its chief window up, its workers (Zeus, unless told) in the staff from the start. */
async function withStaff(context, workers = ['zeus']) {
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    harness: 'claude-code',
    staff: workers.map((agent) => ({
      agent,
      harness: 'claude-code',
      role: 'worker',
      tier: 'standard',
    })),
  })
  const id = (handle) =>
    context.ledger.project(project.id).participants.find((p) => p.handle === handle).id
  return { project, id }
}

describe('the dispatcher', () => {
  it('opens a project with its chief window and binds the chief conversation', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      const chief = context.host.last('chief')
      assert.equal(chief.id, `p${project.id}-chief`)
      assert.deepEqual(chief.argv, ['/bin/fake-agent', 'chief'])
      assert.equal(chief.cwd, '/work/app')
      assert.equal(chief.env.FAKE_AGENT, 'chief')
      assert.equal(chief.env.CONSENSFLOW_PARTICIPANT, 'chief')
      assert.equal(chief.env.CONSENSFLOW_TOKEN, 'token-chief')
      assert.equal(context.adapter.prepared[0].message, null, 'a chief opens without a task')
      assert.equal(context.adapter.prepared[0].role, 'chief')
      assert.equal(context.adapter.prepared[0].instructions, 'instructions for chief')
      const conversation = context.ledger.currentConversation(id('chief'))
      assert.equal(conversation.nativeSession, `native-${chief.launch}`)
      assert.equal(context.dispatcher.activity(id('chief')).state, 'starting')
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(id('chief')).state, 'idle')
    })
  })

  it('launches a worker with its task as the first message and records its answer as the result', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      const { message } = context.ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Write the parser',
      })
      await context.dispatcher.pass()
      const first = context.adapter.prepared.at(-1)
      assert.equal(first.participant.handle, 'zeus')
      assert.equal(first.message, deliveryText(context.ledger.task(project.id, 1).messages[0]))
      assert.match(
        first.message,
        new RegExp(`^\\[ConsensFlow m-${message.id} · T-1 · task from @chief\\]\\n`),
      )
      assert.equal(context.ledger.task(project.id, 1).state, 'queued', 'not yet seen by the agent')

      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'working')
      assert.equal(context.dispatcher.activity(id('zeus')).state, 'working')

      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'done')
      const result = task.messages.find((m) => m.kind === 'result')
      assert.deepEqual([result.body, result.recipient], ['Parser done', 'chief'])
    })
  })

  it('opens no window for a brief the human has not approved, and delivers a result only once approved', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.ledger.setGate(project.id, true)
      context.ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Write the parser',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const brief = context.ledger.task(project.id, 1).messages[0]
      assert.deepEqual(
        [context.ledger.task(project.id, 1).state, brief.state],
        ['queued', 'gated'],
        'assigned, but held for the human',
      )
      assert.equal(context.host.last('zeus'), undefined, 'no window yet')
      context.ledger.approveMessage(brief.id, { by: 'human' })
      await context.dispatcher.pass()
      assert.equal(context.adapter.prepared.at(-1).participant.handle, brief.recipient)
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'working')
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      const result = context.ledger.task(project.id, 1).messages.find((m) => m.kind === 'result')
      assert.equal(result.state, 'gated')
      await context.dispatcher.pass()
      assert.equal(context.adapter.agent('chief').items.length, 0, 'the chief waits for the human')
      context.ledger.approveMessage(result.id, { by: 'human' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.inbox(id('chief'))[0].state, 'delivered')
      assert.match(
        context.adapter.agent('chief').items.at(-1).text,
        /result from @zeus-amber-pine\]\nParser done/,
      )
    })
  })

  it('gives out a task only once every task it needs is accepted, and says nothing while it waits', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context, ['zeus', 'diana'])
      const add = (body, extra = {}) =>
        context.ledger.createTask(project.id, {
          from: 'chief',
          pool: 'worker',
          tier: 'standard',
          body,
          ...extra,
        }).task
      add('Lexer')
      add('Parser', { needs: [1] })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const states = () => [1, 2].map((number) => context.ledger.task(project.id, number).state)
      assert.deepEqual(states(), ['working', 'open'], 'the parser waits for the lexer')
      assert.equal(
        context.ledger.inbox(id('chief')).some((m) => m.kind === 'note'),
        false,
        'no "waits for a free worker" note: it waits for its need',
      )
      context.adapter.answer('zeus', 'Lexer done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(states(), ['done', 'open'], 'done is not accepted')
      context.ledger.acceptTask(project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(states(), ['accepted', 'working'])
      assert.match(context.adapter.prepared.at(-1).message, /T-2 · task from @chief\]\nParser$/)
    })
  })

  it("keeps its own copy of each window's conversation, item by item, as it grows", async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Write the parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const before = context.ledger.transcript(project.id, 1)
      assert.deepEqual(
        before.items.map((i) => [i.role, i.text.split('\n')[0]]),
        [['user', '[ConsensFlow m-1 · T-1 · task from @chief]']],
        'the brief, as the window got it',
      )
      const zeus = context.adapter.agent('zeus')
      zeus.items.push(item('assistant', 'Half', { complete: false, settled: false }))
      zeus.settled = false
      await context.dispatcher.pass()
      const half = context.ledger.transcript(project.id, 1).items.at(-1)
      assert.deepEqual([half.role, half.text, half.complete], ['assistant', 'Half', false])
      zeus.items.at(-1).text = 'Half done, then all done'
      zeus.items.at(-1).complete = true
      zeus.items.at(-1).settled = true
      zeus.settled = true
      await context.dispatcher.pass()
      const { items, total } = context.ledger.transcript(project.id, 1)
      assert.equal(total, 2)
      assert.deepEqual([items[1].text, items[1].complete], ['Half done, then all done', true])
      assert.equal(context.ledger.task(project.id, 1).state, 'done', 'and the result was collected')
    })
  })

  it("delivers the chief's tell into the paused window once the agent is interrupted, and collects no result from it", async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('zeus')
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      const told = context.ledger.ask(project.id, {
        from: 'chief',
        to: 'zeus',
        task: 1,
        body: 'Stop: use grammar v2',
        urgent: true,
      })
      await context.dispatcher.pass()
      const pane = context.host.last('zeus')
      assert.deepEqual(
        context.host.requests.filter(([op]) => op === 'pane.input'),
        [['pane.input', { id: pane.id, generation: pane.generation, bytes: [27], draft: false }]],
        'the agent is interrupted first',
      )
      const zeus = context.adapter.agent('zeus')
      assert.ok(
        !zeus.items.some((i) => i.text.includes(`m-${told.id}`)),
        'nothing pasted while it works',
      )
      context.adapter.answer('zeus', 'Half a parser')
      await context.dispatcher.pass()
      assert.match(
        zeus.items.at(-1).text,
        /^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nStop: use grammar v2\n\nT-1 is paused for this\. Run in your shell: cf answer m-\d+ "…"; the chief resumes the task\.$/,
        'the tell goes in once the window is idle',
      )
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'paused',
        'its half-done output was not a result',
      )
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(told.id).state, 'delivered')
    })
  })

  it("leaves the window alone once the chief's tell reaches it, answered or not, until the task goes on", async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('zeus')
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      const told = context.ledger.ask(project.id, {
        from: 'chief',
        to: 'zeus',
        task: 1,
        body: 'Which file?',
        urgent: true,
      })
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Stopped')
      await context.dispatcher.pass()
      const escapes = () => context.host.requests.filter(([op]) => op === 'pane.input').length
      assert.equal(escapes(), 1, 'one Escape stopped the task')
      // The tell is in; the agent works on its answer, past the next round's time.
      context.adapter.busy('zeus')
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      assert.equal(escapes(), 1, 'its answer to the tell is not interrupted')
      // Answered, it ends its own turn: an Escape now would cut its last words.
      context.ledger.answer(told.id, {
        from: context.ledger.task(project.id, 1).assignee,
        body: 'a.txt',
      })
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      assert.equal(escapes(), 1, 'nor its wrap-up after the answer')
    })
  })

  it('stops a member window that made no progress for ten minutes and tells the requester', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('zeus')
      await context.dispatcher.pass()
      context.clock.advance(5 * 60_000)
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'working')
      context.clock.advance(6 * 60_000)
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'paused')
      const chief = context.ledger
        .project(project.id)
        .participants.find((p) => p.handle === 'chief')
      const note = context.ledger.inbox(chief.id).find((m) => m.kind === 'note')
      assert.match(
        note.body,
        /^T-1 is paused: @zeus's window made no progress for 11 minutes, so it was stopped\. Resume it with: cf task resume T-1 "…"; its window comes back on its own conversation\.$/,
      )
      await context.dispatcher.pass()
      assert.equal(context.host.killed.length, 1, 'the stall closes the window')
    })
  })

  it('closes a stuck member window, so a resume reopens it on its own conversation', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Report' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const native = context.ledger.currentConversation(id('zeus')).nativeSession
      const stuck = context.host.last('zeus')
      context.adapter.busy('zeus')
      await context.dispatcher.pass()
      context.clock.advance(11 * 60_000)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      // A model request that never returns does not answer Escape (a Pi window,
      // 2026-09-27): the window closes, or a resume would wait on it for good.
      assert.deepEqual(context.host.killed, [{ id: stuck.id, generation: stuck.generation }])
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'paused')
      assert.equal(
        task.messages.filter((m) => m.kind === 'note').length,
        1,
        'one note: the stall, not a second one for the window it closed',
      )
      context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual([launch.participant.handle, launch.resume], ['zeus', native])
      assert.match(launch.message, /Resumed: Go on$/)
    })
  })

  it('gives a window whose tool is running an hour before it counts as stuck', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Build' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('zeus')
      context.adapter.agent('zeus').openTools = 1
      await context.dispatcher.pass()
      context.clock.advance(59 * 60_000)
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'working',
        'a long build is not a hang',
      )
      context.clock.advance(2 * 60_000)
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'paused',
        'a tool that never returns is',
      )
      const chief = context.ledger
        .project(project.id)
        .participants.find((p) => p.handle === 'chief')
      assert.match(
        context.ledger.inbox(chief.id).find((m) => m.kind === 'note').body,
        /made no progress for 61 minutes, a tool still running, so it was stopped/,
      )
    })
  })

  it('keeps a fresh window starting until its screen is drawn: printed, then still a moment', async () => {
    await setup(async (context) => {
      context.host.snapshot = { outputQuietMs: null }
      const { project } = await withStaff(context)
      const chief = context.ledger
        .project(project.id)
        .participants.find((p) => p.handle === 'chief')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(chief.id).state, 'starting', 'nothing printed yet')
      context.host.snapshot = { outputQuietMs: 300 }
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(chief.id).state, 'starting', 'still drawing')
      context.host.snapshot = { outputQuietMs: 2_000 }
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(chief.id).state, 'idle')
    })
  })

  it('shows a chief window that made no progress for ten minutes as stalled, until it moves again', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      await context.dispatcher.pass()
      const chief = context.ledger
        .project(project.id)
        .participants.find((p) => p.handle === 'chief')
      context.clock.advance(11 * 60_000)
      await context.dispatcher.pass()
      assert.deepEqual(context.dispatcher.activity(chief.id), {
        state: 'stalled',
        reason:
          'no progress for 11 minutes: a request may have hung; Escape in its window stops it',
      })
      context.adapter.answer('chief', 'Back')
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(chief.id).state, 'idle')
    })
  })

  it('presses Escape twice in a row for a harness that asks for it', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.adapter.interrupt = { presses: 2 }
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      const pane = context.host.last('zeus')
      assert.deepEqual(
        context.host.requests.filter(([op]) => op === 'pane.input'),
        [
          ['pane.input', { id: pane.id, generation: pane.generation, bytes: [27], draft: false }],
          ['pane.input', { id: pane.id, generation: pane.generation, bytes: [27], draft: false }],
        ],
      )
    })
  })

  it('delivers results to an idle chief one at a time and proves each arrived', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context, ['zeus', 'diana'])
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'One' })
      context.ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Two' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      context.adapter.answer('zeus', 'one done')
      context.adapter.answer('diana', 'two done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const chief = () => context.adapter.agent('chief')
      assert.equal(chief().items.length, 0, 'a busy chief is not interrupted')

      chief().settled = true
      await context.dispatcher.pass()
      assert.equal(chief().items.length, 1)
      assert.match(
        chief().items[0].text,
        /result from @zeus\]\none done\n\nDecide with: cf task accept T-1/,
      )
      await context.dispatcher.pass()
      const [first, second] = context.ledger.inbox(id('chief')).reverse()
      assert.equal(first.state, 'delivered')
      assert.equal(first.receipt.item, chief().items[0].id)
      assert.equal(second.state, 'queued', 'the next waits until the chief is idle again')

      context.adapter.answer('chief', 'noted')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.inbox(id('chief'))[0].state, 'delivered')
      assert.match(
        chief().items.at(-1).text,
        /result from @diana\]\ntwo done\n\nDecide with: cf task accept T-2/,
      )
    })
  })

  it('retries a delivery the harness refused, then gives up', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.adapter.agent('chief').admit = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      for (let n = 0; n < 5; n += 1) await context.dispatcher.pass()
      const failed = context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.deepEqual([failed.state, failed.attempts], ['failed', 3])
      assert.equal(failed.reason, 'refused by the test')
    })
  })

  it('retries a delivery whose arrival never shows in the harness record', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.adapter.agent('chief').arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      const delivering = () => context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.equal(delivering().state, 'delivering')
      context.clock.advance(29_000)
      await context.dispatcher.pass()
      assert.equal(delivering().state, 'delivering', 'still inside the arrival window')
      context.clock.advance(2_000)
      await context.dispatcher.pass()
      assert.deepEqual([delivering().state, delivering().attempts], ['delivering', 2])
      assert.equal(delivering().reason, null)
    })
  })

  it('tries an uncertain handover again once its arrival window passes with no sign of it', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        if (context.adapter.agent('chief').items.length === 0 && !context.retried) {
          context.retried = true
          return { admitted: null, reason: 'the plugin did not answer' }
        }
        return deliver(request)
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      context.clock.advance(31_000)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['delivered', 2])
      assert.equal(context.adapter.agent('chief').items.length, 1, 'delivered once')
    })
  })

  it('records an adapter that throws as a failed attempt with its error, not as uncertain', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.adapter.deliver = async () => {
        throw new Error('the channel needs pane, generation and observed epoch')
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['queued', 1])
      assert.match(message.reason, /the channel needs pane, generation and observed epoch/)
    })
  })

  it('pauses the task and tells the requester when the worker window closes mid-task, and resumes it on its conversation', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const native = context.ledger.currentConversation(id('zeus')).nativeSession
      await context.host.exit('zeus')
      const task = context.ledger.task(project.id, 1)
      assert.deepEqual([task.state, task.assignee], ['paused', 'zeus'])
      const note = task.messages.find((m) => m.kind === 'note')
      assert.equal(note.recipient, 'chief')
      assert.match(
        note.body,
        /^T-1 is paused: @zeus's window closed\. Resume it with: cf task resume T-1 "…"/,
      )
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'paused', 'nothing happens on its own')
      context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Carry on' })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual([launch.participant.handle, launch.resume], ['zeus', native])
      assert.match(launch.message, /T-1 · task from @chief\]\nResumed: Carry on$/)
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'working')
    })
  })

  it("brings a tell to a task whose window is gone on that window's own conversation, still paused", async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const native = context.ledger.currentConversation(id('zeus')).nativeSession
      await context.host.exit('zeus')
      assert.equal(context.ledger.task(project.id, 1).state, 'paused')
      const told = context.ledger.ask(project.id, {
        from: 'chief',
        to: 'zeus',
        task: 1,
        body: 'Which grammar did you start from?',
        urgent: true,
      })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['zeus', native],
        'the same conversation',
      )
      assert.match(
        launch.message,
        /^\[ConsensFlow m-\d+ · T-1 · question from @chief\]\nWhich grammar did you start from\?\n\nT-1 is paused for this\./,
      )
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'paused',
        'the task waits for the chief',
      )
      assert.notEqual(context.ledger.message(told.id).state, 'queued')
    })
  })

  it("pauses a working task in its open window: the agent is interrupted once, its output not collected, and the chief's words resume it there", async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const launches = context.adapter.prepared.length
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const pane = context.host.last('zeus')
      const escapes = () => context.host.requests.filter(([op]) => op === 'pane.input')
      assert.deepEqual(
        escapes(),
        [['pane.input', { id: pane.id, generation: pane.generation, bytes: [27], draft: false }]],
        'Escape, once',
      )
      assert.deepEqual(context.host.killed, [], 'the window stays')
      // Still working three seconds later (a harness that ignored the key while it thought): again, up to three times.
      context.adapter.busy('zeus')
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      assert.equal(escapes().length, 2, 'pressed again while the window still works')
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      assert.equal(escapes().length, 3, 'and then no more')
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'paused',
        'said after the pause: not a result',
      )
      context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Add the tests too' })
      await context.dispatcher.pass()
      assert.equal(context.adapter.prepared.length, launches, 'no new window')
      assert.match(
        context.adapter.agent('zeus').items.at(-1).text,
        /T-1 · task from @chief\]\nResumed: Add the tests too$/,
      )
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'working')
      context.adapter.answer('zeus', 'Parser and tests done')
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'done')
    })
  })

  it('fails the task when the worker window cannot open', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      context.host.refuse = true
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'failed')
      assert.match(context.ledger.task(project.id, 1).messages[0].reason, /refused by the test/)
    })
  })

  it('waits on a message a harness queued itself, and re-sends only a paste the record never showed', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'ready')
      // The chief's own queue took it (a peer inbox, a broker): it will show
      // when the harness gets to it, maybe minutes later; sending it again
      // would only make a duplicate the harness may even drop.
      context.adapter.agent('chief').queued = true
      context.adapter.agent('chief').arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Queued' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivering')
      context.clock.advance(150_000)
      await context.dispatcher.pass()
      assert.deepEqual(
        [context.ledger.message(note.id).state, context.ledger.message(note.id).attempts],
        ['delivering', 1],
        'still in flight, not sent again',
      )
      context.adapter
        .agent('chief')
        .items.push(item('user', `[ConsensFlow m-${note.id} · note from @zeus]\nQueued`))
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivered')

      // A paste has no such receipt: the record is the only proof, and 60 s
      // without it means the paste was lost.
      context.adapter.agent('chief').queued = false
      const pasted = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Pasted' })
      await context.dispatcher.pass()
      context.clock.advance(61_000)
      await context.dispatcher.pass()
      assert.deepEqual(
        [context.ledger.message(pasted.id).state, context.ledger.message(pasted.id).attempts],
        ['delivering', 2],
        'sent again',
      )
    })
  })

  it('fails a launch whose first message never arrives', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      const original = context.adapter.prepare
      context.adapter.prepare = async (request) => {
        const plan = await original(request)
        context.adapter.agent(request.participant.handle).items = []
        return plan
      }
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      context.clock.advance(121_000)
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'failed')
      assert.deepEqual(context.host.killed.at(-1).id, context.host.last('zeus').id)
    })
  })

  it('fails the first message at once when the harness cannot take it after the window opens', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.adapter.started = async () => {
        throw new Error('the server never answered')
      }
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'failed')
      assert.match(task.messages[0].reason, /the server never answered/)
      assert.equal(context.host.killed.at(-1).id, context.host.last('zeus').id)
    })
  })

  it('leaves a task waiting on a question alone, and resumes it with the answer', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const question = context.ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Which format?',
      })
      context.adapter.answer('zeus', 'I asked the chief.')
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'waiting',
        'no result from a waiting turn',
      )

      await context.dispatcher.pass()
      context.adapter.answer('chief', 'JSON, I will reply')
      context.ledger.answer(question.id, { from: 'chief', body: 'JSON' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'working')
      context.adapter.answer('zeus', 'Parser done, in JSON')
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'done')
      assert.equal(task.messages.at(-1).body, 'Parser done, in JSON')
    })
  })

  it('releases the typing latch once the harness shows the message the human submitted', async () => {
    await setup(async (context) => {
      await withStaff(context)
      await context.dispatcher.pass()
      const clears = () => context.host.requests.filter(([op]) => op === 'draft.clear')
      context.host.enter('chief', 5)
      await context.dispatcher.pass()
      assert.deepEqual(clears(), [], 'an Enter alone proves nothing')

      context.adapter.agent('chief').items.push(item('user', 'the human asks something'))
      await context.dispatcher.pass()
      const chief = context.host.last('chief')
      assert.deepEqual(clears(), [
        [
          'draft.clear',
          { id: chief.id, generation: chief.generation, epoch: 5, submission: 'human-1' },
        ],
      ])
      await context.dispatcher.pass()
      assert.equal(clears().length, 1, 'each Enter is released once')
    })
  })

  it('does not count its own deliveries as the human submitting', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.host.enter('chief', 9)
      context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'from zeus' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        context.host.requests.filter(([op]) => op === 'draft.clear'),
        [],
        'a ConsensFlow message is not the human submitting their draft',
      )
    })
  })

  it('suspends the project when its chief window closes', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.host.exit('chief')
      assert.equal(context.ledger.project(project.id).state, 'suspended')
      assert.equal(context.ledger.project(project.id).resumeOnStart, false)
    })
  })

  it('opens a project with the staff it is given, and only the chief window', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        harness: 'claude-code',
        staff: [{ agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' }],
      })
      assert.deepEqual(
        project.participants.map((p) => p.handle),
        ['human', 'chief', 'zeus'],
      )
      assert.deepEqual(
        context.host.opened.map((pane) => pane.id),
        [`p${project.id}-chief`],
      )
    })
  })

  it('closes the window of a member who leaves, and its exit fails nothing', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      const zeus = id('zeus')
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()

      const { cancelled } = await context.dispatcher.removeMember(project.id, 'zeus')
      assert.deepEqual(cancelled, [1])
      const pane = context.host.last('zeus')
      assert.deepEqual(context.host.killed, [{ id: pane.id, generation: pane.generation }])

      await context.host.exit('zeus')
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'cancelled')
      assert.deepEqual(
        task.messages.map((message) => message.kind),
        ['task'],
        'no "window closed" note for a member who left',
      )
      assert.equal(context.dispatcher.pane(zeus), null)
      assert.equal(context.host.opened.filter((b) => b.id.endsWith('-zeus')).length, 1)
    })
  })

  it('waits for a window still opening before closing it for a member who leaves', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      let open
      context.host.hold = new Promise((resolve) => {
        open = resolve
      })
      const passing = context.dispatcher.pass()
      const removing = context.dispatcher.removeMember(project.id, 'zeus')
      await new Promise((resolve) => setImmediate(resolve))
      assert.deepEqual(context.host.killed, [], 'the launch is still in progress')

      open()
      await passing
      const { cancelled } = await removing
      const pane = context.host.last('zeus')
      assert.deepEqual(cancelled, [1])
      assert.deepEqual(context.host.killed, [{ id: pane.id, generation: pane.generation }])
    })
  })

  it('closes a project: its windows go, work in them pauses, and Resume brings the chief back', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const chief = context.host.last('chief')
      const zeus = context.host.last('zeus')
      const native = context.ledger.currentConversation(id('chief')).nativeSession
      const closed = await context.dispatcher.closeProject(project.id)
      assert.equal(closed.state, 'suspended')
      assert.deepEqual(context.host.killed, [
        { id: chief.id, generation: chief.generation },
        { id: zeus.id, generation: zeus.generation },
      ])
      await context.host.exit('chief')
      await context.host.exit('zeus')
      assert.equal(context.dispatcher.pane(id('chief')), null)
      const task = context.ledger.task(project.id, 1)
      assert.deepEqual(
        [task.state, task.assignee],
        ['paused', 'zeus-amber-pine'],
        'the work waits, with its session, for the chief to resume it',
      )
      await context.dispatcher.pass()
      assert.equal(context.host.opened.length, 2, 'nothing reopens while suspended')

      await context.dispatcher.resumeProject(project.id)
      assert.equal(context.adapter.prepared.at(-1).resume, native)
      const nativeZeus = context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession
      context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      await context.dispatcher.pass()
      const back = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [back.participant.handle, back.resume],
        ['zeus-amber-pine', nativeZeus],
        'the same session of zeus, on its own conversation',
      )
    })
  })

  it('deletes a closed project for good, refuses an open one, and leaves one line in the trace', async () => {
    const entries = []
    const forgotten = []
    await setup(
      async (context) => {
        const { project } = await withStaff(context)
        await assert.rejects(context.dispatcher.deleteProject(project.id), {
          code: 'project-open',
        })
        await context.dispatcher.closeProject(project.id)
        const gone = await context.dispatcher.deleteProject(project.id)
        assert.deepEqual([gone.id, gone.name, gone.directory], [project.id, 'app', '/work/app'])
        assert.deepEqual(context.ledger.projects(), [])
        const record = entries.filter((entry) => entry.kind === 'project.deleted')
        assert.equal(record.length, 1)
        assert.deepEqual([record[0].project, record[0].data.name], [null, 'app'])
        assert.ok(forgotten.includes(project.id), "the project's own lines are dropped")
        await context.dispatcher.pass()
        assert.equal(context.host.opened.length, 1, 'nothing reopens')
      },
      {
        trace: Object.assign((entry) => entries.push(entry), {
          forget: (project) => forgotten.push(project),
        }),
      },
    )
  })

  it('brings back the projects that were open before a restart, on their own conversations', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      const native = context.ledger.currentConversation(id('chief')).nativeSession
      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      const resumed = context.adapter.prepared.at(-1)
      assert.deepEqual([resumed.participant.handle, resumed.resume], ['chief', native])
      assert.equal(context.ledger.project(project.id).state, 'open')
      assert.equal(context.ledger.project(project.id).resumeOnStart, false)
      assert.equal(context.ledger.currentConversation(id('chief')).nativeSession, native)
    })
  })
})

describe('the delivered text', () => {
  it('names the message, the task and the sender, and tells the reader how to answer a question', () => {
    const base = { id: 12, taskNumber: 3, sender: 'zeus', body: 'Which format?' }
    assert.equal(
      deliveryText({ ...base, kind: 'question' }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 "…"',
    )
    const options = [{ question: 'Which?', header: 'Format', options: [], multiple: false }]
    assert.equal(
      deliveryText({ ...base, kind: 'question', questions: options }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 "…" (a label or your own words)',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'question', questions: [...options, ...options] }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nRun in your shell: cf answer m-12 "…" (a label or your own words; one line per question)',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'note', sender: null, taskNumber: null, body: 'hi' }),
      '[ConsensFlow m-12 · note from ConsensFlow]\nhi',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'result', body: 'Parser done' }),
      '[ConsensFlow m-12 · T-3 · result from @zeus]\nParser done\n\nDecide with: cf task accept T-3 · cf task reopen T-3 "…"',
      'a result says what to do with it: it is not a request',
    )
  })

  it('sends a long body as its opening and the command that reads the rest', () => {
    const text = deliveryText({
      id: 7,
      taskNumber: 1,
      sender: 'zeus',
      kind: 'result',
      body: 'x'.repeat(20_000),
    })
    assert.ok(text.length < 5_000)
    assert.match(
      text,
      /\n… \(20000 characters; read all of it with: cf inbox read m-7\)\n\nDecide with: /,
    )
  })
})

/** A tiered staff: standard workers (zeus and diana, unless told), a light worker, and two standard reviewers. */
async function withTiers(context, { workers = ['zeus', 'diana'] } = {}) {
  const member = (agent, role, tier) => ({ agent, harness: 'claude-code', role, tier })
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    harness: 'claude-code',
    staff: [
      ...workers.map((agent) => member(agent, 'worker', 'standard')),
      member('hera', 'worker', 'light'),
      member('calliope', 'reviewer', 'standard'),
      member('astraeus', 'reviewer', 'standard'),
    ],
  })
  const id = (handle) =>
    context.ledger.project(project.id).participants.find((p) => p.handle === handle).id
  const open = (extra = {}) =>
    context.ledger.createTask(project.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Write the parser',
      ...extra,
    }).task
  const task = (number) => context.ledger.task(project.id, number)
  const notes = (handle) =>
    context.ledger
      .inbox(id(handle))
      .filter((m) => m.kind === 'note')
      .reverse()
      .map((m) => m.body)
  return { project, id, open, task, notes }
}

describe('the dispatcher assigns open tasks', () => {
  it('gives an open task to the free member of its tier with the fewest tasks so far, the earliest joined first', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['queued', 'zeus-amber-pine'])
      assert.equal(context.host.last('zeus').id, 'p1-zeus-amber-pine')
      open({ body: 'Write the docs' })
      await context.dispatcher.pass()
      assert.match(task(2).assignee, /^diana-/, 'zeus is busy')
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'parser done')
      context.adapter.answer('diana', 'docs done')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(2).state], ['done', 'done'])

      open({ body: 'Lexer' })
      await context.dispatcher.pass()
      assert.match(
        task(3).assignee,
        /^zeus-/,
        'one task each: the earliest joined of two free members',
      )
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'lexer done')
      await context.dispatcher.pass()
      open({ body: 'Tests' })
      await context.dispatcher.pass()
      assert.match(task(4).assignee, /^diana-/, 'the member with fewer tasks so far')
      open({ tier: 'light', body: 'Rename a file' })
      await context.dispatcher.pass()
      assert.match(task(5).assignee, /^hera-/)
    })
  })

  it('closes the old window of a task the human reassigned and gives the task to another member', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      const old = context.host.last('zeus')
      context.ledger.releaseTask(1, 1, { because: 'by @human' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(context.host.killed, [{ id: old.id, generation: old.generation }])
      assert.match(task(1).assignee, /^diana-/, 'zeus has the most tasks taken now')
    })
  })

  it('tells the requester once when nobody of the tier is free, and assigns when one frees up', async () => {
    await setup(async (context) => {
      const { open, task, notes } = await withTiers(context)
      // Nothing caps a member's sessions: only quota keeps one from new work.
      open({ body: 'One' })
      open({ body: 'Two' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.quota('zeus', { state: 'low', usedPercent: 97 })
      context.adapter.quota('diana', { state: 'low', usedPercent: 96 })
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'done')
      context.adapter.answer('diana', 'done')
      await context.dispatcher.pass()
      open({ body: 'Third' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(3).state, 'open')
      assert.deepEqual(notes('chief'), [
        'T-3 waits for a free standard worker: @zeus is low on quota; @diana is low on quota.',
      ])
      await context.dispatcher.pass()
      assert.equal(notes('chief').length, 1, 'told once')
      context.clock.advance(2 * 3_600_000)
      await context.dispatcher.pass()
      assert.match(task(3).assignee, /^zeus-/, 'the earliest joined, once the hour has passed')
    })
  })
})

describe('the dispatcher runs a review like any task', () => {
  it('gives a review to a reviewer of its tier in a session of its own, and brings its findings back as the result', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'done', 'nothing is reviewed on its own')
      open({ pool: 'reviewer', body: 'Review T-1: the parser in src/parse.js' })
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(2).pool, task(2).assignee, task(2).state],
        ['reviewer', 'calliope-brisk-birch', 'queued'],
        'the earliest joined reviewer of the tier',
      )
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.role, launch.instructions],
        ['reviewer', 'instructions for reviewer'],
      )
      assert.match(launch.message, /Review T-1: the parser in src\/parse\.js/)
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'working')
      context.adapter.answer('calliope', 'No test for empty input.')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(2).state], ['done', 'done'], 'the chief decides both')
      const result = task(2).messages.at(-1)
      assert.deepEqual(
        [result.kind, result.recipient, result.body],
        ['result', 'chief', 'No test for empty input.'],
      )
    })
  })
})

describe('the dispatcher watches quota', () => {
  const soon = (context, hours) =>
    new Date(context.clock.now().getTime() + hours * 3_600_000).toISOString()

  it('takes a task back from a member that ran out and gives it to another, telling the requester', async () => {
    await setup(async (context) => {
      const { open, task, notes, id } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      const refused = context.host.last('zeus')
      context.adapter.quota('zeus', { state: 'exhausted', resetsAt: soon(context, 2) })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['queued', 'diana-brisk-birch'])
      assert.match(
        task(1).body,
        /Reassigned from @zeus-amber-pine \(ran out of quota after starting\); check the working tree/,
      )
      assert.deepEqual(notes('chief'), [
        'T-1 was taken back from @zeus-amber-pine (ran out of quota after starting) and waits for another standard worker.',
      ])
      assert.equal(
        context.ledger.project(1).participants.find((p) => p.id === id('zeus')).outUntil,
        soon(context, 2),
      )
      assert.equal(
        context.ledger.project(1).participants.some((p) => p.handle === 'zeus-amber-pine'),
        true,
        'the session that ran out stays for the human; the member is out until its reset',
      )
      // A harness that waits out its limit (OpenCode) would take the task up
      // again at the reset, beside the member that has it now.
      assert.deepEqual(
        context.host.killed,
        [{ id: refused.id, generation: refused.generation }],
        "the refused session's window closes with its work",
      )
    })
  })

  it('keeps a member out only until its reset, though its harness still shows the old refusal', async () => {
    await setup(async (context) => {
      const { open, task, id } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const refusedAt = context.clock.now().toISOString()
      // A real transcript keeps its last record: the refusal stays in view
      // until the member gets a turn, which it cannot while it is out.
      context.adapter.quota('zeus', {
        state: 'exhausted',
        at: refusedAt,
        resetsAt: soon(context, 1),
      })
      context.adapter.agent('zeus').settled = true
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.match(task(1).assignee, /^diana-/)
      context.adapter.answer('diana', 'done')
      await context.dispatcher.pass()

      context.clock.advance(2 * 3_600_000)
      open({ body: 'Lexer' })
      await context.dispatcher.pass()
      assert.match(task(2).assignee, /^zeus-/, 'the old refusal is not a new one')
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'working', 'and zeus receives again')
      assert.equal(context.dispatcher.activity(id(task(2).assignee)).state, 'working')

      context.adapter.quota('zeus', {
        state: 'exhausted',
        at: context.clock.now().toISOString(),
        resetsAt: soon(context, 1),
      })
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'open', 'a fresh refusal counts')
      assert.equal(
        context.ledger.project(1).participants.find((p) => p.id === id('zeus')).outUntil,
        soon(context, 1),
      )
    })
  })

  it("queues a delivery in flight at the refusal again, and keeps a coordinator's own tasks for after its reset", async () => {
    await setup(async (context) => {
      const { open, task, id } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const question = context.ledger.ask(1, {
        from: 'zeus-amber-pine',
        to: 'chief',
        task: 1,
        body: 'Which?',
      })
      context.adapter.agent('zeus').arrive = false
      context.adapter.answer('zeus', 'asked')
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'This one, replying')
      const answer = context.ledger.answer(question.id, { from: 'chief', body: 'This one' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(answer.id).state, 'delivering')
      context.adapter.quota('zeus', { state: 'exhausted', at: context.clock.now().toISOString() })
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(1).assignee, context.ledger.message(answer.id).state],
        ['open', null, 'cancelled'],
        'the task goes back to the board and the answer in flight goes with it',
      )

      context.adapter.answer('chief', 'noted')
      const own = context.ledger.createTask(1, { from: 'human', to: 'chief', body: 'Plan' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(own.task.number).state, 'working')
      context.adapter.quota('chief', { state: 'exhausted', at: context.clock.now().toISOString() })
      context.adapter.agent('chief').settled = true
      await context.dispatcher.pass()
      assert.equal(task(own.task.number).state, 'working', 'a coordinator keeps its task')
      assert.equal(context.dispatcher.activity(id('chief')).state, 'out')
      const later = context.ledger.note(1, { from: 'zeus', to: 'chief', body: 'Ready' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(later.id).state, 'queued', 'nothing reaches it while out')
      context.clock.advance(2 * 3_600_000)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.message(later.id).state,
        'delivered',
        'its queue resumes after the reset',
      )
    })
  })

  it('gives no new work to a member low on quota, keeps one out for an hour when its reset is unknown, and takes it again after', async () => {
    await setup(async (context) => {
      const { open, task, id } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.quota('zeus', { state: 'low', usedPercent: 97 })
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'done')
      await context.dispatcher.pass()
      open({ body: 'Lexer' })
      await context.dispatcher.pass()
      assert.match(task(2).assignee, /^diana-/, 'zeus is low on quota')
      open({ body: 'Tests' })
      await context.dispatcher.pass()
      assert.match(
        task(3).assignee,
        /^diana-/,
        'diana again: nothing caps her sessions, zeus is low',
      )
      context.clock.advance(2 * 3_600_000)
      open({ body: 'Docs' })
      await context.dispatcher.pass()
      assert.match(task(4).assignee, /^zeus-/, 'eligible again after the hour, in a fresh window')
      await context.dispatcher.pass()
      context.adapter.quota('zeus', { state: 'exhausted' })
      await context.dispatcher.pass()
      assert.deepEqual(
        [
          task(4).state,
          context.ledger.project(1).participants.find((p) => p.id === id('zeus')).outUntil,
        ],
        ['open', soon(context, 1)],
        'out of quota mid-task: the work goes back, zeus is out for an hour',
      )
    })
  })
})

describe('a member out of quota mid-task', () => {
  it('holds the task with its window when the reset is near, and goes on by itself when it passes', async () => {
    await setup(async (context) => {
      const { open, task, notes, id } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['working', 'zeus-amber-pine'])
      const native = context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession
      const resetsAt = new Date(context.clock.now().getTime() + 20 * 60_000).toISOString()
      context.adapter.quota('zeus', { state: 'exhausted', resetsAt })
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(1).assignee, task(1).heldUntil],
        ['paused', 'zeus-amber-pine', resetsAt],
        'held with its window: diana is free, but the reset is twenty minutes away',
      )
      assert.match(
        notes('chief').at(-1),
        /^T-1 waits with @zeus-amber-pine: out of quota until .*; it goes on by itself then\.$/,
      )
      assert.deepEqual(context.host.killed, [], 'the window waits, as any paused task’s')
      // The agent, stopped, says where it was; that is not a result while the task is held.
      context.adapter.answer('zeus', 'Stopped at the lexer.')
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'paused', 'nothing moves before the reset')
      context.clock.advance(21 * 60_000)
      // The window no longer shows the refusal once the reset has passed.
      context.adapter.quota('zeus', null)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['working', 'zeus-amber-pine'])
      // The same window, on the same conversation, with the words of a Resume.
      const resumed = context.ledger.inbox(id('zeus-amber-pine'))[0]
      assert.deepEqual([resumed.state, resumed.kind], ['delivered', 'task'])
      assert.match(resumed.body, /^Resumed: Go on where you stopped\.$/)
      assert.equal(context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession, native)
    })
  })

  it('holds the task when nobody else of its tier is free, whatever the reset', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const resetsAt = new Date(context.clock.now().getTime() + 3 * 3_600_000).toISOString()
      context.adapter.quota('zeus', { state: 'exhausted', resetsAt })
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).heldUntil], ['paused', resetsAt])
    })
  })
})

describe('a member with several roles', () => {
  it('opens with the text of the role its task needs, in a session per task', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        harness: 'claude-code',
        staff: [
          { agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' },
          {
            agent: 'hera',
            harness: 'claude-code',
            roles: ['worker', 'reviewer', 'advisor'],
            tier: 'complex',
          },
        ],
      })
      const task = (number) => context.ledger.task(project.id, number)
      const launches = (handle) =>
        context.adapter.prepared
          .filter((request) => request.participant.handle.startsWith(`${handle}-`))
          .map((request) => [request.role, request.instructions])
      const open = (body, tier, pool = 'worker') =>
        context.ledger.createTask(project.id, { from: 'chief', pool, tier, body })
      open('Write the parser', 'standard')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).assignee, 'zeus-amber-pine')
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'done')
      open('Review T-1: the parser', 'complex', 'reviewer')
      await context.dispatcher.pass()
      assert.equal(task(2).assignee, 'hera-brisk-birch')
      assert.deepEqual(
        launches('hera'),
        [['reviewer', 'instructions for reviewer']],
        'a review opens the reviewer text',
      )
      open('Write the docs', 'complex')
      await context.dispatcher.pass()
      assert.match(task(3).assignee, /^hera-/, 'hera reviews in one session and works in another')
      assert.notEqual(task(3).assignee, task(2).assignee)
      await context.dispatcher.pass()
      context.adapter.answer(task(2).assignee, 'Fine.')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'done')
      await context.dispatcher.pass()
      assert.deepEqual(
        launches('hera').at(-1),
        ['worker', 'instructions for worker'],
        'and opens with the worker text',
      )
      context.adapter.answer(task(3).assignee, 'Docs done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      open('Which parser design?', 'complex', 'advisor')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        launches('hera').at(-1),
        ['advisor', 'instructions for advisor'],
        'advice opens with the advisor text, whatever role the member was saved with first',
      )
      assert.match(task(4).assignee, /^hera-/)
      context.adapter.answer(task(4).assignee, 'The second.')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(4).state, 'done')
      assert.equal(context.ledger.task(project.id, 4).messages.at(-1).recipient, 'chief')
    })
  })
})

describe('one task per member session', () => {
  const zeusWindows = (context) => context.host.opened.filter((b) => windowOf(b.id, 'zeus'))

  /** T-1 to zeus, the only standard worker here, delivered and answered. */
  async function finished(context) {
    const fixture = await withTiers(context, { workers: ['zeus'] })
    fixture.open()
    await context.dispatcher.pass()
    await context.dispatcher.pass()
    assert.equal(fixture.task(1).state, 'working')
    context.adapter.answer('zeus', 'Parser done')
    await context.dispatcher.pass()
    return fixture
  }

  it("closes a session's window with its task but keeps its conversation, and opens a fresh session for the next task", async () => {
    await setup(async (context) => {
      const { project, id, open, task } = await finished(context)
      const first = context.host.last('zeus')
      const session = id('zeus-amber-pine')
      assert.equal(task(1).state, 'done')
      assert.deepEqual(context.host.killed, [{ id: first.id, generation: first.generation }])
      assert.ok(
        context.ledger.currentConversation(session),
        'the conversation stays until the work is accepted, for a follow-up',
      )
      assert.equal(context.dispatcher.pane(session), null, 'the window went with the task')

      open({ body: 'Write the lexer' })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['zeus-brisk-birch', null],
        'a fresh session of the same member',
      )
      assert.match(launch.message, /Write the lexer/)
      assert.equal(zeusWindows(context).length, 2)

      context.ledger.acceptTask(project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      assert.notEqual(
        context.ledger.currentConversation(session),
        null,
        'accepted: the session keeps its conversation for the human',
      )
      await context.dispatcher.endSession(project.id, 'zeus-amber-pine')
      assert.equal(context.ledger.currentConversation(session), null)
      assert.equal(
        context.ledger.project(project.id).participants.some((p) => p.id === session),
        false,
      )
    })
  })

  it("opens the next task's session while the old window is still closing", async () => {
    await setup(async (context) => {
      context.host.holdExits = true
      const { open, task } = await finished(context)
      open({ body: 'Write the lexer' })
      await context.dispatcher.pass()
      assert.equal(task(2).assignee, 'zeus-brisk-birch')
      assert.equal(zeusWindows(context).length, 2, 'a session of its own waits for no window')
      assert.equal(context.adapter.prepared.at(-1).resume, null)
      await context.host.exit('zeus-amber-pine')
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'working', 'the old exit lands on the old session only')
    })
  })

  it('never closes a coordinator: the chief keeps its window after its own task', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Plan the week' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'Planned.')
      context.ledger.recordResult(project.id, 1, { body: 'Planned.' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'done')
      assert.deepEqual(context.host.killed, [])
      assert.notEqual(context.dispatcher.pane(id('chief')), null)
    })
  })

  it('after a restart, pauses a member task with no window for the chief to resume, and resumes one whose answer is due', async () => {
    await setup(async (context) => {
      const { project, id, open, task } = await withTiers(context)
      open()
      open({ body: 'Write the lexer' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).assignee, task(2).assignee],
        ['zeus-amber-pine', 'diana-brisk-birch'],
      )
      const question = context.ledger.ask(project.id, {
        from: 'diana-brisk-birch',
        to: 'chief',
        task: 2,
        body: 'Which dialect?',
      })
      context.adapter.answer('diana', 'I asked the chief.')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(2).state], ['working', 'waiting'])
      const native = context.ledger.currentConversation(id('diana-brisk-birch')).nativeSession

      const nativeZeus = context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession
      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      context.ledger.answer(question.id, { from: 'chief', body: 'ANSI' })
      await after.pass()
      assert.deepEqual(
        [task(1).state, task(1).assignee],
        ['paused', 'zeus-amber-pine'],
        'its window is gone; its session and conversation wait for the chief',
      )
      assert.match(
        task(1).messages.find((m) => m.kind === 'note').body,
        /^T-1 is paused: @zeus-amber-pine's window is gone\. Resume it with: cf task resume T-1/,
      )
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['diana-brisk-birch', native],
        'its own session, with the brief in it',
      )
      assert.match(launch.message, /^\[ConsensFlow m-\d+ · T-2 · answer from @chief\]\nANSI$/)
      await after.pass()
      assert.equal(task(2).state, 'working')
      context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      await after.pass()
      const back = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [back.participant.handle, back.resume],
        ['zeus-amber-pine', nativeZeus],
        'the same conversation, with its memory',
      )
      assert.match(back.message, /T-1 · task from @chief\]\nResumed: Go on$/)
    })
  })

  it('reopens a finished task on its own session, resumed with the follow-up and nothing else', async () => {
    await setup(async (context) => {
      const { project, id, task } = await finished(context)
      const native = context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession
      context.ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Handle empty input too' })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['zeus-amber-pine', native],
        'the same window comes back on its own conversation',
      )
      assert.match(
        launch.message,
        /^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nHandle empty input too$/,
        'it remembers the brief: only the follow-up goes in',
      )
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
    })
  })

  it("continues a finished task's session with --after: the same window comes back on its conversation", async () => {
    await setup(async (context) => {
      const { project, id, task } = await finished(context)
      const native = context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession
      context.ledger.createTask(project.id, {
        from: 'chief',
        after: 1,
        body: 'Now the lexer, in the same style',
      })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['zeus-amber-pine', native],
        'the session that did T-1, on its own conversation',
      )
      assert.match(
        launch.message,
        /^\[ConsensFlow m-\d+ · T-2 · task from @chief\]\nNow the lexer, in the same style$/,
        'no brief in front: the window remembers',
      )
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'working')
      context.adapter.answer('zeus-amber-pine', 'Lexer done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'done')
      assert.equal(context.host.killed.length, 2, 'the window closed after each task')
    })
  })

  it("keeps a session after its work until the human ends it, and opens, closes and ends its window at the human's hand", async () => {
    await setup(async (context) => {
      const { project, open, task } = await finished(context)
      await context.dispatcher.pass()
      assert.ok(
        context.ledger.project(project.id).participants.some((p) => p.handle === 'zeus-amber-pine'),
        'nothing expires',
      )
      assert.equal(task(1).state, 'done', 'its work stays for the chief to accept')
      const native = context.ledger.currentConversation(
        context.ledger.project(project.id).participants.find((p) => p.handle === 'zeus-amber-pine')
          .id,
      ).nativeSession

      // The human opens the window again: on its conversation, with nothing to deliver, and it stays.
      await context.dispatcher.openWindow(project.id, 'zeus-amber-pine')
      const reopened = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [reopened.participant.handle, reopened.resume, reopened.message],
        ['zeus-amber-pine', native, null],
      )
      const killed = context.host.killed.length
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.host.killed.length, killed, 'a window the human opened is not retired')
      await context.dispatcher.closeWindow(project.id, 'zeus-amber-pine')
      assert.equal(context.host.killed.length, killed + 1, "closed at the human's hand")

      // Ending the session closes its window and folds it away.
      open({ body: 'Docs' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const working = task(2)
      assert.equal(working.state, 'working')
      await assert.rejects(context.dispatcher.endSession(project.id, working.assignee), {
        code: 'session-busy',
      })
      context.ledger.cancelTask(project.id, 2, { by: 'chief' })
      await context.dispatcher.pass()
      assert.equal(
        context.host.killed.at(-1).id,
        `p${project.id}-${working.assignee}`,
        'the window closes with its work, the session stays',
      )
      await context.dispatcher.endSession(project.id, working.assignee)
      assert.equal(
        context.ledger.project(project.id).participants.some((p) => p.handle === working.assignee),
        false,
      )
    })
  })
})

describe('the dispatcher traces what its windows do', () => {
  it("tells a trace each change of a window's activity, by participant", async () => {
    const entries = []
    await setup(
      async (context) => {
        const { open, task } = await withTiers(context)
        open()
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.equal(task(1).state, 'working')
        const activity = entries.filter((entry) => entry.kind === 'window.activity')
        assert.deepEqual(activity.map((entry) => [entry.participant, entry.state]).slice(0, 2), [
          ['chief', 'idle'],
          ['zeus-amber-pine', 'working'],
        ])
        assert.deepEqual(Object.keys(activity[0]).sort(), [
          'at',
          'kind',
          'participant',
          'project',
          'reason',
          'state',
        ])
      },
      { trace: (entry) => entries.push(entry) },
    )
  })
})

describe('a window that is not ready for a paste', () => {
  it('says so once in the trace, and delivers when the window is ready', async () => {
    const entries = []
    let ready = false
    await setup(
      async (context) => {
        const { id, open, task } = await withTiers(context)
        context.adapter.ready = async () => ready
        open()
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.equal(task(1).state, 'working')
        context.adapter.answer('zeus', 'Parser done.')
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.equal(task(1).state, 'done')
        const result = () => context.ledger.inbox(id('chief')).find((m) => m.kind === 'result')
        assert.equal(result().state, 'queued', 'the chief is typing: the result waits')
        const held = entries.filter(
          (entry) => entry.kind === 'delivery.held' && entry.message === result().id,
        )
        assert.equal(held.length, 1, 'said once, not every pass')
        assert.deepEqual([held[0].participant, held[0].project], ['chief', 1])
        assert.match(held[0].reason, /not ready for a paste/)
        ready = true
        await context.dispatcher.pass()
        assert.notEqual(result().state, 'queued', 'delivered once the window is ready')
      },
      { trace: (entry) => entries.push(entry) },
    )
  })
})

describe('a member whose saved agent is gone', () => {
  it('gives a member whose agent is gone no work: the task waits and the chief hears why', async () => {
    await setup(
      async (context) => {
        const { open, task, notes } = await withTiers(context, { workers: ['zeus'] })
        open()
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.deepEqual([task(1).state, task(1).assignee], ['open', null])
        assert.match(
          notes('chief').at(-1),
          /T-1 waits for a free standard worker: @zeus has no agent any more \(zeus is not among your agents: define it, or remove the member\)/,
        )
        assert.equal(context.host.opened.length, 1, 'only the chief window opened')
      },
      { roster: (name) => (name === 'zeus' ? null : { id: name, model: 'm', profile: {} }) },
    )
  })

  it('after a release drops the agent of a working member, a restart sends its task back to the board', async () => {
    let gone = false
    await setup(
      async (context) => {
        const { open, task, notes } = await withTiers(context, { workers: ['zeus'] })
        open()
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.deepEqual([task(1).state, task(1).assignee], ['working', 'zeus-amber-pine'])
        // The human installs a release without zeus's entry and the daemon starts again.
        gone = true
        context.ledger.suspendForRestart()
        const after = context.make()
        await after.resumeAfterRestart()
        await after.pass()
        assert.deepEqual([task(1).state, task(1).assignee], ['open', null])
        assert.match(
          task(1).body,
          /Reassigned from @zeus-amber-pine \(zeus is no longer among your agents\)/,
        )
        await after.pass()
        assert.match(
          notes('chief').at(-1),
          /waits for a free standard worker: @zeus has no agent any more/,
        )
        assert.equal(
          context.host.opened.length,
          3,
          'the chief before and after the restart, zeus before it, nothing for zeus after',
        )
      },
      {
        roster: (name) => (name === 'zeus' && gone ? null : { id: name, model: 'm', profile: {} }),
      },
    )
  })
})
