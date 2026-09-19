import { randomUUID } from 'node:crypto'

/**
 * The dispatcher: the only actor in the daemon. Agents never open panes or
 * type into them; they change the ledger (a task, a question, an answer), and
 * the dispatcher makes it happen in the panes.
 *
 * On every pass, for each participant with something to do:
 * - A queued message for a participant without a window launches the window
 *   with the message as its first input; the launch is the delivery.
 * - A live window that its harness reports idle (and not waiting for the
 *   human) gets the head of its queue through the adapter's native path.
 * - A delivery counts only when the harness's own record shows the message
 *   (its `[ConsensFlow m-<id> ·` header in a user item). A refused delivery, or
 *   one that never shows in time (admitted or uncertain), is tried again, up
 *   to `maxAttempts`; a launch whose first message never shows is ended and its
 *   task failed. Adapters say when a window can take input at all (`ready`).
 * - A worker's turn that ends after its task's latest message finishes the
 *   task with the answer written after that message. A task waiting on a
 *   question is left alone. Coordinators (lead, PM) finish their own tasks
 *   explicitly, because their turns end while they wait for workers.
 * - A worker window that closes mid-task fails the task, and the requester is
 *   told. A lead window that closes suspends its project. A member who leaves
 *   the team has its window closed once its step in progress ends, and that
 *   exit fails nothing: its open tasks were cancelled when it left.
 * - A human typing in a window latches it against pastes. Their Enter releases
 *   the latch once the harness shows a new message of theirs; the pane host
 *   keeps it if they typed again after that Enter.
 *
 * Harness specifics live in the adapters (`src/adapters/`); the pane host is
 * the Rust PTY host behind the bridge. Time is an argument, so every rule is
 * testable with explicit passes (`tests/core-dispatcher.test.mjs`).
 */

const INLINE_LIMIT = 4000
const OPENING = 3000
const RECEIVED_ROLES = new Set(['user', 'custom', 'tool'])
const COORDINATORS = new Set(['lead', 'pm'])

/** How a message reads in the recipient's pane. The header doubles as the arrival marker. */
export function deliveryText(message) {
  const from = message.sender === null ? 'ConsensFlow' : `@${message.sender}`
  const task =
    message.taskNumber === null || message.taskNumber === undefined
      ? ''
      : ` · T-${message.taskNumber}`
  const body =
    message.body.length <= INLINE_LIMIT
      ? message.body
      : `${message.body.slice(0, OPENING)}\n… (${message.body.length} characters; read all of it with: cf inbox read m-${message.id})`
  const footer = message.kind === 'question' ? `\n\nAnswer with: cf answer m-${message.id} "…"` : ''
  return `[ConsensFlow m-${message.id}${task} · ${message.kind} from ${from}]\n${body}${footer}`
}

const markerOf = (messageId) => `[ConsensFlow m-${messageId} ·`

/** How many messages in a harness record the human typed (ConsensFlow's own carry its header). */
const humanMessages = (observed) =>
  observed.items.filter((item) => item.role === 'user' && !item.text.includes('[ConsensFlow m-'))
    .length

export class Dispatcher {
  #ledger
  #host
  #adapters
  #clock
  #credentials
  #paneEnv
  #roster
  #roles
  #arrivalTimeoutMs
  #launchTimeoutMs
  #maxAttempts
  #runtime = new Map()
  #listeners = new Set()
  #generation = 0

  constructor({
    ledger,
    host,
    adapters,
    clock = { now: () => new Date() },
    credentials,
    paneEnv = () => ({}),
    roster = () => null,
    roles = () => undefined,
    arrivalTimeoutMs = 60_000,
    launchTimeoutMs = 180_000,
    maxAttempts = 3,
  }) {
    this.#ledger = ledger
    this.#host = host
    this.#adapters = adapters
    this.#clock = clock
    this.#credentials = credentials
    this.#paneEnv = paneEnv
    this.#roster = roster
    this.#roles = roles
    this.#arrivalTimeoutMs = arrivalTimeoutMs
    this.#launchTimeoutMs = launchTimeoutMs
    this.#maxAttempts = maxAttempts
    host.onExit((pane) => this.paneExited(pane))
    host.onEnter?.((enter) => this.#humanEnter(enter))
  }

  onChange(listener) {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  /** What a participant's window is doing: starting, working, idle, waiting (with why), closed. */
  activity(participantId) {
    return this.#runtime.get(participantId)?.activity ?? { state: 'closed' }
  }

  /** The live window of a participant, `{id, generation}`, or null. */
  pane(participantId) {
    return this.#runtime.get(participantId)?.pane ?? null
  }

  /** A new project: the ledger records it with its team, and its lead window opens. */
  async openProject({ directory, name, harness, team = [] }) {
    const project = this.#ledger.createProject({ directory, name, lead: { harness }, team })
    const lead = project.participants.find((participant) => participant.handle === 'lead')
    await this.#exclusive(lead.id, () => this.#launch(project, lead, null))
    return this.#ledger.project(project.id)
  }

  /** The human's Resume, and the restore after a restart: coordinators come back on their conversations. */
  async resumeProject(projectId) {
    const project = this.#ledger.setProjectState(projectId, 'open')
    for (const participant of project.participants) {
      if (!COORDINATORS.has(participant.role) || this.pane(participant.id) !== null) continue
      if (participant.role === 'pm' && this.#ledger.currentConversation(participant.id) === null)
        continue
      await this.#exclusive(participant.id, () => this.#launch(project, participant, null))
    }
    this.#changed()
    return this.#ledger.project(projectId)
  }

  /** Once, at start: the projects that were open when the previous process ended come back. */
  async resumeAfterRestart() {
    const outcomes = []
    for (const project of this.#ledger.projects().filter((s) => s.resumeOnStart)) {
      try {
        await this.resumeProject(project.id)
        outcomes.push({ project: project.id, resumed: true })
      } catch (cause) {
        outcomes.push({ project: project.id, resumed: false, error: cause.message })
      } finally {
        this.#ledger.forgetResume(project.id)
      }
    }
    return outcomes
  }

  /**
   * The human takes a member off the team. It waits for the member's step in
   * progress, so a window that is still opening is closed too, not left behind.
   */
  async removeMember(projectId, handle) {
    const member = this.#ledger
      .project(projectId)
      ?.participants.find((participant) => participant.handle === handle)
    // Not in the team: the ledger refuses it and says why.
    if (member === undefined) return this.#ledger.removeMember(projectId, handle)
    return this.#exclusive(
      member.id,
      async () => {
        const removed = this.#ledger.removeMember(projectId, handle)
        const { pane } = this.#runtimeOf(member.id)
        if (pane !== null) await this.#host.kill(pane).catch(() => {})
        this.#changed()
        return removed
      },
      { wait: true },
    )
  }

  /** One pass over every participant; each moves on its own, so a slow launch holds up no one else. */
  async pass() {
    const steps = []
    for (const project of this.#ledger.projects()) {
      for (const participant of project.participants) {
        if (participant.role === 'human') continue
        steps.push(this.#exclusive(participant.id, () => this.#step(project, participant)))
      }
    }
    await Promise.all(steps)
  }

  /** A window ended: `pane.exit` from the pane host. */
  async paneExited({ id, generation }) {
    const entry = [...this.#runtime].find(
      ([, runtime]) => runtime.pane?.id === id && runtime.pane.generation === generation,
    )
    if (entry === undefined) return
    const [participantId, runtime] = entry
    this.#credentials.revoke(runtime.token)
    const delivering = runtime.delivering
    Object.assign(runtime, {
      pane: null,
      launch: null,
      token: null,
      delivering: null,
      activity: { state: 'closed' },
    })
    const project = this.#projectOf(participantId)
    if (project === null) return
    const participant = project.participants.find((p) => p.id === participantId)
    if (delivering !== null) {
      this.#settleFailure(delivering, `@${participant.handle}'s window closed`, {
        retry: !delivering.launch,
      })
    }
    if (participant.role === 'lead') {
      if (project.state === 'open') this.#ledger.setProjectState(project.id, 'suspended')
    } else if (!COORDINATORS.has(participant.role)) {
      const task = this.#ledger.activeTask(participantId)
      if (task !== null) this.#failTask(project, task, `@${participant.handle}'s window closed`)
    }
    this.#changed()
  }

  // --- one participant's step ----------------------------------------------------

  async #step(project, participant) {
    const runtime = this.#runtimeOf(participant.id)
    if (runtime.pane !== null) return this.#stepOpen(project, participant, runtime)
    if (project.state !== 'open') return
    const next = this.#ledger.nextDelivery(participant.id)
    if (next !== null) await this.#launch(project, participant, next)
  }

  async #stepOpen(project, participant, runtime) {
    let observed
    try {
      observed = await runtime.adapter.observe({
        launch: runtime.launch,
        pane: runtime.pane,
        conversation: this.#ledger.currentConversation(participant.id),
        host: this.#host,
      })
    } catch (cause) {
      this.#setActivity(runtime, { state: 'unknown', reason: cause.message })
      return
    }
    this.#setActivity(
      runtime,
      observed.waiting
        ? { state: 'waiting', reason: observed.waiting.reason ?? null }
        : { state: observed.settled ? 'idle' : 'working' },
    )
    if (runtime.delivering !== null) this.#watchArrival(runtime, observed)
    await this.#releaseDraft(runtime, observed)
    if (!COORDINATORS.has(participant.role)) this.#collect(project, participant, observed)
    if (runtime.delivering === null && observed.settled && !observed.waiting) {
      const next = this.#ledger.nextDelivery(participant.id)
      if (next !== null) await this.#deliver(runtime, next)
    }
  }

  #watchArrival(runtime, observed) {
    const { delivering } = runtime
    const arrived = observed.items.find(
      (item) => RECEIVED_ROLES.has(item.role) && item.text.includes(delivering.marker),
    )
    if (arrived !== undefined) {
      this.#ledger.confirmDelivery(delivering.messageId, { item: arrived.id })
      runtime.delivering = null
      this.#changed()
      return
    }
    const waited = this.#now() - delivering.since
    if (delivering.launch) {
      if (waited <= this.#launchTimeoutMs) return
      runtime.delivering = null
      this.#host.kill(runtime.pane).catch(() => {})
      this.#settleFailure(delivering, 'the window never showed its first message', { retry: false })
      return
    }
    if (waited <= this.#arrivalTimeoutMs) return
    runtime.delivering = null
    // Admitted or uncertain, a message whose header is still missing from the
    // harness record after the whole window did not land: sending it again is
    // how it reaches the reader, and the header would show a late duplicate.
    this.#settleFailure(delivering, 'the harness record never showed it', { retry: true })
  }

  #humanEnter({ id, generation, epoch }) {
    const runtime = [...this.#runtime.values()].find(
      (candidate) => candidate.pane?.id === id && candidate.pane.generation === generation,
    )
    runtime?.enters.push({ epoch, baseline: runtime.humanItems })
  }

  /** Each Enter counts once a new message of the human's (no ConsensFlow header) is on record. */
  async #releaseDraft(runtime, observed) {
    const human = humanMessages(observed)
    let expected = null
    let released = null
    for (const enter of runtime.enters) {
      enter.baseline ??= runtime.humanItems ?? human
      expected = Math.max(enter.baseline, expected ?? enter.baseline) + 1
      if (human >= expected) released = enter
    }
    runtime.humanItems = human
    if (released === null) return
    runtime.enters = runtime.enters.filter((enter) => enter.epoch > released.epoch)
    await this.#host
      .request('draft.clear', {
        ...runtime.pane,
        epoch: released.epoch,
        submission: `human-${human}`,
      })
      .catch(() => {})
  }

  /** A worker's answer to its task's latest message finishes the task. */
  #collect(project, participant, observed) {
    const task = this.#ledger.activeTask(participant.id)
    if (task === null || task.state !== 'working') return
    const latest = task.messages
      .filter(
        (message) =>
          message.recipient === participant.handle &&
          message.state === 'delivered' &&
          (message.kind === 'task' || message.kind === 'answer'),
      )
      .at(-1)
    if (latest === undefined) return
    const start = observed.items.findIndex((item) => item.text.includes(markerOf(latest.id)))
    if (start === -1) return
    if (observed.failed) {
      this.#failTask(project, task, `@${participant.handle}'s harness reported a failure`)
      return
    }
    if (!observed.settled) return
    const answer = observed.items
      .slice(start + 1)
      .filter((item) => item.role === 'assistant' && item.complete)
      .at(-1)
    if (answer === undefined) return
    this.#ledger.recordResult(project.id, task.number, {
      body: answer.text.trim() || '(the agent ended its turn without a written answer)',
    })
    this.#changed()
  }

  async #deliver(runtime, message) {
    if (runtime.adapter.ready !== undefined) {
      const ready = await runtime.adapter.ready({
        launch: runtime.launch,
        pane: runtime.pane,
        host: this.#host,
      })
      if (!ready) return
    }
    this.#ledger.beginDelivery(message.id)
    let outcome
    try {
      outcome = await runtime.adapter.deliver({
        launch: runtime.launch,
        pane: runtime.pane,
        host: this.#host,
        text: deliveryText(message),
      })
    } catch (cause) {
      // Uncertain is for a harness that may have taken it; an adapter that
      // throws never handed it over, and its error must show.
      outcome = { admitted: false, reason: `the delivery failed: ${cause.message}` }
    }
    const delivering = {
      messageId: message.id,
      marker: markerOf(message.id),
      since: this.#now(),
      launch: false,
      admitted: outcome.admitted,
    }
    if (outcome.admitted === false) {
      this.#settleFailure(delivering, outcome.reason ?? 'the harness refused it', { retry: true })
      return
    }
    runtime.delivering = delivering
    this.#changed()
  }

  // --- launches --------------------------------------------------------------------

  async #launch(project, participant, message) {
    const runtime = this.#runtimeOf(participant.id)
    const adapter = this.#adapters[participant.harness]
    if (adapter === undefined) throw new Error(`no adapter for ${participant.harness}`)
    const conversation = this.#ledger.currentConversation(participant.id)
    const resume = conversation?.nativeSession ?? null
    const launchId = randomUUID()
    const generation = this.#nextGeneration()
    const delivering =
      message === null
        ? null
        : {
            messageId: message.id,
            marker: markerOf(message.id),
            since: this.#now(),
            launch: true,
            admitted: true,
          }
    if (message !== null) this.#ledger.beginDelivery(message.id)

    let plan
    try {
      plan = await adapter.prepare({
        launchId,
        participant,
        role: participant.role,
        project,
        directory: project.directory,
        resume,
        message: message === null ? null : deliveryText(message),
        agent: participant.agent === null ? null : this.#roster(participant.agent),
        instructions: this.#roles(participant, project),
      })
    } catch (cause) {
      if (delivering !== null)
        this.#settleFailure(delivering, `the launch failed: ${cause.message}`, { retry: false })
      return
    }
    const token = this.#credentials.issue({ participant, project, generation })
    const pane = { id: `p${project.id}-${participant.handle}`, generation }
    const opened = await this.#host
      .open({
        ...pane,
        launch: launchId,
        cwd: project.directory,
        argv: plan.argv,
        env: { ...this.#paneEnv(participant, project), ...plan.env, CONSENSFLOW_TOKEN: token },
        dropEnv: plan.dropEnv,
      })
      .catch((cause) => ({ ok: false, error: cause.message }))
    if (opened?.ok !== true) {
      this.#credentials.revoke(token)
      if (delivering !== null) {
        this.#settleFailure(
          delivering,
          `the window did not open: ${opened?.error ?? 'no answer from the pane host'}`,
          { retry: false },
        )
      }
      return
    }

    const resumed = resume !== null && plan.nativeSession === resume
    let conversationId = conversation?.id
    if (!resumed) {
      conversationId = this.#ledger.startConversation(participant.id, {
        harness: participant.harness,
      }).id
      if (plan.nativeSession !== null && plan.nativeSession !== undefined)
        this.#ledger.bindConversation(conversationId, plan.nativeSession)
    }
    Object.assign(runtime, {
      adapter,
      pane,
      launch: plan.launch,
      token,
      delivering,
      enters: [],
      humanItems: null,
      activity: { state: 'starting' },
    })
    const started = await adapter
      .started({ launch: plan.launch, pane, host: this.#host })
      .catch((cause) => ({ error: cause.message }))
    if (started.error !== undefined && delivering !== null) {
      runtime.delivering = null
      this.#host.kill(pane).catch(() => {})
      this.#settleFailure(
        delivering,
        `the window could not take its first message: ${started.error}`,
        { retry: false },
      )
      this.#changed()
      return
    }
    if (started.nativeSession && started.nativeSession !== plan.nativeSession) {
      this.#ledger.bindConversation(conversationId, started.nativeSession)
    }
    // The human's messages so far, counted before anyone can type into the
    // window: an Enter is released only by a message after this count.
    const opening = await adapter
      .observe({ launch: plan.launch, pane, host: this.#host })
      .catch(() => null)
    if (opening !== null) runtime.humanItems = humanMessages(opening)
    this.#changed()
  }

  // --- failures ----------------------------------------------------------------------

  /** A delivery that did not arrive: try again while attempts remain, or give up. */
  #settleFailure(delivering, reason, { retry }) {
    const message = this.#ledger.message(delivering.messageId)
    if (message === null || message.state !== 'delivering') return
    if (retry && message.attempts < this.#maxAttempts) {
      this.#ledger.retryDelivery(message.id, reason)
    } else {
      this.#ledger.failDelivery(message.id, reason)
      if (message.kind === 'task' && message.taskNumber !== null) {
        const project = this.#ledger.project(message.projectId)
        const task = this.#ledger.task(project.id, message.taskNumber)
        if (task.state === 'failed') this.#tellRequester(project, task, reason)
      }
    }
    this.#changed()
  }

  #failTask(project, task, reason) {
    this.#ledger.failTask(project.id, task.number, { reason })
    this.#tellRequester(project, task, reason)
  }

  #tellRequester(project, task, reason) {
    this.#ledger.note(project.id, {
      to: task.requester,
      task: task.number,
      body: `T-${task.number} failed: ${reason}. Reopen it with: cf task reopen T-${task.number} "…"`,
    })
  }

  // --- small helpers -------------------------------------------------------------------

  /**
   * Runs `work` for one participant at a time. A pass that finds it busy moves
   * on; `wait` queues behind the step in progress instead.
   */
  async #exclusive(participantId, work, { wait = false } = {}) {
    const runtime = this.#runtimeOf(participantId)
    while (runtime.running !== null) {
      if (!wait) return undefined
      await runtime.running.catch(() => {})
    }
    runtime.running = (async () => {
      try {
        return await work()
      } finally {
        runtime.running = null
      }
    })()
    return runtime.running
  }

  #runtimeOf(participantId) {
    let runtime = this.#runtime.get(participantId)
    if (runtime === undefined) {
      runtime = {
        adapter: null,
        pane: null,
        launch: null,
        token: null,
        delivering: null,
        running: null,
        enters: [],
        humanItems: null,
        activity: { state: 'closed' },
      }
      this.#runtime.set(participantId, runtime)
    }
    return runtime
  }

  #setActivity(runtime, activity) {
    if (runtime.activity.state === activity.state && runtime.activity.reason === activity.reason)
      return
    runtime.activity = activity
    this.#changed()
  }

  #projectOf(participantId) {
    return (
      this.#ledger
        .projects()
        .find((project) => project.participants.some((p) => p.id === participantId)) ?? null
    )
  }

  #nextGeneration() {
    this.#generation = Math.max(this.#generation + 1, this.#now())
    return this.#generation
  }

  #now() {
    return this.#clock.now().getTime()
  }

  #changed() {
    for (const listener of this.#listeners) listener()
  }
}
