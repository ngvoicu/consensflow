import { join } from 'node:path'

import { harnessPath } from './harnesses.js'
import { preparePrivateIntegration } from './private-integration.js'

const FILES = [
  'hosts/pi-extension/consensflow-delivery.mjs',
  'hosts/lib/receiver.js',
  'package.json',
]

export function preparePiExtension(env) {
  if (!harnessPath('pi', env)) return { state: 'not-installed', path: null }
  try {
    const root = preparePrivateIntegration(env, 'pi', FILES)
    return { state: 'installed-unverified', path: join(root, FILES[0]) }
  } catch (error) {
    return { state: 'error', path: null, reason: error.message }
  }
}
