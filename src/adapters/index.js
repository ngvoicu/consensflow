import { claudeCodeAdapter } from './claude-code.js'
import { codexAdapter } from './codex.js'
import { devinAdapter } from './devin.js'
import { openCodeAdapter } from './opencode.js'
import { piAdapter } from './pi.js'

/**
 * One adapter per supported harness, keyed the way the ledger names them.
 * An image agent runs in Codex's own window, whose image tool draws on the
 * Codex login.
 */
export function createAdapters(env) {
  return {
    'claude-code': claudeCodeAdapter({ env }),
    codex: codexAdapter({ env }),
    image: codexAdapter({ env, harness: 'image' }),
    devin: devinAdapter({ env }),
    opencode: openCodeAdapter({ env }),
    pi: piAdapter({ env }),
  }
}
