import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { presetDrift, syncAgentWithPreset } from '../hosts/lib/presets.js'

/**
 * The roster IS the shared ConsensFlow file that the v1 Claude Code plugin
 * (consensflow-cc) and the pi extension (consensflow-pi) already read and
 * write: `~/.consensflow/agents.json`. One roster, three consumers —
 * edit it here and cc/pi see the change on their next invocation.
 *
 * That makes v1-schema fidelity a hard contract: reads map v1 rows to the
 * v3 view (kind→harness, thinking/effort→effort),
 * and writes touch only the mapped keys, preserving every field v3 does not
 * understand (display name, skillsPolicy, preset, anything future). Rows of
 * kinds v3 cannot render as commands (e.g. `image`) are listed and marked,
 * never hidden and never dropped.
 *
 * Every function takes the environment explicitly — nothing reads
 * process.env — so tests run against throwaway homes.
 */

// `image` is a harness in the sense that matters here: it is what runs the
// agent. There is no CLI behind it — gpt-image-2 is reached through the Codex
// login — but the roster, the catalog and `cf run` treat it like any other, so
// @pygmalion works wherever the rest do.
export const HARNESSES = ['claude', 'codex', 'pi', 'opencode', 'image']

const NAME_PATTERN = /^[a-z][a-z0-9-]*$/
const KIND_TO_HARNESS = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  image: 'image',
}
const HARNESS_TO_KIND = {
  claude: 'claude-code',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  image: 'image',
}

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
  return join(env?.HOME ?? homedir(), '.consensflow', 'agents.json')
}

/**
 * What the roster was called before the vocabulary settled (2026-08-21):
 * `participants.json`, with a `participants` key. A machine that has one keeps
 * working — it is read as-is, and the next write lands in the new file.
 */
function legacyRosterPath(env) {
  return join(env?.HOME ?? homedir(), '.consensflow', 'participants.json')
}

function readRoster(env) {
  for (const path of [rosterPath(env), legacyRosterPath(env)]) {
    try {
      return JSON.parse(readFileSync(path, 'utf8'))
    } catch {
      // Missing or unreadable: try the next spelling.
    }
  }
  return undefined
}

function loadDocument(env) {
  const parsed = readRoster(env)
  if (parsed === undefined) return { schemaVersion: 1, agents: [] }
  const rows = Array.isArray(parsed.agents)
    ? parsed.agents
    : Array.isArray(parsed.participants)
      ? parsed.participants
      : []
  // The old key is dropped on the way out; everything else the file carried is
  // preserved, because rows and fields we do not understand are not ours.
  const { participants: _legacy, ...rest } = parsed
  return { ...rest, schemaVersion: parsed.schemaVersion ?? 1, agents: rows }
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
  const harness = KIND_TO_HARNESS[row.kind]
  return {
    name: row.id,
    harness: harness ?? row.kind,
    model: row.model,
    ...(effortOf(row) ? { effort: effortOf(row) } : {}),
    ...(row.description ? { description: row.description } : {}),
    ...(row.preset ? { preset: row.preset } : {}),
    ...(harness === undefined ? { unsupported: true } : {}),
  }
}

/**
 * The row as it sits in the file, not the manager's view of it.
 *
 * The runner and the packet builder are the payload's, and they speak the
 * stored shape (`kind`, `thinking`) — handing them `listAgents()` output
 * would quietly drop the fields they run on.
 */
export function agentRow(name, env) {
  const wanted = String(name ?? '').replace(/^@/, '')
  return loadDocument(env).agents.find((row) => row.id === wanted)
}

export function listAgents(env) {
  return loadDocument(env).agents.map(toView)
}

function validateAdd(input) {
  if (typeof input.name !== 'string' || !NAME_PATTERN.test(input.name)) {
    throw new Error(
      `agent names are lowercase [a-z0-9-] starting with a letter; got ${JSON.stringify(input.name)}`,
    )
  }
  if (!HARNESSES.includes(input.harness)) {
    throw new Error(
      `unknown harness ${JSON.stringify(input.harness)}; expected ${HARNESSES.join(', ')}`,
    )
  }
  if (typeof input.model !== 'string' || input.model.length === 0) {
    throw new Error('an agent needs a model (any identifier its harness accepts)')
  }
}

export function addAgent(input, env) {
  validateAdd(input)
  const document = loadDocument(env)
  if (document.agents.some((row) => row.id === input.name)) {
    throw new Error(`an agent named ${input.name} already exists`)
  }

  const now = new Date().toISOString()
  const row = {
    id: input.name,
    // The display name cc shows; capitalized to match its convention.
    name: input.name.charAt(0).toUpperCase() + input.name.slice(1),
    kind: HARNESS_TO_KIND[input.harness],
    skillsPolicy: 'default',
    createdAt: now,
    updatedAt: now,
    model: input.model,
    ...(input.effort
      ? input.harness === 'pi'
        ? { thinking: input.effort }
        : { effort: input.effort }
      : {}),
    ...(input.description ? { description: input.description } : {}),
    // Which catalog entry this came from, when it came from one. The payload
    // has always read this field; the manager never wrote it, which is why a
    // agent added in the app could never be told its model had moved.
    ...(input.preset ? { preset: input.preset } : {}),
  }
  document.agents.push(row)
  saveDocument(document, env)
  return toView(row)
}

function findRow(document, name) {
  const row = document.agents.find((p) => p.id === name)
  if (row === undefined) throw new Error(`no agent named ${name}`)
  return row
}

export function editAgent(name, patch, env) {
  const document = loadDocument(env)
  const row = findRow(document, name)
  const supported = KIND_TO_HARNESS[row.kind] !== undefined

  // Two different refusals that used to be one: a kind this build cannot run
  // at all, and `image`, which it runs but which has no effort to set —
  // gpt-image-2 takes a prompt, not a thinking level.
  if (patch.effort !== undefined && (!supported || row.kind === 'image')) {
    throw new Error(
      supported
        ? `${name} is an image agent: it has no effort level — only its model and description can be edited`
        : `${name} is a ${row.kind} agent, which this build does not run; only its model and description can be edited here`,
    )
  }

  if (patch.model !== undefined) {
    if (typeof patch.model !== 'string' || patch.model.length === 0) {
      throw new Error('an agent needs a model (any identifier its harness accepts)')
    }
    row.model = patch.model
  }
  if (patch.description !== undefined) row.description = patch.description
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

export function removeAgent(name, env) {
  const document = loadDocument(env)
  findRow(document, name)
  document.agents = document.agents.filter((p) => p.id !== name)
  saveDocument(document, env)
}

/**
 * What the catalog would change on each agent that came from it.
 *
 * A preset moves — a family gets a new release, an effort level is renamed —
 * and an agent created from it keeps whatever it was created with. The
 * comparison is the payload's own `presetDrift`, not a second implementation,
 * so the app and the running harness always agree about what has moved. Rows
 * with no provenance, and rows whose preset the catalog has since dropped,
 * report nothing: they are pinned, and pinned is a valid state.
 */
export function agentDrift(env) {
  return loadDocument(env).agents.flatMap((row) => {
    const changes = presetDrift(row)
    return changes.length === 0 ? [] : [{ name: row.id, preset: row.preset, changes }]
  })
}

/**
 * Re-resolves preset-backed agents against the catalog. Only the fields
 * the preset owns move (kind, model, effort/thinking, skillsPolicy) — a
 * description you wrote is yours and is never overwritten.
 */
export function syncAgents(env, options = {}) {
  const { name, dryRun = false } = options
  const document = loadDocument(env)
  const applied = []

  document.agents = document.agents.map((row) => {
    if (name !== undefined && row.id !== name) return row
    const { agent, changes } = syncAgentWithPreset(row)
    if (changes.length === 0) return row
    applied.push({ name: row.id, changes })
    return dryRun ? row : { ...agent, updatedAt: new Date().toISOString() }
  })

  if (!dryRun && applied.length > 0) saveDocument(document, env)
  return applied
}
