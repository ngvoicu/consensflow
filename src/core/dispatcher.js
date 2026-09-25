import { randomUUID } from 'node:crypto'
import { RESUME_WORDS } from '../ledger/index.js'

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
 *   question is left alone. The chief finishes its own tasks explicitly,
 *   because its turns end while it waits for workers.
 * - One task per member session: a worker, advisor, reviewer or designer
 *   window opens with its task and closes once it holds no task (assigned,
 *   working or waiting); the session and its conversation stay until the
 *   human deletes it. A fresh window whose first message is not the brief (an
 *   answer, a follow-up) gets the brief in front of it. The chief keeps its
 *   window and conversation.
 * - A member window lost mid-task (a restart, a crash, a closed window)
 *   pauses the task, and the requester is told how to resume it. A chief
 *   window that closes suspends its project. A member who leaves
 *   the staff has its window closed once its step in progress ends, and that
 *   exit fails nothing: its open tasks were cancelled when it left.
 * - A task for a tier of member starts open: each pass gives it to a free
 *   member of that pool and tier that is not out of quota, the one with the
 *   fewest tasks so far, then the earliest joined; a task taken back from a
 *   member goes to another one first. When none is free the requester is
 *   told once. A review is such a task, for a reviewer. A member whose
 *   harness reports a fresh refusal (one after it was last marked out) is out
 *   until the reset it names (an hour when it names none): its tiered task
 *   goes back to open for another member and its window closes, a delivery
 *   in flight is queued again, the chief keeps its own tasks for after the
 *   reset, and nothing reaches it while out. A refusal still in the record
 *   after the reset is history, not a new one; the member is simply
 *   eligible again. A member low on quota takes nothing new. The human may
 *   also give a working or paused task back to the board (Reassign).
 * - A human typing in a window latches it against pastes. Their Enter releases
 *   the latch once the harness shows a new message of theirs; the pane host
 *   keeps it if they typed again after that Enter.
 *
 * Harness specifics live in the adapters (`src/adapters/`); the pane host is
 * the Rust PTY host behind the bridge. Time is an argument, so every rule is
 * testable with explicit passes (`tests/core-dispatcher.test.mjs`).
 */

/** The key that interrupts a harness's current turn, how often it is pressed again for a window still working, how soon, and the gap of a double press. */
const ESCAPE = 27
const INTERRUPT_ROUNDS = 3
const INTERRUPT_AGAIN_MS = 3_000
const DOUBLE_PRESS_MS = 150
const INLINE_LIMIT = 4000
const OPENING = 3000
const RECEIVED_ROLES = new Set(['user', 'custom', 'tool'])

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
  // A question says how to answer it; a result says what to do with it, so
  // the reader decides on the board even when its harness frames the message
  // as a request.
  const footer =
    message.kind === 'question'
      ? message.questions
        ? `\n\nAnswer with: cf answer m-${message.id} "…" (a label or your own words${message.questions.length > 1 ? '; one line per question' : ''})`
        : message.urgent && message.taskNumber != null
          ? `\n\nT-${message.taskNumber} is paused for this. Answer with: cf answer m-${message.id} "…"; the chief resumes the task.`
          : `\n\nAnswer with: cf answer m-${message.id} "…"`
      : message.kind === 'result' && message.taskNumber != null
        ? `\n\nDecide with: cf task accept T-${message.taskNumber} · cf task reopen T-${message.taskNumber} "…"`
        : ''
  return `[ConsensFlow m-${message.id}${task} · ${message.kind} from ${from}]\n${body}${footer}`
}

const markerOf = (messageId) => `[ConsensFlow m-${messageId} ·`

/** How many messages in a harness record the human typed (ConsensFlow's own carry its header). */
const humanMessages = (observed) =>
  observed.items.filter((item) => item.role === 'user' && !item.text.includes('[ConsensFlow m-'))
    .length

/** A reset this near holds a task with its window rather than sending it back to the board. */
const HOLD_MS = 30 * 60_000

export class Dispatcher {
  #ledger
  #host
  #adapters
  #clock
  #credentials
  #paneEnv
  #roster
  #launchFiles
  #roles
  #arrivalTimeoutMs
  #launchTimeoutMs
  #maxAttempts
  /** Told each change of a window's activity, for the event file in the home. */
  #trace
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
    trace = () => {},
    launchFiles = { forget: () => {} },
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
    this.#trace = trace
    this.#launchFiles = launchFiles
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

  /** A new project: the ledger records it with its staff, and its chief window opens. */
  async openProject({ directory, name, harness, staff = [], gate }) {
    const project = this.#ledger.createProject({
      directory,
      name,
      chief: { harness },
      staff,
      ...(gate === undefined ? {} : { gate }),
    })
    const chief = project.participants.find((participant) => participant.handle === 'chief')
    await this.#exclusive(chief.id, () => this.#launch(project, chief, null))
    return this.#ledger.project(project.id)
  }

  /** The human's Resume, and the restore after a restart: the chief comes back on its conversation. */
  async resumeProject(projectId) {
    const project = this.#ledger.setProjectState(projectId, 'open')
    const chief = project.participants.find((participant) => participant.role === 'chief')
    if (this.pane(chief.id) === null) {
      await this.#exclusive(chief.id, () => this.#launch(project, chief, null))
    }
    this.#changed()
    return this.#ledger.project(projectId)
  }

  /**
   * The human's Close: the project is suspended and every window of it goes.
   * Each window's exit settles what it was doing, the way any closed window
   * does; Resume brings the chief back on its conversation.
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

  // --- the human's hand on a session's window ------------------------------------------

  /**
   * The human opens a session's window with nothing to deliver: it comes back
   * on its own conversation, with its history, and stays open until the
   * human closes it, whatever work comes and goes meanwhile.
   */
  async openWindow(projectId, handle) {
    const { project, participant } = this.#sessionOf(projectId, handle)
    const runtime = this.#runtimeOf(participant.id)
    runtime.pinned = true
    if (runtime.pane === null) {
      await this.#exclusive(participant.id, () => this.#launch(project, participant, null))
    }
    this.#changed()
    return this.#ledger.project(projectId)
  }

  /** The human closes a session's window; work in it pauses, as any lost window's does. */
  async closeWindow(projectId, handle) {
    const { participant } = this.#sessionOf(projectId, handle)
    const runtime = this.#runtimeOf(participant.id)
    runtime.pinned = false
    if (runtime.pane !== null) await this.#retire(participant, runtime)
    return this.#ledger.project(projectId)
  }

  /** The human ends a session for good: the ledger folds it, and its window goes. */
  async endSession(projectId, handle) {
    const { participant } = this.#sessionOf(projectId, handle)
    const project = this.#ledger.endSession(projectId, handle, { by: 'human' })
    const runtime = this.#runtimeOf(participant.id)
    runtime.pinned = false
    if (runtime.pane !== null) await this.#retire(participant, runtime)
    this.#changed()
    return project
  }

  #sessionOf(projectId, handle) {
    const project = this.#ledger.project(projectId)
    const participant = project?.participants.find((p) => p.handle === handle && p.member !== null)
    if (participant === undefined) throw new Error(`no session @${handle} in project ${projectId}`)
    return { project, participant }
  }

  /** A closed project goes for good; the ledger refuses an open one. Its windows are already gone. */
  async deleteProject(projectId) {
    const project = this.#ledger.project(projectId)
    const deleted = this.#ledger.deleteProject(projectId)
    for (const participant of project?.participants ?? []) {
      const runtime = this.#runtime.get(participant.id)
      if (runtime?.token) this.#credentials.revoke(runtime.token)
      this.#launchFiles.forget(runtime?.launchId)
      this.#runtime.delete(participant.id)
    }
    // A deleted project leaves no trace but the line that says it was:
    // its own lines go, and the record of it names no project id, so a
    // later project with the same id never takes it along.
    this.#trace.forget?.(projectId)
    this.#trace({
      at: new Date(this.#now()).toISOString(),
      kind: 'project.deleted',
      project: null,
      data: deleted,
    })
    this.#changed()
    return deleted
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
   * The human takes a member off the staff. It waits for the member's step in
   * progress, so a window that is still opening is closed too, not left behind.
   */
  async removeMember(projectId, handle) {
    const member = this.#ledger
      .project(projectId)
      ?.participants.find((participant) => participant.handle === handle)
    // Not in the staff: the ledger refuses it and says why.
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
    this.#resumeHeld()
    for (const project of this.#ledger.projects()) {
      if (project.state !== 'open') continue
      this.#assignOpenTasks(project)
    }
    const steps = []
    for (const project of this.#ledger.projects()) {
      for (const participant of project.participants) {
        if (participant.role === 'human') continue
        steps.push(this.#exclusive(participant.id, () => this.#step(project, participant)))
      }
    }
    await Promise.all(steps)
    await this.#closeLeftWindows()
  }

  /** A window whose participant has left (a session ended, a member removed) closes. */
  async #closeLeftWindows() {
    const live = new Set(
      this.#ledger.projects().flatMap((project) => project.participants.map((p) => p.id)),
    )
    for (const [participantId, runtime] of this.#runtime) {
      if (runtime.pane === null || runtime.retiring || live.has(participantId)) continue
      runtime.retiring = true
      await this.#host.kill(runtime.pane).catch(() => {})
      this.#changed()
    }
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
    // The window's files in the home go with it.
    this.#launchFiles.forget(runtime.launchId)
    Object.assign(runtime, {
      pane: null,
      launch: null,
      launchId: null,
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
    if (participant.role === 'chief') {
      if (project.state === 'open') this.#ledger.setProjectState(project.id, 'suspended')
    } else {
      const task = this.#ledger.activeTask(participantId)
      if (task !== null) this.#stall(project, task, `@${participant.handle}'s window closed`)
    }
    this.#changed()
  }

  /**
   * A task whose window went away mid-work (a restart, a crash, a window the
   * human closed) is paused, not given up: its session and conversation stay,
   * and the chief resumes it into the same window with its memory.
   */
  #stall(project, task, because) {
    this.#ledger.pauseTask(project.id, task.number, { because })
    this.#ledger.note(project.id, {
      to: task.requester,
      task: task.number,
      body: `T-${task.number} is paused: ${because}. Resume it with: cf task resume T-${task.number} "…"; its window comes back on its own conversation.`,
    })
  }

  // --- one participant's step ----------------------------------------------------

  async #step(project, participant) {
    const runtime = this.#runtimeOf(participant.id)
    if (runtime.pane !== null) return this.#stepOpen(project, participant, runtime)
    if (project.state !== 'open') return
    const next = this.#ledger.nextDelivery(participant.id)
    if (next !== null) return this.#launch(project, participant, next)
    if (participant.role === 'chief') return
    // A member whose agent is gone must not wait for a window that will not
    // open: its held work goes back to the board now.
    if (participant.agent !== null && this.#roster(participant.agent) === null)
      return this.#withoutAgent(project, participant, null)
    // A member's session is its task's: with the window gone (a restart, a
    // crash) and nothing due to it, nobody is doing the work any more.
    const task = this.#ledger.activeTask(participant.id)
    if (task !== null) {
      this.#stall(project, task, `@${participant.handle}'s window is gone`)
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
    // Quota belongs to the member: a session that runs out takes its member out.
    const owner = this.#memberOf(project, participant)
    // An out member's window says so, and nothing else, until the reset.
    if (!this.#isOut(owner)) {
      this.#setActivity(
        runtime,
        observed.waiting
          ? { state: 'waiting', reason: observed.waiting.reason ?? null }
          : { state: observed.settled ? 'idle' : 'working' },
      )
    }
    this.#copyTranscript(participant, runtime, observed)
    if (observed.quota !== undefined) {
      runtime.quota = observed.quota ?? null
      // Low is soft: the current task continues, nothing new comes until the
      // reset it names (an hour when it names none). It outlives the window,
      // which closes with the task, so a low member is not asked again at once.
      if (runtime.quota?.state === 'low') {
        this.#runtimeOf(owner.id).lowUntil =
          runtime.quota.resetsAt ?? new Date(this.#now() + 3_600_000).toISOString()
      } else if (runtime.quota !== null) this.#runtimeOf(owner.id).lowUntil = null
    }
    const out = this.#isOut(owner)
    if (!out && this.#freshRefusal(owner, runtime.quota)) {
      await this.#outOfQuota(project, participant, runtime, owner)
      return
    }
    if (out) {
      this.#setActivity(runtime, {
        state: 'out',
        reason: `out of quota until ${owner.outUntil}`,
      })
      // A held task's agent stops, as any paused task's; the window waits for the reset.
      if (participant.role !== 'chief') await this.#interruptIfPaused(participant, runtime)
      return
    }
    if (runtime.delivering !== null) this.#watchArrival(runtime, observed)
    await this.#releaseDraft(runtime, observed)
    if (participant.role !== 'chief') {
      await this.#interruptIfPaused(participant, runtime)
      this.#collect(project, participant, observed)
      // The window may have gone during this step (a launch that timed out).
      if (
        runtime.pane !== null &&
        runtime.delivering === null &&
        !runtime.pinned &&
        !this.#ledger.holdsWork(participant.id)
      ) {
        await this.#retire(participant, runtime)
        return
      }
    }
    if (runtime.delivering === null && observed.settled && !observed.waiting) {
      const next = this.#ledger.nextDelivery(participant.id)
      if (next !== null) await this.#deliver(runtime, next)
    }
  }

  /**
   * ConsensFlow's own copy of the window's conversation, kept in the home:
   * what the agent was told, wrote and got back from its tools, readable on
   * the card once the window is gone. Each look copies what is new and the
   * item still being written; a record that shrank (a resumed window rewrote
   * it) is copied over from the start.
   */
  #copyTranscript(participant, runtime, observed) {
    const conversation = this.#ledger.currentConversation(participant.id)
    if (conversation === null) return
    const items = observed.items
    const copied = runtime.copied?.conversation === conversation.id ? runtime.copied.count : 0
    const from = items.length < copied ? 0 : Math.max(0, copied - 1)
    if (items.length > from) {
      this.#ledger.copyTranscript(conversation.id, items.slice(from), { from })
    }
    runtime.copied = { conversation: conversation.id, count: items.length }
  }

  /**
   * A paused task's window stays open, but the agent stops: the Escape key
   * interrupts the turn (twice in a row where the harness asks for it), and
   * again a few seconds later while the window still reads as working, since
   * a harness may ignore the key while it thinks. Whatever the agent still
   * writes is not collected, since the task is not working.
   */
  async #interruptIfPaused(participant, runtime) {
    const paused = this.#ledger.pausedTask(participant.id)
    if (paused === null) return
    const done = runtime.interrupted?.task === paused.id ? runtime.interrupted : null
    if (
      done !== null &&
      (runtime.activity.state !== 'working' ||
        done.rounds >= INTERRUPT_ROUNDS ||
        this.#now() - done.at < INTERRUPT_AGAIN_MS)
    ) {
      return
    }
    runtime.interrupted = { task: paused.id, rounds: (done?.rounds ?? 0) + 1, at: this.#now() }
    const presses = runtime.adapter.interrupt?.presses ?? 1
    for (let press = 0; press < presses; press += 1) {
      if (press > 0) await new Promise((resolve) => setTimeout(resolve, DOUBLE_PRESS_MS))
      await this.#host.request('pane.input', { ...runtime.pane, bytes: [ESCAPE] }).catch(() => {})
    }
  }

  /**
   * One task per member session: the window and its conversation end with the
   * work, so the next task starts a fresh session with nothing carried over.
   */
  /**
   * A session's window closes with its task; its conversation stays until the
   * session ends (the ledger ends both together), so a follow-up given with
   * `--after` comes back on the same conversation.
   */
  async #retire(_participant, runtime) {
    runtime.retiring = true
    await this.#host.kill(runtime.pane).catch(() => {})
    this.#changed()
  }

  /**
   * A member's fresh session starts from nothing: when its first message is
   * not the task's own brief (a reopening, an answer), the brief
   * goes in first. A resumed conversation has it already.
   */
  #launchText(project, participant, message, resume) {
    const text = deliveryText(message)
    if (resume !== null || participant.role === 'chief' || message.taskNumber == null) {
      return text
    }
    const task = this.#ledger.task(project.id, message.taskNumber)
    const brief = task?.messages.find(
      (m) => m.kind === 'task' && m.recipient === participant.handle && m.state !== 'cancelled',
    )
    if (brief === undefined || brief.id === message.id) return text
    return `${deliveryText(brief)}\n\n${text}`
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
    this.#ledger.recordResult(project.id, task.number, { body })
    this.#changed()
  }

  async #deliver(runtime, message) {
    if (runtime.adapter.ready !== undefined) {
      const ready = await runtime.adapter.ready({
        launch: runtime.launch,
        pane: runtime.pane,
        host: this.#host,
      })
      if (ready !== true) {
        // Said once per message, so a wait is in the trace, not a mystery.
        if (runtime.held !== message.id) {
          runtime.held = message.id
          const project = this.#projectOf(runtime.id)
          this.#trace({
            at: new Date(this.#now()).toISOString(),
            kind: 'delivery.held',
            project: project?.id ?? null,
            participant: project?.participants.find((p) => p.id === runtime.id)?.handle ?? null,
            message: message.id,
            reason: `the window is not ready for a paste: ${typeof ready === 'string' ? ready : 'someone is typing there, or a paste is on its way'}`,
          })
        }
        return
      }
    }
    runtime.held = null
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
      // A session plays the role of the task it was started for; a member or
      // the chief its own.
      const { role } = participant
      // A member runs on its saved agent's model, read now; one the human
      // has deleted from their agents must not fall back to a harness default.
      const agent = participant.agent === null ? null : this.#roster(participant.agent)
      if (participant.agent !== null && agent === null) {
        await this.#withoutAgent(project, participant, delivering)
        return
      }
      plan = await adapter.prepare({
        launchId,
        participant,
        role,
        project,
        directory: project.directory,
        resume,
        message: message === null ? null : this.#launchText(project, participant, message, resume),
        agent,
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
      launchId,
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

  // --- assignment ---------------------------------------------------------------------

  /**
   * Each open task goes to the best free member of its tier; the requester
   * hears once when none is. A task that needs others waits for them to be
   * accepted, in silence: its card says what it waits for.
   */
  #assignOpenTasks(project) {
    for (const task of this.#ledger.board(project.id).open) {
      if (task.blockedBy.length > 0 || task.assignee !== null) continue
      const candidates = this.#ledger.candidates(project.id, task.number)
      const free = candidates.filter((member) => this.#available(member))
      if (free.length > 0) {
        this.#ledger.assignTask(project.id, task.number, this.#rank(free)[0].id)
        this.#waitingNoted.delete(task.id)
        this.#changed()
      } else if (!this.#waitingNoted.has(task.id)) {
        this.#waitingNoted.add(task.id)
        this.#ledger.note(project.id, {
          to: task.requester,
          task: task.number,
          body: `T-${task.number} waits for a free ${task.pool === 'designer' ? 'image designer' : `${task.tier} ${task.pool}`}: ${this.#whyNotFree(candidates)}.`,
        })
        this.#changed()
      }
    }
  }

  /** The member a session belongs to; a member or the chief is its own. */
  #memberOf(project, participant) {
    if (participant.memberId === null) return participant
    return project.participants.find((p) => p.id === participant.memberId) ?? participant
  }

  /** Free: nothing on its hands, not out of quota, not low on it. */
  #available(member) {
    return this.#roster(member.agent) !== null && !this.#isOut(member) && !this.#isLow(member)
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
    // A refusal whose reset has passed is old news, whatever record still shows it.
    if (quota.resetsAt && Date.parse(quota.resetsAt) <= this.#now()) return false
    if (participant.outSince === null || !quota.at) return true
    return Date.parse(quota.at) > Date.parse(participant.outSince)
  }

  /**
   * A member the task was taken back from last, then the fewest tasks taken,
   * then the earliest joined: a reassigned task goes to another member when
   * one is free, and back to the same one only when it is the only one.
   */
  #rank(members) {
    return [...members].sort(
      (a, b) =>
        Number(a.hadIt === true) - Number(b.hadIt === true) || a.taken - b.taken || a.id - b.id,
    )
  }

  #whyNotFree(candidates) {
    const gone = candidates.filter((m) => this.#roster(m.agent) === null)
    const out = candidates.filter(
      (m) => !gone.includes(m) && m.outUntil !== null && Date.parse(m.outUntil) > this.#now(),
    )
    const low = candidates.filter((m) => !gone.includes(m) && !out.includes(m) && this.#isLow(m))
    const parts = []
    for (const member of gone)
      parts.push(
        `@${member.handle} has no agent any more (${member.agent} is not among your agents: define it, or remove the member)`,
      )
    for (const member of out)
      parts.push(`@${member.handle} is out of quota until ${member.outUntil}`)
    for (const member of low) parts.push(`@${member.handle} is low on quota`)
    return parts.join('; ')
  }

  /**
   * A member whose agent is gone from the human's agents (a release dropped
   * the catalog entry, or the human removed one of their own) runs on no
   * default: its tiered work goes back to the board for another member, with
   * whatever was on its way to it withdrawn, and a request given to it by
   * name fails so the requester hears why. The board says why it sits.
   */
  async #withoutAgent(project, participant, delivering) {
    const because = `${participant.agent} is no longer among your agents`
    const lane = this.#ledger
      .board(project.id)
      .lanes.find((l) => l.participant.id === participant.id)
    let released = 0
    for (const task of lane?.tasks ?? []) {
      if (!['queued', 'working', 'waiting'].includes(task.state) || task.pool === null) continue
      this.#ledger.releaseTask(project.id, task.number, { because })
      released += 1
    }
    if (delivering !== null)
      this.#settleFailure(
        delivering,
        `${because}: add it back under Agents, or remove @${participant.handle} from the staff`,
        { retry: false },
      )
    else if (released > 0) this.#changed()
  }

  /**
   * A member whose harness just refused it: out until the reset it names (an
   * hour when it names none). What it was receiving is queued again, its
   * tiered work goes back to the board; its own tasks (the chief's) wait for it.
   * The session's window then closes, as any window whose work left it: a
   * harness that waits out its limit (OpenCode) would otherwise take the task
   * up again at the reset, beside whoever has it now.
   */
  async #outOfQuota(project, participant, runtime, owner) {
    const until = runtime.quota.resetsAt ?? new Date(this.#now() + 3_600_000).toISOString()
    this.#setActivity(runtime, { state: 'out', reason: `out of quota until ${until}` })
    this.#ledger.markOut(owner.id, { until, reason: 'out of quota' })
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
      if (task.pool === null) continue
      // Near the reset, or with nobody else to take it, the task keeps its
      // window and goes on by itself; otherwise it goes back to the board.
      const soon = Date.parse(until) - this.#now() <= HOLD_MS
      const teammate = this.#ledger
        .candidates(project.id, task.number)
        .some((member) => member.id !== owner.id && this.#available(member))
      if (soon || !teammate) {
        this.#ledger.holdTask(project.id, task.number, { until, because: 'out of quota' })
        this.#ledger.note(project.id, {
          to: task.requester,
          task: task.number,
          body: `T-${task.number} waits with @${participant.handle}: out of quota until ${until}; it goes on by itself then.`,
        })
      } else {
        this.#ledger.releaseTask(project.id, task.number, {
          because: 'ran out of quota after starting',
        })
      }
    }
    if (
      participant.role !== 'chief' &&
      !runtime.pinned &&
      !this.#ledger.holdsWork(participant.id)
    ) {
      await this.#retire(participant, runtime)
    }
    this.#changed()
  }

  /** A held task whose time has come goes on in its own window, unless its member is still out. */
  #resumeHeld() {
    for (const held of this.#ledger.heldTasksDue(new Date(this.#now()).toISOString())) {
      const project = this.#ledger.project(held.projectId)
      if (project === null || project.state !== 'open') continue
      const assignee = project.participants.find((p) => p.id === held.assigneeId)
      const owner = assignee === undefined ? null : this.#memberOf(project, assignee)
      if (owner !== null && this.#isOut(owner)) continue
      this.#ledger.resumeTask(held.projectId, held.number, { body: RESUME_WORDS })
      this.#changed()
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
        id: participantId,
        adapter: null,
        pane: null,
        launch: null,
        token: null,
        delivering: null,
        held: null,
        quota: null,
        lowUntil: null,
        running: null,
        retiring: false,
        enters: [],
        humanItems: null,
        copied: null,
        interrupted: null,
        pinned: false,
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
    const project = this.#projectOf(runtime.id)
    this.#trace({
      at: new Date(this.#now()).toISOString(),
      kind: 'window.activity',
      project: project?.id ?? null,
      participant: project?.participants.find((p) => p.id === runtime.id)?.handle ?? null,
      state: activity.state,
      reason: activity.reason ?? null,
    })
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
