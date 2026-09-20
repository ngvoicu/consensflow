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
 * - One task per member session: a worker, advisor or reviewer window opens
 *   with its task and is closed, its conversation ended, once it holds no
 *   task (assigned, working, waiting or under review), so the next task starts
 *   from nothing; a fresh window whose first message is not the brief (an
 *   answer, a review's findings) gets the brief in front of it. A member's
 *   active task with no window and nothing due is given up (tiered work back
 *   to the board). Coordinators keep their windows and conversations.
 * - A worker window that closes mid-task fails the task, and the requester is
 *   told. A lead window that closes suspends its project. A member who leaves
 *   the team has its window closed once its step in progress ends, and that
 *   exit fails nothing: its open tasks were cancelled when it left.
 * - A task for a tier of member starts open: each pass gives it to a free
 *   member of that pool and tier that is not out of quota, the one matching
 *   most of its tags, then the one with the fewest tasks so far; when none is
 *   free the requester is told once. Under the project's review policy a
 *   finished task waits for a reviewer whose model differs from the author's
 *   (a coordinator's model is taken to be its harness); none on the team and
 *   the review is skipped with a note, all busy and it waits. A member whose
 *   harness reports a fresh refusal (one after it was last marked out) is out
 *   until the reset it names (an hour when it names none): its tiered task
 *   goes back to open for another member, a review it held is withdrawn, a
 *   delivery in flight is queued again, a coordinator keeps its own tasks for
 *   after the reset, and nothing reaches it while out. A refusal still in the
 *   record after the reset is history, not a new one; the member is simply
 *   eligible again. A member low on quota takes nothing new.
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

/**
 * How a message reads in the recipient's pane. The header doubles as the
 * arrival marker. A result carries its reviews under it, in plain words: one
 * delivery per finished task, the findings readable in full on the board.
 */
export function deliveryText(message, { reviews = [], unreviewed = null } = {}) {
  const from = message.sender === null ? 'ConsensFlow' : `@${message.sender}`
  const task =
    message.taskNumber === null || message.taskNumber === undefined
      ? ''
      : ` · T-${message.taskNumber}`
  const body =
    message.body.length <= INLINE_LIMIT
      ? message.body
      : `${message.body.slice(0, OPENING)}\n… (${message.body.length} characters; read all of it with: cf inbox read m-${message.id})`
  const footer =
    message.kind !== 'question'
      ? ''
      : message.questions
        ? `\n\nAnswer with: cf answer m-${message.id} "…" (a label or your own words${message.questions.length > 1 ? '; one line per question' : ''})`
        : `\n\nAnswer with: cf answer m-${message.id} "…"`
  return `[ConsensFlow m-${message.id}${task} · ${message.kind} from ${from}]\n${body}${reviewText(reviews, unreviewed)}${footer}`
}

function reviewText(reviews, unreviewed) {
  const done = reviews.filter((review) => review.state === 'done')
  const parts = done.map((review) => {
    const findings =
      review.findings === null
        ? ''
        : review.findings.length <= OPENING
          ? `\n${review.findings}`
          : `\n${review.findings.slice(0, OPENING)}\n… (read all of it with: cf task get T-${review.number})`
    return `Reviewed by @${review.reviewer}, round ${review.round}: ${review.verdict ?? 'pass (no verdict line)'}${findings}`
  })
  if (done.length >= 2 && done.at(-1).verdict === 'changes') {
    parts.push(
      'The reviewer asked for changes twice. Accept it, or send it back with what to change.',
    )
  }
  if (unreviewed !== null) parts.push(`Unreviewed: ${unreviewed}.`)
  return parts.length === 0 ? '' : `\n\n${parts.join('\n\n')}`
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
  #waitingNoted = new Set()
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
  async openProject({ directory, name, harness, team = [], review }) {
    const project = this.#ledger.createProject({
      directory,
      name,
      lead: { harness },
      team,
      ...(review === undefined ? {} : { review }),
    })
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

  /**
   * The human's Close: the project is suspended and every window of it goes.
   * Each window's exit settles what it was doing, the way any closed window
   * does; Resume brings the coordinators back on their conversations.
   */
  async closeProject(projectId) {
    const project = this.#ledger.setProjectState(projectId, 'suspended')
    for (const participant of project.participants) {
      await this.#exclusive(
        participant.id,
        async () => {
          const { pane } = this.#runtimeOf(participant.id)
          if (pane !== null) await this.#host.kill(pane).catch(() => {})
        },
        { wait: true },
      )
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
    for (const project of this.#ledger.projects()) {
      if (project.state !== 'open') continue
      this.#assignOpenTasks(project)
      this.#findReviewers(project)
    }
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
      retiring: false,
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
      if (task !== null) this.#giveUp(project, task, `@${participant.handle}'s window closed`)
    }
    this.#changed()
  }

  // --- one participant's step ----------------------------------------------------

  async #step(project, participant) {
    const runtime = this.#runtimeOf(participant.id)
    if (runtime.pane !== null) return this.#stepOpen(project, participant, runtime)
    if (project.state !== 'open') return
    const next = this.#ledger.nextDelivery(participant.id)
    if (next !== null) return this.#launch(project, participant, next)
    if (COORDINATORS.has(participant.role)) return
    // A member's session is its task's: with the window gone (a restart, a
    // crash) and nothing due to it, nobody is doing the work any more.
    const task = this.#ledger.activeTask(participant.id)
    if (task !== null) {
      this.#giveUp(project, task, `@${participant.handle}'s window is gone`)
      this.#changed()
    }
  }

  async #stepOpen(project, participant, runtime) {
    if (runtime.retiring) return
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
    if (observed.quota !== undefined) {
      runtime.quota = observed.quota ?? null
      // Low is soft: the current task continues, nothing new comes until the
      // reset it names (an hour when it names none). It outlives the window,
      // which closes with the task, so a low member is not asked again at once.
      if (runtime.quota?.state === 'low') {
        runtime.lowUntil = runtime.quota.resetsAt ?? new Date(this.#now() + 3_600_000).toISOString()
      } else if (runtime.quota !== null) runtime.lowUntil = null
    }
    const out = this.#isOut(participant)
    if (!out && this.#freshRefusal(participant, runtime.quota)) {
      this.#outOfQuota(project, participant, runtime)
      return
    }
    if (out) {
      this.#setActivity(runtime, {
        state: 'out',
        reason: `out of quota until ${participant.outUntil}`,
      })
      return
    }
    if (runtime.delivering !== null) this.#watchArrival(runtime, observed)
    await this.#releaseDraft(runtime, observed)
    if (!COORDINATORS.has(participant.role)) {
      this.#collect(project, participant, observed)
      // The window may have gone during this step (a launch that timed out).
      if (
        runtime.pane !== null &&
        runtime.delivering === null &&
        !this.#ledger.holdsWork(participant.id)
      ) {
        await this.#retire(participant, runtime)
        return
      }
    }
    if (runtime.delivering === null && observed.settled && !observed.waiting) {
      const next = this.#ledger.nextDelivery(participant.id)
      if (next !== null) await this.#deliver(project, runtime, next)
    }
  }

  /**
   * One task per member session: the window and its conversation end with the
   * work, so the next task starts a fresh session with nothing carried over.
   */
  async #retire(participant, runtime) {
    runtime.retiring = true
    const conversation = this.#ledger.currentConversation(participant.id)
    if (conversation !== null) this.#ledger.endConversation(conversation.id)
    await this.#host.kill(runtime.pane).catch(() => {})
    this.#changed()
  }

  /**
   * A member's fresh session starts from nothing: when its first message is
   * not the task's own brief (a reopening, a review's findings), the brief
   * goes in first. A resumed conversation has it already.
   */
  #launchText(project, participant, message, resume) {
    const text = this.#textFor(project, message)
    if (resume !== null || COORDINATORS.has(participant.role) || message.taskNumber == null) {
      return text
    }
    const task = this.#ledger.task(project.id, message.taskNumber)
    const brief = task?.messages.find(
      (m) => m.kind === 'task' && m.recipient === participant.handle,
    )
    if (brief === undefined || brief.id === message.id) return text
    return `${deliveryText(brief)}\n\n${text}`
  }

  /**
   * The role a window plays: a member with several roles opens with the text
   * of the one its task needs, a reviewer's for a review, its own otherwise.
   */
  #roleFor(project, participant, message) {
    if (message === null || message.taskNumber == null || COORDINATORS.has(participant.role)) {
      return participant.role
    }
    const task = this.#ledger.task(project.id, message.taskNumber)
    return task?.kind === 'review' ? 'reviewer' : participant.role
  }

  /** A work task's result reads with its reviews under it; anything else reads as it is. */
  #textFor(project, message) {
    if (message.kind !== 'result' || message.taskNumber == null) return deliveryText(message)
    const task = this.#ledger.task(project.id, message.taskNumber)
    if (task === null || task.kind !== 'work') return deliveryText(message)
    return deliveryText(message, {
      reviews: this.#ledger.reviewsOf(project.id, task.number),
      unreviewed: task.unreviewed,
    })
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
    if (delivering.queued || waited <= this.#arrivalTimeoutMs) return
    runtime.delivering = null
    // A paste the harness record never showed after the whole window did not
    // land: sending it again is how it reaches the reader, and the header
    // would show a late duplicate. (A message the harness queued itself waits
    // for the record, or for the window to close.)
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
    const body = answer.text.trim() || '(the agent ended its turn without a written answer)'
    if (task.kind === 'review') this.#ledger.recordVerdict(project.id, task.number, { body })
    else this.#ledger.recordResult(project.id, task.number, { body })
    this.#changed()
  }

  async #deliver(project, runtime, message) {
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
        text: this.#textFor(project, message),
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
      // The harness's own queue took it (a peer inbox, a broker, a plugin):
      // it shows when the harness gets to it, and sending it again would only
      // make a duplicate, which Claude even drops as a repeat.
      queued: outcome.queued === true,
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
      const role = this.#roleFor(project, participant, message)
      plan = await adapter.prepare({
        launchId,
        participant,
        role,
        project,
        directory: project.directory,
        resume,
        message: message === null ? null : this.#launchText(project, participant, message, resume),
        agent: participant.agent === null ? null : this.#roster(participant.agent),
        instructions: this.#roles({ ...participant, role }, project),
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

  // --- assignment and review ------------------------------------------------------------

  /** Each open task goes to the best free member of its tier; the requester hears once when none is. */
  #assignOpenTasks(project) {
    for (const task of this.#ledger.board(project.id).open) {
      const candidates = this.#ledger.candidates(project.id, task.number)
      const free = candidates.filter((member) => this.#available(member))
      if (free.length > 0) {
        this.#ledger.assignTask(project.id, task.number, this.#rank(free, task.tags)[0].id)
        this.#waitingNoted.delete(task.id)
        this.#changed()
      } else if (!this.#waitingNoted.has(task.id)) {
        this.#waitingNoted.add(task.id)
        this.#ledger.note(project.id, {
          to: task.requester,
          task: task.number,
          body: `T-${task.number} waits for a free ${task.tier} ${task.pool}: ${this.#whyNotFree(candidates)}.`,
        })
        this.#changed()
      }
    }
  }

  /** Each task in review gets a free reviewer on another model, or goes on unreviewed. */
  #findReviewers(project) {
    for (const task of this.#ledger.reviewsPending(project.id)) {
      const author = project.participants.find((p) => p.handle === task.assignee)
      const authorModel = this.#modelOf(author)
      const independent = this.#ledger.members(project.id, 'reviewer').filter((reviewer) => {
        const participant = project.participants.find((p) => p.id === reviewer.id)
        return this.#modelOf(participant) !== authorModel
      })
      if (independent.length === 0) {
        this.#ledger.skipReview(project.id, task.number, {
          reason: 'no independent reviewer on the team',
        })
        this.#changed()
        continue
      }
      const free = independent.filter((member) => this.#available(member))
      if (free.length === 0) continue
      this.#ledger.createReview(project.id, task.number, {
        reviewer: this.#rank(free, task.tags)[0].id,
      })
      this.#changed()
    }
  }

  /** Free: nothing on its hands, not out of quota, not low on it. */
  #available(member) {
    return !member.busy && !this.#isOut(member) && !this.#isLow(member)
  }

  #isLow(member) {
    const until = this.#runtime.get(member.id)?.lowUntil ?? null
    return until !== null && Date.parse(until) > this.#now()
  }

  #isOut(member) {
    return member.outUntil !== null && Date.parse(member.outUntil) > this.#now()
  }

  /**
   * A harness keeps its last record, so a refusal stays in view long after
   * its reset: only one dated after the member was last marked out is news.
   */
  #freshRefusal(participant, quota) {
    if (quota?.state !== 'exhausted') return false
    if (participant.outSince === null || !quota.at) return true
    return Date.parse(quota.at) > Date.parse(participant.outSince)
  }

  /** Most matching tags first, then the fewest tasks taken, then the earliest joined. */
  #rank(members, tags) {
    const matches = (member) => tags.filter((tag) => member.tags.includes(tag)).length
    return [...members].sort((a, b) => matches(b) - matches(a) || a.taken - b.taken || a.id - b.id)
  }

  #whyNotFree(candidates) {
    const names = (list) => list.map((member) => `@${member.handle}`)
    const out = candidates.filter(
      (m) => m.outUntil !== null && Date.parse(m.outUntil) > this.#now(),
    )
    const low = candidates.filter((m) => !out.includes(m) && this.#isLow(m))
    const busy = candidates.filter((m) => !out.includes(m) && !low.includes(m) && m.busy)
    const parts = []
    if (busy.length > 0) {
      parts.push(`${names(busy).join(' and ')} ${busy.length === 1 ? 'is' : 'are'} busy`)
    }
    for (const member of out)
      parts.push(`@${member.handle} is out of quota until ${member.outUntil}`)
    for (const member of low) parts.push(`@${member.handle} is low on quota`)
    return parts.join('; ')
  }

  /** What makes two agents the same model: the roster's model identity; a coordinator's is its harness. */
  #modelOf(participant) {
    const row = participant.agent === null ? null : this.#roster(participant.agent)
    return row?.profile?.modelKey ?? row?.model ?? participant.harness
  }

  /**
   * A member whose harness just refused it: out until the reset it names (an
   * hour when it names none). What it was receiving is queued again, its
   * tiered work goes back to the board, a review it held is withdrawn; its
   * own tasks (a coordinator's) wait for it.
   */
  #outOfQuota(project, participant, runtime) {
    const until = runtime.quota.resetsAt ?? new Date(this.#now() + 3_600_000).toISOString()
    this.#setActivity(runtime, { state: 'out', reason: `out of quota until ${until}` })
    this.#ledger.markOut(participant.id, { until, reason: 'out of quota' })
    if (runtime.delivering !== null) {
      const { delivering } = runtime
      runtime.delivering = null
      this.#settleFailure(delivering, 'the harness ran out of quota', { retry: true })
    }
    const lane = this.#ledger
      .board(project.id)
      .lanes.find((l) => l.participant.id === participant.id)
    for (const task of lane.tasks) {
      if (!['queued', 'working', 'waiting'].includes(task.state)) continue
      if (task.kind === 'review') {
        this.#ledger.withdrawReview(project.id, task.number, { reason: 'ran out of quota' })
      } else if (task.pool !== null) {
        this.#ledger.releaseTask(project.id, task.number, {
          because: 'ran out of quota after starting',
        })
      }
    }
    this.#changed()
  }

  /** Work a member cannot go on with: a review is withdrawn for another reviewer, tiered work goes back open, the rest fails. */
  #giveUp(project, task, because) {
    if (task.kind === 'review') {
      this.#ledger.withdrawReview(project.id, task.number, { reason: because })
    } else if (task.pool !== null) {
      this.#ledger.releaseTask(project.id, task.number, { because })
    } else {
      this.#failTask(project, task, because)
    }
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
        quota: null,
        lowUntil: null,
        running: null,
        retiring: false,
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
