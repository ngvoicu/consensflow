import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { unnamed } from '../src/adapters/shared.js'
import { deliveryText, markerOf } from '../src/core/delivery-text.js'
import { Dispatcher } from '../src/core/dispatcher.js'
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
        launch: { launchId: request.launchId, nativeSession: agent.native },
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
    async record({ conversation }) {
      const agent = [...agents.values()]
        .filter((a) => a.native === conversation.nativeSession)
        .at(-1)
      return agent === undefined ? { unknown: true } : { items: [...agent.items] }
    },
    async observe({ launch }) {
      const agent = agents.get(launch.launchId)
      // A window that shows another conversation than its launch's: that
      // record's last look, and which session it shows now.
      if (agent.shows !== undefined && agent.shows !== launch.nativeSession) {
        return {
          items: [...agent.records[launch.nativeSession]],
          settled: false,
          waiting: null,
          quota: agent.quota,
          failed: false,
          switched: { nativeSession: agent.shows },
        }
      }
      const observed = {
        items: [...agent.items],
        settled: agent.settled,
        waiting: agent.waiting,
        quota: agent.quota,
        failed: false,
      }
      // A window that has not said which conversation it shows, and why a message waits.
      return agent.unnamed === undefined ? observed : unnamed(observed, agent.unnamed)
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
  // The human switches the window to another conversation (/clear, /new,
  // /resume): from then on it writes that one's record, idle at first.
  adapter.switchTo = (handle, native) => {
    const agent = adapter.agent(handle)
    agent.records ??= {}
    agent.records[agent.shows ?? agent.native] = agent.items
    agent.items = agent.records[native] ?? []
    agent.shows = native
    agent.settled = true
  }
  return adapter
}

/** The saved model of each fake agent, as the roster gives it at launch. */
const MODELS = {
  apollo: 'claude-opus-5',
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
  const host = {
    opened: [],
    killed: [],
    requests: [],
    refuse: false,
    hold: null,
    holdExits: false,
    async request(op, body) {
      host.requests.push([op, body])
      return { ok: true, ...(op === 'pane.snapshot' ? host.snapshot : {}) }
    },
    async open(body) {
      if (host.refuse) {
        host.refuse = false
        return { ok: false, error: 'refused by the test' }
      }
      await host.hold
      host.opened.push(body)
      // The window's process, when the test names one.
      return { ok: true, id: body.id, generation: body.generation, pid: host.pid }
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

/** One turn of the event loop: what the fakes started (a launch, a delivery) is over by then. */
const flush = () => new Promise((resolve) => setImmediate(resolve))

/**
 * A launch or a delivery goes on apart from the pass or the operation that
 * started it. The fakes here answer at once, so one turn of the event loop
 * sees it through: each of these calls a test awaits waits that turn too.
 */
function settled(dispatcher) {
  for (const name of [
    'pass',
    'openProject',
    'resumeProject',
    'openWindow',
    'switchChief',
    'resumeAfterRestart',
  ]) {
    const call = dispatcher[name].bind(dispatcher)
    dispatcher[name] = async (...args) => {
      const value = await call(...args)
      await flush()
      return value
    }
  }
  return dispatcher
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
  // What failed apart from any pass is written down; no test may leave one.
  const failures = []
  const { adapters: more = {}, ...rest } = options
  const make = () =>
    settled(
      new Dispatcher({
        ledger,
        host,
        // The fake answers for any harness; OpenCode is here for a mixed staff.
        adapters: { 'claude-code': adapter, opencode: adapter, ...more },
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
        log: { error: (_message, cause) => failures.push(cause) },
        ...rest,
      }),
    )
  try {
    await fn({ ledger, adapter, host, clock, dispatcher: make(), make, dir })
    assert.deepEqual(failures, [], 'nothing failed unseen')
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
    chief: { harness: 'claude-code', agent: 'apollo' },
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
      assert.equal(conversation.nativeSession, `native-${context.adapter.prepared[0].launchId}`)
      assert.equal(context.dispatcher.activity(id('chief')).state, 'starting')
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(id('chief')).state, 'idle')
    })
  })

  it('opens a project whose chief runs on its saved agent, and none whose chief names no agent or one not among the agents', async () => {
    await setup(
      async (context) => {
        await assert.rejects(
          context.dispatcher.openProject({
            directory: '/work/app',
            name: 'app',
            chief: { harness: 'claude-code' },
          }),
          {
            message:
              'pick one of your saved agents for the chief: its harness, model and effort come with it',
          },
        )
        await assert.rejects(
          context.dispatcher.openProject({
            directory: '/work/app',
            name: 'app',
            chief: { harness: 'claude-code', agent: 'nobody' },
          }),
          /nobody is not among your agents/,
        )
        assert.deepEqual(context.ledger.projects(), [], 'nothing was opened')
        const { project } = await withStaff(context)
        const chief = project.participants.find((p) => p.handle === 'chief')
        assert.deepEqual([chief.harness, chief.agent], ['claude-code', 'apollo'])
        assert.equal(context.adapter.prepared[0].agent.model, 'claude-opus-5', "its agent's model")
      },
      {
        roster: (name) =>
          name === 'nobody' ? null : { id: name, model: MODELS[name], profile: {} },
      },
    )
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

  it('keeps everything a member wrote in its turn, not only its last message', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Write the report' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      // The report, a command, then a short last word: collecting the last
      // message alone would give the chief "Committed." and nothing else.
      const report = `${'Line of the report.\n'.repeat(500)}The code at the end: CEDRU-7314`
      const zeus = context.adapter.agent('zeus')
      // As every adapter reports it: a message that ended in a tool call is not
      // complete; only the one that ends the turn is.
      zeus.items.push(
        item('assistant', report, { complete: false }),
        item('tool', 'git commit: 1 file changed'),
      )
      context.adapter.answer('zeus', 'Committed.')
      await context.dispatcher.pass()
      const result = context.ledger.task(project.id, 1).messages.find((m) => m.kind === 'result')
      assert.ok(result.body.includes(report), 'the report, whole')
      assert.ok(result.body.includes('Committed.'), 'and the last word')
      assert.ok(
        !result.body.includes('git commit: 1 file changed'),
        "a tool's output is not the member's words",
      )
    })
  })

  it("leaves a harness's commentary out of a result: Codex's progress notes are not its answer", async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Review the commit',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      // Codex writes notes as it works, each marked as commentary, and ends
      // the turn with its final answer: the verdict, and all the chief needs.
      const zeus = context.adapter.agent('zeus')
      zeus.items.push(
        item('assistant', "I'll read the commit, then check it on disk.", {
          complete: false,
          commentary: true,
        }),
        item('tool', 'git show --stat'),
        item('assistant', 'The diff matches; checking the counts now.', {
          complete: false,
          commentary: true,
        }),
      )
      context.adapter.answer('zeus', 'PASS: no findings.')
      await context.dispatcher.pass()
      const result = context.ledger.task(project.id, 1).messages.find((m) => m.kind === 'result')
      assert.equal(result.body, 'PASS: no findings.')
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
        [['pane.input', { id: pane.id, generation: pane.generation, bytes: [27] }]],
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

  it('interrupts a task paused again after a resume, however many rounds its first pause took', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const escapes = () => context.host.requests.filter(([op]) => op === 'pane.input').length
      // A harness that ignores the key: three rounds, then no more for this pause.
      context.adapter.busy('zeus')
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      for (let round = 0; round < 4; round += 1) {
        await context.dispatcher.pass()
        context.clock.advance(3_100)
      }
      assert.equal(escapes(), 3, 'three rounds for the first pause')
      context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Carry on' })
      await context.dispatcher.pass()
      context.adapter.busy('zeus')
      context.clock.advance(1_000)
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      assert.equal(escapes(), 4, 'the second pause is a stop of its own')
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
        from: told.recipientId,
        body: 'a.txt',
      })
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      assert.equal(escapes(), 1, 'nor its wrap-up after the answer')
    })
  })

  it('never stops a window for how long it works: a six-hour command or model turn is left alone', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context, ['zeus', 'diana'])
      context.ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Run the Rust tests',
      })
      context.ledger.createTask(project.id, {
        from: 'chief',
        to: 'diana',
        body: 'Write the report',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      context.adapter.busy('zeus')
      context.adapter.busy('diana')
      // One command that runs for hours (Rust tests take 4 to 6), or a model
      // turn that goes on: nothing in any record moves, and nothing is stopped.
      await context.dispatcher.pass()
      for (let hour = 0; hour < 6; hour += 1) {
        context.clock.advance(60 * 60_000)
        await context.dispatcher.pass()
      }
      assert.deepEqual(context.host.killed, [], 'no window closed')
      assert.deepEqual(
        [1, 2].map((n) => context.ledger.task(project.id, n).state),
        ['working', 'working'],
      )
      assert.equal(context.ledger.project(project.id).state, 'open')
      const participants = context.ledger.project(project.id).participants
      for (const handle of ['chief', 'zeus', 'diana']) {
        const participant = participants.find((p) => p.handle === handle)
        assert.equal(context.dispatcher.activity(participant.id).state, 'working', handle)
      }
      const chief = participants.find((p) => p.handle === 'chief')
      assert.deepEqual(
        context.ledger.inbox(chief.id).filter((m) => m.kind === 'note'),
        [],
        'no note',
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

  it("tells the adapter the window's process, when the pane host names it, before it starts", async () => {
    await setup(async (context) => {
      context.host.pid = 4242
      const seen = []
      const { started, observe } = context.adapter
      context.adapter.started = async (request) => {
        seen.push(['started', request.launch.pid])
        return started(request)
      }
      context.adapter.observe = async (request) => {
        seen.push(['observe', request.launch.pid])
        return observe(request)
      }
      await withStaff(context)
      await context.dispatcher.pass()
      assert.deepEqual(seen, [
        ['started', 4242],
        ['observe', 4242],
      ])
    })
  })

  it('reads a window that has not named its first conversation as starting, its messages held', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      // A resumed window has its record from the start; its harness has not
      // said yet which conversation the window shows.
      const chief = context.adapter.agent('chief')
      chief.items.push(item('user', 'earlier'), item('assistant', 'earlier answer'))
      chief.unnamed = 'the window has not said yet which conversation it shows'
      const note = context.ledger.note(project.id, { to: 'chief', body: 'A result came' })
      await context.dispatcher.pass()
      assert.deepEqual(context.dispatcher.activity(id('chief')), { state: 'starting' })
      assert.equal(context.ledger.message(note.id).state, 'queued', 'its message waits')

      chief.unnamed = undefined
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivered')
      // Once the window has named one, showing none is a wait the board names.
      chief.unnamed = 'the window shows no conversation: its session list is open'
      await context.dispatcher.pass()
      assert.deepEqual(context.dispatcher.activity(id('chief')), {
        state: 'waiting',
        reason: 'the window shows no conversation: its session list is open',
      })
    })
  })

  it('reads a window that names no conversation as starting while it still draws its screen', async () => {
    await setup(async (context) => {
      context.host.snapshot = { outputQuietMs: 300 }
      const { id } = await withStaff(context)
      const chief = context.adapter.agent('chief')
      chief.items.push(item('user', 'earlier'), item('assistant', 'earlier answer'))
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(id('chief')).state, 'idle', 'it named its own')
      chief.unnamed = 'the window is reconnecting'
      await context.dispatcher.pass()
      assert.deepEqual(context.dispatcher.activity(id('chief')), { state: 'starting' })
      context.host.snapshot = { outputQuietMs: 2_000 }
      await context.dispatcher.pass()
      assert.deepEqual(context.dispatcher.activity(id('chief')), {
        state: 'waiting',
        reason: 'the window is reconnecting',
      })
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
          ['pane.input', { id: pane.id, generation: pane.generation, bytes: [27] }],
          ['pane.input', { id: pane.id, generation: pane.generation, bytes: [27] }],
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

  it('presses Enter once more for a paste its window has not sent, into a quiet window only', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      const chief = context.adapter.agent('chief')
      chief.arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      const enters = () =>
        context.host.requests.filter(([op, body]) => op === 'pane.input' && body.bytes[0] === 13)
      context.clock.advance(9_000)
      context.host.snapshot = { outputQuietMs: 5_000 }
      await context.dispatcher.pass()
      assert.equal(enters().length, 0, 'not before the record had its time')
      // Printing a moment ago: being typed into, or still drawing.
      context.host.snapshot = { outputQuietMs: 500 }
      context.clock.advance(2_000)
      await context.dispatcher.pass()
      assert.equal(enters().length, 0, 'not into a window that just printed')
      // Quiet now: the Enter sends what sat in the input, and the record shows it.
      context.host.snapshot = { outputQuietMs: 5_000 }
      const request = context.host.request
      context.host.request = async (op, body) => {
        if (op === 'pane.input' && body.bytes[0] === 13) {
          chief.items.push(item('user', deliveryText(context.ledger.message(note.id))))
        }
        return request(op, body)
      }
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['delivered', 1])
      assert.equal(enters().length, 1, 'pressed once')
    })
  })

  it('pastes again, once its arrival window passes, when one more Enter did not bring it', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.adapter.agent('chief').arrive = false
      context.host.snapshot = { outputQuietMs: 5_000 }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      const enters = () =>
        context.host.requests.filter(([op, body]) => op === 'pane.input' && body.bytes[0] === 13)
      context.clock.advance(11_000)
      await context.dispatcher.pass()
      context.clock.advance(5_000)
      await context.dispatcher.pass()
      assert.equal(enters().length, 1, 'once for the attempt')
      context.clock.advance(15_000)
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['delivering', 2])
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

  it('counts a delivery only by its header in what the window was given, never in a tool’s output', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.agent('chief').arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      // The agent's own command prints the header (cf inbox read, a grep of a log).
      const header = deliveryText(context.ledger.message(note.id))
      context.adapter.agent('chief').items.push(item('tool', header), item('custom', header))
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivering', 'not proof it arrived')
    })
  })

  it('records an adapter that throws as a failed attempt with its error, not as uncertain', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.adapter.deliver = async () => {
        throw new Error('the channel needs pane and generation')
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'hello' })
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('chief')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['queued', 1])
      assert.match(message.reason, /the channel needs pane and generation/)
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
        [['pane.input', { id: pane.id, generation: pane.generation, bytes: [27] }]],
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

  it('forgets the files a launch wrote when its window never opens: refused by the host, or its adapter failed', async () => {
    const forgotten = []
    await setup(
      async (context) => {
        const { project } = await withStaff(context)
        context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
        context.host.refuse = true
        await context.dispatcher.pass()
        assert.equal(context.ledger.task(project.id, 1).state, 'failed')
        assert.ok(
          forgotten.includes(context.adapter.prepared.at(-1).launchId),
          'the host refused the window',
        )

        let written = null
        context.adapter.prepare = async (request) => {
          written = request.launchId
          throw new Error('the settings could not be written')
        }
        context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Lexer' })
        await context.dispatcher.pass()
        assert.equal(context.ledger.task(project.id, 2).state, 'failed')
        assert.ok(forgotten.includes(written), 'the adapter failed after writing them')
      },
      { launchFiles: { forget: (launch) => forgotten.push(launch) } },
    )
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
      context.ledger.answer(question.id, { from: question.recipientId, body: 'JSON' })
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

  it('carries an advisor’s and a reviewer’s question to the chief and the answer back to their own window', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code', agent: 'apollo' },
        staff: [
          { agent: 'athena', harness: 'claude-code', role: 'advisor', tier: 'standard' },
          { agent: 'calliope', harness: 'claude-code', role: 'reviewer', tier: 'standard' },
        ],
      })
      const task = (number) => context.ledger.task(project.id, number)
      for (const [pool, agent, body] of [
        ['advisor', 'athena', 'Which law applies to the page?'],
        ['reviewer', 'calliope', 'Review the legislation page'],
      ]) {
        const { task: created } = context.ledger.createTask(project.id, {
          from: 'chief',
          pool,
          tier: 'standard',
          body,
        })
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        const session = task(created.number).assignee
        assert.match(session, new RegExp(`^${agent}-`), `${pool}: its own session`)
        const question = context.ledger.ask(project.id, {
          from: session,
          to: 'chief',
          task: created.number,
          body: `${pool}: which audience, managers or HR?`,
        })
        context.adapter.answer(agent, 'I asked the chief.')
        await context.dispatcher.pass()
        assert.equal(
          task(created.number).state,
          'waiting',
          `${pool}: waits for the answer, no result`,
        )
        // The chief's window gets the question, answers it.
        await context.dispatcher.pass()
        const chiefSaw = context.adapter
          .agent('chief')
          .items.map((i) => i.text)
          .join('\n')
        assert.match(chiefSaw, new RegExp(`question from @${session}\\]\\n${pool}: which audience`))
        context.adapter.answer('chief', 'Answered.')
        context.ledger.answer(question.id, { from: question.recipientId, body: 'Managers first.' })
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        const memberSaw = context.adapter
          .agent(agent)
          .items.map((i) => i.text)
          .join('\n')
        assert.match(
          memberSaw,
          /answer from @chief\]\nManagers first\./,
          `${pool}: the answer in its own window`,
        )
        assert.equal(task(created.number).state, 'working')
        context.adapter.answer(agent, `${pool} findings, for managers`)
        await context.dispatcher.pass()
        assert.equal(task(created.number).state, 'done')
        assert.equal(task(created.number).messages.at(-1).body, `${pool} findings, for managers`)
        // The chief takes one message at a time: it reads the result first.
        await context.dispatcher.pass()
        context.adapter.answer('chief', 'Read.')
        await context.dispatcher.pass()
      }
    })
  })

  it('closes the project when its chief window closes by itself, as Close does: every window goes and the work in them pauses', async () => {
    await setup(async (context) => {
      const { project, id, open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      const session = task(1).assignee
      const zeus = context.host.last('zeus')
      // The human types /exit in the chief's terminal, or the chief crashes.
      await context.host.exit('chief')
      assert.equal(context.ledger.project(project.id).state, 'suspended')
      assert.equal(context.ledger.project(project.id).resumeOnStart, false)
      assert.deepEqual(context.host.killed, [{ id: zeus.id, generation: zeus.generation }])
      assert.equal(context.dispatcher.pane(id(session)), null)
      assert.deepEqual([task(1).state, task(1).assignee], ['paused', session])
      assert.match(
        task(1).messages.find((m) => m.kind === 'note').body,
        /^T-1 is paused: @zeus-amber-pine's window closed\./,
      )
      await context.dispatcher.pass()
      assert.equal(context.host.opened.length, 2, 'nothing opens while it is closed')
    })
  })

  it('opens a project with the staff it is given, and only the chief window', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code', agent: 'apollo' },
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

  it('closes every window it still has of a deleted project, however the project was closed, before it forgets them', async () => {
    await setup(async (context) => {
      const { project, open } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const windows = [context.host.last('chief'), context.host.last('zeus')]
      // Closed in the ledger alone, as a restart marks a project, with its windows still up.
      context.ledger.setProjectState(project.id, 'suspended')
      await context.dispatcher.deleteProject(project.id)
      assert.deepEqual(
        context.host.killed,
        windows.map(({ id, generation }) => ({ id, generation })),
      )
    })
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

describe('a restart while a message is on its way', () => {
  it('gives a message its window never showed back to the queue with its attempt, and the chief comes back to it and to what follows', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.agent('chief').arrive = false
      const one = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'One' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(one.id).state, 'delivering')

      // The app quits with it on its way: the daemon stops before the window does.
      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      assert.deepEqual(
        [context.ledger.message(one.id).state, context.ledger.message(one.id).attempts],
        ['queued', 0],
        'its window died before it landed: the attempt comes back',
      )
      const two = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Two' })
      await after.pass()
      await after.pass()
      context.adapter.answer('chief', 'Read one.')
      await after.pass()
      await after.pass()
      assert.deepEqual(
        [context.ledger.message(one.id).state, context.ledger.message(two.id).state],
        ['delivered', 'delivered'],
      )
      assert.deepEqual(
        context.adapter
          .agent('chief')
          .items.filter((i) => i.role === 'user')
          .map((i) => i.text.split('\n')[1]),
        ['One', 'Two'],
      )
    })
  })

  it('sends nothing again that its harness took, though the copy of its window missed it', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Taken' })
      await context.dispatcher.pass()
      // The harness took it, and its own record has it; the daemon stopped
      // before its next look copied the window, so the copy does not.
      assert.equal(context.ledger.message(note.id).state, 'delivering')
      assert.equal(context.ledger.copiedItemWith(id('chief'), `m-${note.id}`), null)
      const taken = context.adapter
        .agent('chief')
        .items.find((entry) => entry.role === 'user' && entry.text.includes('Taken'))

      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      const settled = context.ledger.message(note.id)
      assert.deepEqual([settled.state, settled.receipt], ['delivered', { item: taken.id }])
      await after.pass()
      await after.pass()
      assert.equal(
        context.adapter
          .agent('chief')
          .items.filter((entry) => entry.role === 'user' && entry.text.includes('Taken')).length,
        0,
        'the window that came back is not given it again',
      )
    })
  })

  it('confirms a message whose header the copy of its window already shows', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.agent('chief').arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Shown' })
      await context.dispatcher.pass()
      // The window showed it and the copy has it; the daemon stopped before it confirmed it.
      const shown = item('user', deliveryText(context.ledger.message(note.id)))
      context.ledger.copyTranscript(context.ledger.currentConversation(id('chief')).id, [shown])

      context.ledger.suspendForRestart()
      await context.make().resumeAfterRestart()
      const settled = context.ledger.message(note.id)
      assert.deepEqual([settled.state, settled.receipt], ['delivered', { item: shown.id }])
    })
  })

  it("opens a member's session again on its own conversation with the brief it was opened for", async () => {
    await setup(async (context) => {
      const { id, open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      const session = id('zeus-amber-pine')
      const native = context.ledger.currentConversation(session).nativeSession
      // The window opened, but its record never showed the brief before the stop.
      context.adapter.agent('zeus').items = []

      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      await after.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual([launch.participant.handle, launch.resume], ['zeus-amber-pine', native])
      assert.match(
        launch.message,
        /^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nWrite the parser$/,
      )
      await after.pass()
      assert.equal(task(1).state, 'working')
    })
  })

  it('gives a paste back to the queue when its window closes while the harness takes it', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        const outcome = await deliver(request)
        await context.host.exit('chief')
        return outcome
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Lost' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'queued', 'not left on its way')

      context.adapter.deliver = deliver
      await context.dispatcher.resumeProject(project.id)
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivering')
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivered')
    })
  })
})

/** A tiered staff: standard workers (zeus and diana, unless told), a light worker, and two standard reviewers. */
async function withTiers(context, { workers = ['zeus', 'diana'] } = {}) {
  const member = (agent, role, tier) => ({ agent, harness: 'claude-code', role, tier })
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    chief: { harness: 'claude-code', agent: 'apollo' },
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

  it("reads the projects once a pass, and opens a task's new session in the pass that gave it out", async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      await context.dispatcher.pass()
      let reads = 0
      const projects = context.ledger.projects.bind(context.ledger)
      context.ledger.projects = () => {
        reads += 1
        return projects()
      }
      await context.dispatcher.pass()
      assert.equal(reads, 1, 'a pass with nothing to give out')
      open()
      reads = 0
      await context.dispatcher.pass()
      assert.equal(reads, 1, 'a pass that gives a task out')
      assert.equal(task(1).assignee, 'zeus-amber-pine')
      assert.equal(
        context.host.last('zeus').id,
        'p1-zeus-amber-pine',
        "its session's window opened",
      )
    })
  })

  it('shares the work of one tier across harnesses: the harness with the fewest tasks first', async () => {
    await setup(async (context) => {
      const member = (agent, harness) => ({ agent, harness, role: 'worker', tier: 'standard' })
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code', agent: 'apollo' },
        staff: [
          member('zeus', 'claude-code'),
          member('diana', 'claude-code'),
          member('ares', 'opencode'),
        ],
      })
      const open = (body) =>
        context.ledger.createTask(project.id, {
          from: 'chief',
          pool: 'worker',
          tier: 'standard',
          body,
        })
      const task = (number) => context.ledger.task(project.id, number)
      open('Parser')
      await context.dispatcher.pass()
      assert.match(task(1).assignee, /^zeus-/, 'nothing taken yet: the earliest joined')
      open('Docs')
      await context.dispatcher.pass()
      // Before: diana, the next Claude worker, since members ranked by their own count.
      assert.match(task(2).assignee, /^ares-/, 'Claude has one task, OpenCode none')
      open('Tests')
      await context.dispatcher.pass()
      assert.match(task(3).assignee, /^diana-/, 'one each: the free member with fewest tasks')
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

  it('steps no session with no window and nothing for it, and steps it again once something is', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const session = context.ledger
        .project(1)
        .participants.find((participant) => participant.handle === task(1).assignee)
      context.adapter.answer('zeus', 'Done')
      await context.dispatcher.pass()
      context.ledger.acceptTask(1, 1, { by: 'chief' })
      // After a restart no window is open: an aged project's sessions all idle.
      const after = context.make()
      const stepped = []
      const next = context.ledger.nextDelivery.bind(context.ledger)
      context.ledger.nextDelivery = (id) => {
        stepped.push(id)
        return next(id)
      }
      await after.pass()
      assert.ok(!stepped.includes(session.id), 'the idle session is not stepped')
      assert.ok(stepped.includes(context.ledger.project(1).participants[1].id), 'the chief is')
      context.ledger.note(1, { to: session.handle, body: 'One more thing' })
      await after.pass()
      assert.ok(stepped.includes(session.id), 'a message on its way makes it worth a step')
    })
  })

  it('stops the window of a task the human reassigns before the task leaves it, even one the human opened', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const session = task(1).assignee
      await context.dispatcher.openWindow(1, session)
      const old = context.host.last('zeus')
      await context.dispatcher.reassignTask(1, 1)
      // Stopped before anyone else could take the task: no two windows work on one task.
      assert.deepEqual(context.host.killed, [{ id: old.id, generation: old.generation }])
      assert.deepEqual([task(1).state, task(1).assignee], ['open', null])
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.match(task(1).assignee, /^diana-/)
    })
  })

  it('stops no window for a task that may not go back to the board', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'By name' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      await assert.rejects(context.dispatcher.reassignTask(project.id, 1), /given by name/)
      assert.deepEqual(context.host.killed, [])
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

  it("tells the requester of a waiting task, though a deleted project's waiting task was told before it", async () => {
    await setup(
      async (context) => {
        const waits =
          'T-1 waits for a free standard worker: @zeus has no agent any more (zeus is not among your agents: define it, or remove the member).'
        const first = await withTiers(context, { workers: ['zeus'] })
        const gone = first.open()
        await context.dispatcher.pass()
        assert.deepEqual(first.notes('chief'), [waits])
        await context.dispatcher.closeProject(first.project.id)
        await context.dispatcher.deleteProject(first.project.id)

        const second = await withTiers(context, { workers: ['zeus'] })
        assert.notEqual(
          second.open().id,
          gone.id,
          "the ledger never gives the deleted task's id again",
        )
        await context.dispatcher.pass()
        assert.deepEqual(second.notes('chief'), [waits])
      },
      { roster: (name) => (name === 'zeus' ? null : { id: name, model: 'm', profile: {} }) },
    )
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
      const answer = context.ledger.answer(question.id, {
        from: question.recipientId,
        body: 'This one',
      })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(answer.id).state, 'delivering')
      context.adapter.quota('zeus', { state: 'exhausted', at: context.clock.now().toISOString() })
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(1).assignee, context.ledger.message(answer.id).state],
        ['open', null, 'cancelled'],
        'the task goes back to the board and the answer in flight goes with it',
      )

      // A pass runs every window at once, so the note may reach the chief on
      // this pass or the next; it answers, then takes its own work.
      await context.dispatcher.pass()
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

  it('closes the window of a held task that is cancelled, though its member is still out', async () => {
    await setup(async (context) => {
      const { project, open, task } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const resetsAt = new Date(context.clock.now().getTime() + 3 * 3_600_000).toISOString()
      context.adapter.quota('zeus', { state: 'exhausted', resetsAt })
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, context.host.killed], ['paused', []], 'held with its window')
      const pane = context.host.last('zeus')
      context.ledger.cancelTask(project.id, 1, { by: 'human' })
      await context.dispatcher.pass()
      assert.deepEqual(
        context.host.killed,
        [{ id: pane.id, generation: pane.generation }],
        'it waits for no reset now',
      )
    })
  })
})

describe('a member with several roles', () => {
  it('opens with the text of the role its task needs, in a session per task', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code', agent: 'apollo' },
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
      context.ledger.answer(question.id, { from: question.recipientId, body: 'ANSI' })
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

  it("waits for a session's window still opening before it closes it at the human's hand", async () => {
    await setup(async (context) => {
      const { project } = await finished(context)
      const killed = context.host.killed.length
      let open
      context.host.hold = new Promise((resolve) => {
        open = resolve
      })
      // The human opens the finished session's window, and closes it before it is up.
      await context.dispatcher.openWindow(project.id, 'zeus-amber-pine')
      const closing = context.dispatcher.closeWindow(project.id, 'zeus-amber-pine')
      await new Promise((resolve) => setImmediate(resolve))
      assert.equal(context.host.killed.length, killed, 'the launch is still in progress')

      open()
      await closing
      const pane = context.host.last('zeus-amber-pine')
      assert.deepEqual(context.host.killed.at(-1), { id: pane.id, generation: pane.generation })
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

  it("opens no session's window in a closed project", async () => {
    await setup(async (context) => {
      const { project } = await finished(context)
      await context.dispatcher.closeProject(project.id)
      const opened = context.host.opened.length
      await assert.rejects(
        context.dispatcher.openWindow(project.id, 'zeus-amber-pine'),
        /app is closed: resume it first/,
      )
      await context.dispatcher.pass()
      assert.equal(context.host.opened.length, opened)
    })
  })

  it("tells the human why a session's window they opened did not start", async () => {
    let gone = false
    await setup(
      async (context) => {
        const { project, notes } = await finished(context)
        context.host.refuse = true
        await context.dispatcher.openWindow(project.id, 'zeus-amber-pine')
        assert.deepEqual(notes('human'), [
          '@zeus-amber-pine could not start: the window did not open: refused by the test.',
        ])
        gone = true
        await context.dispatcher.openWindow(project.id, 'zeus-amber-pine')
        assert.equal(
          notes('human').at(-1),
          '@zeus-amber-pine could not start: zeus is no longer among your agents.',
        )
      },
      {
        roster: (name) =>
          name === 'zeus' && gone
            ? null
            : { id: name, model: MODELS[name], profile: { modelKey: MODELS[name] } },
      },
    )
  })
})

describe('a participant that leaves', () => {
  it('is forgotten: a member that comes back to the staff starts clean, not low on quota', async () => {
    await setup(async (context) => {
      const { project, open, task } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.quota('zeus', { state: 'low', usedPercent: 97 })
      await context.dispatcher.pass()
      await context.dispatcher.removeMember(project.id, 'zeus')
      context.ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      open({ body: 'Write the lexer' })
      await context.dispatcher.pass()
      assert.ok(task(2).assignee?.startsWith('zeus-'), 'zeus takes it: the low quota was before')
    })
  })

  it('is forgotten with its project: a session of the next project starts clean', async () => {
    await setup(async (context) => {
      const first = await withTiers(context, { workers: ['zeus'] })
      first.open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const gone = first.id('zeus-amber-pine')
      // Its window copied two items, the human pinned it open, and its agent was interrupted.
      context.adapter.agent('zeus').items.push(item('assistant', 'Half', { complete: false }))
      await context.dispatcher.openWindow(first.project.id, 'zeus-amber-pine')
      context.ledger.pauseTask(first.project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      await context.dispatcher.removeMember(first.project.id, 'zeus')
      await context.dispatcher.pass()
      await context.dispatcher.closeProject(first.project.id)
      await context.dispatcher.deleteProject(first.project.id)

      const second = await withTiers(context, { workers: ['zeus'] })
      second.open()
      await context.dispatcher.pass()
      assert.notEqual(
        second.id(second.task(1).assignee),
        gone,
        'the ledger never gives its id again',
      )
      context.adapter.agent('zeus').items.push(item('assistant', 'Half', { complete: false }))
      await context.dispatcher.pass()
      assert.deepEqual(
        context.ledger.transcript(second.project.id, 1).items.map((copied) => copied.role),
        ['user', 'assistant'],
        'its copy starts with its brief',
      )
      const pane = context.host.last('zeus')
      context.ledger.pauseTask(second.project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      assert.equal(
        context.host.requests.filter(
          ([op, body]) => op === 'pane.input' && body.generation === pane.generation,
        ).length,
        1,
        'its agent is interrupted',
      )
      context.ledger.cancelTask(second.project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      assert.ok(
        context.host.killed.some((killed) => killed.generation === pane.generation),
        'its window closes with its work: nobody pinned it',
      )
    })
  })

  it('keeps the chief of a project created while a deleted one still closes its windows', async () => {
    await setup(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The old chief takes a paste its harness holds: its window closes once that is over.
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        await held
        return deliver(request)
      }
      context.ledger.note(old.id, { from: 'zeus', to: 'chief', body: 'Held' })
      await context.dispatcher.pass()
      const closing = context.dispatcher.closeProject(old.id)
      const deleting = context.dispatcher.deleteProject(old.id)
      const fresh = await context.dispatcher.openProject({
        directory: '/work/api',
        name: 'api',
        chief: { harness: 'claude-code', agent: 'apollo' },
      })
      const chief = fresh.participants.find((participant) => participant.role === 'chief')
      release()
      await closing
      await deleting
      await flush()
      const window = context.host.last('chief')
      assert.deepEqual(
        context.dispatcher.pane(chief.id),
        { id: window.id, generation: window.generation },
        'its chief is known',
      )
      assert.ok(
        !context.host.killed.some((pane) => pane.generation === window.generation),
        'and was never closed',
      )
    })
  })

  it("opens nothing for a session's Open that waited while its project was deleted, nor for a new project's member", async () => {
    await setup(async (context) => {
      const first = await withTiers(context, { workers: ['zeus'] })
      first.open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      assert.equal(first.task(1).state, 'done', "the session's window went with its task")
      const session = first.id('zeus-amber-pine')
      // The human opens the session's window, and again while the first one
      // comes up; that one exits at once, before its open is answered.
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const prepare = context.adapter.prepare
      context.adapter.prepare = async (request) => {
        if (request.participant.handle === 'zeus-amber-pine') await held
        return prepare(request)
      }
      const open = context.host.open
      context.host.open = async (body) => {
        const opened = await open(body)
        if (windowOf(body.id, 'zeus-amber-pine')) await context.host.exit('zeus-amber-pine')
        return opened
      }
      const launches = context.adapter.prepared.length
      await context.dispatcher.openWindow(first.project.id, 'zeus-amber-pine')
      await context.dispatcher.openWindow(first.project.id, 'zeus-amber-pine')
      // Meanwhile the project is closed and deleted, and a new one is opened with a member added.
      const closing = context.dispatcher.closeProject(first.project.id)
      const deleting = context.dispatcher.deleteProject(first.project.id)
      const second = await withTiers(context, { workers: ['zeus'] })
      const diana = context.ledger.addMember(second.project.id, {
        agent: 'diana',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      assert.notEqual(diana.id, session, "the ledger never gives the session's id to a new member")
      release()
      await closing
      await deleting
      await flush()
      assert.deepEqual(
        context.adapter.prepared
          .slice(launches)
          .filter((request) => [session, diana.id].includes(request.participant.id))
          .map((request) => request.participant.handle),
        ['zeus-amber-pine'],
        'the first Open launched for the session, and the second opened nothing, nor for diana',
      )
      assert.equal(context.dispatcher.pane(session), null, 'no window has its id')
    })
  })

  it("stops a closed project's windows acting at once, before their exits come", async () => {
    // A token names a participant and a project by id. Once the project is
    // closed none of its windows may act, exit or no exit; once it is
    // deleted, those ids name nothing.
    const revoked = []
    await setup(
      async (context) => {
        const { project } = await withStaff(context)
        await context.dispatcher.pass()
        context.host.holdExits = true
        await context.dispatcher.closeProject(project.id)
        assert.deepEqual(revoked, ['token-chief'], 'closing revokes its windows')
        await context.dispatcher.deleteProject(project.id)
        assert.deepEqual(revoked, ['token-chief'], 'deleting finds none left')
      },
      {
        credentials: {
          issue: ({ participant }) => `token-${participant.handle}`,
          revoke: (token) => revoked.push(token),
        },
      },
    )
  })
})

describe('a window that closes', () => {
  /** How often a window was killed. */
  const kills = (context, window) =>
    context.host.killed.filter((pane) => pane.generation === window.generation).length

  it('is killed once when its launch never showed its first message, though its exit comes late', async () => {
    await setup(async (context) => {
      context.host.holdExits = true
      const { project } = await withStaff(context)
      const prepare = context.adapter.prepare
      context.adapter.prepare = async (request) => {
        const plan = await prepare(request)
        context.adapter.agent(request.participant.handle).items = []
        return plan
      }
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      context.clock.advance(121_000)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'failed')
      assert.equal(kills(context, context.host.last('zeus')), 1)
    })
  })

  it('is killed once when its project closes while it already goes with its work', async () => {
    await setup(async (context) => {
      context.host.holdExits = true
      const { project, open, task } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'done')
      await context.dispatcher.closeProject(project.id)
      assert.equal(kills(context, context.host.last('zeus')), 1)
    })
  })

  it('is killed once when its member leaves the staff, though its exit comes late', async () => {
    await setup(async (context) => {
      context.host.holdExits = true
      const { project } = await withStaff(context)
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      await context.dispatcher.removeMember(project.id, 'zeus')
      await context.dispatcher.pass()
      assert.equal(kills(context, context.host.last('zeus')), 1)
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

describe('a message withdrawn on its way into a window', () => {
  /** T-1 waits on its window's question; its answer goes to that window next. */
  async function asked(context) {
    const fixture = await withTiers(context)
    fixture.open()
    await context.dispatcher.pass()
    await context.dispatcher.pass()
    const question = context.ledger.ask(fixture.project.id, {
      from: fixture.task(1).assignee,
      to: 'chief',
      task: 1,
      body: 'Which grammar?',
    })
    context.adapter.answer('zeus', 'I asked.')
    await context.dispatcher.pass()
    const answer = context.ledger.answer(question.id, {
      from: fixture.id('chief'),
      body: 'The small one',
    })
    return { ...fixture, answer }
  }

  it('is not confirmed when it shows after all, and its window goes with its work', async () => {
    await setup(async (context) => {
      const { project, task, answer } = await asked(context)
      const zeus = context.adapter.agent('zeus')
      zeus.arrive = false
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(answer.id).state, 'delivering', 'handed over')
      const pane = context.host.last('zeus')
      // The human takes the task back: the answer on its way is withdrawn,
      // and the window's harness shows it anyway.
      context.ledger.releaseTask(project.id, 1, { because: 'by @human' })
      zeus.items.push(item('user', deliveryText(context.ledger.message(answer.id))))
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(answer.id).state, 'cancelled', 'it stays withdrawn')
      assert.ok(
        context.host.killed.some((killed) => killed.generation === pane.generation),
        'the window went with its work',
      )
      assert.match(task(1).assignee, /^diana-/, 'and the task went on elsewhere')
    })
  })

  it('is handed nothing when it was withdrawn while its window got ready, and fails nothing', async () => {
    await setup(async (context) => {
      const { project, answer } = await asked(context)
      let ready
      const readying = new Promise((resolve) => {
        ready = resolve
      })
      context.adapter.ready = async () => {
        await readying
        return true
      }
      await context.dispatcher.pass()
      context.ledger.cancelTask(project.id, 1, { by: 'chief' })
      ready()
      await flush()
      assert.equal(context.ledger.message(answer.id).state, 'cancelled')
      assert.ok(
        !context.adapter.agent('zeus').items.some((i) => i.text.includes(markerOf(answer.id))),
        'nothing pasted',
      )
    })
  })
})

describe('a cancelled task', () => {
  it('stops its window: an agent at work on it is interrupted, the window closes, and the session stays', async () => {
    await setup(async (context) => {
      const { project, id, open, task } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      const session = task(1).assignee
      const native = context.ledger.currentConversation(id(session)).nativeSession
      const pane = context.host.last('zeus')
      const steps = []
      const { request, kill } = context.host
      context.host.request = async (op, body) => {
        if (op === 'pane.input') steps.push(['keys', body.generation, body.bytes])
        return request(op, body)
      }
      context.host.kill = async (killed) => {
        steps.push(['kill', killed.generation])
        return kill(killed)
      }
      // One still on the board, and one given out whose window has not opened: they just cancel.
      open({ body: 'Write the lexer' })
      open({ body: 'Write the docs' })
      context.ledger.assignTask(project.id, 3, id('zeus'))
      for (const number of [2, 3, 1]) context.ledger.cancelTask(project.id, number, { by: 'human' })
      const opened = context.host.opened.length
      await context.dispatcher.pass()
      assert.deepEqual(steps, [
        ['keys', pane.generation, [27]],
        ['kill', pane.generation],
      ])
      assert.equal(context.host.opened.length, opened, 'no window opens for the other two')
      const kept = context.ledger.project(project.id).participants.find((p) => p.handle === session)
      assert.equal(kept.leftAt, null, 'the session stays on the board')
      await context.dispatcher.openWindow(project.id, session)
      const reopened = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [reopened.participant.handle, reopened.resume, reopened.message],
        [session, native, null],
        'and opens again on its own conversation',
      )
    })
  })

  it('interrupts a window the human opened, which stays, and lets a turn the human began there since go on', async () => {
    await setup(async (context) => {
      const { project, id, open, task } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const session = task(1).assignee
      await context.dispatcher.openWindow(project.id, session)
      const escapes = () => context.host.requests.filter(([op]) => op === 'pane.input').length
      context.ledger.cancelTask(project.id, 1, { by: 'human' })
      await context.dispatcher.pass()
      assert.equal(escapes(), 1, 'interrupted')
      assert.deepEqual(context.host.killed, [], 'the human opened it: it stays open')
      // A harness that ignores the key while it thinks is pressed again, a few seconds on.
      context.clock.advance(3_100)
      await context.dispatcher.pass()
      assert.equal(escapes(), 2)
      // The agent stops; the human asks it something in the window, and it works on that.
      context.adapter.answer('zeus', 'Stopped.')
      await context.dispatcher.pass()
      const zeus = context.adapter.agent('zeus')
      zeus.items.push(item('user', 'What did you change so far?'))
      zeus.settled = false
      for (let look = 0; look < 2; look += 1) {
        context.clock.advance(3_100)
        await context.dispatcher.pass()
      }
      assert.equal(escapes(), 2, "the human's own turn is not interrupted")
      assert.equal(context.dispatcher.activity(id(session)).state, 'working')
      assert.equal(task(1).state, 'cancelled', 'and nothing it wrote became a result')
    })
  })

  it('closes its window at once though words were on their way in, and never opens one for it again', async () => {
    await setup(async (context) => {
      const { project, open, notes } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.ledger.pauseTask(project.id, 1, { by: 'chief' })
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Stopped.')
      await context.dispatcher.pass()
      // Resumed into its window, which has not shown the words yet.
      const { message } = context.ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Go on' })
      context.adapter.agent('zeus').arrive = false
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(message.id).state, 'delivering')
      const pane = context.host.last('zeus')
      context.ledger.cancelTask(project.id, 1, { by: 'human' })
      await context.dispatcher.pass()
      assert.deepEqual(
        context.host.killed.at(-1),
        { id: pane.id, generation: pane.generation },
        'closed on the next look, not once the words were given up on',
      )
      const opened = context.host.opened.length
      for (let look = 0; look < 3; look += 1) {
        context.clock.advance(31_000)
        await context.dispatcher.pass()
      }
      assert.equal(context.host.opened.length, opened, 'no window opens for it again')
      assert.equal(context.ledger.message(message.id).state, 'cancelled')
      assert.deepEqual(notes('human'), [], 'and nothing failed for the human to hear of')
    })
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

/** A setup where Codex has a fake of its own, so a test sees which harness a window opened on. */
const withCodex = (fn, options = {}) => {
  const codex = fakeAdapter('codex')
  return setup((context) => fn({ ...context, codex }), { adapters: { codex }, ...options })
}
const chiefOf = (context, project) =>
  context.ledger.project(project.id).participants.find((p) => p.handle === 'chief')
/** The handoffs, newest first. */
const handoffsOf = (context, project) =>
  context.ledger
    .inbox(chiefOf(context, project).id)
    .filter((m) => m.body.startsWith('You are the chief now'))
/**
 * The human closes and deletes a project while something of it waits, and
 * opens another (with these workers), whose ids are its own: the ledger
 * never gives the deleted one's again. `gone` ends once the old windows have
 * closed.
 */
async function replaceProject(context, old, workers = []) {
  const gone = Promise.all([
    context.dispatcher.closeProject(old.id),
    context.dispatcher.deleteProject(old.id),
  ])
  const fresh = await context.dispatcher.openProject({
    directory: '/work/api',
    name: 'api',
    chief: { harness: 'claude-code', agent: 'apollo' },
    staff: workers.map((agent) => ({
      agent,
      harness: 'claude-code',
      role: 'worker',
      tier: 'standard',
    })),
  })
  assert.notEqual(fresh.id, old.id, 'the ledger never gives its id again')
  return { fresh, gone }
}

describe('switching the chief to another agent', () => {
  it('hands over to the new chief: the old window closes, the project stays open, the new one opens with the handoff, and what was queued follows it', async () => {
    await withCodex(async (context) => {
      const { codex } = context
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.agent('chief').items.push(item('user', 'The codeword is tern'))
      context.adapter.answer('chief', 'Noted: tern')
      context.adapter.busy('chief')
      await context.dispatcher.pass()
      const ready = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Ready' })

      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.equal(context.ledger.project(project.id).state, 'open', 'a switch is not a close')
      assert.deepEqual(
        context.host.killed.map((pane) => pane.id),
        [`p${project.id}-chief`],
      )
      assert.deepEqual(
        [chiefOf(context, project).harness, chiefOf(context, project).agent],
        ['codex', 'astraeus'],
      )
      const prepared = codex.prepared.at(-1)
      assert.deepEqual(
        [prepared.role, prepared.resume, prepared.agent.model],
        ['chief', null, 'gpt-6-astra'],
        'a fresh conversation, on the chosen model',
      )
      assert.match(
        prepared.message,
        /^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nYou are the chief now\. The human switched this project's chief from Claude Code \(apollo\) to you, Codex \(astraeus\)\./,
      )
      assert.match(prepared.message, /cut off in the middle of a turn/)
      assert.match(
        prepared.message,
        /The human's last message to the chief: "The codeword is tern"/,
      )
      assert.equal(context.ledger.message(ready.id).state, 'queued', 'the handoff goes first')

      // The new chief takes the handoff and answers it; then its queue goes on.
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(id('chief')).state, 'working')
      codex.answer('chief', 'I have taken over; next is the parser.')
      await context.dispatcher.pass()
      assert.match(codex.agent('chief').items.at(-1).text, /note from @zeus\]\nReady/)
      assert.deepEqual(
        context.ledger.chiefHistory(project.id).map((c) => c.harness),
        ['claude-code'],
        "the old chief's words are history",
      )
    })
  })

  it('gives a delivery that had not landed back to the queue with its attempt, and keeps one that had', async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      const landed = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'One' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(landed.id).state, 'delivering', 'not yet confirmed')
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.equal(
        context.ledger.message(landed.id).state,
        'delivered',
        'the last look saw it arrive',
      )

      await context.dispatcher.pass()
      context.codex.answer('chief', 'Taken over')
      await context.dispatcher.pass()
      context.codex.agent('chief').arrive = false
      const lost = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Two' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(lost.id).state, 'delivering')
      await context.dispatcher.switchChief(project.id, { harness: 'claude-code', agent: 'apollo' })
      assert.deepEqual(
        [context.ledger.message(lost.id).state, context.ledger.message(lost.id).attempts],
        ['queued', 0],
        'the window went before it could land: its attempt comes back',
      )
    })
  })

  it('lets a chief at work finish its turn first, and gives it nothing else meanwhile', async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      const ready = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Ready' })
      await context.dispatcher.switchChief(project.id, {
        harness: 'codex',
        agent: 'astraeus',
        when: 'turn',
      })
      assert.equal(chiefOf(context, project).harness, 'claude-code', 'not while it works')
      assert.deepEqual(context.host.killed, [])

      context.adapter.answer('chief', 'Done with that')
      await context.dispatcher.pass()
      assert.equal(chiefOf(context, project).harness, 'codex', 'its turn is over: it goes')
      assert.equal(
        context.ledger.message(ready.id).state,
        'queued',
        'Ready waited for the new chief',
      )
      assert.ok(!context.adapter.agent('chief').items.some((i) => i.text.includes('Ready')))
    })
  })

  it('first asks the chief where things stand, when the human wants that, and switches once it has answered', async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      await context.dispatcher.switchChief(project.id, {
        harness: 'codex',
        agent: 'astraeus',
        note: true,
      })
      await context.dispatcher.pass()
      const asked = context.adapter.agent('chief').items.at(-1).text
      assert.match(
        asked,
        /moving this project's chief to codex \(astraeus\) once you answer\. Write down where things stand/,
      )
      await context.dispatcher.pass()
      assert.equal(chiefOf(context, project).harness, 'claude-code', 'it has not answered yet')

      context.adapter.answer('chief', 'Where things stand: the parser is half done.')
      await context.dispatcher.pass()
      assert.equal(chiefOf(context, project).harness, 'codex')
      const words = context.ledger
        .chiefHistory(project.id)
        .at(-1)
        .items.map((i) => i.text)
      assert.ok(words.includes('Where things stand: the parser is half done.'))
    })
  })

  it('never gives the new chief the note that asked the old one where things stand', async () => {
    await withCodex(async (context) => {
      const { codex } = context
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      const asking = (n) =>
        context.ledger
          .inbox(chiefOf(context, project).id)
          .filter((m) => m.body.includes('Write down where things stand'))[n]
      // Asked twice while the chief works: the second ask replaces the first.
      await context.dispatcher.switchChief(project.id, {
        harness: 'codex',
        agent: 'astraeus',
        when: 'turn',
        note: true,
      })
      const first = asking(0)
      await context.dispatcher.switchChief(project.id, {
        harness: 'codex',
        agent: 'astraeus',
        when: 'turn',
        note: true,
      })
      assert.equal(context.ledger.message(first.id).state, 'cancelled', 'replaced, never sent')
      await context.dispatcher.pass()
      // The human does not wait for the answer: Switch chief, now.
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.equal(chiefOf(context, project).harness, 'codex')
      assert.equal(context.ledger.message(asking(0).id).state, 'cancelled')
      await context.dispatcher.pass()
      codex.answer('chief', 'I have taken over.')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.ok(
        !codex.agent('chief').items.some((i) => i.text.includes('Write down where things stand')),
        'the new chief never saw it',
      )
    })
  })

  it('switches a chief out of quota at once, and the new chief is not out', async () => {
    await withCodex(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      context.ledger.markOut(id('chief'), { until: '2026-09-20T12:00:00.000Z', reason: 'quota' })
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(id('chief')).state, 'out')
      await context.dispatcher.switchChief(project.id, {
        harness: 'codex',
        agent: 'astraeus',
        when: 'turn',
      })
      assert.equal(chiefOf(context, project).harness, 'codex', 'an out chief has no turn to finish')
      assert.equal(chiefOf(context, project).outUntil, null)
      await context.dispatcher.pass()
      assert.notEqual(context.dispatcher.activity(id('chief')).state, 'out')
    })
  })

  it('replaces a handoff still on its way, and Resume of a closed project brings the chief back with the handoff it had not shown', async () => {
    await withCodex(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'Hello')
      await context.dispatcher.pass()
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      // The new window has not shown its handoff yet when the human switches again.
      context.codex.agent('chief').items = []
      const first = context.ledger
        .inbox(id('chief'))
        .find((m) => m.body.startsWith('You are the chief now'))
      await context.dispatcher.switchChief(project.id, { harness: 'claude-code', agent: 'apollo' })
      assert.equal(
        context.ledger.message(first.id).state,
        'cancelled',
        'that handoff was for Codex',
      )
      const handoffs = context.ledger
        .inbox(id('chief'))
        .filter((m) => m.body.startsWith('You are the chief now') && m.state !== 'cancelled')
      assert.equal(handoffs.length, 1)
      assert.match(handoffs[0].body, /from Codex \(astraeus\) to you, Claude Code \(apollo\)\./)

      // Closed before any look saw the new chief show it: the handoff waits for Resume.
      await context.dispatcher.closeProject(project.id)
      await context.dispatcher.resumeProject(project.id)
      assert.match(
        context.adapter.prepared.at(-1).message,
        /You are the chief now\. The human switched this project's chief from Codex \(astraeus\) to you, Claude Code \(apollo\)\./,
      )
    })
  })

  it('takes a handoff titled for the lead, as a ledger from before 2026-10-03 holds it, as the handoff: it goes first, no other is written, and a switch replaces it', async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'Hello')
      await context.dispatcher.pass()
      // The build before switched the chief and wrote its handoff, and the
      // project closed before the new window showed it.
      await context.dispatcher.closeProject(project.id)
      context.ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      const old = context.ledger.note(project.id, {
        to: 'chief',
        body: "You are the lead now. The human switched this project's lead from Claude Code (apollo) to you, Codex (astraeus).",
      })

      await context.dispatcher.resumeProject(project.id)
      assert.match(
        context.codex.prepared.at(-1).message,
        /\nYou are the lead now\./,
        'it goes first',
      )
      assert.deepEqual(handoffsOf(context, project), [], 'no other is written')

      // The window has not shown it yet when the human switches again.
      context.codex.agent('chief').items = []
      await context.dispatcher.switchChief(project.id, { harness: 'claude-code', agent: 'apollo' })
      assert.equal(context.ledger.message(old.id).state, 'cancelled', 'that handoff was for Codex')
      assert.equal(handoffsOf(context, project).length, 1)
      assert.match(
        context.adapter.prepared.at(-1).message,
        /You are the chief now\. The human switched this project's chief from Codex \(astraeus\) to you, Claude Code \(apollo\)\./,
      )
    })
  })

  it('switches no chief of a project it does not know, and says so', async () => {
    await withCodex(async (context) => {
      await assert.rejects(
        context.dispatcher.switchChief(42, { harness: 'codex', agent: 'astraeus' }),
        {
          message: 'no project 42',
        },
      )
    })
  })

  it("switches no chief to a harness's own default model: it names a saved agent", async () => {
    await withCodex(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      for (const when of ['now', 'turn']) {
        await assert.rejects(
          context.dispatcher.switchChief(project.id, { harness: 'codex', when }),
          /pick one of your saved agents for the chief/,
        )
      }
      assert.deepEqual(
        [chiefOf(context, project).harness, chiefOf(context, project).agent],
        ['claude-code', 'apollo'],
      )
      assert.equal(context.dispatcher.pendingSwitch(id('chief')), null, 'nothing waits to switch')
      assert.deepEqual([context.host.killed, context.codex.prepared], [[], []])
    })
  })

  it('opens no project on an image agent, nor switches a chief to one: an image agent only designs', async () => {
    const designing = {
      message: 'pygmalion is an image agent, which can only be an image designer',
    }
    await withCodex(
      async (context) => {
        const { project, id } = await withStaff(context)
        await context.dispatcher.pass()
        context.adapter.busy('chief')
        await assert.rejects(
          context.dispatcher.openProject({
            directory: '/work/api',
            name: 'api',
            chief: { harness: 'codex', agent: 'pygmalion' },
          }),
          designing,
        )
        for (const when of ['now', 'turn']) {
          await assert.rejects(
            context.dispatcher.switchChief(project.id, {
              harness: 'codex',
              agent: 'pygmalion',
              when,
            }),
            designing,
          )
        }
        assert.deepEqual(
          context.ledger.projects().map((p) => p.name),
          ['app'],
          'nothing was opened',
        )
        assert.deepEqual(
          [chiefOf(context, project).harness, chiefOf(context, project).agent],
          ['claude-code', 'apollo'],
        )
        assert.equal(context.dispatcher.pendingSwitch(id('chief')), null, 'nothing waits to switch')
        assert.deepEqual([context.host.killed, context.codex.prepared], [[], []])
      },
      {
        roster: (name) =>
          name === 'pygmalion'
            ? { id: name, model: 'codex-image', designer: true, profile: {} }
            : { id: name, model: MODELS[name], profile: { modelKey: MODELS[name] } },
      },
    )
  })

  it("keeps a chief on its harness's own default model as it is, until Switch chief moves it to an agent", async () => {
    await withCodex(async (context) => {
      // Opened before a chief was always a saved agent: its record names none.
      const project = context.ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code' },
      })
      await context.dispatcher.resumeProject(project.id)
      assert.equal(context.adapter.prepared.at(-1).agent, null, "on its harness's own default")
      await context.dispatcher.pass()
      context.adapter.agent('chief').items.push(item('user', 'The codeword is tern'))
      context.adapter.answer('chief', 'Noted: tern')
      await context.dispatcher.pass()
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.deepEqual(
        [chiefOf(context, project).harness, chiefOf(context, project).agent],
        ['codex', 'astraeus'],
      )
      assert.match(
        context.codex.prepared.at(-1).message,
        /The human switched this project's chief from Claude Code to you, Codex \(astraeus\)\./,
      )
    })
  })

  it('switches no chief of a closed project', async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.closeProject(project.id)
      await assert.rejects(
        context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' }),
        /app is closed: resume it first/,
      )
      assert.equal(chiefOf(context, project).harness, 'claude-code')
      assert.equal(context.codex.prepared.length, 0)
    })
  })

  it('keeps a chief whose agent was deleted closed, tells the human, and holds what it was to receive', async () => {
    const gone = new Set()
    await withCodex(
      async (context) => {
        const { project, id } = await withStaff(context)
        await context.dispatcher.pass()
        await assert.rejects(
          context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'nobody' }),
          /nobody is not among your agents/,
        )
        await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
        await context.dispatcher.closeProject(project.id)
        gone.add('astraeus')
        const opened = context.host.opened.length
        await context.dispatcher.resumeProject(project.id)
        assert.equal(context.host.opened.length, opened, 'no window on a model that is gone')
        const told = context.ledger
          .inbox(id('human'))
          .find((m) => m.body.includes('no longer among your agents'))
        assert.match(
          told.body,
          /The chief runs on astraeus, which is no longer among your agents: add it back under Agents, or switch the chief\./,
        )
        await context.dispatcher.pass()
        assert.equal(context.host.opened.length, opened, 'and no pass tries again')
      },
      {
        roster: (name) =>
          name === 'nobody' || gone.has(name)
            ? null
            : { id: name, model: MODELS[name], profile: { modelKey: MODELS[name] } },
      },
    )
  })

  it("never closes the new chief's window for taking long to show its handoff", async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'Hello')
      await context.dispatcher.pass()
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      context.codex.agent('chief').items = []
      context.clock.advance(10 * 120_000)
      await context.dispatcher.pass()
      assert.equal(context.host.killed.length, 1, "only the old chief's window was closed")
      assert.equal(context.ledger.project(project.id).state, 'open')
    })
  })

  it('switches a chief that had a message on its way when the daemon stopped', async () => {
    await withCodex(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'Hello')
      await context.dispatcher.pass()
      context.adapter.agent('chief').arrive = false
      const one = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'One' })
      await context.dispatcher.pass()
      // The daemon starts again a few seconds later.
      context.clock.advance(5_000)
      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()

      await after.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.equal(chiefOf(context, project).harness, 'codex')
      assert.match(context.codex.prepared.at(-1).message, /You are the chief now\./)
      await after.pass()
      context.codex.answer('chief', 'Taken over.')
      await after.pass()
      await after.pass()
      assert.equal(context.ledger.message(one.id).state, 'delivered', 'it follows the new chief')
    })
  })

  it('switches nothing once its project is deleted while the switch waits for the old chief, nor a project created meanwhile', async () => {
    await withCodex(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The old chief takes a paste its harness holds; the switch waits for it.
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        await held
        return deliver(request)
      }
      context.ledger.note(old.id, { from: 'zeus', to: 'chief', body: 'Held' })
      await context.dispatcher.pass()
      // After the turn, as the page asks by default, and asking where things stand.
      const switched = context.dispatcher
        .switchChief(old.id, { harness: 'codex', agent: 'astraeus', when: 'turn', note: true })
        .then(
          () => 'switched',
          (cause) => cause.message,
        )
      const { fresh, gone } = await replaceProject(context, old)
      const chief = chiefOf(context, fresh)
      const window = context.host.last('chief')
      release()
      await gone
      await flush()
      assert.deepEqual(
        context.ledger.inbox(chief.id).map((m) => m.body),
        [],
        'the new chief is asked nothing',
      )
      assert.equal(context.dispatcher.pendingSwitch(chief.id), null, 'nothing waits to switch it')
      assert.deepEqual(
        [chiefOf(context, fresh).harness, context.codex.prepared.length],
        ['claude-code', 0],
        'the new chief is not switched',
      )
      assert.ok(
        !context.host.killed.some((pane) => pane.generation === window.generation),
        'and its window stays',
      )
      assert.equal(await switched, `no project ${old.id}`, 'the switch says its project is gone')
    })
  })

  it("hands nothing to an old chief whose switch waited for its turn's end once its project is deleted, nor switches a project created meanwhile", async () => {
    await withCodex(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      context.adapter.busy('chief')
      await context.dispatcher.switchChief(old.id, {
        harness: 'codex',
        agent: 'astraeus',
        when: 'turn',
        note: true,
      })
      const asked = context.ledger
        .inbox(chiefOf(context, old).id)
        .find((m) => m.body.includes('Write down where things stand'))
      assert.equal(asked.state, 'queued', 'the note waits for the turn to end')
      // The turn ends, and the next look at the old window waits on its harness.
      context.adapter.answer('chief', 'Done with that')
      const launch = context.adapter.agent('chief').launchId
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const observe = context.adapter.observe
      context.adapter.observe = async (request) => {
        if (request.launch.launchId === launch) await held
        return observe(request)
      }
      const looking = context.dispatcher.pass()
      const { fresh, gone } = await replaceProject(context, old)
      const welcome = context.ledger.note(fresh.id, { to: 'chief', body: 'Welcome' })
      assert.notEqual(welcome.id, asked.id, "the ledger never gives the note's id again")
      release()
      await looking
      await gone
      await flush()
      assert.equal(context.ledger.message(welcome.id).state, 'queued', 'it waits for the new chief')
      assert.deepEqual(
        [chiefOf(context, fresh).harness, context.codex.prepared.length],
        ['claude-code', 0],
        'the new chief is not switched',
      )
    })
  })

  it("copies and confirms nothing of the old chief's last look once its project is deleted, nor switches a project created meanwhile", async () => {
    await withCodex(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // A note is on its way to the old chief, and the human told it something
      // since the last look: the switch's last look, which shows both, waits
      // on its harness.
      context.ledger.note(old.id, { from: 'zeus', to: 'chief', body: 'Parser done' })
      await context.dispatcher.pass()
      const told = 'The codeword is tern'
      context.adapter.agent('chief').items.push(item('user', told))
      const launch = context.adapter.agent('chief').launchId
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const observe = context.adapter.observe
      context.adapter.observe = async (request) => {
        if (request.launch.launchId === launch) await held
        return observe(request)
      }
      const switched = context.dispatcher
        .switchChief(old.id, { harness: 'codex', agent: 'astraeus' })
        .then(
          () => 'switched',
          (cause) => cause.message,
        )
      await flush()
      const { fresh, gone } = await replaceProject(context, old)
      release()
      await gone
      await flush()
      const before = context.ledger.chiefHistory(fresh.id).flatMap((c) => c.items)
      assert.deepEqual(
        [
          context.ledger.copiedItemWith(chiefOf(context, fresh).id, told),
          before.filter((copied) => copied.text === told).length,
        ],
        [null, 0],
        "the new chief's conversations, now or before, have nothing of the old window",
      )
      assert.deepEqual(
        [chiefOf(context, fresh).harness, context.codex.prepared.length],
        ['claude-code', 0],
        'the new chief is not switched',
      )
      assert.equal(
        await switched,
        `no project ${old.id}`,
        'the switch says its project is gone, and confirmed none of its messages',
      )
    })
  })

  it("switches nothing once its project is deleted while the old chief's window closes, nor a project created meanwhile", async () => {
    await withCodex(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The old chief's window takes its time to close.
      const window = context.host.last('chief')
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const kill = context.host.kill
      context.host.kill = async (pane) => {
        if (pane.generation === window.generation) await held
        return kill(pane)
      }
      const switched = context.dispatcher
        .switchChief(old.id, { harness: 'codex', agent: 'astraeus' })
        .then(
          () => 'switched',
          (cause) => cause.message,
        )
      await flush()
      const { fresh, gone } = await replaceProject(context, old)
      release()
      await gone
      await flush()
      assert.deepEqual(
        [chiefOf(context, fresh).harness, context.codex.prepared.length],
        ['claude-code', 0],
        'the new chief is not switched',
      )
      assert.equal(await switched, `no project ${old.id}`, 'the switch says its project is gone')
    })
  })

  it("switches nothing once its project is deleted while the old chief's window closes after its turn, nor a project created meanwhile", async () => {
    await withCodex(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The chief is at work, so the switch waits for the end of its turn.
      context.adapter.busy('chief')
      await context.dispatcher.switchChief(old.id, {
        harness: 'codex',
        agent: 'astraeus',
        when: 'turn',
      })
      // The turn ends, and the step that switches the chief closes its old
      // window, which takes its time.
      context.adapter.answer('chief', 'Done with that')
      const window = context.host.last('chief')
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const kill = context.host.kill
      context.host.kill = async (pane) => {
        if (pane.generation === window.generation) await held
        return kill(pane)
      }
      const stepping = context.dispatcher.pass()
      await flush()
      const { fresh, gone } = await replaceProject(context, old)
      release()
      // The step stops there, and fails nothing: the deleted project's chief
      // has nothing left to switch.
      await stepping
      await gone
      await flush()
      const chief = chiefOf(context, fresh)
      assert.deepEqual(
        [chief.harness, context.codex.prepared.length, context.dispatcher.pendingSwitch(chief.id)],
        ['claude-code', 0, null],
        'the new chief is not switched, and nothing waits to switch it',
      )
    })
  })
})

describe('work in flight when its participant is forgotten', () => {
  /**
   * Holds each call of `object[name]` that `which` picks until the test lets
   * it go, then makes it, or `instead` in its place.
   */
  function hold(object, name, which, instead = null) {
    const call = object[name]
    let release
    const held = new Promise((resolve) => {
      release = resolve
    })
    object[name] = async (...args) => {
      if (!which(...args)) return call(...args)
      await held
      return (instead ?? call)(...args)
    }
    return release
  }
  /** A look at the window of this launch. */
  const looksAt =
    (launchId) =>
    ({ launch }) =>
      launch.launchId === launchId

  it("leaves no low quota mark on a member taken off the staff while its session's window was looked at", async () => {
    await setup(async (context) => {
      const { project, open, task, notes } = await withTiers(context, { workers: ['zeus'] })
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      // The next look at the session's window waits on its harness, and finds it low on quota.
      context.adapter.quota('zeus', { state: 'low', usedPercent: 97 })
      const release = hold(
        context.adapter,
        'observe',
        looksAt(context.adapter.agent('zeus').launchId),
      )
      const looking = context.dispatcher.pass()
      // Meanwhile the human takes zeus off the staff, and adds it back.
      const removing = context.dispatcher.removeMember(project.id, 'zeus')
      await flush()
      context.ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      release()
      await looking
      await removing
      open({ body: 'Write the lexer' })
      await context.dispatcher.pass()
      assert.deepEqual(
        notes('chief').filter((note) => note.startsWith('T-2 waits')),
        [],
        'nothing holds zeus back',
      )
      assert.ok(task(2).assignee?.startsWith('zeus-'), 'zeus takes it: the low quota was before')
    })
  })

  it("copies and delivers nothing once the chief's project is deleted while its window is looked at, nor for a project created meanwhile", async () => {
    await setup(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The human told the old chief something since the last look, which waits on its harness.
      const told = 'The codeword is tern'
      context.adapter.agent('chief').items.push(item('user', told))
      const release = hold(
        context.adapter,
        'observe',
        looksAt(context.adapter.agent('chief').launchId),
      )
      const looking = context.dispatcher.pass()
      const { fresh, gone } = await replaceProject(context, old)
      const welcome = context.ledger.note(fresh.id, { to: 'chief', body: 'Welcome' })
      release()
      await looking
      await gone
      await flush()
      assert.equal(
        context.ledger.copiedItemWith(chiefOf(context, fresh).id, told),
        null,
        "the new chief's conversation has nothing of the old window",
      )
      assert.equal(
        context.ledger.message(welcome.id).state,
        'queued',
        'its message waits for its own window',
      )
    })
  })

  it("says nothing once the chief's project is deleted while its window's look fails, of it or a project created meanwhile", async () => {
    const entries = []
    await setup(
      async (context) => {
        const { project: old } = await withStaff(context)
        await context.dispatcher.pass()
        const release = hold(
          context.adapter,
          'observe',
          looksAt(context.adapter.agent('chief').launchId),
          () => {
            throw new Error('the record could not be read')
          },
        )
        const looking = context.dispatcher.pass()
        const { gone } = await replaceProject(context, old)
        release()
        await looking
        await gone
        assert.deepEqual(
          entries.filter((entry) => entry.kind === 'window.activity' && entry.state === 'unknown'),
          [],
          "no line says the old chief's window, or the new chief's, could not be read",
        )
      },
      { trace: (entry) => entries.push(entry) },
    )
  })

  it("delivers nothing to a session's window once its project is deleted while its paused task's agent is interrupted, nor for a project created meanwhile", async () => {
    await setup(async (context) => {
      const first = await withTiers(context, { workers: ['zeus'] })
      first.open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(first.task(1).state, 'working')
      const session = first.id('zeus-amber-pine')
      // The human keeps the session's window open, so it stays after its
      // work. The chief pauses the task; the Escape that stops its agent
      // waits on the pane host.
      await context.dispatcher.openWindow(first.project.id, 'zeus-amber-pine')
      context.ledger.pauseTask(first.project.id, 1, { by: 'chief' })
      context.adapter.agent('zeus').settled = true
      const window = context.host.last('zeus')
      const release = hold(
        context.host,
        'request',
        (op, body) => op === 'pane.input' && body.generation === window.generation,
      )
      const stepping = context.dispatcher.pass()
      await flush()
      // Meanwhile the project is closed and deleted, and a member of a new one is given a task.
      const closing = context.dispatcher.closeProject(first.project.id)
      const deleting = context.dispatcher.deleteProject(first.project.id)
      const second = await withTiers(context, { workers: ['zeus'] })
      const diana = context.ledger.addMember(second.project.id, {
        agent: 'diana',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      assert.notEqual(diana.id, session, "the ledger never gives the session's id to a new member")
      context.ledger.createTask(second.project.id, { from: 'chief', to: 'diana', body: 'Docs' })
      release()
      await stepping
      await closing
      await deleting
      await flush()
      assert.deepEqual(
        context.ledger.inbox(diana.id).map((message) => message.state),
        ['queued'],
        'its brief waits for its own window',
      )
    })
  })

  it('opens no window for a chief whose project was deleted while its launch was prepared', async () => {
    await setup(async (context) => {
      const release = hold(
        context.adapter,
        'prepare',
        (request) => request.directory === '/work/app',
      )
      const { project: old } = await withStaff(context)
      const { fresh, gone } = await replaceProject(context, old)
      const own = context.ledger.currentConversation(chiefOf(context, fresh).id)
      release()
      await gone
      await flush()
      assert.deepEqual(
        context.host.opened.filter((body) => body.cwd === '/work/app'),
        [],
        'nothing opens for the deleted project',
      )
      assert.equal(
        context.ledger.currentConversation(chiefOf(context, fresh).id).id,
        own.id,
        'the new chief keeps its conversation',
      )
    })
  })

  it('tells nobody of a launch that failed once its project was deleted, not a project created meanwhile', async () => {
    await setup(async (context) => {
      const release = hold(
        context.adapter,
        'prepare',
        (request) => request.directory === '/work/app',
        () => {
          throw new Error('the harness would not start')
        },
      )
      const { project: old } = await withStaff(context)
      const { fresh, gone } = await replaceProject(context, old)
      release()
      await gone
      await flush()
      const human = context.ledger
        .project(fresh.id)
        .participants.find((participant) => participant.role === 'human')
      assert.deepEqual(
        context.ledger.inbox(human.id).map((message) => message.body),
        [],
        "the new project's human hears nothing of it",
      )
    })
  })

  it('starts no conversation for a chief whose project is deleted while its window opens, nor for a project created meanwhile, and that window closes', async () => {
    await setup(async (context) => {
      const release = hold(context.host, 'open', (body) => body.cwd === '/work/app')
      const { project: old } = await withStaff(context)
      const { fresh, gone } = await replaceProject(context, old)
      const own = context.ledger.currentConversation(chiefOf(context, fresh).id)
      release()
      await gone
      await flush()
      assert.equal(
        context.ledger.currentConversation(chiefOf(context, fresh).id).id,
        own.id,
        'the new chief keeps its conversation',
      )
      const window = context.host.opened.find((body) => body.cwd === '/work/app')
      assert.ok(
        context.host.killed.some((pane) => pane.generation === window.generation),
        'the old window closes',
      )
    })
  })

  it("kills no window of a deleted project's chief that exited before its open was answered", async () => {
    await setup(async (context) => {
      const open = context.host.open
      const release = hold(
        context.host,
        'open',
        (body) => body.cwd === '/work/app',
        async (body) => {
          const opened = await open(body)
          // The host sends the exit first, in the same read as its answer to the open.
          await context.host.exit('chief')
          return opened
        },
      )
      const { project: old } = await withStaff(context)
      const { gone } = await replaceProject(context, old)
      release()
      await gone
      await flush()
      const window = context.host.opened.find((body) => body.cwd === '/work/app')
      assert.ok(
        !context.host.killed.some((pane) => pane.generation === window.generation),
        'it had gone already',
      )
    })
  })

  it("binds no conversation, of a deleted project's chief or of a project created meanwhile, to the thread the old chief's window named", async () => {
    await setup(async (context) => {
      // The old chief's window takes long to come up, then names its thread.
      let calls = 0
      const release = hold(
        context.adapter,
        'started',
        () => ++calls === 1,
        () => ({ nativeSession: 'native-named' }),
      )
      const { project: old } = await withStaff(context)
      const { fresh, gone } = await replaceProject(context, old)
      const own = context.ledger.currentConversation(chiefOf(context, fresh).id)
      release()
      await gone
      await flush()
      assert.equal(
        context.ledger.currentConversation(chiefOf(context, fresh).id).nativeSession,
        own.nativeSession,
        "the new chief's conversation keeps its own thread",
      )
    })
  })

  it("hands nothing to the old chief's window that was getting ready for a paste once its project is deleted, nor anything of a project created meanwhile", async () => {
    await setup(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The old chief's window takes its time to be ready for a paste.
      context.adapter.ready = async () => true
      const release = hold(
        context.adapter,
        'ready',
        looksAt(context.adapter.agent('chief').launchId),
      )
      const waiting = context.ledger.note(old.id, { from: 'zeus', to: 'chief', body: 'Waiting' })
      await context.dispatcher.pass()
      const { fresh, gone } = await replaceProject(context, old)
      const welcome = context.ledger.note(fresh.id, { to: 'chief', body: 'Welcome' })
      assert.notEqual(welcome.id, waiting.id, "the ledger never gives the old message's id again")
      release()
      await gone
      await flush()
      assert.equal(
        context.ledger.message(welcome.id).state,
        'queued',
        'it waits for its own window',
      )
    })
  })

  it("settles nothing by what the old chief's harness did with a paste once its project is deleted, and a project created meanwhile gets its message once", async () => {
    await setup(async (context) => {
      const { project: old } = await withStaff(context)
      await context.dispatcher.pass()
      // The old chief's harness takes long to refuse a paste.
      const before = context.adapter.agent('chief')
      const release = hold(context.adapter, 'deliver', looksAt(before.launchId))
      const refused = context.ledger.note(old.id, { from: 'zeus', to: 'chief', body: 'Refused' })
      await context.dispatcher.pass()
      const { fresh, gone } = await replaceProject(context, old)
      // Meanwhile the new chief is handed its first message.
      const welcome = context.ledger.note(fresh.id, { to: 'chief', body: 'Welcome' })
      assert.notEqual(welcome.id, refused.id, "the ledger never gives the old message's id again")
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(welcome.id).state, 'delivering')
      before.admit = false
      release()
      await gone
      await flush()
      assert.equal(
        context.ledger.message(welcome.id).state,
        'delivering',
        "the old harness's refusal does not send it back",
      )
      await context.dispatcher.pass()
      assert.deepEqual(
        [context.ledger.message(welcome.id).state, context.ledger.message(welcome.id).attempts],
        ['delivered', 1],
        'it arrives, once',
      )
    })
  })

  it('takes nobody off the staff for a removal that waited while its project was deleted, not a member of a project created meanwhile', async () => {
    await setup(async (context) => {
      const { project: old } = await withStaff(context)
      // zeus's window takes long to come up with its task, and the removal waits for it.
      const release = hold(
        context.adapter,
        'prepare',
        (request) => request.participant.handle === 'zeus',
      )
      context.ledger.createTask(old.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      const removed = context.dispatcher.removeMember(old.id, 'zeus').then(
        () => 'removed',
        (cause) => cause.message,
      )
      const { fresh, gone } = await replaceProject(context, old, ['zeus'])
      release()
      await gone
      await flush()
      assert.ok(
        context.ledger.project(fresh.id).participants.some((p) => p.handle === 'zeus'),
        'its zeus stays on the staff',
      )
      assert.equal(await removed, '@zeus left the staff', 'the removal says its zeus has left')
    })
  })

  it("keeps the trace of a project created while a deleted one's windows close, and drops the deleted one's", async () => {
    // As the event file in the home: forgetting a project drops its lines.
    const lines = []
    const trace = Object.assign((entry) => lines.push(entry), {
      forget: (project) => {
        const kept = lines.filter((line) => line.project !== project)
        lines.splice(0, lines.length, ...kept)
      },
    })
    await setup(
      async (context) => {
        const { project: old } = await withStaff(context)
        await context.dispatcher.pass()
        // The old chief takes a paste its harness holds: its window closes once that is over.
        const release = hold(
          context.adapter,
          'deliver',
          looksAt(context.adapter.agent('chief').launchId),
        )
        context.ledger.note(old.id, { from: 'zeus', to: 'chief', body: 'Held' })
        await context.dispatcher.pass()
        const { fresh, gone } = await replaceProject(context, old)
        // The new chief's window is looked at meanwhile, and the trace says so.
        await context.dispatcher.pass()
        release()
        await gone
        assert.deepEqual(
          lines
            .filter((line) => line.kind === 'window.activity' && line.project === fresh.id)
            .map((line) => line.state),
          ['idle'],
          "the new chief's line stays",
        )
        assert.deepEqual(
          lines.filter((line) => line.project === old.id),
          [],
          "and the deleted project's went",
        )
      },
      { trace },
    )
  })
})

describe('a chief whose window does not come up', () => {
  /** A project whose Claude chief has said something, so a switch has a history to hand over. */
  async function spoken(context) {
    const fixture = await withStaff(context)
    await context.dispatcher.pass()
    context.adapter.answer('chief', 'Hello')
    await context.dispatcher.pass()
    return fixture
  }
  const toHuman = (context, project) =>
    context.ledger
      .inbox(context.ledger.project(project.id).participants.find((p) => p.role === 'human').id)
      .map((m) => m.body)

  it('gives the new chief its handoff again when its first window closes before showing it', async () => {
    await withCodex(async (context) => {
      const { project } = await spoken(context)
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      context.codex.agent('chief').items = []
      const [handoff] = handoffsOf(context, project)
      // The window crashes, or the human closes the project, before the handoff shows.
      await context.host.exit('chief')
      assert.equal(context.ledger.project(project.id).state, 'suspended')
      assert.deepEqual(
        [context.ledger.message(handoff.id).state, context.ledger.message(handoff.id).attempts],
        ['queued', 0],
      )

      await context.dispatcher.resumeProject(project.id)
      const launch = context.codex.prepared.at(-1)
      assert.match(
        launch.message,
        /^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nYou are the chief now\./,
      )
      assert.deepEqual(
        handoffsOf(context, project).map((m) => [m.id, m.state]),
        [[handoff.id, 'delivering']],
        'the same handoff, and no other',
      )
    })
  })

  it('keeps the project open when the new chief cannot take its handoff, and opens it again with it', async () => {
    await withCodex(async (context) => {
      const { codex } = context
      const { project, id } = await spoken(context)
      let slow = true
      codex.started = async () => {
        if (!slow) return {}
        slow = false
        throw new Error('the Codex broker never named the thread it opened')
      }
      context.ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      const killed = context.host.killed.length
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.equal(context.ledger.project(project.id).state, 'open', 'the staff keeps working')
      assert.equal(context.host.killed.length, killed + 2, 'the old chief, then the new window')
      assert.equal(context.dispatcher.pane(id('chief')), null)
      assert.notEqual(context.dispatcher.pane(id('zeus')), null)
      const [handoff] = handoffsOf(context, project)
      assert.deepEqual([handoff.state, handoff.attempts], ['queued', 0])
      assert.deepEqual(toHuman(context, project), [
        'The chief could not start: the window could not take its first message: the Codex broker never named the thread it opened. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief.',
      ])

      await context.dispatcher.pass()
      assert.equal(codex.prepared.length, 1, 'not again at once')
      context.clock.advance(5_000)
      await context.dispatcher.pass()
      assert.equal(codex.prepared.length, 2)
      assert.match(codex.prepared.at(-1).message, /You are the chief now\./)
      assert.equal(handoffsOf(context, project).length, 1)
    })
  })

  it('a chief whose launch keeps failing: its handoff waits, no other is written, it is tried ever more slowly, and the human hears once', async () => {
    await withCodex(async (context) => {
      const { codex } = context
      const { project } = await spoken(context)
      const prepare = codex.prepare
      let tries = 0
      codex.prepare = async () => {
        tries += 1
        throw new Error('codex is broken')
      }
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      const ready = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Ready' })
      for (let n = 0; n < 10; n += 1) await context.dispatcher.pass()
      assert.equal(tries, 1, 'no pass tries again at once')
      for (const wait of [5_000, 10_000, 20_000]) {
        context.clock.advance(wait - 1)
        await context.dispatcher.pass()
        context.clock.advance(1)
        await context.dispatcher.pass()
      }
      assert.equal(tries, 4, 'after 5, 10 and 20 seconds')
      assert.deepEqual(
        handoffsOf(context, project).map((m) => [m.state, m.attempts]),
        [['queued', 0]],
        'one handoff, never spent',
      )
      assert.deepEqual(
        [context.ledger.message(ready.id).state, context.ledger.message(ready.id).attempts],
        ['queued', 0],
      )
      assert.equal(toHuman(context, project).length, 1, 'told once')
      assert.match(
        toHuman(context, project)[0],
        /could not start: the launch failed: codex is broken/,
      )

      codex.prepare = prepare
      context.clock.advance(40_000)
      await context.dispatcher.pass()
      assert.match(codex.prepared.at(-1).message, /You are the chief now\./)
      assert.equal(toHuman(context, project).length, 1)
    })
  })

  it('notices a chief window that exits before its open is answered', async () => {
    await withCodex(async (context) => {
      const { project, id } = await spoken(context)
      const open = context.host.open.bind(context.host)
      let die = true
      context.host.open = async (body) => {
        const opened = await open(body)
        // The host sends the exit first, in the same read as its answer to the open.
        if (die) {
          die = false
          await context.host.exit('chief')
        }
        return opened
      }
      await context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.equal(context.dispatcher.pane(id('chief')), null)
      assert.equal(
        context.ledger.project(project.id).state,
        'suspended',
        'the chief went, as it would have later',
      )
      assert.equal(handoffsOf(context, project)[0].state, 'queued', 'its handoff waits')

      await context.dispatcher.resumeProject(project.id)
      assert.notEqual(context.dispatcher.pane(id('chief')), null)
      assert.match(context.codex.prepared.at(-1).message, /You are the chief now\./)
    })
  })

  it('keeps what was queued for a first chief whose window does not open', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      await context.dispatcher.closeProject(project.id)
      context.host.refuse = true
      await context.dispatcher.resumeProject(project.id)
      assert.match(
        toHuman(context, project)[0],
        /^The chief could not start: the window did not open: refused by the test\./,
      )
      const result = context.ledger.note(project.id, {
        from: 'zeus',
        to: 'chief',
        body: 'A result',
      })
      // Tried again with the result as its first message, and refused again.
      context.clock.advance(5_000)
      context.host.refuse = true
      await context.dispatcher.pass()
      assert.deepEqual(
        [context.ledger.message(result.id).state, context.ledger.message(result.id).attempts],
        ['queued', 0],
        'not spent on a window that did not open',
      )
      assert.equal(toHuman(context, project).length, 1)
      context.clock.advance(10_000)
      await context.dispatcher.pass()
      assert.match(context.adapter.prepared.at(-1).message, /note from @zeus\]\nA result$/)
    })
  })
})

describe('a window that takes long', () => {
  /** Every window opened from now on waits until the returned function lets it. */
  const holdOpens = (host) => {
    let open
    host.hold = new Promise((resolve) => {
      open = resolve
    })
    return async () => {
      host.hold = null
      open()
      await flush()
    }
  }
  const LATE = Symbol('late')
  /** What a call answers, which must come without waiting for a window. */
  async function answered(call) {
    let timer
    const late = new Promise((resolve) => {
      timer = setTimeout(resolve, 1_000, LATE)
    })
    try {
      const value = await Promise.race([call, late])
      assert.notEqual(value, LATE, 'it waited for a window')
      return value
    } finally {
      clearTimeout(timer)
    }
  }

  it('holds up only itself: the pass moves on, and other windows are delivered to and looked at meanwhile', async () => {
    await setup(async (context) => {
      const { project: slow } = await withStaff(context)
      const quick = await context.dispatcher.openProject({
        directory: '/work/api',
        name: 'api',
        chief: { harness: 'claude-code', agent: 'apollo' },
      })
      await context.dispatcher.pass()
      const slowLaunch = context.adapter.prepared[0].launchId
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        // Pi waits up to 30 s for a paste's acknowledgement; this one waits for the test.
        if (request.launch.launchId === slowLaunch) await held
        return deliver(request)
      }
      try {
        const one = context.ledger.note(slow.id, { to: 'chief', body: 'Slow' })
        const two = context.ledger.note(quick.id, { to: 'chief', body: 'Quick' })
        await answered(context.dispatcher.pass())
        assert.equal(context.ledger.message(two.id).state, 'delivering')
        await context.dispatcher.pass()
        assert.deepEqual(
          [context.ledger.message(one.id).state, context.ledger.message(two.id).state],
          ['delivering', 'delivered'],
          'the other window was looked at again while the slow one waited',
        )
        release()
        await flush()
        await context.dispatcher.pass()
        assert.equal(context.ledger.message(one.id).state, 'delivered')
      } finally {
        release()
      }
    })
  })

  it('answers New project, Switch chief, Resume and a session’s Open once the ledger has the change; the window opens after', async () => {
    await withCodex(async (context) => {
      const { host, codex } = context
      let opened = holdOpens(host)
      const project = await answered(
        context.dispatcher.openProject({
          directory: '/work/app',
          name: 'app',
          chief: { harness: 'claude-code', agent: 'apollo' },
          staff: [{ agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' }],
        }),
      )
      assert.equal(project.state, 'open')
      assert.equal(host.opened.length, 0, 'its chief is still opening')
      await opened()
      const chief = chiefOf(context, project).id
      assert.notEqual(context.dispatcher.pane(chief), null)
      await context.dispatcher.pass()
      context.adapter.answer('chief', 'Hello')
      await context.dispatcher.pass()

      opened = holdOpens(host)
      await answered(
        context.dispatcher.switchChief(project.id, { harness: 'codex', agent: 'astraeus' }),
      )
      assert.equal(chiefOf(context, project).harness, 'codex')
      assert.equal(context.dispatcher.pane(chief), null, 'the new chief is still opening')
      await opened()
      assert.match(codex.prepared.at(-1).message, /You are the chief now\./)
      assert.notEqual(context.dispatcher.pane(chief), null)

      await context.dispatcher.closeProject(project.id)
      opened = holdOpens(host)
      assert.equal((await answered(context.dispatcher.resumeProject(project.id))).state, 'open')
      assert.equal(context.dispatcher.pane(chief), null)
      await opened()
      assert.notEqual(context.dispatcher.pane(chief), null)

      context.ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      const session = context.ledger.task(project.id, 1).assignee
      const sessionId = context.ledger
        .project(project.id)
        .participants.find((p) => p.handle === session).id
      assert.equal(context.dispatcher.pane(sessionId), null, 'its window went with its task')
      opened = holdOpens(host)
      await answered(context.dispatcher.openWindow(project.id, session))
      assert.equal(context.dispatcher.pane(sessionId), null)
      await opened()
      assert.notEqual(context.dispatcher.pane(sessionId), null)
    })
  })

  it('closes a window only once the paste its harness is taking is over, and that message goes again', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context)
      await context.dispatcher.pass()
      let release
      const held = new Promise((resolve) => {
        release = resolve
      })
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        await held
        return deliver(request)
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Late' })
      await context.dispatcher.pass()
      const closing = context.dispatcher.closeProject(project.id)
      await flush()
      assert.deepEqual(context.host.killed, [], 'not while the harness takes it')
      release()
      await closing
      assert.equal(context.host.killed.length, 1)
      assert.deepEqual(
        [context.ledger.message(note.id).state, context.ledger.message(note.id).attempts],
        ['queued', 1],
        'on its way when the window went: it goes again',
      )
    })
  })

  it('opens the chief again when the human resumes a project that is still closing', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      const closing = context.dispatcher.closeProject(project.id)
      await context.dispatcher.resumeProject(project.id)
      await closing
      await flush()
      assert.equal(context.ledger.project(project.id).state, 'open')
      assert.notEqual(context.dispatcher.pane(id('chief')), null)
      assert.equal(
        context.host.opened.filter((pane) => pane.id === `p${project.id}-chief`).length,
        2,
      )
    })
  })
})

describe('work that throws before its first await', () => {
  it('lets go of its participant: its next step runs, the failure is written down, and an operation waiting on it ends', async () => {
    const written = []
    await setup(
      async (context) => {
        const { project, open, task } = await withTiers(context, { workers: ['zeus'] })
        open()
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        context.adapter.answer('zeus', 'Parser done')
        await context.dispatcher.pass()
        assert.equal(task(1).state, 'done', "the session's window closed with its task")
        // One read of the ledger fails, in the work that opens the session's window.
        const projects = context.ledger.projects.bind(context.ledger)
        let fail = true
        context.ledger.projects = () => {
          if (!fail) return projects()
          fail = false
          throw new Error('the ledger could not be read')
        }
        await context.dispatcher.openWindow(project.id, 'zeus-amber-pine')
        context.ledger.createTask(project.id, { from: 'chief', after: 1, body: 'Now the lexer' })
        const launches = context.adapter.prepared.length
        await context.dispatcher.pass()
        assert.deepEqual(
          context.adapter.prepared.slice(launches).map((request) => request.participant.handle),
          ['zeus-amber-pine'],
          'its next step ran',
        )
        assert.deepEqual(
          written.map((cause) => cause.message),
          ['the ledger could not be read'],
        )
        assert.equal((await context.dispatcher.closeProject(project.id)).state, 'suspended')
      },
      { log: { error: (_message, cause) => written.push(cause) } },
    )
  })
})

describe('a window the human switches to another conversation', () => {
  it('follows it: the conversation it shows becomes the session’s, and deliveries go and count there', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      await context.dispatcher.pass()
      const first = context.ledger.currentConversation(id('chief'))
      // A note is pasted, and before the next look the human types /clear.
      const one = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'One' })
      await context.dispatcher.pass()
      context.adapter.switchTo('chief', 'native-cleared')
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.message(one.id).state,
        'delivered',
        'the record it went to showed it',
      )
      const cleared = context.ledger.currentConversation(id('chief'))
      assert.deepEqual([cleared.nativeSession, cleared.harness], ['native-cleared', 'claude-code'])
      assert.equal(
        context.ledger.chiefHistory(project.id).find((c) => c.id === first.id).items.length,
        1,
        'the first conversation ended with its copy',
      )

      const two = context.ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Two' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(two.id).state, 'delivered')
      assert.match(context.adapter.agent('chief').items.at(-1).text, /note from @zeus\]\nTwo$/)
      assert.equal(context.adapter.agent('chief').items.length, 1, 'in the conversation it shows')

      // /resume back to the first: it is the chief's conversation again.
      context.adapter.switchTo('chief', first.nativeSession)
      await context.dispatcher.pass()
      assert.equal(context.ledger.currentConversation(id('chief')).id, first.id)
    })
  })
})

describe('a message that cannot be delivered', () => {
  it('tells the human once what it was, for whom and why, whatever its kind', async () => {
    await setup(async (context) => {
      const { project, id, open, task } = await withTiers(context)
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      context.adapter.agent('chief').admit = false
      const question = context.ledger.ask(project.id, {
        from: 'zeus-amber-pine',
        to: 'chief',
        task: 1,
        body: 'Which dialect?',
      })
      context.adapter.answer('zeus', 'I asked the chief.')
      for (let n = 0; n < 5; n += 1) await context.dispatcher.pass()
      assert.equal(context.ledger.message(question.id).state, 'failed')
      const told = () => context.ledger.inbox(id('human')).map((m) => m.body)
      assert.deepEqual(told(), [
        `m-${question.id}, a question from @zeus-amber-pine on T-1, did not reach @chief: refused by the test.`,
      ])
      await context.dispatcher.pass()
      assert.equal(told().length, 1, 'once')
    })
  })

  it('tells the human once when a task they gave fails', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context)
      context.adapter.agent('chief').admit = false
      context.ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Plan the week' })
      for (let n = 0; n < 5; n += 1) await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'failed')
      assert.deepEqual(
        context.ledger.inbox(id('human')).map((m) => m.body),
        ['T-1 failed: refused by the test. Reopen it with: cf task reopen T-1 "…"'],
      )
    })
  })
})

describe('an agents file that cannot be read', () => {
  it('gives out no new work and takes none back until it can, and every window goes on', async () => {
    let broken = false
    await setup(
      async (context) => {
        const { open, task, notes } = await withTiers(context)
        open()
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.equal(task(1).state, 'working')
        broken = true
        open({ body: 'Write the docs' })
        await context.dispatcher.pass()
        await context.dispatcher.pass()
        assert.deepEqual(
          [task(1).state, task(2).state],
          ['working', 'open'],
          'nothing taken back, nothing given out',
        )
        assert.match(
          notes('chief').at(-1),
          /^T-2 waits for a free standard worker: @zeus's agent cannot be read \(your agents file needs fixing: see Agents\)/,
        )
        context.adapter.answer('zeus', 'Parser done')
        await context.dispatcher.pass()
        assert.equal(task(1).state, 'done', 'the windows went on')
        broken = false
        await context.dispatcher.pass()
        assert.notEqual(task(2).assignee, null)
      },
      {
        roster: (name) => {
          if (broken) {
            throw new Error(
              'Your agents file agents.json is not valid JSON: fix it or move it away; ConsensFlow left it as it is.',
            )
          }
          return { id: name, model: MODELS[name], profile: { modelKey: MODELS[name] } }
        },
      },
    )
  })
})

/** Pi has no adapter in these tests: it stands for a harness ConsensFlow cannot open. */
describe('a harness ConsensFlow has no adapter for', () => {
  const onPi = (context, project) =>
    context.ledger.addMember(project.id, {
      agent: 'hera',
      harness: 'pi',
      role: 'worker',
      tier: 'standard',
    })

  it('gives a member on it no work, says why, and every pass goes on', async () => {
    await setup(async (context) => {
      const { project, id } = await withStaff(context, [])
      onPi(context, project)
      context.ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Write the parser',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.deepEqual([task.state, task.assignee], ['open', null])
      assert.deepEqual(
        context.ledger
          .inbox(id('chief'))
          .filter((m) => m.kind === 'note')
          .map((m) => m.body),
        [
          'T-1 waits for a free standard worker: @hera runs on pi, whose windows ConsensFlow cannot open.',
        ],
      )
      assert.equal(context.host.opened.length, 1, 'only the chief window')
    })
  })

  it('fails what was given to a member on it by name, and its requester hears why', async () => {
    await setup(async (context) => {
      const { project } = await withStaff(context, [])
      onPi(context, project)
      context.ledger.createTask(project.id, { from: 'chief', to: 'hera', body: 'Parser' })
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'failed')
      assert.match(
        task.messages.find((m) => m.kind === 'note').body,
        /^T-1 failed: the launch failed: ConsensFlow cannot open pi windows\./,
      )
    })
  })

  it('opens no project, member or chief on it', async () => {
    await setup(async (context) => {
      await assert.rejects(
        context.dispatcher.openProject({
          directory: '/work/app',
          name: 'app',
          chief: { harness: 'pi', agent: 'leto' },
        }),
        /ConsensFlow cannot open pi windows/,
      )
      await assert.rejects(
        context.dispatcher.openProject({
          directory: '/work/app',
          name: 'app',
          chief: { harness: 'claude-code', agent: 'apollo' },
          staff: [{ agent: 'hera', harness: 'pi', role: 'worker', tier: 'standard' }],
        }),
        /ConsensFlow cannot open pi windows/,
      )
      assert.deepEqual(context.ledger.projects(), [])
      const { project } = await withStaff(context)
      await assert.rejects(
        context.dispatcher.switchChief(project.id, { harness: 'pi', agent: 'leto' }),
        /ConsensFlow cannot open pi windows/,
      )
    })
  })
})
