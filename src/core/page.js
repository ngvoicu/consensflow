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
    'sessions.list': async () => ({ sessions: ledger.sessions() }),

    'session.open': change(async ({ directory, name, harness }) => ({
      session: await dispatcher.openSession({
        directory,
        name: name ?? basename(directory),
        harness,
      }),
    })),

    'session.resume': change(async ({ session }) => ({
      session: await dispatcher.resumeSession(session),
    })),

    'agents.list': async () => ({ agents: listAgents(env) }),

    'member.add': change(async ({ session, agent, role = 'worker' }) => {
      const row = agentRow(agent, env)
      if (!row) throw new Error(`no agent named ${agent} in your agents`)
      return { member: ledger.addMember(session, { agent, harness: row.kind, role }) }
    }),

    'board.get': async ({ session }) => {
      const board = ledger.board(session)
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

    'inbox.get': async ({ session, participant = 'human' }) => {
      const owner = ledger.session(session)?.participants.find((p) => p.handle === participant)
      if (owner === undefined) throw new Error(`${participant} is not in session ${session}`)
      return { messages: ledger.inbox(owner.id) }
    },

    'task.get': async ({ session, task }) => {
      const found = ledger.task(session, task)
      if (found === null) throw new Error(`no task T-${task} in this session`)
      return { task: found }
    },

    'task.add': change(async ({ session, to, body, title }) =>
      ledger.createTask(session, { from: 'human', to, body, title }),
    ),

    'task.accept': change(async ({ session, task }) => ({
      task: ledger.acceptTask(session, task, { by: 'human' }),
    })),

    'task.reopen': change(async ({ session, task, body }) =>
      ledger.reopenTask(session, task, { by: 'human', body }),
    ),

    'task.cancel': change(async ({ session, task }) => ({
      task: ledger.cancelTask(session, task, { by: 'human' }),
    })),

    'message.read': change(async ({ message }) => ({ message: ledger.markRead(message) })),

    'message.answer': change(async ({ question, body }) => ({
      message: ledger.answer(question, { from: 'human', body }),
    })),
  }
}
