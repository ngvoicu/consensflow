/**
 * A chief as a bare harness: no ConsensFlow, the same workspace, prompt,
 * model and effort as the other arms, the owner answering in its terminal.
 * With nothing of ConsensFlow in the window, its session is found in the
 * harness's own store: Claude and Pi open on an id given to them; Codex,
 * Devin and OpenCode name theirs only in their stores, by folder.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { countQuestions, ownerQuestions } from './measure.mjs'

/**
 * The newest top-level session `kind` opened in `workspace` at or after
 * `since` (ms), from its store under `home`; null while there is none.
 * The workspace is fresh for each run and a bare run has no staff, so the
 * newest one there is the chief's.
 */
export function findSession(kind, { workspace, since, home, env = {} }) {
  if (kind === 'codex')
    return codexSession(workspace, since, env.CODEX_HOME ?? join(home, '.codex'))
  if (kind === 'opencode') {
    return newestRow(
      join(env.XDG_DATA_HOME ?? join(home, '.local', 'share'), 'opencode', 'opencode.db'),
      'SELECT id FROM session WHERE directory = ? AND parent_id IS NULL AND time_created >= ? ORDER BY time_created DESC LIMIT 1',
      [workspace, since],
    )
  }
  if (kind === 'devin') {
    // Devin keeps seconds.
    return newestRow(
      join(env.XDG_DATA_HOME ?? join(home, '.local', 'share'), 'devin', 'cli', 'sessions.db'),
      'SELECT id FROM sessions WHERE working_directory = ? AND created_at >= ? ORDER BY created_at DESC LIMIT 1',
      [workspace, Math.floor(since / 1000)],
    )
  }
  throw new Error(`${kind} opens on a session id of its own choosing: none to find`)
}

function newestRow(file, sql, params) {
  let db
  try {
    db = new DatabaseSync(file, { readOnly: true })
  } catch {
    return null
  }
  try {
    return db.prepare(sql).get(...params)?.id ?? null
  } catch {
    return null
  } finally {
    db.close()
  }
}

/** Codex writes `sessions/YYYY/MM/DD/rollout-….jsonl`, its first line naming the session and its folder. */
function codexSession(workspace, since, codexHome) {
  const root = join(codexHome, 'sessions')
  const files = []
  const walk = (dir, depth) => {
    let names
    try {
      names = readdirSync(dir)
    } catch {
      return
    }
    for (const name of names) {
      const path = join(dir, name)
      const stat = statSync(path, { throwIfNoEntry: false })
      if (stat === undefined) continue
      // A folder's time moves only when an entry is added right inside it:
      // the year and month folders are older than any run. Only files are dated.
      if (stat.isDirectory() && depth < 3) walk(path, depth + 1)
      else if (
        stat.isFile() &&
        stat.mtimeMs >= since &&
        name.startsWith('rollout-') &&
        name.endsWith('.jsonl')
      )
        files.push({ path, at: stat.mtimeMs })
    }
  }
  walk(root, 0)
  for (const { path } of files.sort((a, b) => b.at - a.at)) {
    const first = readFileSync(path, 'utf8').split('\n', 1)[0]
    try {
      const meta = JSON.parse(first)
      if (meta.type === 'session_meta' && meta.payload?.cwd === workspace) return meta.payload.id
    } catch {}
  }
  return null
}

/**
 * The run's numbers from the chief's own record, shaped as the other arms'
 * so the same expectations read them: no board, so no tasks, notes or board
 * questions; what the owner was asked is its turns' ends.
 */
export function bareMetrics(items, { filesChanged = [], ends = new Set() } = {}) {
  const assistant = items.filter((item) => item.role === 'assistant')
  const turnEnds = assistant.filter((item) => endsTurn(item, ends)).map((item) => item.text)
  return {
    chief: 'chief',
    tasks: [],
    parallel: 0,
    advice: 0,
    reviews: 0,
    questionsToHuman: [],
    notesToHuman: [],
    notesText: '',
    longestResult: { worker: 0, advisor: 0, reviewer: 0 },
    answersToChief: [],
    taskCount: 0,
    chiefTurns: assistant.length,
    chiefEdits: null,
    chiefLastWords: assistant.at(-1)?.text.slice(0, 1500) ?? '',
    ownerQuestions: ownerQuestions({ board: [], terminal: turnEnds }),
    filesChanged,
  }
}

/**
 * A message ended a turn when its record says so, or when the window sat idle
 * after it: without ConsensFlow, Devin's and OpenCode's records never mark
 * the message that ends a turn, so the runner keeps the ids it saw at rest.
 */
const endsTurn = (item, ends) => item.complete || ends.has(item.id)

/** The chief's newest message that ended a turn, if it asked the owner anything. */
export function askingTurnEnd(items, ends = new Set()) {
  const end = items.filter((item) => item.role === 'assistant' && endsTurn(item, ends)).at(-1)
  return end !== undefined && countQuestions(end.text) > 0 ? end : undefined
}
