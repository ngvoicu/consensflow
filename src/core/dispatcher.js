import { randomUUID } from 'node:crypto'
import { Deliveries } from './deliveries.js'
import { deliveryText, markerOf } from './delivery-text.js'
import { HANDOFF_TITLE, handoffText, historyPages, lastWords } from './handoff.js'
import { Scheduler } from './scheduler.js'

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
 *   A message withdrawn on its way (its task cancelled, or taken back from
 *   the window) is waited for no longer, and never handed over again.
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
 * - A task cancelled under its window stops it: an agent still at work on
 *   it is interrupted, as a paused task's is, and the window closes as any
 *   that holds no task, unless the human opened it.
 * - A member window lost mid-task (a restart, a crash, a closed window)
 *   pauses the task, and the requester is told how to resume it. A chief
 *   window that closes by itself closes its project, as the human's Close
 *   does: every window of it goes. A participant that leaves (a member off
 *   the staff with its sessions, a session the human ends, every one of a
 *   deleted project) is forgotten at once, quota marks and all, so a member
 *   that comes back, under its own id again, starts clean; its window closes
 *   once its step in progress ends, and that exit fails nothing: a member's
 *   open tasks were cancelled when it left. Work on it that was waiting
 *   meanwhile (a step, a launch, a delivery, an Open, a Switch lead, a
 *   removal) does nothing more by its ids. The ledger never gives an id
 *   twice: those ids name rows gone with their project, which the work
 *   would fail on, or a member that may be back on the staff.
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

/**
 * A lead runs on one of the human's saved agents, never on a harness's own
 * default model: asked for without one, the core says what to pick.
 */
export function requireLeadAgent(agent) {
  if (typeof agent !== 'string' || agent.length === 0) {
    throw new Error(
      'pick one of your saved agents for the lead: its harness, model and effort come with it',
    )
  }
  return agent
}

/** A note from ConsensFlow that hands the lead to a new window (`handoff.js`). */
const isHandoff = (message) =>
  message.kind === 'note' && message.sender === null && message.body.startsWith(HANDOFF_TITLE)

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
  /** Told each change of a window's activity, a delivery a window is not ready for and a deleted project, for the event file in the home. */
  #trace
  /** The daemon's log, for a launch or a delivery that failed apart from any pass. */
  #log
  /** What goes into each window and what comes back out (`deliveries.js`). */
  #deliveries
  /** Who takes which open task, and who is out of quota (`scheduler.js`). */
  #scheduler
  #runtime = new Map()
  /** The records of participants forgotten while their window was still open, until it exits. */
  #leaving = new Set()
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
    this.#trace = trace
    this.#log = log
    this.#launchFiles = launchFiles
    this.#deliveries = new Deliveries({
      ledger,
      host,
      adapters,
      arrivalTimeoutMs,
      launchTimeoutMs,
      maxAttempts,
      traceWindow: (runtime, kind, details) => this.#traceWindow(runtime, kind, details),
      retire: (runtime) => this.#retire(runtime),
      forgotten: (runtime) => this.#forgotten(runtime),
      now: () => this.#now(),
      changed: () => this.#changed(),
    })
    this.#scheduler = new Scheduler({
      ledger,
      adapters,
      roster,
      deliveries: this.#deliveries,
      runtimeOf: (participantId) => this.#runtimeOf(participantId),
      runtime: (participantId) => this.#runtime.get(participantId),
      now: () => this.#now(),
      changed: () => this.#changed(),
    })
    host.onExit((pane) => this.paneExited(pane))
  }

  onChange(listener) {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  /** What a participant's window is doing: starting, working, idle, waiting (with why), closed. */
  activity(participantId) {
    return this.#runtime.get(participantId)?.window.activity ?? { state: 'closed' }
  }

  /** The Switch lead waiting for this lead's turn to end, `{harness, agent}`, or null. */
  pendingSwitch(participantId) {
    const pending = this.#runtime.get(participantId)?.pendingSwitch ?? null
    return pending === null ? null : { harness: pending.harness, agent: pending.agent }
  }

  /** The live window of a participant, `{id, generation}`, or null. */
  pane(participantId) {
    return this.#runtime.get(participantId)?.window.pane ?? null
  }

  /** Refuses a harness ConsensFlow has no adapter for: none of its windows could open. */
  requireAdapter(harness) {
    if (this.#adapters[harness] === undefined) {
      throw new Error(`ConsensFlow cannot open ${harness} windows`)
    }
  }

  /**
   * The lead asked for: one of the human's saved agents, on its harness,
   * whose windows ConsensFlow opens.
   */
  #requireLead({ harness, agent }) {
    requireLeadAgent(agent)
    this.requireAdapter(harness)
    if (this.#roster(agent) === null) throw new Error(`${agent} is not among your agents`)
  }

  /**
   * A new project: the ledger records it with its chief, on the saved agent
   * `chief` names, and its staff, and its chief window opens after the
   * answer (see `#openSoon`).
   */
  async openProject({ directory, name, chief = {}, staff = [], gate }) {
    this.#requireLead(chief)
    for (const member of staff) this.requireAdapter(member.harness)
    const project = this.#ledger.createProject({
      directory,
      name,
      chief: { harness: chief.harness, agent: chief.agent },
      staff,
      ...(gate === undefined ? {} : { gate }),
    })
    const lead = project.participants.find((participant) => participant.handle === 'chief')
    this.#openSoon(lead.id)
    return this.#ledger.project(project.id)
  }

  /** The human's Resume, and the restore after a restart: the chief comes back on its conversation. */
  async resumeProject(projectId) {
    const project = this.#ledger.setProjectState(projectId, 'open')
    const chief = project.participants.find((participant) => participant.role === 'chief')
    // The human asks for the lead now: a lead that failed before is tried at once.
    this.#runtimeOf(chief.id).window.relaunch = null
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
   * waited on, forgotten meanwhile or not: no record is made again for a
   * participant that left.
   */
  async #closeWindows(participants) {
    await Promise.all(
      participants.map((participant) => {
        const runtime = this.#runtimeOf(participant.id)
        return this.#exclusive(
          participant.id,
          async () => {
            if (runtime.window.pane !== null) await this.#closeOwn(runtime, runtime.window.pane)
          },
          { wait: true },
        )
      }),
    )
  }

  /**
   * Participants that left (a session ended, a member removed, a project
   * deleted) are forgotten at once, quota marks and all: nothing of them
   * stays for a member that comes back under its own id. A window one still
   * has closes once its step in progress is over.
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
    if (runtime.window.pane === null) this.#leaving.delete(runtime)
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
    this.#runtimeOf(participant.id).window.pinned = true
    this.#openSoon(participant.id)
    this.#changed()
    return this.#ledger.project(projectId)
  }

  /**
   * The human gives a task in a window to another member of its tier. Its
   * window is stopped first, even one the human opened, and the task leaves
   * it only then: no two windows ever work on one task.
   */
  async reassignTask(projectId, number) {
    this.#ledger.checkRelease(projectId, number)
    const { assignee } = this.#ledger.task(projectId, number)
    const holder = this.#knownProject(projectId).participants.find(
      (participant) => participant.handle === assignee,
    )
    const release = () => {
      const released = this.#ledger.releaseTask(projectId, number, { because: 'by @human' })
      this.#changed()
      return released
    }
    // Paused before anyone took it: there is no window to stop.
    if (holder === undefined) return release()
    return this.#exclusive(
      holder.id,
      async () => {
        const runtime = this.#runtimeOf(holder.id)
        runtime.window.pinned = false
        if (runtime.window.pane !== null) await this.#retire(runtime)
        return release()
      },
      { wait: true },
    )
  }

  /** The human closes a session's window; work in it pauses, as any lost window's does. */
  async closeWindow(projectId, handle) {
    const { participant } = this.#sessionOf(projectId, handle)
    const runtime = this.#runtimeOf(participant.id)
    runtime.window.pinned = false
    if (runtime.window.pane !== null) await this.#retire(runtime)
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
    // What is remembered of its tasks and participants goes now, and so do
    // its own lines in the trace. The ledger never gives their ids to another
    // project; its participants are forgotten before their windows close, so
    // its work still waiting stops instead of acting on rows that are gone.
    this.#scheduler.forgetProject(deleted.id)
    this.#trace.forget?.(projectId)
    await this.#forget((project?.participants ?? []).map((participant) => participant.id))
    // A deleted project leaves no trace but the line that says it was. That
    // line names no project; its data says which project went.
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
    await this.#deliveries.settleInFlight()
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
        // its deleted project: refused as the ledger refuses a member that
        // left.
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
    this.#scheduler.resumeHeld()
    const projects = this.#ledger
      .projects()
      .map((project) =>
        project.state === 'open' && this.#scheduler.assignOpenTasks(project)
          ? this.#ledger.project(project.id)
          : project,
      )
    const steps = []
    for (const project of projects) {
      // Sessions stay until the human deletes them, so an aged project has
      // many: one with no window and nothing on its way or in its hands is
      // not stepped, since its step would find nothing to do.
      const working = this.#ledger.withWork(project.id)
      for (const participant of project.participants) {
        if (participant.role === 'human') continue
        const idle =
          participant.memberId !== null &&
          !working.has(participant.id) &&
          (this.#runtime.get(participant.id)?.window.pane ?? null) === null
        if (idle) continue
        steps.push(this.#exclusive(participant.id, () => this.#step(project, participant)))
      }
    }
    await Promise.all(steps)
  }

  /** A window ended: `pane.exit` from the pane host. */
  async paneExited({ id, generation }) {
    const ended = (pane) => pane?.id === id && pane.generation === generation
    const runtime = [...this.#runtime.values(), ...this.#leaving].find(
      (candidate) => ended(candidate.window.pane) || ended(candidate.window.opening?.pane),
    )
    if (runtime === undefined) return
    // Still opening: its launch takes the exit once it has the window.
    if (!ended(runtime.window.pane)) {
      runtime.window.opening.exited = true
      return
    }
    this.#credentials.revoke(runtime.window.token)
    const delivering = runtime.delivery.delivering
    // The window's files in the home go with it.
    this.#launchFiles.forget(runtime.window.launchId)
    Object.assign(runtime.window, {
      pane: null,
      launch: null,
      launchId: null,
      token: null,
      retiring: false,
      activity: { state: 'closed' },
    })
    runtime.delivery.delivering = null
    // A participant that left is forgotten already: its window's exit settles nothing more.
    if (this.#leaving.delete(runtime)) return
    const participantId = runtime.id
    const project = this.#projectOf(participantId)
    if (project === null) return
    const participant = project.participants.find((p) => p.id === participantId)
    if (delivering !== null) {
      const because = `@${participant.handle}'s window closed`
      // A lead's first message (a handoff, most often) waits for its next window.
      if (delivering.chief) this.#deliveries.giveBack(delivering, because)
      else this.#deliveries.settleFailure(delivering, because, { retry: !delivering.launch })
    }
    if (participant.role === 'chief') {
      // The lead's own exit (the human's /exit, a crash) closes the project
      // as Close does: no member's window goes on unseen.
      if (project.state === 'open' && !runtime.window.ownExit) {
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
  async #drawn(runtime) {
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

  // --- one participant's step ----------------------------------------------------

  async #step(project, participant) {
    const runtime = this.#runtimeOf(participant.id)
    if (runtime.window.pane !== null) return this.#stepOpen(project, participant, runtime)
    if (project.state !== 'open') return
    // A lead whose agent is gone stays closed: its launch told the human, who switches it.
    if (participant.role === 'chief' && this.#scheduler.agentGone(participant)) return
    // A lead whose window keeps failing to start is tried again ever more slowly.
    if (runtime.window.relaunch !== null && runtime.window.relaunch.at > this.#now()) return
    const next = this.#ledger.nextDelivery(participant.id)
    if (next !== null) {
      this.#act(runtime, () => this.#launch(project, participant, next))
      return
    }
    if (participant.role === 'chief') return
    // A member whose agent is gone must not wait for a window that will not
    // open: its held work goes back to the board now.
    if (this.#scheduler.agentGone(participant))
      return this.#scheduler.withoutAgent(project, participant, null)
    // A member's session is its task's: with the window gone (a restart, a
    // crash) and nothing due to it, nobody is doing the work any more.
    const task = this.#ledger.activeTask(participant.id)
    if (task !== null) {
      this.#stall(project, task, `@${participant.handle}'s window is gone`)
      this.#changed()
    }
  }

  async #stepOpen(project, participant, runtime) {
    if (runtime.window.retiring) return
    // A participant forgotten while the step waits (it left, or its project
    // was deleted) is done with: nothing the look found is written, and
    // nothing more is done by its ids, which name rows gone with its project
    // or a member that may be back. Its window closes with the record
    // (`#closeLeaving`).
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
    const owner = this.#scheduler.memberOf(project, participant)
    // A window with nothing in its record may still be drawing its screen:
    // Pi and Devin read idle before they could take a keystroke. One that
    // has not said which conversation it shows (a resumed window has its
    // record from the start) holds its messages, and is starting while it
    // draws its screen or until it names its first conversation.
    const unnamed = observed.unnamed === true
    if (!unnamed) runtime.window.named = true
    const drawing = (observed.items.length === 0 || unnamed) && !(await this.#drawn(runtime))
    if (this.#forgotten(runtime)) return
    const starting = drawing || (unnamed && !runtime.window.named)
    // An out member's window says so, and nothing else, until the reset.
    if (!this.#scheduler.isOut(owner)) {
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
      if (runtime.delivery.delivering !== null) this.#deliveries.confirmArrival(runtime, observed)
      this.#follow(participant, runtime, observed.switched.nativeSession)
      return
    }
    if (observed.quota !== undefined) this.#scheduler.recordQuota(runtime, owner, observed.quota)
    const out = this.#scheduler.isOut(owner)
    if (!out && this.#scheduler.freshRefusal(owner, runtime.quota.reported)) {
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
      // A held task's agent stops, as any paused task's; the window waits for
      // the reset, unless its task is gone meanwhile (cancelled, taken back).
      if (participant.role !== 'chief') {
        await this.#interruptIfStopped(participant, runtime, observed)
        if (!this.#forgotten(runtime)) await this.#closeIfFree(runtime)
      }
      return
    }
    if (runtime.delivery.delivering !== null) await this.#deliveries.watchArrival(runtime, observed)
    // A window that began to close in this step (a launch that timed out) is not acted on.
    if (runtime.window.retiring) return
    if (participant.role !== 'chief') {
      await this.#interruptIfStopped(participant, runtime, observed)
      if (this.#forgotten(runtime)) return
      this.#deliveries.collect(project, participant, observed)
      if (await this.#closeIfFree(runtime)) return
    }
    const idle =
      runtime.delivery.delivering === null && observed.settled && !observed.waiting && !drawing
    if (runtime.pendingSwitch !== null) {
      await this.#awaitSwitch(project, participant, runtime, observed, idle)
      return
    }
    if (idle) {
      const next = this.#ledger.nextDelivery(participant.id)
      if (next !== null) this.#act(runtime, () => this.#deliveries.deliver(runtime, next))
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
      this.#act(runtime, () => this.#deliveries.deliver(runtime, asked))
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
   * The human's Switch lead: the chief goes on in a fresh window on the
   * saved `agent` (its model and effort) on `harness`, and the window's
   * first message hands it the lead (`#handoff`).
   * `when: 'turn'` lets a lead at work finish its turn; `note` first asks it
   * to write down where things stand, and switches once it has answered. A
   * lead with no window, or out of quota, switches at once. A project deleted
   * while the switch waits is gone for it, and nothing of it is switched.
   */
  async switchChief(projectId, { harness, agent, when = 'now', note = false }) {
    this.#requireLead({ harness, agent })
    const project = this.#knownProject(projectId)
    const chief = project.participants.find((participant) => participant.role === 'chief')
    const runtime = this.#runtimeOf(chief.id)
    await this.#exclusive(
      chief.id,
      async () => {
        // Asked of the project as it is once the lead's step in progress is
        // over: one deleted meanwhile is no project now.
        requireOpen(this.#knownProject(projectId))
        if (
          runtime.window.pane !== null &&
          !this.#scheduler.isOut(chief) &&
          (when === 'turn' || note)
        ) {
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
                  body: `The human is moving this project's lead to ${harness} (${agent}) once you answer. Write down where things stand, for the lead after you: what you and the human decided, what you promised, what you were about to do, and what is unresolved. Do not start anything new.`,
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
   * deleted on the way (its lead forgotten) stops it there: its rows are
   * gone, so there is no delivery to confirm and no lead to move, and its
   * old window closes with the record.
   */
  async #performSwitch(project, chief, runtime, { harness, agent }) {
    const asked = runtime.pendingSwitch?.note ?? null
    runtime.pendingSwitch = null
    let cut = false
    const { pane } = runtime.window
    if (pane !== null) {
      const observed = await this.#observe(chief, runtime).catch(() => null)
      if (this.#forgotten(runtime)) return
      if (observed !== null) {
        this.#copyTranscript(chief, runtime, observed)
        if (runtime.delivery.delivering !== null) this.#deliveries.confirmArrival(runtime, observed)
        cut = !observed.settled
      }
      const { delivering } = runtime.delivery
      runtime.delivery.delivering = null
      if (delivering !== null)
        this.#deliveries.giveBack(delivering, 'the lead was switched before it arrived')
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
    Object.assign(runtime.quota, { reported: null, lowUntil: null })
    runtime.copied = null
    Object.assign(runtime.window, { interrupted: null, relaunch: null })
    runtime.delivery.held = null
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
      this.#deliveries.giveBack(delivering, `${chief.agent} is no longer among your agents`)
    }
    this.#changed()
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
    runtime.window.launch.nativeSession = nativeSession
    runtime.copied = null
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
  async #interruptIfStopped(participant, runtime, observed) {
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
  async #retire(runtime) {
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
  async #closeIfFree(runtime) {
    if (
      runtime.window.pane === null ||
      runtime.delivery.delivering !== null ||
      runtime.window.pinned ||
      this.#ledger.holdsWork(runtime.id)
    ) {
      return false
    }
    await this.#retire(runtime)
    return true
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
      await this.paneExited(pane)
      return
    }
    const started = await adapter
      .started({ launch: plan.launch, pane, host: this.#host })
      .catch((cause) => ({ error: cause.message }))
    if (started.error !== undefined && delivering !== null) {
      runtime.delivery.delivering = null
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
    runtime.window.relaunch = null
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

  /**
   * Closes a window whose exit is the dispatcher's own (a switch, a Close, a
   * lead that could not take its first message): the exit settles what the
   * window was doing, as any exit does, but a lead's does not close its
   * project. It is the dispatcher's whether its event came already or comes
   * later. A window already going with its work had its kill.
   */
  async #closeOwn(runtime, pane) {
    runtime.window.ownExit = true
    try {
      if (!runtime.window.retiring) await this.#host.kill(pane).catch(() => {})
      if (
        runtime.window.pane?.id === pane.id &&
        runtime.window.pane.generation === pane.generation
      ) {
        await this.paneExited(pane)
      }
    } finally {
      runtime.window.ownExit = false
    }
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
    const until = this.#scheduler.resetOf(runtime.quota.reported)
    this.#setActivity(runtime, { state: 'out', reason: `out of quota until ${until}` })
    this.#ledger.markOut(owner.id, { until, reason: 'out of quota' })
    if (runtime.delivery.delivering !== null) {
      const { delivering } = runtime.delivery
      runtime.delivery.delivering = null
      this.#deliveries.settleFailure(delivering, 'the harness ran out of quota', { retry: true })
    }
    this.#scheduler.holdOrRelease(project, participant, owner, until)
    if (participant.role !== 'chief') await this.#closeIfFree(runtime)
    this.#changed()
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
   * forgotten meanwhile (it left, or its project was deleted): it is in no
   * project now, and its record, taken before the wait, is not made again.
   * Nobody waits for it, so a failure is written down.
   */
  #openSoon(participantId) {
    const runtime = this.#runtimeOf(participantId)
    this.#exclusive(
      participantId,
      () => {
        const project = this.#projectOf(participantId)
        if (runtime.window.pane !== null || project?.state !== 'open') return
        const participant = project.participants.find((p) => p.id === participantId)
        this.#act(runtime, () => this.#launch(project, participant, null))
      },
      { wait: true },
    ).catch((cause) => this.#writeDown(cause))
  }

  /**
   * The dispatcher's record of a participant's window, made the first time
   * it is asked for. Its parts are kept apart by what owns them: the window
   * itself, what is on its way into it, what its harness said of its quota,
   * a Switch lead waiting for the lead's turn to end, and how much of its
   * conversation is copied. `running` and `acting` are whoever holds the
   * participant now (`#exclusive`, `#act`).
   */
  #runtimeOf(participantId) {
    let runtime = this.#runtime.get(participantId)
    if (runtime === undefined) {
      runtime = {
        id: participantId,
        running: null,
        acting: null,
        window: {
          adapter: null,
          pane: null,
          opening: null,
          launch: null,
          launchId: null,
          token: null,
          retiring: false,
          ownExit: false,
          pinned: false,
          relaunch: null,
          drawn: false,
          named: false,
          interrupted: null,
          activity: { state: 'closed' },
        },
        delivery: { delivering: null, held: null },
        quota: { reported: null, lowUntil: null },
        pendingSwitch: null,
        copied: null,
      }
      this.#runtime.set(participantId, runtime)
    }
    return runtime
  }

  /**
   * Whether a record was forgotten (`#forget`) since work took it: its
   * participant left, or its project was deleted. That work does nothing
   * more by the id. The ledger never gives an id twice, but a deleted
   * project's rows are gone, and a member that left keeps its id when it
   * comes back: work that went on would fail on the one, into the log, or
   * act on the other.
   */
  #forgotten(runtime) {
    return this.#runtime.get(runtime.id) !== runtime
  }

  #setActivity(runtime, activity) {
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
