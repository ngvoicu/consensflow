import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'

/**
 * The roster IS the shared ConsensFlow file that the v1 Claude Code plugin
 * (consensflow-cc) and the pi extension (consensflow-pi) already read and
 * write: `~/.consensflow/participants.json`. One roster, three consumers —
 * edit it here and cc/pi see the change on their next invocation.
 *
 * That makes v1-schema fidelity a hard contract: reads map v1 rows to the
 * v3 view (kind→runtime, thinking/effort→effort, toolsPolicy→permission),
 * and writes touch only the mapped keys, preserving every field v3 does not
 * understand (display name, skillsPolicy, preset, anything future). Rows of
 * kinds v3 cannot render as commands (e.g. `image`) are listed and marked,
 * never hidden and never dropped.
 *
 * Every function takes the environment explicitly — nothing reads
 * process.env — so tests run against throwaway homes.
 */

export const RUNTIMES = ['claude', 'codex', 'pi', 'opencode']
export const PERMISSIONS = ['workspace-write', 'full-auto']

const NAME_PATTERN = /^[a-z][a-z0-9-]*$/
const KIND_TO_RUNTIME = { 'claude-code': 'claude', codex: 'codex', pi: 'pi', opencode: 'opencode' }
const RUNTIME_TO_KIND = { claude: 'claude-code', codex: 'codex', pi: 'pi', opencode: 'opencode' }

/** The manifest and other v3-only state; the roster deliberately not here. */
export function configRoot(env) {
  if (typeof env?.CONSENSFLOW_HOME === 'string' && env.CONSENSFLOW_HOME.length > 0) {
    return env.CONSENSFLOW_HOME
  }
  const xdg = env?.XDG_CONFIG_HOME
  const base =
    typeof xdg === 'string' && xdg.length > 0 ? xdg : join(env?.HOME ?? homedir(), '.config')
  return join(base, 'consensflow')
}

export function rosterPath(env) {
  return join(env?.HOME ?? homedir(), '.consensflow', 'participants.json')
}

function loadDocument(env) {
  try {
    const parsed = JSON.parse(readFileSync(rosterPath(env), 'utf8'))
    return {
      ...parsed,
      schemaVersion: parsed.schemaVersion ?? 1,
      participants: Array.isArray(parsed.participants) ? parsed.participants : [],
    }
  } catch {
    return { schemaVersion: 1, participants: [] }
  }
}

function saveDocument(document, env) {
  const path = rosterPath(env)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(document, null, 2)}\n`)
}

/** The pi runner reads `thinking`; every other runner reads `effort`. */
function effortOf(row) {
  return row.kind === 'pi' ? (row.thinking ?? row.effort) : (row.effort ?? undefined)
}

function toView(row) {
  const runtime = KIND_TO_RUNTIME[row.kind]
  return {
    name: row.id,
    runtime: runtime ?? row.kind,
    model: row.model,
    ...(effortOf(row) ? { effort: effortOf(row) } : {}),
    permission: row.toolsPolicy === 'full-auto' ? 'full-auto' : 'workspace-write',
    ...(row.description ? { description: row.description } : {}),
    ...(runtime === undefined ? { unsupported: true } : {}),
  }
}

export function listParticipants(env) {
  return loadDocument(env).participants.map(toView)
}

function validateAdd(input) {
  if (typeof input.name !== 'string' || !NAME_PATTERN.test(input.name)) {
    throw new Error(
      `participant names are lowercase [a-z0-9-] starting with a letter; got ${JSON.stringify(input.name)}`,
    )
  }
  if (!RUNTIMES.includes(input.runtime)) {
    throw new Error(
      `unknown runtime ${JSON.stringify(input.runtime)}; expected ${RUNTIMES.join(', ')}`,
    )
  }
  if (typeof input.model !== 'string' || input.model.length === 0) {
    throw new Error('a participant needs a model (any identifier its runtime accepts)')
  }
  if (input.permission !== undefined && !PERMISSIONS.includes(input.permission)) {
    throw new Error(
      `unknown permission ${JSON.stringify(input.permission)}; expected ${PERMISSIONS.join(', ')}`,
    )
  }
}

export function addParticipant(input, env) {
  validateAdd(input)
  const document = loadDocument(env)
  if (document.participants.some((row) => row.id === input.name)) {
    throw new Error(`a participant named ${input.name} already exists`)
  }

  const now = new Date().toISOString()
  const row = {
    id: input.name,
    // The display name cc shows; capitalized to match its convention.
    name: input.name.charAt(0).toUpperCase() + input.name.slice(1),
    kind: RUNTIME_TO_KIND[input.runtime],
    toolsPolicy: input.permission ?? 'workspace-write',
    skillsPolicy: 'default',
    createdAt: now,
    updatedAt: now,
    model: input.model,
    ...(input.effort
      ? input.runtime === 'pi'
        ? { thinking: input.effort }
        : { effort: input.effort }
      : {}),
    ...(input.description ? { description: input.description } : {}),
  }
  document.participants.push(row)
  saveDocument(document, env)
  return toView(row)
}

function findRow(document, name) {
  const row = document.participants.find((p) => p.id === name)
  if (row === undefined) throw new Error(`no participant named ${name}`)
  return row
}

export function editParticipant(name, patch, env) {
  const document = loadDocument(env)
  const row = findRow(document, name)
  const supported = KIND_TO_RUNTIME[row.kind] !== undefined

  if (!supported && (patch.effort !== undefined || patch.permission !== undefined)) {
    throw new Error(
      `${name} is a ${row.kind} participant, which v3 does not run; only its model and description can be edited here`,
    )
  }

  if (patch.model !== undefined) {
    if (typeof patch.model !== 'string' || patch.model.length === 0) {
      throw new Error('a participant needs a model (any identifier its runtime accepts)')
    }
    row.model = patch.model
  }
  if (patch.description !== undefined) row.description = patch.description
  if (patch.permission !== undefined) {
    if (!PERMISSIONS.includes(patch.permission)) {
      throw new Error(
        `unknown permission ${JSON.stringify(patch.permission)}; expected ${PERMISSIONS.join(', ')}`,
      )
    }
    row.toolsPolicy = patch.permission
  }
  if (patch.effort !== undefined) {
    const key = row.kind === 'pi' ? 'thinking' : 'effort'
    if (patch.effort === '' || patch.effort === null) delete row[key]
    else row[key] = patch.effort
    // Never leave a stale value in the key this kind does not read.
    delete row[key === 'thinking' ? 'effort' : 'thinking']
  }
  row.updatedAt = new Date().toISOString()

  saveDocument(document, env)
  return toView(row)
}

export function removeParticipant(name, env) {
  const document = loadDocument(env)
  findRow(document, name)
  document.participants = document.participants.filter((p) => p.id !== name)
  saveDocument(document, env)
}
