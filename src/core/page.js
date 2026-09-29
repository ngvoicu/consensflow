import { basename } from 'node:path'
import { missingHarnesses, offerable } from '../harnesses.js'
import { RESUME_WORDS } from '../ledger/index.js'
import { agentRow, listAgents } from '../roster.js'

/**
 * What the board page may ask of the new core: each operation is the human
 * acting on the ledger or the dispatcher. The Rust app forwards exactly these
 * names (`core_request` in `app/src-tauri/src/commands.rs`), and every change
 * wakes the dispatcher so it happens in the panes at once.
 */
/** What a paused task's window is told when the human resumes it. */
export { RESUME_WORDS } from '../ledger/index.js'

export function pageOperations({ ledger, dispatcher, env, kick }) {
  const change = (work) => async (body) => {
    const value = await work(body)
    kick()
    return value
  }
  return {
    'projects.list': async () => ({ projects: ledger.projects() }),

    // The staff given, as the roster has those agents now; else the last staff.
    'project.open': change(async ({ directory, name, harness, gate, staff }) => ({
      project: await dispatcher.openProject({
        directory,
        name: name ?? basename(directory),
        harness,
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

    'member.add': change(async ({ project, agent, roles = ['worker'] }) => ({
      member: ledger.addMember(project, { roles, ...membership(agent, env) }),
    })),

    'member.roles': change(async ({ project, agent, roles }) => ({
      member: ledger.setRoles(project, agent, roles),
    })),

    'session.open': change(async ({ project, handle }) => ({
      project: await dispatcher.openWindow(project, handle),
    })),

    'session.close': change(async ({ project, handle }) => ({
      project: await dispatcher.closeWindow(project, handle),
    })),

    'session.end': change(async ({ project, handle }) => ({
      project: await dispatcher.endSession(project, handle),
    })),

    'member.remove': change(async ({ project, agent }) => dispatcher.removeMember(project, agent)),

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
              agentRow(lane.participant.agent, env) === undefined,
            activity: dispatcher.activity(lane.participant.id),
            pane: dispatcher.pane(lane.participant.id),
          })),
        },
      }
    },

    'inbox.get': async ({ project, participant = 'human' }) => {
      const owner = ledger.project(project)?.participants.find((p) => p.handle === participant)
      if (owner === undefined) throw new Error(`${participant} is not in project ${project}`)
      return { messages: ledger.inbox(owner.id) }
    },

    'task.get': async ({ project, task }) => {
      const found = ledger.task(project, task)
      if (found === null) throw new Error(`no task T-${task} in this project`)
      return { task: found }
    },

    'task.transcript': async ({ project, task, limit }) =>
      ledger.transcript(project, task, limit === undefined ? {} : { limit }),

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

    'message.approve': change(async ({ message }) => ({
      message: ledger.approveMessage(message, { by: 'human' }),
    })),

    'message.decline': change(async ({ message }) => ({
      message: ledger.declineMessage(message, { by: 'human' }),
    })),
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

/** The last project's staff for a new one: the members still saved, as the roster has them now. */
function lastStaffNow(ledger, env) {
  const agents = listAgents(env)
  return ledger
    .lastStaff()
    .filter(({ agent }) => agents.some((candidate) => candidate.name === agent))
    .map(({ agent, roles }) => ({ roles, ...membership(agent, env, agents) }))
}
