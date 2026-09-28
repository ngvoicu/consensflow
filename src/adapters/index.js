import { claudeCodeAdapter } from './claude-code.js'
import { codexAdapter } from './codex.js'
import { devinAdapter } from './devin.js'
import { openCodeAdapter } from './opencode.js'
import { piAdapter } from './pi.js'

/**
 * One adapter per supported harness, keyed the way the ledger names them.
 * Kimi is paused (2026-09-19) and has none. An image agent runs in Codex's
 * own window, whose image tool draws on the Codex login. `peer: true` turns
 * Claude's native inbox on instead of pasting (off by default: see the
 * Claude adapter).
 */
export function createAdapters(env, { peer } = {}) {
  return {
    'claude-code': claudeCodeAdapter({ env, ...(peer === undefined ? {} : { peer }) }),
    codex: codexAdapter({ env }),
    image: codexAdapter({ env, harness: 'image' }),
    devin: devinAdapter({ env }),
    opencode: openCodeAdapter({ env }),
    pi: piAdapter({ env }),
  }
}
