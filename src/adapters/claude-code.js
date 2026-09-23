import { randomUUID } from 'node:crypto'
import fs from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { cachedAnswers, hasTranscript } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { send as sendPeer } from '../channels/claude-peer.js'
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
 * - A message is pasted into a live window as if the human typed it, once no
 *   human is typing there. Claude's own peer inbox (`peer: true`, macOS only,
 *   the Rust host checks the peer belongs to this pane) delivers while the
 *   human types, but Claude wraps each such message as a teammate's request
 *   from another Claude session, a hundred tokens of caution per delivery
 *   that misnames the human's own answers; since 2026-09-22 it is off.
 * - Claude's own `sessions/<pid>.json` says busy, idle or waiting (and why);
 *   the transcript holds the conversation and says whether the turn settled.
 */
/** How long a human draft holds pastes after the last keystroke behind it. */
export const DRAFT_GRACE_MS = 120_000

export function claudeCodeAdapter({
  env,
  peer = false,
  answers = cachedAnswers(),
  now = Date.now,
}) {
  // The latched drafts seen per window: the input epoch and since when.
  const drafts = new Map()
  const configDir = path.resolve(
    env.CLAUDE_CONFIG_DIR ?? path.join(env.HOME ?? homedir(), '.claude'),
  )
  return {
    harness: 'claude-code',

    async prepare({ launchId, role, directory, resume, message, agent, instructions }) {
      const executable = executableFor('claude-code', env)
      const settings = await prepareClaudeSettings(env, launchId)
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
        argv: [executable, ...settings, ...roleSetup.args, ...runner.args],
        env: { ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession,
        launch: {
          nativeSession,
          channel: {
            kind: 'claude-peer',
            preservesDraft: 1,
            launchId,
            configDir,
            ackTimeoutMs: 3000,
          },
        },
      }
    },

    async started() {
      return {}
    },

    /**
     * Pasting waits for the human to finish typing; the peer inbox never
     * touches their draft. The pane latches on any keystroke and only a clear
     * lets go, so a latch nobody types behind any more (a stray key, or a
     * submission the record never showed) would hold every message for good:
     * after the grace it is let go and the paste goes ahead.
     */
    async ready({ pane, host }) {
      if (peer) return true
      const snapshot = await host.request('pane.snapshot', pane)
      if (snapshot?.ok !== true || snapshot.pasteInFlight) return false
      const key = `${pane.id}#${pane.generation}`
      if (snapshot.draftLatched !== true) {
        drafts.delete(key)
        return true
      }
      const seen = drafts.get(key)
      if (seen === undefined || seen.epoch !== snapshot.inputEpoch) {
        drafts.set(key, { epoch: snapshot.inputEpoch, since: now() })
        return false
      }
      if (now() - seen.since < DRAFT_GRACE_MS) return false
      const cleared = await host.request('draft.clear', {
        ...pane,
        epoch: snapshot.inputEpoch,
        submission: `stale-${snapshot.inputEpoch}`,
      })
      if (cleared?.ok !== true || cleared.outcome !== 'cleared') return false
      drafts.delete(key)
      return true
    },

    async deliver({ launch, pane, host, text }) {
      const snapshot = await host.request('pane.snapshot', pane)
      if (snapshot?.ok !== true) {
        return {
          admitted: false,
          reason: `the window cannot be read: ${snapshot?.error ?? 'no answer'}`,
        }
      }
      if (peer) {
        const sent = await sendPeer(
          {
            session: launch.nativeSession,
            pane: pane.id,
            generation: pane.generation,
            epoch: snapshot.inputEpoch,
            launch: launch.channel,
            bridge: host,
          },
          text,
        )
        if (sent?.ok === true) return { admitted: true, queued: true }
        if (sent?.admitted === null) return { admitted: null, reason: sent.cause ?? sent.error }
        // An inbox Claude has not registered yet (or never will, on an older
        // build) is not a reason to drop the message: the terminal still works.
        if (sent?.error !== 'native-session-unavailable') {
          return { admitted: false, reason: sent?.cause ?? sent?.error ?? 'the peer refused it' }
        }
        if (snapshot.draftLatched === true) {
          return { admitted: false, reason: 'a human is typing in the window' }
        }
      }
      const written = await host.request('pane.write_paste', {
        ...pane,
        epoch: snapshot.inputEpoch,
        body: text,
      })
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
      // A new window has no transcript until its first message; Claude's own
      // status is then the only word on it.
      const settled =
        live === undefined
          ? transcriptSettled
          : live.state === 'idle' && (transcriptSettled || items.length === 0)
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
 * `sessions/<pid>.json` files it keeps (the same files peer delivery reads):
 * busy, idle, or waiting with the reason (a permission prompt, input needed, a
 * dialog). A file whose process is gone is ignored.
 */
async function claudeStatuses(env) {
  const directory = path.join(
    env.CLAUDE_CONFIG_DIR ?? path.join(env.HOME ?? '', '.claude'),
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
