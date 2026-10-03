import { claudeCodeAdapter } from './claude-code.js'
import { codexAdapter } from './codex.js'
import { devinAdapter } from './devin.js'
import { openCodeAdapter } from './opencode.js'
import { piAdapter } from './pi.js'

/**
 * One adapter per supported harness, keyed the way the ledger names them.
 * An image agent is a Codex agent: Codex's adapter opens its window.
 */
export function createAdapters(env) {
  return {
    'claude-code': claudeCodeAdapter({ env }),
    codex: codexAdapter({ env }),
    devin: devinAdapter({ env }),
    opencode: openCodeAdapter({ env }),
    pi: piAdapter({ env }),
  }
}
