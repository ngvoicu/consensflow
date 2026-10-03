import { RESUME_WORDS } from '../ledger/index.js'

/** A reset this near holds a task with its window rather than sending it back to the board. */
const HOLD_MS = 30 * 60_000

/** How long a quota lasts when its harness names no reset. */
const UNKNOWN_RESET_MS = 60 * 60_000

/**
 * Scheduling, for the dispatcher (`dispatcher.js`): decisions over the
 * ledger's data about who works. Each pass gives every open task to the
 * best free member of its tier, and the requester hears once when none is
 * free; a member out of quota keeps its tiered work or gives it back by how
 * near its reset is; a held task goes on when its time comes; and a member
 * whose saved agent is gone gives its tiered work back. The ledger says
 * what may happen to a task; this says which, and to whom.
 *
 * It keeps the quota part of a participant's record (`runtime.quota`):
 * what its window's harness last said of its quota and, on a member's own
 * record, until when it is low.
 */
export class Scheduler {
  #ledger
  #adapters
  #roster
  /** Settles what was on its way to a member whose agent is gone. */
  #deliveries
  /** The dispatcher's record of a participant, made the first time it is asked for. */
  #runtimeOf
  /** The dispatcher's record of a participant when it has one; asking makes none. */
  #runtime
  #now
  #changed
  /** The open tasks whose requester heard that they wait for a free member, each with its project. */
  #waitingNoted = new Map()

  constructor({ ledger, adapters, roster, deliveries, runtimeOf, runtime, now, changed }) {
    this.#ledger = ledger
    this.#adapters = adapters
    this.#roster = roster
    this.#deliveries = deliveries
    this.#runtimeOf = runtimeOf
    this.#runtime = runtime
    this.#now = now
    this.#changed = changed
  }

  /** A deleted project's open tasks wait for nobody now: what its requesters heard is forgotten. */
  forgetProject(projectId) {
    for (const [task, noted] of this.#waitingNoted) {
      if (noted === projectId) this.#waitingNoted.delete(task)
    }
  }

  /**
   * Each open task goes to the best free member of its tier; the requester
   * hears once when none is. A task that needs others waits for them to be
   * accepted, in silence: its card says what it waits for. Says whether it
   * gave any task out.
   */
  assignOpenTasks(project) {
    let assigned = false
    for (const task of this.#ledger.openTasks(project.id)) {
      if (task.blockedBy.length > 0) continue
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
    if (this.isOut(member)) return `@${member.handle} is out of quota until ${member.outUntil}`
    if (this.#isLow(member)) return `@${member.handle} is low on quota`
    return null
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

  /** The member a session belongs to; a member or the chief is its own. */
  memberOf(project, participant) {
    if (participant.memberId === null) return participant
    return project.participants.find((p) => p.id === participant.memberId) ?? participant
  }

  #isLow(member) {
    const until = this.#runtime(member.id)?.quota.lowUntil ?? null
    return until !== null && Date.parse(until) > this.#now()
  }

  isOut(member) {
    return member.outUntil !== null && Date.parse(member.outUntil) > this.#now()
  }

  /**
   * A harness keeps its last record, so a refusal stays in view long after
   * its reset: only one dated after the member was last marked out is news.
   */
  freshRefusal(participant, quota) {
    if (quota?.state !== 'exhausted') return false
    // A refusal whose reset has passed is old news, whatever record still shows it.
    if (quota.resetsAt && Date.parse(quota.resetsAt) <= this.#now()) return false
    if (participant.outSince === null || !quota.at) return true
    return Date.parse(quota.at) > Date.parse(participant.outSince)
  }

  /** What a window's harness says of its quota, as of this look. */
  recordQuota(runtime, owner, quota) {
    runtime.quota.reported = quota ?? null
    // Low is soft: the current task continues, nothing new comes until the
    // reset it names (an hour when it names none). It outlives the window,
    // which closes with the task, so a low member is not asked again at once.
    if (runtime.quota.reported?.state === 'low') {
      this.#runtimeOf(owner.id).quota.lowUntil = this.resetOf(runtime.quota.reported)
    } else if (runtime.quota.reported !== null) this.#runtimeOf(owner.id).quota.lowUntil = null
  }

  /** When a quota resets: the time its harness names, or an hour from now. */
  resetOf(quota) {
    return quota.resetsAt ?? new Date(this.#now() + UNKNOWN_RESET_MS).toISOString()
  }

  /** The tiered work of a member out of quota until `until`, once it is marked out. */
  holdOrRelease(project, participant, owner, until) {
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
  }

  /** A held task whose time has come goes on in its own window, unless its member is still out. */
  resumeHeld() {
    for (const held of this.#ledger.heldTasksDue(new Date(this.#now()).toISOString())) {
      const project = this.#ledger.project(held.projectId)
      if (project === null || project.state !== 'open') continue
      const assignee = project.participants.find((p) => p.id === held.assigneeId)
      const owner = assignee === undefined ? null : this.memberOf(project, assignee)
      if (owner !== null && this.isOut(owner)) continue
      this.#ledger.resumeTask(held.projectId, held.number, { body: RESUME_WORDS })
      this.#changed()
    }
  }

  /**
   * A member whose agent is gone from the human's agents (a release dropped
   * the catalog entry, or the human removed one of their own) runs on no
   * default: its tiered work goes back to the board for another member, with
   * whatever was on its way to it withdrawn, and a request given to it by
   * name fails so the requester hears why. The board says why it sits.
   */
  withoutAgent(project, participant, delivering) {
    const because = `${participant.agent} is no longer among your agents`
    const tiered = this.#tieredWork(project, participant)
    for (const task of tiered) this.#ledger.releaseTask(project.id, task.number, { because })
    if (delivering !== null)
      this.#deliveries.settleFailure(
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

  /** A participant whose saved agent the human has since deleted. */
  agentGone(participant) {
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
}
