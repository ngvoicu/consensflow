import { mkdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { configRoot } from './roster.js'

const QUESTION_HOOK_SECONDS = 3600

/**
 * The settings file every Claude pane launches with, one per launch under the
 * home (like Devin's and Pi's integrations). Each turn ends in a Stop hook, so
 * Claude records `stop_hook_summary` for every finished turn: its own
 * `turn_duration` record is missing on some turns (every Calliope turn on
 * 2.1.274), and those answers never counted as done. A member's question tool
 * is answered from the board; the chief's shows Claude's own dialog, where the
 * human answers it.
 */
export async function prepareClaudeSettings(env, launch, { boardQuestions = true } = {}) {
  // The channel's filename-safe rule, minus the names that leave the folder.
  if (!/^(?!\.{1,2}$)[A-Za-z0-9._-]{1,200}$/.test(launch)) throw new Error('invalid Claude launch')
  const root = join(configRoot(env), 'integrations', 'claude', launch)
  await mkdir(root, { recursive: true, mode: 0o700 })
  const turnEnd = { hooks: [{ type: 'command', command: 'exit 0' }] }
  // Claude's question tool prompts even in full-permission mode. A member's
  // is answered from the board through this hook (`cf` is first on a pane's
  // PATH); when nobody answers within the hour, the hook is cancelled and the
  // window shows Claude's own dialog.
  const question = {
    matcher: 'AskUserQuestion',
    hooks: [{ type: 'command', command: 'cf hook claude', timeout: QUESTION_HOOK_SECONDS }],
  }
  const settings = join(root, 'settings.json')
  await writeFile(
    settings,
    JSON.stringify({
      // Full permission (windows.js passes the flag) without its one-time
      // acceptance dialog, which no one could answer in a host-started pane.
      permissions: { defaultMode: 'bypassPermissions' },
      skipDangerousModePermissionPrompt: true,
      // A bypass-mode session holds messages from other sessions for approval
      // and drops them after five minutes; ConsensFlow's own messages must land.
      crossSessionInbound: 'accept',
      // The classic renderer writes to the terminal's own scrollback, which the
      // dock scrolls; the fullscreen one draws on the alternate screen, which
      // has none.
      tui: 'default',
      hooks: {
        PreToolUse: boardQuestions ? [question] : [],
        Stop: [turnEnd],
      },
    }),
    { mode: 0o600 },
  )
  return ['--settings', settings]
}
