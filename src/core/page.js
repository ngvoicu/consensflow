import { basename } from 'node:path'
import { missingHarnesses, offerable } from '../harnesses.js'
import { fitsRole, RESUME_WORDS } from '../ledger/index.js'
import { agentRow, harnessForKind, listAgents } from '../roster.js'
import { teamTable } from '../skill.js'
import { requireChiefAgent, requireOpen } from './dispatcher.js'
import { staffOf } from './roles.js'

/**
 * What the board page may ask of the daemon: each operation is the human
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

    // The chief on the saved agent given; the staff given, as the roster has
    // those agents now, else the last staff.
    'project.open': change(async ({ directory, name, agent, gate, staff }) => ({
      project: await dispatcher.openProject({
        directory,
        name: name ?? basename(directory),
        chief: chiefOn(agent, env),
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

    // The human's Switch chief: a saved agent (its harness, model and effort)
    // on a harness installed here. `when: 'turn'` lets a chief at work finish
    // its turn; `note` first asks it where things stand.
    'chief.switch': change(async ({ project, agent, when = 'now', note = false }) => {
      if (!['now', 'turn'].includes(when)) throw new Error('when is now or turn')
      const target = chiefOn(agent, env)
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
      tellChiefOfStaff(ledger, project)
      return { member: added }
    }),

    'member.roles': change(async ({ project, agent, roles }) => {
      const member = ledger.setRoles(project, agent, roles)
      tellChiefOfStaff(ledger, project)
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
      tellChiefOfStaff(ledger, project)
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
            // A Switch chief that waits for the chief's turn to end.
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

    // Back to the board for another member of its tier, once its window is stopped.
    'task.reassign': change(async ({ project, task }) => dispatcher.reassignTask(project, task)),

    // The human resumes without writing to the agent: the words are always these.
    'task.resume': change(async ({ project, task }) =>
      ledger.resumeTask(project, task, { by: 'human', body: RESUME_WORDS }),
    ),

    // Finished tasks off the board for good, the ones the human confirmed or
    // none; nobody is told, and cf task get still reads each.
    'tasks.delete': change(async ({ project, tasks }) => ({
      tasks: ledger.deleteTasks(project, tasks),
    })),

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

/**
 * A saved agent as the staff records it: its harness, whether it is an image
 * agent, and its tier as the roster has it now.
 */
function membership(agent, env, agents = listAgents(env)) {
  const row = agentRow(agent, env)
  const saved = agents.find((candidate) => candidate.name === agent)
  if (row === undefined || saved === undefined) {
    throw new Error(`no agent named ${agent} in your agents`)
  }
  return {
    agent,
    harness: row.kind,
    designer: row.designer === true,
    tier: saved.profile.workTier,
  }
}

/**
 * The chief hears of a change to the staff while its window runs, launched as
 * it was knowing the staff then: one note, which a later change replaces
 * while it still waits. A chief not yet started reads the staff at launch.
 */
function tellChiefOfStaff(ledger, projectId) {
  const project = ledger.project(projectId)
  const chief = project.participants.find((participant) => participant.handle === 'chief')
  if (chief === undefined || ledger.currentConversation(chief.id) === null) return
  ledger.freshNote(projectId, {
    to: 'chief',
    heading: 'The human changed the staff; it is now:',
    body: `\n\n${teamTable(staffOf(project))}`,
  })
}

/** A chief as the dispatcher takes it: the saved agent named, and the harness it runs on. */
function chiefOn(agent, env) {
  const { harness } = membership(requireChiefAgent(agent), env)
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
      return { ...member, roles: roles.filter((role) => fitsRole(member.designer, role)) }
    })
    .filter(({ roles }) => roles.length > 0)
}
