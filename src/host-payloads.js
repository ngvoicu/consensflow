import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/** Report obsolete global Claude hooks without changing native settings. */
export function staleClaudeHooks(env) {
  const path = join(env.CLAUDE_CONFIG_DIR ?? join(home(env), '.claude'), 'settings.json')
  const events = []
  for (const [event, entries] of Object.entries(readJson(path).hooks ?? {})) {
    if (!Array.isArray(entries)) continue
    if (entries.some((entry) => JSON.stringify(entry).includes('consensflow'))) events.push(event)
  }
  return { path, events }
}

function home(env) {
  return env.HOME ?? env.USERPROFILE ?? ''
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return {}
  }
}
