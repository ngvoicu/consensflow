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
  return adapter
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

/** A project with its lead window up and its workers (Zeus, unless told) in the team from the start. */
async function withTeam(context, workers = ['zeus']) {
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    harness: 'claude-code',
    team: workers.map((agent) => ({ agent, harness: 'claude-code', role: 'worker' })),
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
        team: [{ agent: 'zeus', harness: 'claude-code', role: 'worker' }],
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
