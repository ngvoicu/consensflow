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
  const settings = join(dirname(signal), 'settings.json')
  await writeFile(settings, JSON.stringify({ hooks }), { mode: 0o600 })
  return { args: ['--settings', settings], env: { CF_RESULT_SIGNAL: signal } }
}
