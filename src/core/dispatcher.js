import { randomUUID } from 'node:crypto'
import { RESUME_WORDS } from '../ledger/index.js'
import { deliveryText, markerOf } from './delivery-text.js'
import { HANDOFF_TITLE, handoffText, historyPages, lastWords } from './handoff.js'

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
 * - A launch or a delivery goes on apart from the pass, holding only its own
 *   participant, so a harness that takes long holds up no other window; the
 *   human's operations answer once the ledger has their change, and the
 *   window they open follows.
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
 *   window that closes by itself closes its project, as the human's Close
 *   does: every window of it goes. A participant that leaves (a member off
 *   the staff with its sessions, a session the human ends, every one of a
 *   deleted project) is forgotten at once, quota marks and all, so one that
 *   comes back or takes its id starts clean; its window closes once its step
 *   in progress ends, and that exit fails nothing: a member's open tasks
 *   were cancelled when it left. An Open or a Switch lead that waited on it
 *   then does nothing, to it or to one that took its id.
 * - A task for a tier of member starts open: each pass gives it to a free
 *   member of that pool and tier that is not out of quota, on the harness
 *   whose members of the tier have taken the fewest tasks, then the one with
 *   the fewest tasks so far, then the earliest joined; a task taken back from
 *   a member goes to another one first. When none is free the requester is
 *   told once. A review is such a task, for a reviewer. A member whose
 *   harness reports a fresh refusal (one after it was last marked out) is out
 *   until the reset it names (an hour when it names none): its tiered task
 *   goes back to open for another member and its window closes, a delivery
 *   in flight is queued again, the chief keeps its own tasks for after the
 *   reset, and nothing reaches it while out. A refusal still in the record
 *   after the reset is history, not a new one; the member is simply
 *   eligible again. A member low on quota takes nothing new. The human may
 *   also give a working or paused task back to the board (Reassign).
 * - What a human typed in a window and left unsent holds nothing (the
 *   owner's choice, 2026-10-01): a paste goes in behind it. The pane host
 *   holds only their keys pressed during a paste, until it is in.
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

/** How long a fresh window's output must hold still before its screen counts as drawn. */
const DRAWN_QUIET_MS = 1_500

/** A closed project starts and changes no work, whatever asks: it is resumed first. */
export function requireOpen(project) {
  if (project.state !== 'open') throw new Error(`${project.name} is closed: resume it first`)
}

/** A note from ConsensFlow that hands the lead to a new window (`handoff.js`). */
const isHandoff = (message) =>
  message.kind === 'note' && message.sender === null && message.body.startsWith(HANDOFF_TITLE)

/** A reset this near holds a task with its window rather than sending it back to the board. */
const HOLD_MS = 30 * 60_000

/** How long a quota lasts when its harness names no reset. */
const UNKNOWN_RESET_MS = 60 * 60_000

/** How soon a lead that could not start is tried again; the wait doubles with each failure, up to the most. */
const RELAUNCH_MS = 5_000
const RELAUNCH_MAX_MS = 5 * 60_000

/**
 * Starts `work` inside an async function, so one that throws before its
 * first await rejects as one that throws after it does: whoever holds a
 * participant for it lets go either way.
 */
const begin = async (work) => work()

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
  /** Told each change of a window's activity, a delivery a window is not ready for and a deleted project, for the event file in the home. */
  #trace
  /** The daemon's log, for a launch or a delivery that failed apart from any pass. */
  #log
  #runtime = new Map()
  /** The records of participants forgotten while their window was still open, until it exits. */
  #leaving = new Set()
  #listeners = new Set()
  /** The open tasks whose requester heard that they wait for a free member, each with its project. */
  #waitingNoted = new Map()
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
    log = null,
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
    this.#log = log
    this.#launchFiles = launchFiles
    this.#launchTimeoutMs = launchTimeoutMs
    this.#maxAttempts = maxAttempts
    host.onExit((pane) => this.paneExited(pane))
  }

  onChange(listener) {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  /** What a participant's window is doing: starting, working, idle, waiting (with why), closed. */
  activity(participantId) {
    return this.#runtime.get(participantId)?.activity ?? { state: 'closed' }
  }

  /** The Switch lead waiting for this lead's turn to end, `{harness, agent}`, or null. */
  pendingSwitch(participantId) {
    const pending = this.#runtime.get(participantId)?.pendingSwitch ?? null
    return pending === null ? null : { harness: pending.harness, agent: pending.agent }
  }

  /** The live window of a participant, `{id, generation}`, or null. */
  pane(participantId) {
    return this.#runtime.get(participantId)?.pane ?? null
  }

  /** Refuses a harness ConsensFlow has no adapter for: none of its windows could open. */
  requireAdapter(harness) {
    if (this.#adapters[harness] === undefined) {
      throw new Error(`ConsensFlow cannot open ${harness} windows`)
    }
  }

  /**
   * A new project: the ledger records it with its staff, and its chief
   * window opens after the answer (see `#openSoon`).
   */
  async openProject({ directory, name, harness, staff = [], gate }) {
    for (const runs of [harness, ...staff.map((member) => member.harness)]) {
      this.requireAdapter(runs)
    }
    const project = this.#ledger.createProject({
      directory,
      name,
      chief: { harness },
      staff,
      ...(gate === undefined ? {} : { gate }),
    })
    const chief = project.participants.find((participant) => participant.handle === 'chief')
    this.#openSoon(chief.id)
    return this.#ledger.project(project.id)
  }

  /** The human's Resume, and the restore after a restart: the chief comes back on its conversation. */
  async resumeProject(projectId) {
    const project = this.#ledger.setProjectState(projectId, 'open')
    const chief = project.participants.find((participant) => participant.role === 'chief')
    // The human asks for the lead now: a lead that failed before is tried at once.
    this.#runtimeOf(chief.id).relaunch = null
    this.#openSoon(chief.id)
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
    await this.#closeWindows(project.participants)
    this.#changed()
    return this.#ledger.project(projectId)
  }

  /**
   * Closes these participants' windows, each once its step in progress is
   * over, so a window still opening is closed too. Each takes its place at
   * once, so a Resume that follows opens the lead after its window went;
   * and each exit is the dispatcher's own, so a lead's never closes a
   * project resumed meanwhile. What closes is the window of the record
   * waited on: one forgotten meanwhile has left its id to whoever the
   * ledger gives it next.
   */
  async #closeWindows(participants) {
    await Promise.all(
      participants.map((participant) => {
        const runtime = this.#runtimeOf(participant.id)
        return this.#exclusive(
          participant.id,
          async () => {
            if (runtime.pane !== null) await this.#closeOwn(runtime, runtime.pane)
          },
          { wait: true },
        )
      }),
    )
  }

  /**
   * Participants that left (a session ended, a member removed, a project
   * deleted) are forgotten at once, quota marks and all: nothing of them
   * stays for one that comes back, or that the ledger gives one of their
   * ids. A window one still has closes once its step in progress is over.
   */
  async #forget(participantIds) {
    const leaving = []
    for (const id of participantIds) {
      const runtime = this.#runtime.get(id)
      if (runtime === undefined) continue
      this.#runtime.delete(id)
      this.#leaving.add(runtime)
      leaving.push(runtime)
    }
    await Promise.all(leaving.map((runtime) => this.#closeLeaving(runtime)))
  }

  /** A forgotten participant's window closes, and its exit is known by the window alone (`paneExited`). */
  async #closeLeaving(runtime) {
    while (runtime.running !== null || runtime.acting !== null) {
      await (runtime.running ?? runtime.acting).catch(() => {})
    }
    if (runtime.pane === null) this.#leaving.delete(runtime)
    else await this.#retire(runtime)
  }

  // --- the human's hand on a session's window ------------------------------------------

  /**
   * The human opens a session's window with nothing to deliver: it comes back
   * on its own conversation, with its history, and stays open until the
   * human closes it, whatever work comes and goes meanwhile.
   */
  async openWindow(projectId, handle) {
    const { project, participant } = this.#sessionOf(projectId, handle)
    requireOpen(project)
    this.#runtimeOf(participant.id).pinned = true
    this.#openSoon(participant.id)
    this.#changed()
    return this.#ledger.project(projectId)
  }

  /** The human closes a session's window; work in it pauses, as any lost window's does. */
  async closeWindow(projectId, handle) {
    const { participant } = this.#sessionOf(projectId, handle)
    const runtime = this.#runtimeOf(participant.id)
    runtime.pinned = false
    if (runtime.pane !== null) await this.#retire(runtime)
    return this.#ledger.project(projectId)
  }

  /** The human ends a session for good: the ledger folds it, and it is forgotten with its window. */
  async endSession(projectId, handle) {
    const { participant } = this.#sessionOf(projectId, handle)
    const project = this.#ledger.endSession(projectId, handle, { by: 'human' })
    await this.#forget([participant.id])
    this.#changed()
    return project
  }

  #sessionOf(projectId, handle) {
    const project = this.#ledger.project(projectId)
    const participant = project?.participants.find((p) => p.handle === handle && p.member !== null)
    if (participant === undefined) throw new Error(`no session @${handle} in project ${projectId}`)
    return { project, participant }
  }

  /** A project the ledger has; one it does not is refused in the ledger's words. */
  #knownProject(projectId) {
    const project = this.#ledger.project(projectId)
    if (project === null) throw new Error(`no project ${projectId}`)
    return project
  }

  /**
   * A closed project goes for good; the ledger refuses an open one. Its
   * participants are forgotten, and a window of it whose exit has not come
   * yet is closed, so nothing of the project keeps running.
   */
  async deleteProject(projectId) {
    const project = this.#ledger.project(projectId)
    const deleted = this.#ledger.deleteProject(projectId)
    // The ledger gives the next rows it writes the ids this project's had:
    // what is remembered of its tasks and participants goes now, before
    // anything can take one of their ids, and so do its own lines in the
    // trace (a project created while its windows close keeps its own).
    for (const [task, noted] of this.#waitingNoted) {
      if (noted === deleted.id) this.#waitingNoted.delete(task)
    }
    this.#trace.forget?.(projectId)
    await this.#forget((project?.participants ?? []).map((participant) => participant.id))
    // A deleted project leaves no trace but the line that says it was, and
    // that names no project id, so a later project with the same id never
    // takes it along.
    this.#trace({
      at: new Date(this.#now()).toISOString(),
      kind: 'project.deleted',
      project: null,
      data: deleted,
    })
    this.#changed()
    return deleted
  }

  /**
   * Once, at start: what was on its way to a window is settled, and the
   * projects that were open when the previous process ended come back.
   */
  async resumeAfterRestart() {
    this.#settleInFlight()
    const outcomes = []
    const due = this.#ledger.projects().filter((project) => project.resumeOnStart)
    for (const project of due) {
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
   * What was on its way to a window when the previous process ended: no
   * window survives a restart, so none will show it now. A message whose
   * header ConsensFlow's copy of the window shows had arrived; any other
   * goes back to its queue with its attempt, for the window that comes back.
   */
  #settleInFlight() {
    for (const message of this.#ledger.inFlight()) {
      const item = this.#ledger.copiedItemWith(message.recipientId, markerOf(message.id))
      if (item === null) {
        this.#ledger.retryDelivery(message.id, 'the daemon stopped before it arrived', {
          refund: true,
        })
      } else this.#ledger.confirmDelivery(message.id, { item })
    }
  }

  /**
   * The human takes a member off the staff once its step in progress is
   * over. Its sessions leave with it; all are forgotten, and a window still
   * opening is closed too, not left behind.
   */
  async removeMember(projectId, handle) {
    const member = this.#ledger
      .project(projectId)
      ?.participants.find((participant) => participant.handle === handle)
    // Not in the staff: the ledger refuses it and says why.
    if (member === undefined) return this.#ledger.removeMember(projectId, handle)
    const runtime = this.#runtimeOf(member.id)
    const { removed, left } = await this.#exclusive(
      member.id,
      () => {
        // One forgotten meanwhile has left already, by another removal or with
        // its project, whose ids may be another's by now: refused as the
        // ledger refuses a member that left.
        if (this.#forgotten(runtime)) throw new Error(`@${handle} left the staff`)
        const sessions = this.#ledger
          .project(projectId)
          .participants.filter((participant) => participant.memberId === member.id)
        return {
          removed: this.#ledger.removeMember(projectId, handle),
          left: [member, ...sessions].map((participant) => participant.id),
        }
      },
      { wait: true },
    )
    await this.#forget(left)
    this.#changed()
    return removed
  }

  /**
   * One pass over every participant: each looks at its window, and a launch
   * or a delivery it starts goes on apart from the pass (`#act`), so a slow
   * one holds up no one else. The pass reads the projects once; one whose
   * open tasks it gives out is read again, so the sessions it started for
   * them open their windows in this pass.
   */
  async pass() {
    this.#resumeHeld()
    const projects = this.#ledger
      .projects()
      .map((project) =>
        project.state === 'open' && this.#assignOpenTasks(project)
          ? this.#ledger.project(project.id)
          : project,
      )
    const steps = []
    for (const project of projects) {
      for (const participant of project.participants) {
        if (participant.role === 'human') continue
        steps.push(this.#exclusive(participant.id, () => this.#step(project, participant)))
      }
    }
    await Promise.all(steps)
  }

  /** A window ended: `pane.exit` from the pane host. */
  async paneExited({ id, generation }) {
    const ended = (pane) => pane?.id === id && pane.generation === generation
    const runtime = [...this.#runtime.values(), ...this.#leaving].find(
      (candidate) => ended(candidate.pane) || ended(candidate.opening?.pane),
    )
    if (runtime === undefined) return
    // Still opening: its launch takes the exit once it has the window.
    if (!ended(runtime.pane)) {
      runtime.opening.exited = true
      return
    }
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
    // A participant that left is forgotten already: its window's exit settles nothing more.
    if (this.#leaving.delete(runtime)) return
    const participantId = runtime.id
    const project = this.#projectOf(participantId)
    if (project === null) return
    const participant = project.participants.find((p) => p.id === participantId)
    if (delivering !== null) {
      const because = `@${participant.handle}'s window closed`
      // A lead's first message (a handoff, most often) waits for its next window.
      if (delivering.chief) this.#giveBack(delivering, because)
      else this.#settleFailure(delivering, because, { retry: !delivering.launch })
    }
    if (participant.role === 'chief') {
      // The lead's own exit (the human's /exit, a crash) closes the project
      // as Close does: no member's window goes on unseen.
      if (project.state === 'open' && !runtime.ownExit) {
        this.#ledger.setProjectState(project.id, 'suspended')
        await this.#closeWindows(project.participants.filter((p) => p.id !== participantId))
      }
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

  /** One look at a window: what its harness's record shows now. */
  #observe(participant, runtime) {
    return runtime.adapter.observe({
      launch: runtime.launch,
      pane: runtime.pane,
      conversation: this.#ledger.currentConversation(participant.id),
      host: this.#host,
    })
  }

  /**
   * Whether a window has drawn its screen: it printed, then held still for a
   * moment (the pane host says how long it has printed nothing). A host that
   * cannot tell is taken as drawn.
   */
  async #drawn(runtime) {
    if (runtime.drawn) return true
    const snapshot = await this.#host.request('pane.snapshot', runtime.pane).catch(() => null)
    const quiet = snapshot?.outputQuietMs
    if (quiet === undefined || (quiet !== null && quiet >= DRAWN_QUIET_MS)) {
      runtime.drawn = true
      return true
    }
    return false
  }

  // --- one participant's step ----------------------------------------------------

  async #step(project, participant) {
    const runtime = this.#runtimeOf(participant.id)
    if (runtime.pane !== null) return this.#stepOpen(project, participant, runtime)
    if (project.state !== 'open') return
    // A lead whose agent is gone stays closed: its launch told the human, who switches it.
    if (participant.role === 'chief' && this.#agentGone(participant)) return
    // A lead whose window keeps failing to start is tried again ever more slowly.
    if (runtime.relaunch !== null && runtime.relaunch.at > this.#now()) return
    const next = this.#ledger.nextDelivery(participant.id)
    if (next !== null) {
      this.#act(runtime, () => this.#launch(project, participant, next))
      return
    }
    if (participant.role === 'chief') return
    // A member whose agent is gone must not wait for a window that will not
    // open: its held work goes back to the board now.
    if (this.#agentGone(participant)) return this.#withoutAgent(project, participant, null)
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
    // A participant forgotten while the step waits (it left, or its project
    // was deleted) is done with: nothing the look found is written, and
    // nothing more is done by its ids, which may be another's by now. Its
    // window closes with the record (`#closeLeaving`).
    let observed
    try {
      observed = await this.#observe(participant, runtime)
    } catch (cause) {
      if (!this.#forgotten(runtime)) {
        this.#setActivity(runtime, { state: 'unknown', reason: cause.message })
      }
      return
    }
    // Quota belongs to the member: a session that runs out takes its member out.
    const owner = this.#memberOf(project, participant)
    // A window with nothing in its record may still be drawing its screen:
    // Pi and Devin read idle before they could take a keystroke. One that
    // has not said which conversation it shows (a resumed window has its
    // record from the start) holds its messages, and is starting while it
    // draws its screen or until it names its first conversation.
    const unnamed = observed.unnamed === true
    if (!unnamed) runtime.named = true
    const drawing = (observed.items.length === 0 || unnamed) && !(await this.#drawn(runtime))
    if (this.#forgotten(runtime)) return
    const starting = drawing || (unnamed && !runtime.named)
    // An out member's window says so, and nothing else, until the reset.
    if (!this.#isOut(owner)) {
      this.#setActivity(
        runtime,
        starting
          ? { state: 'starting' }
          : observed.waiting
            ? { state: 'waiting', reason: observed.waiting.reason ?? null }
            : observed.settled
              ? { state: 'idle' }
              : { state: 'working' },
      )
    }
    this.#copyTranscript(participant, runtime, observed)
    // The human switched the window to another conversation: this look was
    // the old one's last, and nothing is delivered on it.
    if (observed.switched !== undefined) {
      if (runtime.delivering !== null) this.#confirmArrival(runtime, observed)
      this.#follow(participant, runtime, observed.switched.nativeSession)
      return
    }
    if (observed.quota !== undefined) this.#recordQuota(runtime, owner, observed.quota)
    const out = this.#isOut(owner)
    if (!out && this.#freshRefusal(owner, runtime.quota)) {
      await this.#outOfQuota(project, participant, runtime, owner)
      return
    }
    if (out) {
      // A lead out of quota has no turn to finish: the switch the human asked for goes now.
      if (runtime.pendingSwitch !== null) {
        await this.#performSwitch(project, participant, runtime, runtime.pendingSwitch)
        return
      }
      this.#setActivity(runtime, {
        state: 'out',
        reason: `out of quota until ${owner.outUntil}`,
      })
      // A held task's agent stops, as any paused task's; the window waits for the reset.
      if (participant.role !== 'chief') await this.#interruptIfPaused(participant, runtime)
      return
    }
    if (runtime.delivering !== null) await this.#watchArrival(runtime, observed)
    // A window that began to close in this step (a launch that timed out) is not acted on.
    if (runtime.retiring) return
    if (participant.role !== 'chief') {
      await this.#interruptIfPaused(participant, runtime)
      if (this.#forgotten(runtime)) return
      this.#collect(project, participant, observed)
      // The window may have gone during this step (a launch that timed out).
      if (
        runtime.pane !== null &&
        runtime.delivering === null &&
        !runtime.pinned &&
        !this.#ledger.holdsWork(participant.id)
      ) {
        await this.#retire(runtime)
        return
      }
    }
    const idle = runtime.delivering === null && observed.settled && !observed.waiting && !drawing
    if (runtime.pendingSwitch !== null) {
      await this.#awaitSwitch(project, participant, runtime, observed, idle)
      return
    }
    if (idle) {
      const next = this.#ledger.nextDelivery(participant.id)
      if (next !== null) this.#act(runtime, () => this.#deliver(runtime, next))
    }
  }

  /**
   * A switch the human asked for after the lead's turn: once the turn is
   * over (and the note asking where things stand came and was answered, when
   * they asked for one), the lead goes. Until then nothing else is delivered
   * to it, so its turn can end.
   */
  async #awaitSwitch(project, chief, runtime, observed, idle) {
    if (!idle) return
    const { note } = runtime.pendingSwitch
    const asked = note === null ? null : this.#ledger.message(note)
    if (asked?.state === 'queued') {
      this.#act(runtime, () => this.#deliver(runtime, asked))
      return
    }
    if (asked !== null && asked.state === 'delivered') {
      const at = observed.items.findIndex((item) => item.id === asked.receipt?.item)
      const answered = observed.items
        .slice(at + 1)
        .some((item) => item.role === 'assistant' && item.complete === true)
      if (at === -1 || !answered) return
    }
    await this.#performSwitch(project, chief, runtime, runtime.pendingSwitch)
  }

  /**
   * The human's Switch lead: the chief goes on in a fresh window on
   * `harness`, on the saved `agent` (model and effort) or the harness's own
   * default, and the window's first message hands it the lead (`#handoff`).
   * `when: 'turn'` lets a lead at work finish its turn; `note` first asks it
   * to write down where things stand, and switches once it has answered. A
   * lead with no window, or out of quota, switches at once. A project deleted
   * while the switch waits is gone for it, whatever has its id by then.
   */
  async switchChief(projectId, { harness, agent = null, when = 'now', note = false }) {
    this.requireAdapter(harness)
    if (agent !== null && this.#roster(agent) === null) {
      throw new Error(`${agent} is not among your agents`)
    }
    const project = this.#knownProject(projectId)
    const chief = project.participants.find((participant) => participant.role === 'chief')
    const runtime = this.#runtimeOf(chief.id)
    await this.#exclusive(
      chief.id,
      async () => {
        // Asked of the project as it is once the lead's step in progress is
        // over. One deleted meanwhile forgot its lead, and the ledger may
        // have given its ids to a new project, which is not switched.
        if (this.#forgotten(runtime)) throw new Error(`no project ${projectId}`)
        requireOpen(this.#knownProject(projectId))
        if (runtime.pane !== null && !this.#isOut(chief) && (when === 'turn' || note)) {
          // A switch asked again replaces the one waiting, with a note not yet sent.
          const replaced = runtime.pendingSwitch?.note ?? null
          if (replaced !== null && this.#ledger.message(replaced).state === 'queued') {
            this.#ledger.cancelMessage(replaced, 'the human asked for the switch again')
          }
          runtime.pendingSwitch = {
            harness,
            agent,
            note: note
              ? this.#ledger.note(projectId, {
                  to: 'chief',
                  body: `The human is moving this project's lead to ${harness}${agent === null ? '' : ` (${agent})`} once you answer. Write down where things stand, for the lead after you: what you and the human decided, what you promised, what you were about to do, and what is unresolved. Do not start anything new.`,
                }).id
              : null,
          }
          this.#changed()
          return
        }
        await this.#performSwitch(project, chief, runtime, { harness, agent })
        // One deleted while the old window was looked at or closed stopped the switch there.
        if (this.#forgotten(runtime)) throw new Error(`no project ${projectId}`)
      },
      { wait: true },
    )
    return this.#ledger.project(projectId)
  }

  /**
   * The switch itself, holding the chief's turn: one last look at the old
   * window (its words become history; a delivery whose header shows there
   * arrived), what it was still receiving goes back to the queue with its
   * attempt, the window closes without suspending the project, the ledger
   * moves the chief, and the new window opens with the handoff. A project
   * deleted on the way (its lead forgotten) stops it there: its ids may be a
   * new project's by then, and its old window closes with the record.
   */
  async #performSwitch(project, chief, runtime, { harness, agent }) {
    const asked = runtime.pendingSwitch?.note ?? null
    runtime.pendingSwitch = null
    let cut = false
    const { pane } = runtime
    if (pane !== null) {
      const observed = await this.#observe(chief, runtime).catch(() => null)
      if (this.#forgotten(runtime)) return
      if (observed !== null) {
        this.#copyTranscript(chief, runtime, observed)
        if (runtime.delivering !== null) this.#confirmArrival(runtime, observed)
        cut = !observed.settled
      }
      const { delivering } = runtime
      runtime.delivering = null
      if (delivering !== null) this.#giveBack(delivering, 'the lead was switched before it arrived')
      await this.#closeOwn(runtime, pane)
    }
    if (this.#forgotten(runtime)) return
    // A handoff still on its way is an earlier switch's: this one writes its
    // own. The note asking the old lead where things stand was for it alone,
    // however the switch came (now, or the lead out of quota).
    for (const message of this.#ledger.pending(chief.id)) {
      if (isHandoff(message)) this.#ledger.cancelMessage(message.id, 'the lead was switched again')
      else if (message.id === asked) {
        this.#ledger.cancelMessage(message.id, 'the lead was switched before it came')
      }
    }
    this.#ledger.switchChief(project.id, { harness, agent, cut })
    Object.assign(runtime, {
      quota: null,
      lowUntil: null,
      copied: null,
      interrupted: null,
      held: null,
      relaunch: null,
    })
    this.#changed()
    const current = this.#ledger.project(project.id)
    if (current.state !== 'open') return
    const lead = current.participants.find((participant) => participant.role === 'chief')
    this.#act(runtime, () => this.#launch(current, lead, null))
  }

  /**
   * The note that hands the lead to a fresh window when there is a history
   * to hand over, written from the ledger (see `handoff.js`), so it needs no
   * turn of the old lead's; null for a project's first lead.
   */
  #handoff(project, chief) {
    const history = this.#ledger.leadHistory(project.id)
    if (!history.some((conversation) => conversation.items.length > 0)) return null
    const switched = this.#ledger.lastSwitch(project.id)
    // The page count is cf history's: a line names only this project's messages.
    const message = (id) => {
      const found = this.#ledger.message(id)
      return found?.projectId === project.id ? found : null
    }
    return this.#ledger.note(project.id, {
      to: 'chief',
      body: handoffText({
        from: switched?.from ?? { harness: history.at(-1).harness, agent: null },
        to: { harness: chief.harness, agent: chief.agent },
        open: this.#ledger.leadOpenWork(project.id),
        last: lastWords(history),
        cut: switched?.cut === true,
        pages: historyPages(history, { message }),
      }),
    })
  }

  /** A participant whose saved agent the human has since deleted. */
  #agentGone(participant) {
    return participant.agent !== null && this.#savedAgent(participant.agent) === null
  }

  /**
   * A saved agent as the roster has it now: its row, or null when it is gone.
   * While the human's agents file cannot be read (the roster throws, saying
   * why) it is undefined: nobody counts as free for new work, and nobody's
   * agent as gone, so no work is taken back for a typo.
   */
  #savedAgent(name) {
    try {
      return this.#roster(name)
    } catch {
      return undefined
    }
  }

  /**
   * A lead whose agent is gone does not open: the human hears why, and what
   * it was to receive waits for the lead they switch in, its attempt given back.
   */
  #leadWithoutAgent(project, chief, delivering) {
    this.#ledger.note(project.id, {
      to: 'human',
      body: `The lead runs on ${chief.agent}, which is no longer among your agents: add it back under Agents, or switch the lead.`,
    })
    if (delivering !== null) {
      this.#giveBack(delivering, `${chief.agent} is no longer among your agents`)
    }
    this.#changed()
  }

  /**
   * What a window was to receive goes back to its queue with its attempt:
   * the window went before it had the chance to land.
   */
  #giveBack(delivering, reason) {
    if (this.#ledger.message(delivering.messageId)?.state !== 'delivering') return
    this.#ledger.retryDelivery(delivering.messageId, reason, { refund: true })
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
   * A window the human switched to another conversation (/clear, /new,
   * /resume) is followed: the participant's conversation is the one it shows
   * now, and its launch names it, so later looks, deliveries and the
   * transcript copy go there. A delivery still on its way counts once its
   * header shows in the record the window now writes.
   */
  #follow(participant, runtime, nativeSession) {
    this.#ledger.followConversation(participant.id, {
      harness: participant.harness,
      nativeSession,
    })
    runtime.launch.nativeSession = nativeSession
    runtime.copied = null
    this.#changed()
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
    // The chief's tell reached the window during this pause: the agent answers
    // it and ends its own turn, uninterrupted; the chief resumes the task.
    if (this.#ledger.toldSincePaused(participant.id, paused.id)) return
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
   * A session's window closes with its task; its conversation stays until the
   * session ends (the ledger ends both together), so a follow-up given with
   * `--after` comes back on the same conversation. A window already closing
   * had its kill.
   */
  async #retire(runtime) {
    if (runtime.retiring) return
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

  /**
   * Whether the message on its way shows in the window's record; it is
   * confirmed if so. Only what the window was given counts: a tool's output
   * that prints a header (cf inbox read, a log) proves nothing arrived.
   */
  #confirmArrival(runtime, observed) {
    const { delivering } = runtime
    const arrived = observed.items.find(
      (item) => item.role === 'user' && item.text.includes(delivering.marker),
    )
    if (arrived === undefined) return false
    this.#ledger.confirmDelivery(delivering.messageId, { item: arrived.id })
    runtime.delivering = null
    this.#changed()
    return true
  }

  async #watchArrival(runtime, observed) {
    if (this.#confirmArrival(runtime, observed)) return
    const { delivering } = runtime
    const waited = this.#now() - delivering.since
    if (delivering.launch) {
      // The lead's window is the human's: however long it takes to show its
      // first message (the handoff), it is never closed for that.
      if (delivering.chief || waited <= this.#launchTimeoutMs) return
      runtime.delivering = null
      const closing = this.#retire(runtime)
      this.#settleFailure(delivering, 'the window never showed its first message', { retry: false })
      await closing
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
    // The turn is over once its last message is complete (the one that ended
    // it; a message that ended in a tool call never is). The result is
    // everything the member wrote in it, in order: a report written before a
    // last command, then "Committed.", is not the last word alone. A tool's
    // output is not the member's words.
    const written = observed.items.slice(start + 1).filter((item) => item.role === 'assistant')
    if (written.at(-1)?.complete !== true) return
    const body =
      written
        .map((item) => item.text.trim())
        .filter((text) => text !== '')
        .join('\n\n') || '(the agent ended its turn without a written answer)'
    this.#ledger.recordResult(project.id, task.number, { body })
    this.#changed()
  }

  async #deliver(runtime, message) {
    // A participant forgotten while its delivery waits (it left, or its
    // project was deleted) is handed nothing, and what its harness did with
    // the message settles nothing: the message's id may be another
    // project's by now.
    if (runtime.adapter.ready !== undefined) {
      const ready = await runtime.adapter.ready({
        launch: runtime.launch,
        pane: runtime.pane,
        host: this.#host,
      })
      if (this.#forgotten(runtime)) return
      if (ready !== true) {
        // Said once per message, so a wait is in the trace, not a mystery.
        if (runtime.held !== message.id) {
          runtime.held = message.id
          this.#traceWindow(runtime, 'delivery.held', {
            message: message.id,
            reason: `the window is not ready for a paste: ${typeof ready === 'string' ? ready : 'a paste is on its way'}`,
          })
        }
        return
      }
    }
    runtime.held = null
    this.#ledger.beginDelivery(message.id)
    const { pane } = runtime
    let outcome
    try {
      outcome = await runtime.adapter.deliver({
        launch: runtime.launch,
        pane,
        host: this.#host,
        text: deliveryText(message),
      })
    } catch (cause) {
      // Uncertain is for a harness that may have taken it; an adapter that
      // throws never handed it over, and its error must show.
      outcome = { admitted: false, reason: `the delivery failed: ${cause.message}` }
    }
    if (this.#forgotten(runtime)) return
    const delivering = {
      messageId: message.id,
      marker: markerOf(message.id),
      since: this.#now(),
      launch: false,
      // The harness's own queue took it (a peer inbox, a broker, a plugin):
      // it shows when the harness gets to it, and sending it again would only
      // make a duplicate, which Claude even drops as a repeat.
      queued: outcome.queued === true,
    }
    if (outcome.admitted === false) {
      this.#settleFailure(delivering, outcome.reason ?? 'the harness refused it', { retry: true })
      return
    }
    // A window that closed while its harness took the message had nothing
    // on its way to settle when it went: the message goes again.
    if (runtime.pane !== pane) {
      this.#settleFailure(delivering, 'its window closed while it was handed over', {
        retry: true,
      })
      return
    }
    runtime.delivering = delivering
    this.#changed()
  }

  // --- launches --------------------------------------------------------------------

  async #launch(project, participant, message) {
    const runtime = this.#runtimeOf(participant.id)
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
        else this.#withoutAgent(project, participant, delivering)
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
    // was deleted) gets nothing more under its ids, which may be another's
    // by now: no window opens for it, and one already open goes with the
    // record (`#closeLeaving`).
    if (this.#forgotten(runtime)) {
      this.#launchFiles.forget(launchId)
      return
    }
    const token = this.#credentials.issue({ participant, project })
    const pane = { id: `p${project.id}-${participant.handle}`, generation }
    // The host watches a window's process before it answers the open, so an
    // exit can come first, even in the same read: `paneExited` finds it here.
    runtime.opening = { pane, exited: false }
    const opened = await this.#host
      .open({
        ...pane,
        cwd: project.directory,
        argv: plan.argv,
        env: { ...this.#paneEnv(participant, project), ...plan.env, CONSENSFLOW_TOKEN: token },
        dropEnv: plan.dropEnv,
      })
      .catch((cause) => ({ ok: false, error: cause.message }))
    const { exited } = runtime.opening
    runtime.opening = null
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
      Object.assign(runtime, { pane, launchId, token })
      // One that exited before its open was answered has gone already.
      if (exited) await this.paneExited(pane)
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
      activity: { state: 'starting' },
      drawn: false,
      named: false,
    })
    // A window that exited before its open was answered goes as any exit does.
    if (exited) {
      await this.paneExited(pane)
      return
    }
    const started = await adapter
      .started({ launch: plan.launch, pane, host: this.#host })
      .catch((cause) => ({ error: cause.message }))
    if (started.error !== undefined && delivering !== null) {
      runtime.delivering = null
      // The lead's window goes without taking its project with it: the lead is tried again.
      if (participant.role === 'chief') await this.#closeOwn(runtime, pane)
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
    runtime.relaunch = null
    if (started.nativeSession && started.nativeSession !== plan.nativeSession) {
      this.#ledger.bindConversation(conversationId, started.nativeSession)
    }
    this.#changed()
  }

  /**
   * A lead's first message. A handoff still on its way goes first: the
   * window it was written for never came up, or closed before showing it. A
   * lead that starts fresh with earlier conversations behind it (the human
   * switched it) is handed the lead. Either way, what was queued for it
   * waits for its next turn.
   */
  #leadFirst(project, chief, conversation, message) {
    const handoff = this.#ledger
      .pending(chief.id)
      .find((pending) => isHandoff(pending) && pending.state === 'queued')
    if (handoff !== undefined) return handoff
    if (conversation === null) return this.#handoff(project, chief) ?? message
    return message
  }

  /**
   * A launch that did not come up. A member's first message fails with it,
   * so its task fails and the requester hears why; a window the human opened
   * with nothing to deliver tells them why it did not come. The lead's first
   * message goes back to its queue with its attempt, and the lead is tried
   * again, ever more slowly while it keeps failing. The human hears why
   * once, until the lead starts or they ask for it again. A participant
   * forgotten while it launched hears nothing and settles nothing: its
   * project may be gone, and its ids another's.
   */
  #launchFailed(runtime, project, participant, delivering, reason) {
    if (this.#forgotten(runtime)) return
    if (participant.role !== 'chief') {
      if (delivering !== null) this.#settleFailure(delivering, reason, { retry: false })
      else {
        this.#ledger.note(project.id, {
          to: 'human',
          body: `@${participant.handle} could not start: ${reason}.`,
        })
        this.#changed()
      }
      return
    }
    if (delivering !== null) this.#giveBack(delivering, reason)
    const failures = (runtime.relaunch?.failures ?? 0) + 1
    runtime.relaunch = {
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

  /**
   * Closes a window whose exit is the dispatcher's own (a switch, a Close, a
   * lead that could not take its first message): the exit settles what the
   * window was doing, as any exit does, but a lead's does not close its
   * project. It is the dispatcher's whether its event came already or comes
   * later. A window already going with its work had its kill.
   */
  async #closeOwn(runtime, pane) {
    runtime.ownExit = true
    try {
      if (!runtime.retiring) await this.#host.kill(pane).catch(() => {})
      if (runtime.pane?.id === pane.id && runtime.pane.generation === pane.generation) {
        await this.paneExited(pane)
      }
    } finally {
      runtime.ownExit = false
    }
  }

  // --- assignment ---------------------------------------------------------------------

  /**
   * Each open task goes to the best free member of its tier; the requester
   * hears once when none is. A task that needs others waits for them to be
   * accepted, in silence: its card says what it waits for. Says whether it
   * gave any task out.
   */
  #assignOpenTasks(project) {
    let assigned = false
    for (const task of this.#ledger.board(project.id).open) {
      if (task.blockedBy.length > 0 || task.assignee !== null) continue
      const candidates = this.#ledger.candidates(project.id, task.number)
      const free = candidates.filter((member) => this.#available(member))
      if (free.length > 0) {
        this.#ledger.assignTask(project.id, task.number, this.#rank(free, candidates)[0].id)
        this.#waitingNoted.delete(task.id)
        this.#changed()
        assigned = true
      } else if (!this.#waitingNoted.has(task.id)) {
        this.#waitingNoted.set(task.id, project.id)
        const why = candidates
          .map((member) => this.#whyNotFree(member))
          .filter((reason) => reason !== null)
        this.#ledger.note(project.id, {
          to: task.requester,
          task: task.number,
          body: `T-${task.number} waits for a free ${task.pool === 'designer' ? 'image designer' : `${task.tier} ${task.pool}`}: ${why.join('; ')}.`,
        })
        this.#changed()
      }
    }
    return assigned
  }

  /** The member a session belongs to; a member or the chief is its own. */
  #memberOf(project, participant) {
    if (participant.memberId === null) return participant
    return project.participants.find((p) => p.id === participant.memberId) ?? participant
  }

  /** Free: on a harness whose windows open, its agent saved, not out of quota, not low on it. */
  #available(member) {
    return this.#whyNotFree(member) === null
  }

  /** Why a member is not free, the first reason that holds for it; null when it is free. */
  #whyNotFree(member) {
    if (this.#adapters[member.harness] === undefined) {
      return `@${member.handle} runs on ${member.harness}, whose windows ConsensFlow cannot open`
    }
    const agent = this.#savedAgent(member.agent)
    if (agent === undefined) {
      return `@${member.handle}'s agent cannot be read (your agents file needs fixing: see Agents)`
    }
    if (agent === null) {
      return `@${member.handle} has no agent any more (${member.agent} is not among your agents: define it, or remove the member)`
    }
    if (this.#isOut(member)) return `@${member.handle} is out of quota until ${member.outUntil}`
    if (this.#isLow(member)) return `@${member.handle} is low on quota`
    return null
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
   * Who of the free members takes a task: one it was taken from goes last;
   * then the harness whose members of this role and tier have taken the fewest
   * tasks, so a tier's work is shared across harnesses; then the member with
   * the fewest; then the earliest joined.
   */
  #rank(members, candidates) {
    const load = new Map()
    for (const member of candidates) {
      load.set(member.harness, (load.get(member.harness) ?? 0) + member.taken)
    }
    return [...members].sort(
      (a, b) =>
        Number(a.hadIt === true) - Number(b.hadIt === true) ||
        load.get(a.harness) - load.get(b.harness) ||
        a.taken - b.taken ||
        a.id - b.id,
    )
  }

  /**
   * A member whose agent is gone from the human's agents (a release dropped
   * the catalog entry, or the human removed one of their own) runs on no
   * default: its tiered work goes back to the board for another member, with
   * whatever was on its way to it withdrawn, and a request given to it by
   * name fails so the requester hears why. The board says why it sits.
   */
  #withoutAgent(project, participant, delivering) {
    const because = `${participant.agent} is no longer among your agents`
    const tiered = this.#tieredWork(project, participant)
    for (const task of tiered) this.#ledger.releaseTask(project.id, task.number, { because })
    if (delivering !== null)
      this.#settleFailure(
        delivering,
        `${because}: add it back under Agents, or remove @${participant.handle} from the staff`,
        { retry: false },
      )
    else if (tiered.length > 0) this.#changed()
  }

  /**
   * The work a participant holds that was given to its tier (queued, working
   * or waiting): another member of the tier could take it.
   */
  #tieredWork(project, participant) {
    const lane = this.#ledger
      .board(project.id)
      .lanes.find((l) => l.participant.id === participant.id)
    return (lane?.tasks ?? []).filter(
      (task) => task.pool !== null && ['queued', 'working', 'waiting'].includes(task.state),
    )
  }

  /** What a window's harness says of its quota, as of this look. */
  #recordQuota(runtime, owner, quota) {
    runtime.quota = quota ?? null
    // Low is soft: the current task continues, nothing new comes until the
    // reset it names (an hour when it names none). It outlives the window,
    // which closes with the task, so a low member is not asked again at once.
    if (runtime.quota?.state === 'low') {
      this.#runtimeOf(owner.id).lowUntil = this.#resetOf(runtime.quota)
    } else if (runtime.quota !== null) this.#runtimeOf(owner.id).lowUntil = null
  }

  /** When a quota resets: the time its harness names, or an hour from now. */
  #resetOf(quota) {
    return quota.resetsAt ?? new Date(this.#now() + UNKNOWN_RESET_MS).toISOString()
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
    const until = this.#resetOf(runtime.quota)
    this.#setActivity(runtime, { state: 'out', reason: `out of quota until ${until}` })
    this.#ledger.markOut(owner.id, { until, reason: 'out of quota' })
    if (runtime.delivering !== null) {
      const { delivering } = runtime
      runtime.delivering = null
      this.#settleFailure(delivering, 'the harness ran out of quota', { retry: true })
    }
    // Near the reset, or with nobody else to take it, a task keeps its window
    // and goes on by itself; otherwise it goes back to the board.
    const soon = Date.parse(until) - this.#now() <= HOLD_MS
    for (const task of this.#tieredWork(project, participant)) {
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
      await this.#retire(runtime)
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
    } else this.#failDelivery(message, reason)
    this.#changed()
  }

  /**
   * A message that will not be delivered. A brief fails its task, and whoever
   * gave it hears why. Whoever waits on any other kind (a worker on its
   * answer, the chief on a result) would wait forever, so the human hears,
   * once, what it was, for whom and why.
   */
  #failDelivery(message, reason) {
    this.#ledger.failDelivery(message.id, reason)
    const project = this.#ledger.project(message.projectId)
    if (message.kind === 'task' && message.taskNumber !== null) {
      const task = this.#ledger.task(project.id, message.taskNumber)
      if (task.state === 'failed') {
        this.#tellRequester(project, task, reason)
        // A task the human gave: that note was theirs.
        if (task.requester === 'human') return
      }
    }
    const kind = message.kind === 'answer' ? 'an answer' : `a ${message.kind}`
    const from = message.sender === null ? 'ConsensFlow' : `@${message.sender}`
    const on = message.taskNumber === null ? '' : ` on T-${message.taskNumber}`
    this.#ledger.note(project.id, {
      to: 'human',
      ...(message.taskNumber === null ? {} : { task: message.taskNumber }),
      body: `m-${message.id}, ${kind} from ${from}${on}, did not reach @${message.recipient}: ${reason}.`,
    })
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
   * Runs `work` for one participant at a time. A pass that finds it busy (a
   * step, or a launch or a delivery still going on) moves on; `wait` queues
   * behind it instead.
   */
  async #exclusive(participantId, work, { wait = false } = {}) {
    const runtime = this.#runtimeOf(participantId)
    while (runtime.running !== null || runtime.acting !== null) {
      if (!wait) return undefined
      await (runtime.running ?? runtime.acting).catch(() => {})
    }
    runtime.running = (async () => {
      try {
        return await begin(work)
      } finally {
        runtime.running = null
      }
    })()
    return runtime.running
  }

  /**
   * A launch or a delivery may wait long on its harness (Codex naming its
   * thread, a paste waiting for Pi's acknowledgement). Started from a step,
   * it goes on apart from the pass and still holds its participant, so the
   * pass and every other window move on. Nobody waits for it, so a failure
   * is written down (`#writeDown`).
   */
  #act(runtime, work) {
    runtime.acting = (async () => {
      try {
        await begin(work)
      } catch (cause) {
        this.#writeDown(cause)
      } finally {
        runtime.acting = null
      }
    })()
  }

  /** What failed apart from any pass or request is written down, as a failed pass is. */
  #writeDown(cause) {
    console.error('consensflow dispatcher:', cause)
    this.#log?.error('a launch or a delivery failed', cause)
  }

  /**
   * Opens a participant's window once its step in progress is over, apart
   * from whoever asked: a page operation answers once the ledger has its
   * change, and a launch that fails says so on the board. A window open by
   * then, or a project closed meanwhile, opens nothing; so does a participant
   * forgotten meanwhile (it left, or its project was deleted), whose id may
   * be another's by then, and no record is made for it again. Nobody waits
   * for it, so a failure is written down.
   */
  #openSoon(participantId) {
    const runtime = this.#runtimeOf(participantId)
    this.#exclusive(
      participantId,
      () => {
        if (this.#forgotten(runtime)) return
        const project = this.#projectOf(participantId)
        if (runtime.pane !== null || project?.state !== 'open') return
        const participant = project.participants.find((p) => p.id === participantId)
        this.#act(runtime, () => this.#launch(project, participant, null))
      },
      { wait: true },
    ).catch((cause) => this.#writeDown(cause))
  }

  /** The dispatcher's record of a participant's window, made the first time it is asked for. */
  #runtimeOf(participantId) {
    let runtime = this.#runtime.get(participantId)
    if (runtime === undefined) {
      runtime = {
        id: participantId,
        adapter: null,
        pane: null,
        opening: null,
        launch: null,
        launchId: null,
        token: null,
        delivering: null,
        held: null,
        quota: null,
        lowUntil: null,
        running: null,
        acting: null,
        retiring: false,
        copied: null,
        interrupted: null,
        pinned: false,
        pendingSwitch: null,
        ownExit: false,
        relaunch: null,
        drawn: false,
        named: false,
        activity: { state: 'closed' },
      }
      this.#runtime.set(participantId, runtime)
    }
    return runtime
  }

  /**
   * Whether a record was forgotten (`#forget`) since work took it: its
   * participant left, or its project was deleted, and the ledger may have
   * given its id to another since. That work does nothing more by the id.
   */
  #forgotten(runtime) {
    return this.#runtime.get(runtime.id) !== runtime
  }

  #setActivity(runtime, activity) {
    if (runtime.activity.state === activity.state && runtime.activity.reason === activity.reason)
      return
    runtime.activity = activity
    this.#traceWindow(runtime, 'window.activity', {
      state: activity.state,
      reason: activity.reason ?? null,
    })
    this.#changed()
  }

  /** Tells the trace what happened at a window, named by its project and participant. */
  #traceWindow(runtime, kind, details) {
    const project = this.#projectOf(runtime.id)
    this.#trace({
      at: new Date(this.#now()).toISOString(),
      kind,
      project: project?.id ?? null,
      participant: project?.participants.find((p) => p.id === runtime.id)?.handle ?? null,
      ...details,
    })
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
