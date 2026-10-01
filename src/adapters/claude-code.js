import { randomUUID } from 'node:crypto'
import fs from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { cachedAnswers, hasTranscript } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { prepareClaudeSettings } from '../claude-install.js'
import { roleConfiguration } from '../role-skills.js'
import { executableFor } from './shared.js'

/**
 * Claude Code, for the new core (see `src/core/dispatcher.js` for the adapter
 * contract). Each launch gets its own settings file under the home: full
 * permission without the one-time dialog, messages from other sessions
 * accepted, and a Stop hook on every turn so every finished turn is recorded.
 *
 * - The session id is ours: minted for a fresh window, resumed for a known one.
 * - A message is pasted into a live window as if the human typed it, whatever
 *   its input box holds: text the human left unsent goes in with it (the
 *   owner's choice, 2026-10-01). Claude's own peer inbox would bypass the
 *   input box, but Claude wraps each such message as a teammate's request
 *   from another Claude session, a hundred tokens of caution per delivery
 *   that misnames the human's own answers, so it is not used (2026-09-22).
 * - Claude's own `sessions/<pid>.json` says busy, idle or waiting (and why);
 *   the transcript holds the conversation and says whether the turn settled.
 */
/**
 * A member runs in full-permission mode and reads what others wrote, so it
 * starts without the human's MCP servers, claude.ai connectors and Claude in
 * Chrome: an eval reviewer reached for the human's own browser, and this Mac's
 * setup includes a brokerage connector. The chief, which works with the human,
 * keeps them. ConsensFlow's own hooks come from --settings, not from MCP.
 */
const MEMBER_ISOLATION = ['--strict-mcp-config', '--no-chrome']

export function claudeCodeAdapter({ env, answers = cachedAnswers() }) {
  const configDir = path.resolve(
    env.CLAUDE_CONFIG_DIR ?? path.join(env.HOME ?? homedir(), '.claude'),
  )
  return {
    harness: 'claude-code',

    async prepare({ launchId, role, directory, resume, message, agent, instructions }) {
      const executable = executableFor('claude-code', env)
      const settings = await prepareClaudeSettings(env, launchId, {
        boardQuestions: role !== 'chief',
      })
      const roleSetup = await roleConfiguration('claude-code', {
        role,
        env,
        cwd: directory,
        executable,
        content: instructions,
      })
      const identity = { kind: 'claude-code', model: agent?.model, effort: agent?.effort }
      // Claude keeps a conversation only once something was said in it: a
      // window that closed before that (opened by hand, then lost to a
      // restart) has nothing to resume, and `--resume` would exit at once.
      // It starts afresh under the same id, so the conversation stays bound.
      const nativeSession = resume ?? randomUUID()
      const resumable = resume !== null && (await hasTranscript('claude-code', resume, env))
      const runner = resumable
        ? interactiveResume(identity, resume, message)
        : interactiveStart(identity, nativeSession, message)
      return {
        argv: [
          executable,
          ...settings,
          ...roleSetup.args,
          ...(role === 'chief' ? [] : MEMBER_ISOLATION),
          ...runner.args,
        ],
        env: { ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession,
        launch: { nativeSession },
      }
    },

    async started() {
      return {}
    },

    /** A paste waits only for the window to be readable and another paste to be in. */
    async ready({ pane, host }) {
      const snapshot = await host.request('pane.snapshot', pane)
      if (snapshot?.ok !== true)
        return `the window cannot be read: ${snapshot?.error ?? 'no answer'}`
      if (snapshot.pasteInFlight) return 'a paste is on its way to the window'
      return true
    },

    async deliver({ pane, host, text }) {
      const written = await host.request('pane.write_paste', { ...pane, body: text })
      return written?.ok === true
        ? { admitted: true }
        : { admitted: false, reason: written?.error ?? 'the window refused the paste' }
    },

    async observe({ launch }) {
      const [record, statuses] = await Promise.all([
        answers('claude-code', launch.nativeSession, env),
        claudeStatuses(env),
      ])
      const live = statuses.get(launch.nativeSession)
      const items = Array.isArray(record.items) ? record.items : []
      const transcriptSettled = record.settlement?.state === 'settled'
      // Claude's own status is the word on whether the window is at its
      // prompt: a new window has no transcript until its first message, and a
      // resumed one carries a transcript that settled before this window
      // opened, so a paste on its word alone lands before the prompt is up.
      const settled =
        live !== undefined && live.state === 'idle' && (transcriptSettled || items.length === 0)
      return {
        items,
        settled,
        waiting: live?.state === 'waiting' ? { reason: live.reason ?? null } : null,
        failed: record.failed === true,
        quota: record.quota ?? null,
      }
    },

    transcript({ launch }) {
      return { harness: 'claude-code', session: launch.nativeSession, configDir }
    },
  }
}

/**
 * Claude Code's own live status for each running session, from the
 * `sessions/<pid>.json` files it keeps: busy, idle, or waiting with the
 * reason (a permission prompt, input needed, a dialog). A file whose process
 * is gone is ignored.
 */
async function claudeStatuses(env) {
  const directory = path.join(
    env.CLAUDE_CONFIG_DIR ?? path.join(env.HOME ?? homedir(), '.claude'),
    'sessions',
  )
  const statuses = new Map()
  const names = await fs.readdir(directory).catch(() => [])
  for (const name of names) {
    if (!/^\d+\.json$/.test(name)) continue
    const row = await fs
      .readFile(path.join(directory, name), 'utf8')
      .then(JSON.parse)
      .catch(() => null)
    if (typeof row?.sessionId !== 'string' || !Number.isSafeInteger(row.pid) || !alive(row.pid))
      continue
    const state = { busy: 'working', waiting: 'waiting', idle: 'idle', shell: 'idle' }[row.status]
    if (!state) continue
    statuses.set(row.sessionId, {
      state,
      ...(state === 'waiting' && typeof row.waitingFor === 'string'
        ? { reason: row.waitingFor }
        : {}),
    })
  }
  return statuses
}

function alive(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return error.code === 'EPERM'
  }
}
