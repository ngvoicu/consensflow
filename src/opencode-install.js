import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { harnessPath } from './harnesses.js'
import { preparePrivateIntegration } from './private-integration.js'

const FILES = ['hosts/opencode-extension/consensflow-session.mjs', 'hosts/lib/receiver.js']

export function prepareOpenCodeExtension(env) {
  if (!harnessPath('opencode', env)) return { state: 'not-installed', path: null }
  try {
    const root = preparePrivateIntegration(env, 'opencode', FILES, (destination) => [
      [
        'hosts/opencode-extension/tui.json',
        Buffer.from(JSON.stringify({ plugin: [pathToFileURL(join(destination, FILES[0])).href] })),
      ],
    ])
    return { state: 'installed-unverified', path: join(root, FILES[0]) }
  } catch (error) {
    return { state: 'error', path: null, reason: error.message }
  }
}
