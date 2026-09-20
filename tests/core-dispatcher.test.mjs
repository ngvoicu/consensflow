import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { Dispatcher, deliveryText } from '../src/core/dispatcher.js'
import { openLedger, SESSION_IDLE_MS } from '../src/ledger/index.js'

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

/** The saved model of each fake agent: what makes a reviewer independent of an author. */
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
      return { ok: true, outcome: 'cleared' }
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

  it('opens no window for a brief the human has not approved, and delivers a result only once approved', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      context.ledger.setGate(project.id, true)
      context.ledger.createTask(project.id, {
        from: 'lead',
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
      assert.equal(context.adapter.agent('lead').items.length, 0, 'the lead waits for the human')
      context.ledger.approveMessage(result.id, { by: 'human' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.inbox(id('lead'))[0].state, 'delivered')
      assert.match(
        context.adapter.agent('lead').items.at(-1).text,
        /result from @zeus-amber-pine\]\nParser done/,
      )
    })
  })

  it('gives out a task only once every task it needs is accepted, and says nothing while it waits', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context, ['zeus', 'diana'])
      const add = (body, extra = {}) =>
        context.ledger.createTask(project.id, {
          from: 'lead',
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
        context.ledger.inbox(id('lead')).some((m) => m.kind === 'note'),
        false,
        'no "waits for a free worker" note: it waits for its need',
      )
      context.adapter.answer('zeus', 'Lexer done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(states(), ['done', 'open'], 'done is not accepted')
      context.ledger.acceptTask(project.id, 1, { by: 'lead' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(states(), ['accepted', 'working'])
      assert.match(context.adapter.prepared.at(-1).message, /T-2 · task from @lead\]\nParser$/)
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
      assert.match(
        lead().items[0].text,
        /result from @zeus\]\none done\n\nDecide with: cf task accept T-1/,
      )
      await context.dispatcher.pass()
      const [first, second] = context.ledger.inbox(id('lead')).reverse()
      assert.equal(first.state, 'delivered')
      assert.equal(first.receipt.item, lead().items[0].id)
      assert.equal(second.state, 'queued', 'the next waits until the lead is idle again')

      context.adapter.answer('lead', 'noted')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(context.ledger.inbox(id('lead'))[0].state, 'delivered')
      assert.match(
        lead().items.at(-1).text,
        /result from @diana\]\ntwo done\n\nDecide with: cf task accept T-2/,
      )
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

  it('waits on a message a harness queued itself, and re-sends only a paste the record never showed', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      await context.dispatcher.pass()
      context.adapter.answer('lead', 'ready')
      // The lead's own queue took it (a peer inbox, a broker): it will show
      // when the harness gets to it, maybe minutes later; sending it again
      // would only make a duplicate the harness may even drop.
      context.adapter.agent('lead').queued = true
      context.adapter.agent('lead').arrive = false
      const note = context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'Queued' })
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
        .agent('lead')
        .items.push(item('user', `[ConsensFlow m-${note.id} · note from @zeus]\nQueued`))
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(note.id).state, 'delivered')

      // A paste has no such receipt: the record is the only proof, and 60 s
      // without it means the paste was lost.
      context.adapter.agent('lead').queued = false
      const pasted = context.ledger.note(project.id, { from: 'zeus', to: 'lead', body: 'Pasted' })
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

  it('closes a project: its windows go, tiered work returns to the backlog, and Resume brings it back', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      context.ledger.createTask(project.id, {
        from: 'lead',
        pool: 'worker',
        tier: 'standard',
        body: 'Parser',
      })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const lead = context.host.last('lead')
      const zeus = context.host.last('zeus')
      const native = context.ledger.currentConversation(id('lead')).nativeSession
      const closed = await context.dispatcher.closeProject(project.id)
      assert.equal(closed.state, 'suspended')
      assert.deepEqual(context.host.killed, [
        { id: lead.id, generation: lead.generation },
        { id: zeus.id, generation: zeus.generation },
      ])
      await context.host.exit('lead')
      await context.host.exit('zeus')
      assert.equal(context.dispatcher.pane(id('lead')), null)
      const task = context.ledger.task(project.id, 1)
      assert.deepEqual(
        [task.state, task.assignee],
        ['open', null],
        'the work waits for a member again',
      )
      await context.dispatcher.pass()
      assert.equal(context.host.opened.length, 2, 'nothing reopens while suspended')

      await context.dispatcher.resumeProject(project.id)
      assert.equal(context.adapter.prepared.at(-1).resume, native)
      await context.dispatcher.pass()
      assert.match(context.ledger.task(project.id, 1).assignee, /^zeus-/, 'a fresh session of zeus')
    })
  })

  it('deletes a closed project for good, and refuses an open one', async () => {
    await setup(async (context) => {
      const { project } = await withTeam(context)
      await assert.rejects(context.dispatcher.deleteProject(project.id), { code: 'project-open' })
      await context.dispatcher.closeProject(project.id)
      assert.deepEqual(await context.dispatcher.deleteProject(project.id), {
        id: project.id,
        name: 'app',
      })
      assert.deepEqual(context.ledger.projects(), [])
      await context.dispatcher.pass()
      assert.equal(context.host.opened.length, 1, 'nothing reopens')
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
    const options = [{ question: 'Which?', header: 'Format', options: [], multiple: false }]
    assert.equal(
      deliveryText({ ...base, kind: 'question', questions: options }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nAnswer with: cf answer m-12 "…" (a label or your own words)',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'question', questions: [...options, ...options] }),
      '[ConsensFlow m-12 · T-3 · question from @zeus]\nWhich format?\n\nAnswer with: cf answer m-12 "…" (a label or your own words; one line per question)',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'note', sender: null, taskNumber: null, body: 'hi' }),
      '[ConsensFlow m-12 · note from ConsensFlow]\nhi',
    )
    assert.equal(
      deliveryText({ ...base, kind: 'result', body: 'Parser done' }),
      '[ConsensFlow m-12 · T-3 · result from @zeus]\nParser done\n\nDecide with: cf task accept T-3 · cf task reopen T-3 "…" · cf task review T-3',
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

/** A tiered team: standard workers (zeus and diana, unless told), a light worker, and reviewers on two models. */
async function withTiers(
  context,
  { review = 'none', workers = ['zeus', 'diana'], reviewers = ['calliope', 'astraeus'] } = {},
) {
  const member = (agent, role, tier) => ({ agent, harness: 'claude-code', role, tier })
  const project = await context.dispatcher.openProject({
    directory: '/work/app',
    name: 'app',
    harness: 'claude-code',
    review,
    team: [
      ...workers.map((agent) => member(agent, 'worker', 'standard')),
      member('hera', 'worker', 'light'),
      ...reviewers.map((agent) => member(agent, 'reviewer', 'standard')),
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

  it('tells the requester once when nobody of the tier is free, and assigns when one frees up', async () => {
    await setup(async (context) => {
      const { open, task, notes } = await withTiers(context)
      // Two sessions per member: four tasks fill both standard workers.
      for (const body of ['One', 'Two', 'Three', 'Four']) open({ body })
      await context.dispatcher.pass()
      open({ body: 'Fifth' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(5).state, 'open')
      assert.deepEqual(notes('lead'), [
        'T-5 waits for a free standard worker: @zeus and @diana are busy.',
      ])
      context.adapter.answer('zeus', 'done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(5).state, 'queued')
      assert.match(task(5).assignee, /^zeus-/, 'a new session of the member that freed a slot')
      assert.equal(notes('lead').length, 1)
    })
  })
})

describe('the dispatcher runs the review gate', () => {
  /** A worker's task through to its result, under review. */
  async function reviewed(context, body = 'Parser done', options = {}) {
    const fixture = await withTiers(context, { review: 'members', ...options })
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
        ['review', 'astraeus-brisk-birch', 'queued'],
        "calliope shares the author's model; astraeus does not",
      )
      assert.match(
        context.adapter.prepared.at(-1).message,
        /Review T-1 \(round 1\) by @zeus-amber-pine\./,
      )
      await context.dispatcher.pass()
      assert.equal(task(2).state, 'working')
      context.adapter.answer('astraeus', 'Looks right.\n\nVERDICT: pass')
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(1).round, task(2).state, task(2).verdict],
        ['done', 1, 'done', 'pass'],
      )
      for (let n = 0; n < 4; n += 1) await context.dispatcher.pass()
      assert.match(
        lead(context)[0],
        /T-1 · result from @zeus-amber-pine\]\nParser done\n\nReviewed by @astraeus-brisk-birch, round 1: pass\nLooks right\.\n\nVERDICT: pass\n\nDecide with: cf task accept T-1 · cf task reopen T-1 "…" · cf task review T-1$/,
        'one delivery: the result with its review under it',
      )
      context.adapter.answer('lead', 'noted')
      for (let n = 0; n < 3; n += 1) await context.dispatcher.pass()
      assert.ok(
        !lead(context).some((text) => text.includes('result from @astraeus')),
        'no second message for the review',
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
      assert.deepEqual(
        [task(1).state, task(1).assignee, task(1).round],
        ['queued', 'zeus-amber-pine', 1],
      )
      await context.dispatcher.pass()
      assert.match(
        context.adapter.agent('zeus').items.at(-1).text,
        /task from @astraeus-brisk-birch\]\nReview round 1 by @astraeus-brisk-birch asks for changes:\n\nMissing tests\./,
      )
      assert.deepEqual(lead(context), [], 'the requester has seen nothing')
      context.adapter.answer('zeus', 'Tests added')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(3).kind, task(3).assignee],
        ['review', 'review', 'astraeus-calm-brook'],
      )
      await context.dispatcher.pass()
      context.adapter.answer('astraeus', 'Still wrong.\n\nVERDICT: changes')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).round], ['done', 2])
      assert.deepEqual(notes('lead'), [], 'no note: the result says it')
      for (let n = 0; n < 4; n += 1) await context.dispatcher.pass()
      assert.match(
        lead(context).at(-1),
        /result from @zeus-amber-pine\]\nTests added\n\nReviewed by @astraeus-brisk-birch, round 1: changes\nMissing tests\.\n\nVERDICT: changes\n\nReviewed by @astraeus-calm-brook, round 2: changes\nStill wrong\.\n\nVERDICT: changes\n\nThe reviewer asked for changes twice\. Accept it, or send it back with what to change\.\n\nDecide with: /,
      )
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
      assert.deepEqual(fixture.notes('lead'), [], 'no note: the result says it')
      for (let n = 0; n < 3; n += 1) await context.dispatcher.pass()
      assert.match(
        context.adapter.agent('lead').items.at(-1).text,
        /Parser done\n\nUnreviewed: no independent reviewer on the team\.\n\nDecide with: /,
      )
      assert.equal(context.host.last('calliope'), undefined, 'no review window opened')
    })
  })

  it('waits for a busy reviewer rather than skipping, and replaces one whose window closes', async () => {
    await setup(async (context) => {
      const { open, task } = await reviewed(context, 'Parser done', { reviewers: ['astraeus'] })
      await context.dispatcher.pass()
      assert.match(task(2).assignee, /^astraeus-/)
      open({ body: 'Lexer' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.match(task(3).assignee, /^diana-/, 'zeus holds its work until the verdict')
      context.adapter.answer('diana', 'Lexer done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(3).state, 'review')
      assert.equal(
        context.ledger.reviewsPending(1).length,
        0,
        "the only reviewer's second slot takes the second review",
      )
      assert.match(task(4).assignee, /^astraeus-/)
      assert.notEqual(task(4).assignee, task(2).assignee, 'in a session of its own')

      await context.host.exit(task(2).assignee)
      assert.equal(task(2).state, 'cancelled', 'the review went with the window')
      assert.equal(task(1).state, 'review', 'the work still waits')
      await context.dispatcher.pass()
      const again = context.ledger
        .board(1)
        .lanes.filter((l) => l.participant.member === 'astraeus')
        .flatMap((l) => l.tasks)
      assert.deepEqual(
        again.filter((t) => t.state === 'queued').map((t) => t.reviewOf),
        [1],
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
      open()
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'working')
      context.adapter.quota('zeus', { state: 'exhausted', resetsAt: soon(context, 2) })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(1).assignee], ['queued', 'diana-brisk-birch'])
      assert.match(
        task(1).body,
        /Reassigned from @zeus-amber-pine, which ran out of quota after starting; check the working tree/,
      )
      assert.deepEqual(notes('lead'), [
        'T-1 was taken back from @zeus-amber-pine (ran out of quota after starting) and waits for another standard worker.',
      ])
      assert.equal(
        context.ledger.project(1).participants.find((p) => p.id === id('zeus')).outUntil,
        soon(context, 2),
      )
      assert.equal(
        context.ledger.project(1).participants.some((p) => p.handle === 'zeus-amber-pine'),
        false,
        'the session that ran out is gone; the member is out until its reset',
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
        to: 'lead',
        task: 1,
        body: 'Which?',
      })
      context.adapter.agent('zeus').arrive = false
      context.adapter.answer('zeus', 'asked')
      await context.dispatcher.pass()
      context.adapter.answer('lead', 'This one, replying')
      const answer = context.ledger.answer(question.id, { from: 'lead', body: 'This one' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.message(answer.id).state, 'delivering')
      context.adapter.quota('zeus', { state: 'exhausted', at: context.clock.now().toISOString() })
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(1).assignee, context.ledger.message(answer.id).state],
        ['open', null, 'cancelled'],
        'the task goes back to the board and the answer in flight goes with it',
      )

      context.adapter.answer('lead', 'noted')
      const own = context.ledger.createTask(1, { from: 'human', to: 'lead', body: 'Plan' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(own.task.number).state, 'working')
      context.adapter.quota('lead', { state: 'exhausted', at: context.clock.now().toISOString() })
      context.adapter.agent('lead').settled = true
      await context.dispatcher.pass()
      assert.equal(task(own.task.number).state, 'working', 'a coordinator keeps its task')
      assert.equal(context.dispatcher.activity(id('lead')).state, 'out')
      const later = context.ledger.note(1, { from: 'zeus', to: 'lead', body: 'Ready' })
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
      open({ body: 'Docs' })
      await context.dispatcher.pass()
      assert.match(task(3).assignee, /^diana-/, "diana's second slot")
      assert.equal(task(4).state, 'open', 'diana is full and zeus is low')
      context.clock.advance(2 * 3_600_000)
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

describe('a member with several roles', () => {
  it('opens with the text of the role its task needs, and reviews nothing while its own work is reviewed', async () => {
    await setup(async (context) => {
      const project = await context.dispatcher.openProject({
        directory: '/work/app',
        name: 'app',
        harness: 'claude-code',
        review: 'members',
        team: [
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
        context.ledger.createTask(project.id, { from: 'lead', pool, tier, body })
      open('Write the parser', 'standard')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).assignee, 'zeus-amber-pine')
      context.adapter.answer('zeus', 'Parser done')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(1).state, task(2).kind, task(2).assignee],
        ['review', 'review', 'hera-brisk-birch'],
      )
      assert.deepEqual(
        launches('hera'),
        [['reviewer', 'instructions for reviewer']],
        'a review opens the reviewer text',
      )
      open('Write the docs', 'complex')
      await context.dispatcher.pass()
      assert.match(
        task(3).assignee,
        /^hera-/,
        'hera reviews in one session and, with a slot free, works in another',
      )
      assert.notEqual(task(3).assignee, task(2).assignee)
      await context.dispatcher.pass()
      context.adapter.answer(task(2).assignee, 'Fine.\n\nVERDICT: pass')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.equal(task(1).state, 'done')
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
      assert.equal(task(4).state, 'done', 'advice is never reviewed')
      assert.equal(context.ledger.task(project.id, 4).messages.at(-1).recipient, 'lead')
    })
  })
})

describe('one task per member session', () => {
  const zeusWindows = (context) => context.host.opened.filter((b) => windowOf(b.id, 'zeus'))

  /** T-1 to zeus, the only standard worker here, delivered and answered. */
  async function finished(context, options = {}) {
    const fixture = await withTiers(context, { workers: ['zeus'], ...options })
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

      context.ledger.acceptTask(project.id, 1, { by: 'lead' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.currentConversation(session), null, 'accepted: the session ends')
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

  it('keeps the author open through its review, lands the send-back there, and closes the reviewer after its verdict', async () => {
    await setup(async (context) => {
      const { task } = await finished(context, { review: 'members' })
      assert.equal(task(1).state, 'review')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(
        [task(2).kind, task(2).assignee, task(2).state],
        ['review', 'astraeus-brisk-birch', 'working'],
      )
      assert.deepEqual(context.host.killed, [], 'an author under review keeps its window')
      const reviewer = context.host.last('astraeus')
      context.adapter.answer('astraeus', 'Name the error.\n\nVERDICT: changes')
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      assert.deepEqual(context.host.killed, [{ id: reviewer.id, generation: reviewer.generation }])
      assert.equal(
        zeusWindows(context).length,
        1,
        'the send-back lands in the session that did the work',
      )
      assert.match(
        context.adapter.agent('zeus').items.at(-1).text,
        /Review round 1 by @astraeus-brisk-birch asks for changes:/,
      )
    })
  })

  it('never closes a coordinator: the lead keeps its window after its own task', async () => {
    await setup(async (context) => {
      const { project, id } = await withTeam(context)
      context.ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Plan the week' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      context.adapter.answer('lead', 'Planned.')
      context.ledger.recordResult(project.id, 1, { body: 'Planned.' })
      await context.dispatcher.pass()
      assert.equal(context.ledger.task(project.id, 1).state, 'done')
      assert.deepEqual(context.host.killed, [])
      assert.notEqual(context.dispatcher.pane(id('lead')), null)
    })
  })

  it('after a restart, gives up a member task with no window, and resumes one whose answer is due', async () => {
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
        to: 'lead',
        task: 2,
        body: 'Which dialect?',
      })
      context.adapter.answer('diana', 'I asked the lead.')
      await context.dispatcher.pass()
      assert.deepEqual([task(1).state, task(2).state], ['working', 'waiting'])
      const native = context.ledger.currentConversation(id('diana-brisk-birch')).nativeSession

      context.ledger.suspendForRestart()
      const after = context.make()
      await after.resumeAfterRestart()
      context.ledger.answer(question.id, { from: 'lead', body: 'ANSI' })
      await after.pass()
      assert.deepEqual(
        [task(1).state, task(1).assignee],
        ['open', null],
        'nobody was working on it any more',
      )
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['diana-brisk-birch', native],
        'its own session, with the brief in it',
      )
      assert.match(launch.message, /^\[ConsensFlow m-\d+ · T-2 · answer from @lead\]\nANSI$/)
      await after.pass()
      assert.equal(task(2).state, 'working')
    })
  })

  it('reopens a finished task on its own session, resumed with the follow-up and nothing else', async () => {
    await setup(async (context) => {
      const { project, id, task } = await finished(context)
      const native = context.ledger.currentConversation(id('zeus-amber-pine')).nativeSession
      context.ledger.reopenTask(project.id, 1, { by: 'lead', body: 'Handle empty input too' })
      await context.dispatcher.pass()
      const launch = context.adapter.prepared.at(-1)
      assert.deepEqual(
        [launch.participant.handle, launch.resume],
        ['zeus-amber-pine', native],
        'the same window comes back on its own conversation',
      )
      assert.match(
        launch.message,
        /^\[ConsensFlow m-\d+ · T-1 · task from @lead\]\nHandle empty input too$/,
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
        from: 'lead',
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
        /^\[ConsensFlow m-\d+ · T-2 · task from @lead\]\nNow the lexer, in the same style$/,
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

  it('ends a session left idle after its work, and closes the window of a session that ended', async () => {
    await setup(async (context) => {
      const { project, open, task } = await finished(context)
      context.clock.advance(SESSION_IDLE_MS + 1000)
      await context.dispatcher.pass()
      assert.equal(
        context.ledger.project(project.id).participants.some((p) => p.handle === 'zeus-amber-pine'),
        false,
        'idle past its time: the session ended',
      )
      assert.equal(task(1).state, 'done', 'its work stays for the lead to accept')

      open({ body: 'Docs' })
      await context.dispatcher.pass()
      await context.dispatcher.pass()
      const working = task(2)
      assert.equal(working.state, 'working')
      const killed = context.host.killed.length
      context.ledger.cancelTask(project.id, 2, { by: 'lead' })
      await context.dispatcher.pass()
      assert.equal(context.host.killed.length, killed + 1, "the ended session's window is closed")
      assert.equal(context.host.killed.at(-1).id, `p${project.id}-${working.assignee}`)
    })
  })
})
