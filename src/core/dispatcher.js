import { Deliveries } from './deliveries.js'
import { LeadSwitch } from './lead-switch.js'
import { Scheduler } from './scheduler.js'
import { Transcripts } from './transcripts.js'
import { Windows } from './windows.js'

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

/**
 * Starts `work` inside an async function, so one that throws before its
 * first await rejects as one that throws after it does: whoever holds a
 * participant for it lets go either way.
 */
const begin = async (work) => work()

export class Dispatcher {
  #ledger
  #clock
  #roster
  /** Told each change of a window's activity, a delivery a window is not ready for and a deleted project, for the event file in the home. */
  #trace
  /** The daemon's log, for a launch or a delivery that failed apart from any pass. */
  #log
  /** What goes into each window and what comes back out (`deliveries.js`). */
  #deliveries
  /** Who takes which open task, and who is out of quota (`scheduler.js`). */
  #scheduler
  /** ConsensFlow's copy of each window's conversation (`transcripts.js`). */
  #transcripts
  /** Each window's launch, looks and close (`windows.js`). */
  #windows
  /** The human's Switch lead, and the handoff a new lead starts with (`lead-switch.js`). */
  #lead
  #runtime = new Map()
  /** The records of participants forgotten while their window was still open, until it exits. */
  #leaving = new Set()
  #listeners = new Set()

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
    this.#clock = clock
    this.#roster = roster
    this.#trace = trace
    this.#log = log
    this.#deliveries = new Deliveries({
      ledger,
      host,
      adapters,
      arrivalTimeoutMs,
      launchTimeoutMs,
      maxAttempts,
      traceWindow: (runtime, kind, details) => this.#traceWindow(runtime, kind, details),
      retire: (runtime) => this.#windows.retire(runtime),
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
    this.#transcripts = new Transcripts({ ledger, changed: () => this.#changed() })
    this.#windows = new Windows({
      ledger,
      host,
      adapters,
      credentials,
      paneEnv,
      roster,
      roles,
      launchFiles,
      deliveries: this.#deliveries,
      scheduler: this.#scheduler,
      leadFirst: (project, chief, conversation, message) =>
        this.#lead.leadFirst(project, chief, conversation, message),
      leadWithoutAgent: (project, chief, delivering) =>
        this.#lead.leadWithoutAgent(project, chief, delivering),
      paneExited: (pane) => this.paneExited(pane),
      traceWindow: (runtime, kind, details) => this.#traceWindow(runtime, kind, details),
      forgotten: (runtime) => this.#forgotten(runtime),
      now: () => this.#now(),
      changed: () => this.#changed(),
    })
    this.#lead = new LeadSwitch({
      ledger,
      windows: this.#windows,
      deliveries: this.#deliveries,
      transcripts: this.#transcripts,
      act: (runtime, work) => this.#act(runtime, work),
      forgotten: (runtime) => this.#forgotten(runtime),
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

  /** Refuses a harness ConsensFlow has no adapter for (`windows.js`). */
  requireAdapter(harness) {
    this.#windows.requireAdapter(harness)
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
            if (runtime.window.pane !== null)
              await this.#windows.closeOwn(runtime, runtime.window.pane)
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
    else await this.#windows.retire(runtime)
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
        if (runtime.window.pane !== null) await this.#windows.retire(runtime)
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
    if (runtime.window.pane !== null) await this.#windows.retire(runtime)
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
    const delivering = runtime.delivery.delivering
    this.#windows.closed(runtime)
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
      this.#act(runtime, () => this.#windows.launch(runtime, project, participant, next))
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
      observed = await this.#windows.observe(participant, runtime)
    } catch (cause) {
      if (!this.#forgotten(runtime)) {
        this.#windows.setActivity(runtime, { state: 'unknown', reason: cause.message })
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
    const drawing =
      (observed.items.length === 0 || unnamed) && !(await this.#windows.drawn(runtime))
    if (this.#forgotten(runtime)) return
    const starting = drawing || (unnamed && !runtime.window.named)
    // An out member's window says so, and nothing else, until the reset.
    if (!this.#scheduler.isOut(owner)) {
      this.#windows.setActivity(
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
    this.#transcripts.copy(participant, runtime, observed)
    // The human switched the window to another conversation: this look was
    // the old one's last, and nothing is delivered on it.
    if (observed.switched !== undefined) {
      if (runtime.delivery.delivering !== null) this.#deliveries.confirmArrival(runtime, observed)
      this.#transcripts.follow(participant, runtime, observed.switched.nativeSession)
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
        await this.#lead.performSwitch(project, participant, runtime, runtime.pendingSwitch)
        return
      }
      this.#windows.setActivity(runtime, {
        state: 'out',
        reason: `out of quota until ${owner.outUntil}`,
      })
      // A held task's agent stops, as any paused task's; the window waits for
      // the reset, unless its task is gone meanwhile (cancelled, taken back).
      if (participant.role !== 'chief') {
        await this.#windows.interruptIfStopped(participant, runtime, observed)
        if (!this.#forgotten(runtime)) await this.#windows.closeIfFree(runtime)
      }
      return
    }
    if (runtime.delivery.delivering !== null) await this.#deliveries.watchArrival(runtime, observed)
    // A window that began to close in this step (a launch that timed out) is not acted on.
    if (runtime.window.retiring) return
    if (participant.role !== 'chief') {
      await this.#windows.interruptIfStopped(participant, runtime, observed)
      if (this.#forgotten(runtime)) return
      this.#deliveries.collect(project, participant, observed)
      if (await this.#windows.closeIfFree(runtime)) return
    }
    const idle =
      runtime.delivery.delivering === null && observed.settled && !observed.waiting && !drawing
    if (runtime.pendingSwitch !== null) {
      await this.#lead.awaitSwitch(project, participant, runtime, observed, idle)
      return
    }
    if (idle) {
      const next = this.#ledger.nextDelivery(participant.id)
      if (next !== null) this.#act(runtime, () => this.#deliveries.deliver(runtime, next))
    }
  }

  /**
   * The human's Switch lead: the chief goes on in a fresh window on the
   * saved `agent` (its model and effort) on `harness`, and the window's
   * first message hands it the lead (`lead-switch.js`).
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
          this.#lead.afterTurn(projectId, runtime, { harness, agent, note })
          return
        }
        await this.#lead.performSwitch(project, chief, runtime, { harness, agent })
        // One deleted while the old window was looked at or closed stopped the switch there.
        if (this.#forgotten(runtime)) throw new Error(`no project ${projectId}`)
      },
      { wait: true },
    )
    return this.#ledger.project(projectId)
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
    this.#windows.setActivity(runtime, { state: 'out', reason: `out of quota until ${until}` })
    this.#ledger.markOut(owner.id, { until, reason: 'out of quota' })
    if (runtime.delivery.delivering !== null) {
      const { delivering } = runtime.delivery
      runtime.delivery.delivering = null
      this.#deliveries.settleFailure(delivering, 'the harness ran out of quota', { retry: true })
    }
    this.#scheduler.holdOrRelease(project, participant, owner, until)
    if (participant.role !== 'chief') await this.#windows.closeIfFree(runtime)
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
        this.#act(runtime, () => this.#windows.launch(runtime, project, participant, null))
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

  #now() {
    return this.#clock.now().getTime()
  }

  #changed() {
    for (const listener of this.#listeners) listener()
  }
}
