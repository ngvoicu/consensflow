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
      return { admitted: true }
    },
    async observe({ launch }) {
      const agent = agents.get(launch.launchId)
      return {
        items: [...agent.items],
        settled: agent.settled,
        waiting: agent.waiting,
        quota: agent.quota,
        failed: false,
      }
    },
  }
  adapter.agent = (handle) => [...agents.values()].filter((a) => a.handle === handle).at(-1)
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

/** The saved model of each fake agent: what makes a reviewer independent of an author. */
const MODELS = {
  zeus: 'claude-opus-5',
  diana: 'gpt-5.6-luna',
  hera: 'muse-spark',
  calliope: 'claude-opus-5',
  astraeus: 'gpt-6-astra',
}

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
    async request(op, body) {
      host.requests.push([op, body])
      return { ok: true, outcome: 'cleared' }
    },
    onEnter(listener) {
      enters.push(listener)
    },
    enter(handle, epoch) {
      const body = host.opened.filter((b) => b.id.endsWith(`-${handle}`)).at(-1)
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
      return { ok: true }
    },
    onExit(listener) {
      exits.push(listener)
    },
    async exit(handle) {
      const body = host.opened.filter((b) => b.id.endsWith(`-${handle}`)).at(-1)
      for (const listener of exits) await listener({ id: body.id, generation: body.generation })
    },
    last(handle) {
      return host.opened.filter((b) => b.id.endsWith(`-${handle}`)).at(-1)
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
  const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: clock.now })
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

/** A project with its lead window up, its workers (Zeus, unless told) in the team from the start, and no review gate. */
async function withTeam(context, workers = ['zeus']) {
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    harness: 'claude-code',
    review: 'none',
    team: workers.map((agent) => ({
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
  it('opens a project with its lead window and binds the lead conversation', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      const lead = context.host.last('lead')
      assert.equal(lead.id, `p${project.id}-lead`)
      assert.deepEqual(lead.argv, ['/bin/fake-agent', 'lead'])
      assert.equal(lead.cwd, '/work/app')
      assert.equal(lead.env.FAKE_AGENT, 'lead')
      assert.equal(lead.env.CONSENSFLOW_PARTICIPANT, 'lead')
      assert.equal(lead.env.CONSENSFLOW_TOKEN, 'token-lead')
      assert.equal(context.adapter.prepared[0].message, null, 'a lead opens without a task')
      assert.equal(context.adapter.prepared[0].role, 'lead')
      assert.equal(context.adapter.prepared[0].instructions, 'instructions for lead')
      const conversation = context.ledger.currentConversation(id('lead'))
      assert.equal(conversation.nativeSession, `native-${lead.launch}`)
      assert.equal(context.dispatcher.activity(id('lead')).state, 'starting')
      await context.dispatcher.pass()
      assert.equal(context.dispatcher.activity(id('lead')).state, 'idle')
    })
  })

  it('launches a worker with its task as the first message and records its answer as the result', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      const { message } = context.ledger.createTask(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Write the parser',
      })
      await context.dispatcher.pass()
      const first = context.adapter.prepared.at(-1)
      assert.equal(first.participant.handle, 'zeus')
      assert.equal(first.message, deliveryText(context.ledger.task(project.id, 1).messages[0]))
      assert.match(
        first.message,
        new RegExp(`^\\[ConsensFlow m-${message.id} · T-1 · task from @lead\\]\\n`),
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
      assert.deepEqual([result.body, result.recipient], ['Parser done', 'lead'])
    })
  })

  it('delivers results to an idle lead one at a time and proves each arrived', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context, ['zeus', 'diana'])
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'One' })
      context.ledger.createTask(project.id, { from: 'lead', to: 'diana', body: 'Two' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.busy('lead')
      context.adapter.answer('zeus', 'one done')
      context.adapter.answer('diana', 'two done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const lead = () => context.adapter.agent('lead')
      assert.equal(lead().items.length, 0, 'a busy lead is not interrupted')

      lead().settled = true
      await context.dispatcher.pass()
      assert.equal(lead().items.length, 1)
      assert.match(lead().items[0].text, /result from @zeus\]\none done$/)
      await context.dispatcher.pass()
      const [first, second] = context.ledger.inbox(id('lead')).reverse()
      assert.equal(first.state, 'delivered')
      assert.equal(first.receipt.item, lead().items[0].id)
      assert.equal(second.state, 'queued', 'the next waits until the lead is idle again')

      context.adapter.answer('lead', 'noted')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.inbox(id('lead'))[0].state, 'delivered')
      assert.match(lead().items.at(-1).text, /result from @diana\]\ntwo done$/)
    })
  })

  it('retries a delivery the harness refused, then gives up', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      context.adapter.agent('lead').admit = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'hello' })
      for (let n = 0; n < 5; n += 1) await context.dispatcher.pass()
      const failed = context.ledger.inbox(id('lead')).find((m) => m.id === note.id)
      assert.deepEqual([failed.state, failed.attempts], ['failed', 3])
      assert.equal(failed.reason, 'refused by the test')
    })
  })

  it('retries a delivery whose arrival never shows in the harness record', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      context.adapter.agent('lead').arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'hello' })
      await context.dispatcher.pass()
      const delivering = () => context.ledger.inbox(id('lead')).find((m) => m.id === note.id)
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
      const { project, id } = await withTeam(context)
      const deliver = context.adapter.deliver
      context.adapter.deliver = async (request) => {
        if (context.adapter.agent('lead').items.length === 0 && !context.retried) {
          context.retried = true
          return { admitted: null, reason: 'the plugin did not answer' }
        }
        return deliver(request)
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'hello' })
      await context.dispatcher.pass()
      context.clock.advance(31_000)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('lead')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['delivered', 2])
      assert.equal(context.adapter.agent('lead').items.length, 1, 'delivered once')
    })
  })

  it('records an adapter that throws as a failed attempt with its error, not as uncertain', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      context.adapter.deliver = async () => {
        throw new Error('the channel needs pane, generation and observed epoch')
      }
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'hello' })
      await context.dispatcher.pass()
      const message = context.ledger.inbox(id('lead')).find((m) => m.id === note.id)
      assert.deepEqual([message.state, message.attempts], ['queued', 1])
      assert.match(message.reason, /the channel needs pane, generation and observed epoch/)
    })
  })

  it('fails the task and tells the requester when the worker window closes mid-task', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      await context.host.exit('zeus')
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'failed')
      const note = task.messages.find((m) => m.kind === 'note')
      assert.equal(note.recipient, 'lead')
      assert.match(note.body, /T-1 failed: @zeus's window closed/)
    })
  })

  it('fails the task when the worker window cannot open', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      context.host.refuse = true
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'failed')
      assert.match(context.ledger.task(project.id, 1).messages[0].reason, /refused by the test/)
    })
  })

  it('fails a launch whose first message never arrives', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      const original = context.adapter.prepare
      context.adapter.prepare = async (request) => {
        const plan = await original(request)
        context.adapter.agent(request.participant.handle).items = []
        return plan
      }
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      context.clock.advance(121_000)
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'failed')
      assert.deepEqual(context.host.killed.at(-1).id, context.host.last('zeus').id)
    })
  })

  it('fails the first message at once when the harness cannot take it after the window opens', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      context.adapter.started = async () => {
        throw new Error('the server never answered')
      }
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      const task = context.ledger.task(project.id, 1)
      assert.equal(task.state, 'failed')
      assert.match(task.messages[0].reason, /the server never answered/)
      assert.equal(context.host.killed.at(-1).id, context.host.last('zeus').id)
    })
  })

  it('leaves a task waiting on a question alone, and resumes it with the answer', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const question = context.ledger.ask(project.id, {
        from: 'zeus',
        to: 'lead',
        task: 1,
        body: 'Which format?',
      })
      context.adapter.answer('zeus', 'I asked the lead.')
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.task(project.id, 1).state,
        'waiting',
        'no result from a waiting turn',
      )

      await context.dispatcher.pass()
      context.adapter.answer('lead', 'JSON, I will reply')
      context.ledger.answer(question.id, { from: 'lead', body: 'JSON' })
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
      await withTeam(context)
      await context.dispatcher.pass()
      const clears = () => context.host.requests.filter(([op]) => op === 'draft.clear')
      context.host.enter('lead', 5)
      await context.dispatcher.pass()
      assert.deepEqual(clears(), [], 'an Enter alone proves nothing')

      context.adapter.agent('lead').items.push(item('user', 'the human asks something'))
      await context.dispatcher.pass()
      const lead = context.host.last('lead')
      assert.deepEqual(clears(), [
        [
          'draft.clear',
          { id: lead.id, generation: lead.generation, epoch: 5, submission: 'human-1' },
        ],
      ])
      await context.dispatcher.pass()
      assert.equal(clears().length, 1, 'each Enter is released once')
    })
  })

  it('does not count its own deliveries as the human submitting', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      await context.dispatcher.pass()
      context.host.enter('lead', 9)
      context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'from zeus' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        context.host.requests.filter(([op]) => op === 'draft.clear'),
        [],
        'a ConsensFlow message is not the human submitting their draft',
      )
    })
  })

  it('suspends the project when its lead window closes', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      await context.host.exit('lead')
      assert.equal(context.ledger.project(project.id).state, 'suspended')
      assert.equal(context.ledger.project(project.id).resumeOnStart, false)
    })
  })

  it('opens a project with the team it is given, and only the lead window', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        harness: 'claude-code',
        team: [{ agent: 'zeus', harness: 'claude-code', role: 'worker', tier: 'standard' }],
      })
      assert.deepEqual(
        project.participants.map((p) => p.handle),
        ['human', 'lead', 'zeus'],
      )
      assert.deepEqual(
        context.host.opened.map((pane) => pane.id),
        [`p${project.id}-lead`],
      )
    })
  })

  it('closes the window of a member who leaves, and its exit fails nothing', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      const zeus = id('zeus')
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
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
      const { project } = await withTeam(context)
      context.ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' })
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

  it("opens a PM's window with its first message, not before", async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      context.ledger.addPm(project.id, { harness: 'claude-code' })
      await context.dispatcher.pass()
      assert.equal(context.host.last('pm'), undefined)

      context.ledger.createTask(project.id, { from: 'human', to: 'pm', body: 'Plan the release' })
      await context.dispatcher.pass()
      assert.equal(context.host.last('pm').id, `p${project.id}-pm`)
      assert.match(context.adapter.prepared.at(-1).message, /task from @human\]\nPlan the release/)
    })
  })

  it('brings back the projects that were open before a restart, on their own conversations', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      const native = context.ledger.currentConversation(id('lead')).nativeSession
      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      const resumed = context.adapter.prepared.at(-1)
      assert.deepEqual([resumed.participant.handle, resumed.resume], ['lead', native])
      assert.equal(context.ledger.project(project.id).state, 'open')
      assert.equal(context.ledger.project(project.id).resumeOnStart, false)
      assert.equal(context.ledger.currentConversation(id('lead')).nativeSession, native)
    })
  })
})

describe('the delivered text', () => {
  it('names the message, the task and the sender, and tells the reader how to answer a question', () => {
    const base = { id: 12, taskNumber: 3, sender: 'zeus', body: 'Which format?' }
    assert.equal(
      deliveryText({ ...base, kind: 'question' }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nAnswer with: cf answer m-12 "…"',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'note', sender: null, taskNumber: null, body: 'hi' }),
      '[ConsensFlow m-12 · note from ConsensFlow]\nhi',
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
    assert.match(text, /\n… \(20000 characters; read all of it with: cf inbox read m-7\)$/)
  })
})

/** A tiered team: two standard workers with tags, a light worker, and reviewers on two models. */
async function withTiers(context, { review = 'none', reviewers = ['calliope', 'astraeus'] } = {}) {
  const member = (agent, role, tier, tags) => ({ agent, harness: 'claude-code', role, tier, tags })
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    harness: 'claude-code',
    review,
    team: [
      member('zeus', 'worker', 'standard', ['coding', 'rust']),
      member('diana', 'worker', 'standard', ['coding']),
      member('hera', 'worker', 'light', []),
      ...reviewers.map((agent) => member(agent, 'reviewer', 'standard', [])),
    ],
  })
  const id = (handle) =>
    context.ledger.project(project.id).participants.find((p) => p.handle === handle).id
  const open = (extra = {}) =>
    context.ledger.createTask(project.id, {
      from: 'lead',
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
  it('gives an open task to a free member of its tier, preferring matching tags, then the least loaded', async () => {
    await setup(async (context) => {
      const { open, task } = await withTiers(context)
      open({ tags: ['rust'] })
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['queued', 'zeus'])
      assert.equal(context.host.last('zeus').id, 'p1-zeus')
      open({ body: 'Write the docs' })
      await context.dispatcher.pass()
      assert.equal(task(2).assignee, 'diana', 'zeus is busy')
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'parser done')
      context.adapter.answer('diana', 'docs done')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(2).state], ['done', 'done'])

      open({ tags: ['rust'], body: 'Lexer' })
      await context.dispatcher.pass()
      assert.equal(task(3).assignee, 'zeus', 'the tag decides between two free members')
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'lexer done')
      await context.dispatcher.pass()
      open({ body: 'Tests' })
      await context.dispatcher.pass()
      assert.equal(task(4).assignee, 'diana', 'with no tag, the member with fewer tasks so far')
      open({ tier: 'light', body: 'Rename a file' })
      await context.dispatcher.pass()
      assert.equal(task(5).assignee, 'hera')
    })
  })

  it('tells the requester once when nobody of the tier is free, and assigns when one frees up', async () => {
    await setup(async (context) => {
      const { open, task, notes } = await withTiers(context)
      open()
      open({ body: 'Docs' })
      await context.dispatcher.pass()
      open({ body: 'Third' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(3).state, 'open')
      assert.deepEqual(notes('lead'), [
        'T-3 waits for a free standard worker: @zeus and @diana are busy.',
      ])
      context.adapter.answer('zeus', 'done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual([task(3).state, task(3).assignee], ['queued', 'zeus'])
      assert.equal(notes('lead').length, 1)
    })
  })
})

describe('the dispatcher runs the review gate', () => {
  /** A worker's task through to its result, under review. */
  async function reviewed(context, body = 'Parser done') {
    const fixture = await withTiers(context, { review: 'members' })
    fixture.open()
    await context.dispatcher.pass()
    await context.dispatcher.pass()
    context.adapter.answer('zeus', body)
    await context.dispatcher.pass()
    return fixture
  }
  const lead = (context) => context.adapter.agent('lead').items.map((item) => item.text)

  it("puts a member's result in review with an independent reviewer, then releases it on pass", async () => {
    await setup(async (context) => {
      const { task } = await reviewed(context)
      assert.equal(task(1).state, 'review')
      await context.dispatcher.pass()
      const review = task(2)
      assert.deepEqual(
        [review.kind, review.assignee, review.state],
        ['review', 'astraeus', 'queued'],
        "calliope shares the author's model; astraeus does not",
      )
      assert.match(context.adapter.prepared.at(-1).message, /Review T-1 \(round 1\) by @zeus\./)
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'working')
      context.adapter.answer('astraeus', 'Looks right.\n\nVERDICT: pass')
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(1).round, task(2).state, task(2).verdict],
        ['done', 1, 'done', 'pass'],
      )
      for (let n = 0; n < 4; n += 1) await context.dispatcher.pass()
      assert.match(lead(context)[0], /result from @zeus\]\nParser done$/)
      context.adapter.answer('lead', 'noted')
      for (let n = 0; n < 3; n += 1) await context.dispatcher.pass()
      assert.match(
        lead(context).at(-1),
        /T-2 · result from @astraeus\]\nLooks right\.\n\nVERDICT: pass$/,
      )
    })
  })

  it('sends the work back once on changes, then lets the requester decide', async () => {
    await setup(async (context) => {
      const { task, notes } = await reviewed(context)
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('astraeus', 'Missing tests.\n\nVERDICT: changes')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee, task(1).round], ['queued', 'zeus', 1])
      await context.dispatcher.pass()
      assert.match(
        context.adapter.agent('zeus').items.at(-1).text,
        /task from @astraeus\]\nReview round 1 by @astraeus asks for changes:\n\nMissing tests\./,
      )
      assert.deepEqual(lead(context), [], 'the requester has seen nothing')
      context.adapter.answer('zeus', 'Tests added')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(3).kind, task(3).assignee],
        ['review', 'review', 'astraeus'],
      )
      await context.dispatcher.pass()
      context.adapter.answer('astraeus', 'Still wrong.\n\nVERDICT: changes')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).round], ['done', 2])
      assert.deepEqual(notes('lead'), [
        'T-1 asked for changes twice in review (T-2, T-3); its result and the reviews are in your inbox: accept it or send it back.',
      ])
    })
  })

  it('skips the review when no independent reviewer is on the team, and says so', async () => {
    await setup(async (context) => {
      const fixture = await withTiers(context, { review: 'members', reviewers: ['calliope'] })
      fixture.open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(fixture.task(1).state, 'done')
      assert.deepEqual(fixture.notes('lead'), [
        'T-1 unreviewed: no independent reviewer on the team.',
      ])
      assert.equal(context.host.last('calliope'), undefined, 'no review window opened')
    })
  })

  it('waits for a busy reviewer rather than skipping, and replaces one whose window closes', async () => {
    await setup(async (context) => {
      const { open, task } = await reviewed(context)
      await context.dispatcher.pass()
      assert.equal(task(2).assignee, 'astraeus')
      open({ tags: ['rust'], body: 'Lexer' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'Lexer done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(3).state, 'review')
      assert.equal(
        context.ledger.reviewsPending(1).length,
        1,
        "T-3 waits for astraeus: calliope shares its author's model",
      )

      await context.host.exit('astraeus')
      assert.equal(task(2).state, 'cancelled', 'the review went with the window')
      assert.equal(task(1).state, 'review', 'the work still waits')
      await context.dispatcher.pass()
      const again = context.ledger
        .board(1)
        .lanes.find((l) => l.participant.handle === 'astraeus').tasks
      assert.deepEqual(
        again.filter((t) => t.state === 'queued').map((t) => t.reviewOf),
        [task(1).id],
        'a new review of T-1 for a fresh astraeus window',
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
      open({ tags: ['rust'] })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      context.adapter.quota('zeus', { state: 'exhausted', resetsAt: soon(context, 2) })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['queued', 'diana'])
      assert.match(
        task(1).body,
        /Reassigned from @zeus, which ran out of quota after starting; check the working tree/,
      )
      assert.deepEqual(notes('lead'), [
        'T-1 was taken back from @zeus (ran out of quota after starting) and waits for another standard worker.',
      ])
      assert.equal(
        context.ledger.project(1).participants.find((p) => p.id === id('zeus')).outUntil,
        soon(context, 2),
      )
      assert.deepEqual(context.dispatcher.activity(id('zeus')), {
        state: 'out',
        reason: `out of quota until ${soon(context, 2)}`,
      })
    })
  })

  it('gives no new work to a member low on quota, keeps one out for an hour when its reset is unknown, and takes it again after', async () => {
    await setup(async (context) => {
      const { open, task, id } = await withTiers(context)
      open({ tags: ['rust'] })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('zeus', 'done')
      await context.dispatcher.pass()
      context.adapter.quota('zeus', { state: 'low', usedPercent: 97 })
      await context.dispatcher.pass()
      open({ tags: ['rust'], body: 'Lexer' })
      await context.dispatcher.pass()
      assert.equal(task(2).assignee, 'diana', 'zeus is low on quota')

      context.adapter.quota('zeus', { state: 'exhausted' })
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.project(1).participants.find((p) => p.id === id('zeus')).outUntil,
        soon(context, 1),
      )
      context.adapter.quota('zeus', null)
      open({ body: 'Tests' })
      await context.dispatcher.pass()
      assert.equal(task(3).state, 'open', 'diana is busy and zeus is out')
      context.clock.advance(2 * 3_600_000)
      await context.dispatcher.pass()
      assert.equal(task(3).assignee, 'zeus')
    })
  })
})
