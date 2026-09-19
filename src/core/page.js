import { basename } from 'node:path'
import { agentRow, listAgents } from '../roster.js'

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

    'project.open': change(async ({ directory, name, harness }) => ({
      project: await dispatcher.openProject({
        directory,
        name: name ?? basename(directory),
        harness,
        team: lastTeamNow(ledger, env),
      }),
    })),

    'project.resume': change(async ({ project }) => ({
      project: await dispatcher.resumeProject(project),
    })),

    'agents.list': async () => ({ agents: listAgents(env) }),

    'member.add': change(async ({ project, agent, role = 'worker' }) => {
      const row = agentRow(agent, env)
      if (!row) throw new Error(`no agent named ${agent} in your agents`)
      return { member: ledger.addMember(project, { agent, harness: row.kind, role }) }
    }),

    'member.remove': change(async ({ project, agent }) => dispatcher.removeMember(project, agent)),

    'pm.add': change(async ({ project, harness }) => ({
      member: ledger.addPm(project, { harness }),
    })),

    'board.get': async ({ project }) => {
      const board = ledger.board(project)
      return {
        board: {
          ...board,
          lanes: board.lanes.map((lane) => ({
            ...lane,
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

    'task.add': change(async ({ project, to, body, title }) =>
      ledger.createTask(project, { from: 'human', to, body, title }),
    ),

    'task.accept': change(async ({ project, task }) => ({
      task: ledger.acceptTask(project, task, { by: 'human' }),
    })),

    'task.reopen': change(async ({ project, task, body }) =>
      ledger.reopenTask(project, task, { by: 'human', body }),
    ),

    'task.cancel': change(async ({ project, task }) => ({
      task: ledger.cancelTask(project, task, { by: 'human' }),
    })),

    'message.read': change(async ({ message }) => ({ message: ledger.markRead(message) })),

    'message.answer': change(async ({ question, body }) => ({
      message: ledger.answer(question, { from: 'human', body }),
    })),
  }
}

/** The last project's team for a new one: members whose agents are still saved, on their harness now. */
function lastTeamNow(ledger, env) {
  return ledger.lastTeam().flatMap(({ agent, role }) => {
    const row = agentRow(agent, env)
    return row === undefined ? [] : [{ agent, harness: row.kind, role }]
  })
}
