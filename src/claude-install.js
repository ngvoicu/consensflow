import { mkdir, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { preparePrivateIntegration } from './private-integration.js'
import { configRoot } from './roster.js'

const FILES = ['hosts/claude-receiver.mjs', 'hosts/lib/receiver.js', 'package.json']
const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`

export function receiverSignal(env, launch) {
  if (!/^[A-Za-z0-9_-]{1,200}$/.test(launch)) throw new Error('invalid receiver launch')
  return join(configRoot(env), 'receivers', launch, 'signal')
}

/**
 * The settings file every Claude pane launches with, one per launch under the
 * home (like Devin's and Pi's integrations). Each turn ends in a Stop hook, so
 * Claude records `stop_hook_summary` for every finished turn: its own
 * `turn_duration` record is missing on some turns (every Calliope turn on
 * 2.1.274), and those answers never counted as done. A coordinator's receiver
 * hooks join the same file.
 */
export async function prepareClaudeSettings(env, launch, hooks = {}) {
  // The channel's filename-safe rule, minus the names that leave the folder.
  if (!/^(?!\.{1,2}$)[A-Za-z0-9._-]{1,200}$/.test(launch)) throw new Error('invalid Claude launch')
  const root = join(configRoot(env), 'integrations', 'claude', launch)
  await mkdir(root, { recursive: true, mode: 0o700 })
  const turnEnd = { hooks: [{ type: 'command', command: 'exit 0' }] }
  const settings = join(root, 'settings.json')
  await writeFile(
    settings,
    JSON.stringify({
      // Full permission (runners.js passes the flag) without its one-time
      // acceptance dialog, which no one could answer in a host-started pane.
      permissions: { defaultMode: 'bypassPermissions' },
      skipDangerousModePermissionPrompt: true,
      // A bypass-mode session holds messages from other sessions for approval
      // and drops them after five minutes; ConsensFlow's own messages must land.
      crossSessionInbound: 'accept',
      hooks: { ...hooks, Stop: [...(hooks.Stop ?? []), turnEnd] },
    }),
    { mode: 0o600 },
  )
  return ['--settings', settings]
}

/** Bundled hooks are loaded only in this coordinator process, never global settings. */
export async function prepareClaudeReceiver(env, launch, node) {
  const signal = receiverSignal(env, launch)
  const destination = preparePrivateIntegration(env, 'claude', FILES)
  await mkdir(dirname(signal), { recursive: true, mode: 0o700 })
  await writeFile(signal, '', { flag: 'wx', mode: 0o600 }).catch((error) => {
    if (error.code !== 'EEXIST') throw error
  })
  const command = `${quote(node)} ${quote(join(destination, FILES[0]))}`
  const hooks = Object.fromEntries(
    ['SessionStart', 'UserPromptSubmit', 'FileChanged', 'Stop', 'SessionEnd'].map((name) => [
      name,
      [
        {
          hooks: [
            {
              type: 'command',
              command,
              timeout: 30,
              ...(['FileChanged', 'Stop'].includes(name) ? { asyncRewake: true } : {}),
            },
          ],
        },
      ],
    ]),
  )
  return { hooks, env: { CF_RESULT_SIGNAL: signal } }
}
