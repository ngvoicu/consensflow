import { randomUUID } from 'node:crypto'
import { deliveryText, markerOf } from './delivery-text.js'

/** The key that interrupts a harness's current turn, how often it is pressed again for a window still working, how soon, and the gap of a double press. */
const ESCAPE = 27
const INTERRUPT_ROUNDS = 3
const INTERRUPT_AGAIN_MS = 3_000
const DOUBLE_PRESS_MS = 150

/** How long a fresh window's output must hold still before its screen counts as drawn. */
const DRAWN_QUIET_MS = 1_500

/** How soon a lead that could not start is tried again; the wait doubles with each failure, up to the most. */
const RELAUNCH_MS = 5_000
const RELAUNCH_MAX_MS = 5 * 60_000

/**
 * Each window's lifecycle, for the dispatcher (`dispatcher.js`): a launch
 * opens a participant's window on its harness, with its first message (a
 * lead that could not start is tried again, ever more slowly); a look reads
 * what the harness's record shows; an agent whose task stopped is
 * interrupted; and a window closes once its work leaves it, at the human's
 * hand, or as the dispatcher's own exit.
 *
 * It keeps the window's part of a participant's record (`runtime.window`):
 * its adapter, pane and launch, the token and files it was given, what it
 * is doing, whether the human opened it, and how it closes.
 */
export class Windows {
  #ledger
  #host
  #adapters
  #credentials
  #paneEnv
  #roster
  #roles
  #launchFiles
  /** Fails, or gives back, a first message its window never took. */
  #deliveries
  /** Gives back the tiered work of a member whose saved agent is gone. */
  #scheduler
  /** A lead's first message, and what a lead whose saved agent is gone does instead of opening. */
  #leadFirst
  #leadWithoutAgent
  /** What a window's exit settles, as the dispatcher has it (`paneExited`). */
  #paneExited
  /** Tells the trace what happened at a window: here, a change of what it is doing. */
  #traceWindow
  /** Whether a participant's record was forgotten while its launch waited. */
  #forgotten
  #now
  #changed
  #generation = 0

  constructor({
    ledger,
    host,
    adapters,
    credentials,
    paneEnv,
    roster,
    roles,
    launchFiles,
    deliveries,
    scheduler,
    leadFirst,
    leadWithoutAgent,
    paneExited,
    traceWindow,
    forgotten,
    now,
    changed,
  }) {
    this.#ledger = ledger
    this.#host = host
    this.#adapters = adapters
    this.#credentials = credentials
    this.#paneEnv = paneEnv
    this.#roster = roster
    this.#roles = roles
    this.#launchFiles = launchFiles
    this.#deliveries = deliveries
    this.#scheduler = scheduler
    this.#leadFirst = leadFirst
    this.#leadWithoutAgent = leadWithoutAgent
    this.#paneExited = paneExited
    this.#traceWindow = traceWindow
    this.#forgotten = forgotten
    this.#now = now
    this.#changed = changed
  }

  /**
   * A window that exited: its token is revoked, its files in the home go
   * with it, and its part of the record holds no window.
   */
  closed(runtime) {
    this.#credentials.revoke(runtime.window.token)
    this.#launchFiles.forget(runtime.window.launchId)
    Object.assign(runtime.window, {
      pane: null,
      launch: null,
      launchId: null,
      token: null,
      retiring: false,
      activity: { state: 'closed' },
    })
  }

  /** Refuses a harness ConsensFlow has no adapter for: none of its windows could open. */
  requireAdapter(harness) {
    if (this.#adapters[harness] === undefined) {
      throw new Error(`ConsensFlow cannot open ${harness} windows`)
    }
  }

  async launch(runtime, project, participant, message) {
    const adapter = this.#adapters[participant.harness]
    const conversation = this.#ledger.currentConversation(participant.id)
    const resume = conversation?.nativeSession ?? null
    const first =
      participant.role === 'chief'
        ? this.#leadFirst(project, participant, conversation, message)
        : message
    const launchId = randomUUID()
    const generation = this.#nextGeneration()
    const delivering =
      first === null
        ? null
        : {
            messageId: first.id,
            marker: markerOf(first.id),
            since: this.#now(),
            launch: true,
            chief: participant.role === 'chief',
          }

    let plan
    try {
      if (first !== null) this.#ledger.beginDelivery(first.id)
      // A harness that lost its adapter fails what came for it, and says why.
      this.requireAdapter(participant.harness)
      // A session plays the role of the task it was started for; a member or
      // the chief its own.
      const { role } = participant
      // A member runs on its saved agent's model, read now; one the human
      // has deleted from their agents must not fall back to a harness default.
      const agent = participant.agent === null ? null : this.#roster(participant.agent)
      if (participant.agent !== null && agent === null) {
        if (participant.role === 'chief') this.#leadWithoutAgent(project, participant, delivering)
        else this.#scheduler.withoutAgent(project, participant, delivering)
        // A session's window the human opened, with nothing to deliver, says why it did not come.
        if (participant.role !== 'chief' && delivering === null) {
          this.#launchFailed(
            runtime,
            project,
            participant,
            null,
            `${participant.agent} is no longer among your agents`,
          )
        }
        return
      }
      plan = await adapter.prepare({
        launchId,
        participant,
        role,
        project,
        directory: project.directory,
        resume,
        message: first === null ? null : this.#launchText(project, participant, first, resume),
        agent,
        instructions: this.#roles(participant, project),
      })
    } catch (cause) {
      // An adapter may fail after writing the launch's files; no window will read them.
      this.#launchFiles.forget(launchId)
      this.#launchFailed(
        runtime,
        project,
        participant,
        delivering,
        `the launch failed: ${cause.message}`,
      )
      return
    }
    // A participant forgotten while its launch waits (it left, or its project
    // was deleted) gets nothing more under its ids, whose rows may be gone:
    // no window opens for it, and one already open goes with the record
    // (`#closeLeaving`).
    if (this.#forgotten(runtime)) {
      this.#launchFiles.forget(launchId)
      return
    }
    const token = this.#credentials.issue({ participant, project })
    const pane = { id: `p${project.id}-${participant.handle}`, generation }
    // The host watches a window's process before it answers the open, so an
    // exit can come first, even in the same read: `paneExited` finds it here.
    runtime.window.opening = { pane, exited: false }
    const opened = await this.#host
      .open({
        ...pane,
        cwd: project.directory,
        argv: plan.argv,
        env: { ...this.#paneEnv(participant, project), ...plan.env, CONSENSFLOW_TOKEN: token },
        dropEnv: plan.dropEnv,
      })
      .catch((cause) => ({ ok: false, error: cause.message }))
    const { exited } = runtime.window.opening
    runtime.window.opening = null
    if (opened?.ok !== true) {
      this.#credentials.revoke(token)
      this.#launchFiles.forget(launchId)
      this.#launchFailed(
        runtime,
        project,
        participant,
        delivering,
        `the window did not open: ${opened?.error ?? 'no answer from the pane host'}`,
      )
      return
    }
    // The window's own process, when the pane host knows it: an adapter may
    // find the harness's own status by it from its first look.
    if (opened.pid !== undefined) plan.launch.pid = opened.pid
    if (this.#forgotten(runtime)) {
      Object.assign(runtime.window, { pane, launchId, token })
      // One that exited before its open was answered has gone already.
      if (exited) await this.#paneExited(pane)
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
    Object.assign(runtime.window, {
      adapter,
      pane,
      launch: plan.launch,
      launchId,
      token,
      activity: { state: 'starting' },
      drawn: false,
      named: false,
    })
    runtime.delivery.delivering = delivering
    // A window that exited before its open was answered goes as any exit does.
    if (exited) {
      await this.#paneExited(pane)
      return
    }
    const started = await adapter
      .started({ launch: plan.launch, pane, host: this.#host })
      .catch((cause) => ({ error: cause.message }))
    if (started.error !== undefined && delivering !== null) {
      runtime.delivery.delivering = null
      // The lead's window goes without taking its project with it: the lead is tried again.
      if (participant.role === 'chief') await this.closeOwn(runtime, pane)
      else this.#host.kill(pane).catch(() => {})
      this.#launchFailed(
        runtime,
        project,
        participant,
        delivering,
        `the window could not take its first message: ${started.error}`,
      )
      return
    }
    if (this.#forgotten(runtime)) return
    runtime.window.relaunch = null
    if (started.nativeSession && started.nativeSession !== plan.nativeSession) {
      this.#ledger.bindConversation(conversationId, started.nativeSession)
    }
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

  /**
   * A launch that did not come up. A member's first message fails with it,
   * so its task fails and the requester hears why; a window the human opened
   * with nothing to deliver tells them why it did not come. The lead's first
   * message goes back to its queue with its attempt, and the lead is tried
   * again, ever more slowly while it keeps failing. The human hears why
   * once, until the lead starts or they ask for it again. A participant
   * forgotten while it launched hears nothing and settles nothing: its
   * project may be gone, and its rows with it.
   */
  #launchFailed(runtime, project, participant, delivering, reason) {
    if (this.#forgotten(runtime)) return
    if (participant.role !== 'chief') {
      if (delivering !== null) this.#deliveries.settleFailure(delivering, reason, { retry: false })
      else {
        this.#ledger.note(project.id, {
          to: 'human',
          body: `@${participant.handle} could not start: ${reason}.`,
        })
        this.#changed()
      }
      return
    }
    if (delivering !== null) this.#deliveries.giveBack(delivering, reason)
    const failures = (runtime.window.relaunch?.failures ?? 0) + 1
    runtime.window.relaunch = {
      failures,
      at: this.#now() + Math.min(RELAUNCH_MS * 2 ** (failures - 1), RELAUNCH_MAX_MS),
    }
    if (failures === 1) {
      this.#ledger.note(project.id, {
        to: 'human',
        body: `The lead could not start: ${reason}. What comes for the lead waits for it, and ConsensFlow tries again; you may also switch the lead.`,
      })
    }
    this.#changed()
  }

  /** One look at a window: what its harness's record shows now. */
  observe(participant, runtime) {
    return runtime.window.adapter.observe({
      launch: runtime.window.launch,
      pane: runtime.window.pane,
      conversation: this.#ledger.currentConversation(participant.id),
      host: this.#host,
    })
  }

  /**
   * Whether a window has drawn its screen: it printed, then held still for a
   * moment (the pane host says how long it has printed nothing). A host that
   * cannot tell is taken as drawn.
   */
  async drawn(runtime) {
    if (runtime.window.drawn) return true
    const snapshot = await this.#host
      .request('pane.snapshot', runtime.window.pane)
      .catch(() => null)
    const quiet = snapshot?.outputQuietMs
    if (quiet === undefined || (quiet !== null && quiet >= DRAWN_QUIET_MS)) {
      runtime.window.drawn = true
      return true
    }
    return false
  }

  setActivity(runtime, activity) {
    if (
      runtime.window.activity.state === activity.state &&
      runtime.window.activity.reason === activity.reason
    )
      return
    runtime.window.activity = activity
    this.#traceWindow(runtime, 'window.activity', {
      state: activity.state,
      reason: activity.reason ?? null,
    })
    this.#changed()
  }

  /**
   * A window whose task stopped stops too. A paused task's window stays
   * open, but its agent is interrupted. An agent still at work on a turn
   * about a task cancelled under its window is interrupted as well; the
   * window then closes, as any that holds no work, unless the human opened
   * it. A turn the human began since in a window they opened is theirs, and
   * goes on. Whatever the agent still writes is not collected, since the
   * task is not working.
   */
  async interruptIfStopped(participant, runtime, observed) {
    const paused = this.#ledger.pausedTask(participant.id)
    if (paused !== null) {
      // The chief's tell reached the window during this pause: the agent answers
      // it and ends its own turn, uninterrupted; the chief resumes the task.
      if (!this.#ledger.toldSincePaused(participant.id, paused.id)) {
        await this.#interrupt(runtime, paused.id)
      }
      return
    }
    if (runtime.window.activity.state !== 'working') return
    const cancelled = this.#ledger.lastTask(participant.id)
    if (cancelled?.state !== 'cancelled') return
    const turn = observed.items.findLast((item) => item.role === 'user')
    const about = cancelled.messages.some(
      (message) =>
        message.recipientId === participant.id && turn?.text.includes(markerOf(message.id)),
    )
    if (about) await this.#interrupt(runtime, cancelled.id)
  }

  /**
   * The Escape key interrupts the turn on `taskId` (twice in a row where the
   * harness asks for it), and again a few seconds later while the window
   * still reads as working, since a harness may ignore the key while it
   * thinks: three rounds at most.
   */
  async #interrupt(runtime, taskId) {
    const done = runtime.window.interrupted?.task === taskId ? runtime.window.interrupted : null
    if (
      done !== null &&
      (runtime.window.activity.state !== 'working' ||
        done.rounds >= INTERRUPT_ROUNDS ||
        this.#now() - done.at < INTERRUPT_AGAIN_MS)
    ) {
      return
    }
    runtime.window.interrupted = { task: taskId, rounds: (done?.rounds ?? 0) + 1, at: this.#now() }
    const presses = runtime.window.adapter.interrupt?.presses ?? 1
    for (let press = 0; press < presses; press += 1) {
      if (press > 0) await new Promise((resolve) => setTimeout(resolve, DOUBLE_PRESS_MS))
      await this.#host
        .request('pane.input', { ...runtime.window.pane, bytes: [ESCAPE] })
        .catch(() => {})
    }
  }

  /**
   * A session's window closes with its task; its conversation stays until the
   * session ends (the ledger ends both together), so a follow-up given with
   * `--after` comes back on the same conversation. A window already closing
   * had its kill.
   */
  async retire(runtime) {
    if (runtime.window.retiring) return
    runtime.window.retiring = true
    await this.#host.kill(runtime.window.pane).catch(() => {})
    this.#changed()
  }

  /**
   * A member's window closes once it holds no task, unless the human opened
   * it or a message is still on its way in. Says whether it closed; one gone
   * already during the step (a launch that timed out) has nothing to close.
   */
  async closeIfFree(runtime) {
    if (
      runtime.window.pane === null ||
      runtime.delivery.delivering !== null ||
      runtime.window.pinned ||
      this.#ledger.holdsWork(runtime.id)
    ) {
      return false
    }
    await this.retire(runtime)
    return true
  }

  /**
   * Closes a window whose exit is the dispatcher's own (a switch, a Close, a
   * lead that could not take its first message): the exit settles what the
   * window was doing, as any exit does, but a lead's does not close its
   * project. It is the dispatcher's whether its event came already or comes
   * later. A window already going with its work had its kill.
   */
  async closeOwn(runtime, pane) {
    runtime.window.ownExit = true
    try {
      if (!runtime.window.retiring) await this.#host.kill(pane).catch(() => {})
      if (
        runtime.window.pane?.id === pane.id &&
        runtime.window.pane.generation === pane.generation
      ) {
        await this.#paneExited(pane)
      }
    } finally {
      runtime.window.ownExit = false
    }
  }

  #nextGeneration() {
    this.#generation = Math.max(this.#generation + 1, this.#now())
    return this.#generation
  }
}
