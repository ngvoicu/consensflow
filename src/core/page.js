import { basename } from 'node:path'
import { missingHarnesses, offerable } from '../harnesses.js'
import { fitsRole, RESUME_WORDS } from '../ledger/index.js'
import { agentRow, harnessForKind, listAgents } from '../roster.js'
import { teamTable } from '../skill.js'
import { requireLeadAgent, requireOpen } from './dispatcher.js'
import { staffOf } from './roles.js'

/**
 * What the board page may ask of the new core: each operation is the human
 * acting on the ledger or the dispatcher. The Rust app forwards exactly these
 * names (`core_request` in `app/src-tauri/src/commands.rs`), and every change
 * wakes the dispatcher so it happens in the panes at once.
 */
export function pageOperations({ ledger, dispatcher, env, kick }) {
  const change = (work) => async (body) => {
    const value = await work(body)
    kick()
    return value
  }
  return {
    'projects.list': async () => ({ projects: ledger.projects() }),

    // The lead on the saved agent given; the staff given, as the roster has
    // those agents now, else the last staff.
    'project.open': change(async ({ directory, name, agent, gate, staff }) => ({
      project: await dispatcher.openProject({
        directory,
        name: name ?? basename(directory),
        chief: lead(agent, env),
        gate,
        staff:
          staff === undefined
            ? lastStaffNow(ledger, env)
            : staff.map(({ agent, roles }) => ({ roles, ...membership(agent, env) })),
      }),
    })),

    'project.resume': change(async ({ project }) => ({
      project: await dispatcher.resumeProject(project),
    })),

    'project.close': change(async ({ project }) => ({
      project: await dispatcher.closeProject(project),
    })),

    'project.delete': change(async ({ project }) => ({
      project: await dispatcher.deleteProject(project),
    })),

    // The pickers offer only agents on a harness installed here, and say which are not.
    'agents.list': async () => {
      const missing = missingHarnesses(env)
      return { agents: offerable(listAgents(env), missing), missing }
    },

    'staff.last': async () => ({ staff: lastStaffNow(ledger, env) }),

    // The human's Switch lead: a saved agent (its harness, model and effort)
    // on a harness installed here. `when: 'turn'` lets a lead at work finish
    // its turn; `note` first asks it where things stand.
    'chief.switch': change(async ({ project, agent, when = 'now', note = false }) => {
      if (!['now', 'turn'].includes(when)) throw new Error('when is now or turn')
      const target = lead(agent, env)
      if (missingHarnesses(env).includes(harnessForKind(target.harness))) {
        throw new Error(`${target.harness} is not installed here`)
      }
      return {
        project: await dispatcher.switchChief(project, { ...target, when, note: note === true }),
      }
    }),

    'member.add': change(async ({ project, agent, roles = ['worker'] }) => {
      const member = membership(agent, env)
      dispatcher.requireAdapter(member.harness)
      const added = ledger.addMember(project, { roles, ...member })
      tellLeadOfStaff(ledger, project)
      return { member: added }
    }),

    'member.roles': change(async ({ project, agent, roles }) => {
      const member = ledger.setRoles(project, agent, roles)
      tellLeadOfStaff(ledger, project)
      return { member }
    }),

    'session.open': change(async ({ project, handle }) => ({
      project: await dispatcher.openWindow(project, handle),
    })),

    'session.close': change(async ({ project, handle }) => ({
      project: await dispatcher.closeWindow(project, handle),
    })),

    'session.end': change(async ({ project, handle }) => ({
      project: await dispatcher.endSession(project, handle),
    })),

    'member.remove': change(async ({ project, agent }) => {
      const removed = await dispatcher.removeMember(project, agent)
      tellLeadOfStaff(ledger, project)
      return removed
    }),

    'board.get': async ({ project }) => {
      const board = ledger.board(project)
      return {
        board: {
          ...board,
          lanes: board.lanes.map((lane) => ({
            ...lane,
            // A member whose agent is gone (a release dropped the entry, or
            // the human removed their own) sits, and the board says why.
            agentMissing:
              lane.participant.agent !== null &&
              lane.participant.member === null &&
              agentGone(lane.participant.agent, env),
            activity: dispatcher.activity(lane.participant.id),
            pane: dispatcher.pane(lane.participant.id),
            // A Switch lead that waits for the lead's turn to end.
            switching: dispatcher.pendingSwitch(lane.participant.id),
          })),
        },
      }
    },

    // As much as one frame carries, newest first, and how many there are;
    // `unread` is what For you lists: the human's notes not yet read.
    'inbox.get': async ({ project, participant = 'human', unread = false }) => {
      const owner = ledger.project(project)?.participants.find((p) => p.handle === participant)
      if (owner === undefined) throw new Error(`${participant} is not in project ${project}`)
      return ledger.latestMessages(owner.id, { unread: unread === true })
    },

    // As much of the task as one frame carries, what was cut marked.
    'task.get': async ({ project, task }) => {
      const found = ledger.taskThatFits(project, task)
      if (found === null) throw new Error(`no task T-${task} in this project`)
      return { task: found }
    },

    'task.transcript': async ({ project, task, limit }) =>
      ledger.latestTranscript(project, task, limit === undefined ? {} : { limit }),

    'project.gate': change(async ({ project, gate }) => ({
      project: ledger.setGate(project, gate),
    })),

    'task.cancel': change(async ({ project, task }) => ({
      task: ledger.cancelTask(project, task, { by: 'human' }),
    })),

    'task.pause': change(async ({ project, task }) => ({
      task: ledger.pauseTask(project, task, { by: 'human' }),
    })),

    // Back to the board for another member of its tier; the old window closes
    // on the daemon's next look, unless the human opened it.
    'task.reassign': change(async ({ project, task }) =>
      ledger.releaseTask(project, task, { because: 'by @human' }),
    ),

    // The human resumes without writing to the agent: the words are always these.
    'task.resume': change(async ({ project, task }) =>
      ledger.resumeTask(project, task, { by: 'human', body: RESUME_WORDS }),
    ),

    'message.read': change(async ({ message }) => ({ message: ledger.markRead(message) })),

    'message.approve': change(async ({ message }) => {
      const waiting = ledger.message(message)
      if (waiting !== null) requireOpen(ledger.project(waiting.projectId))
      return { message: ledger.approveMessage(message, { by: 'human' }) }
    }),

    'message.decline': change(async ({ message }) => ({
      message: ledger.declineMessage(message, { by: 'human' }),
    })),
  }
}

/**
 * Whether a saved agent is gone. While the agents file cannot be read, an
 * agent is unknown, not gone: the board still loads, and the Agents page
 * says what to fix.
 */
function agentGone(agent, env) {
  try {
    return agentRow(agent, env) === undefined
  } catch {
    return false
  }
}

/** A saved agent as the staff records it: its harness, and its tier as the roster has it now. */
function membership(agent, env, agents = listAgents(env)) {
  const row = agentRow(agent, env)
  const saved = agents.find((candidate) => candidate.name === agent)
  if (row === undefined || saved === undefined) {
    throw new Error(`no agent named ${agent} in your agents`)
  }
  return { agent, harness: row.kind, tier: saved.profile.workTier }
}

/** A lead as the dispatcher takes it: the saved agent named, and the harness it runs on. */
/**
 * The lead hears of a change to the staff while its window runs, launched as
 * it was knowing the staff then: one note, which a later change replaces
 * while it still waits. A lead not yet started reads the staff at launch.
 */
function tellLeadOfStaff(ledger, projectId) {
  const project = ledger.project(projectId)
  const lead = project.participants.find((participant) => participant.handle === 'chief')
  if (lead === undefined || ledger.currentConversation(lead.id) === null) return
  ledger.freshNote(projectId, {
    to: 'chief',
    heading: 'The human changed the staff; it is now:',
    body: `\n\n${teamTable(staffOf(project))}`,
  })
}

function lead(agent, env) {
  const { harness } = membership(requireLeadAgent(agent), env)
  return { harness, agent }
}

/**
 * The last project's staff for a new one: the members still saved, as the
 * roster has them now, in the roles their agents fit. A role one held from
 * before an image designer had to be an image agent stays behind, and so
 * does a member left with none.
 */
function lastStaffNow(ledger, env) {
  const agents = listAgents(env)
  return ledger
    .lastStaff()
    .filter(({ agent }) => agents.some((candidate) => candidate.name === agent))
    .map(({ agent, roles }) => {
      const member = membership(agent, env, agents)
      return { ...member, roles: roles.filter((role) => fitsRole(member.harness, role)) }
    })
    .filter(({ roles }) => roles.length > 0)
}
