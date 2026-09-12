import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { readBenchmarkCache, withBenchmarks } from '../hosts/lib/benchmarks.js'
import {
  agentProfile,
  presetDrift,
  syncAgentWithPreset,
  validateKimiEffort,
} from '../hosts/lib/presets.js'

/**
 * agents.json stores the saved execution configuration and display profile.
 * Reads map native kind/thinking fields to the app's harness/effort view;
 * writes preserve fields outside the edited configuration, including older
 * and future schema fields.
 *
 * Every function takes the environment explicitly — nothing reads
 * process.env — so tests run against throwaway homes.
 */

// `image` is a harness in the sense that matters here: it is what runs the
// agent. There is no CLI behind it — image generation is reached through the Codex
// login — but the roster, the catalog and `cf run` treat it like any other, so
// @pygmalion works wherever the rest do.
export const HARNESSES = ['claude', 'codex', 'pi', 'opencode', 'kimi', 'image']

const NAME_PATTERN = /^[a-z][a-z0-9-]*$/
const KIND_TO_HARNESS = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  kimi: 'kimi',
  image: 'image',
}
/**
 * The CLI behind a kind. `src/harnesses.js` is keyed by the CLI's own name
 * (`claude`), the store and the roster speak in kinds (`claude-code`), and
 * a launcher needs to cross that line to find the binary.
 */
export function harnessForKind(kind) {
  return KIND_TO_HARNESS[kind] ?? null
}

const HARNESS_TO_KIND = {
  claude: 'claude-code',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  kimi: 'kimi',
  image: 'image',
}

/** The manifest and other v3-only state; the roster deliberately not here. */
/**
 * Everything ConsensFlow owns, in one directory.
 *
 * The state used to live under XDG (`~/.config/consensflow`) while the roster
 * and the run artifacts lived in `~/.consensflow` — two homes for one product,
 * so "where is ConsensFlow on this machine" had two answers and an uninstall
 * had two places to sweep. One root now: the same one the roster and the
 * payload have always used.
 */
export function configRoot(env) {
  return rosterHome(env)
}

/** Where the state lived before the roots were merged (2026-08-22). */
export function legacyConfigRoot(env) {
  const xdg = env?.XDG_CONFIG_HOME
  const base =
    typeof xdg === 'string' && xdg.length > 0 ? xdg : join(env?.HOME ?? homedir(), '.config')
  return join(base, 'consensflow')
}

/** Import legacy app data without changing anything outside the private home. */
export function migrateStateRoot(env) {
  const from = legacyConfigRoot(env)
  const to = configRoot(env)
  if (from === to || !existsSync(from) || existsSync(join(to, 'mode.json'))) return null
  if (lstatSync(from).isSymbolicLink()) return null
  mkdirSync(to, { recursive: true })
  const copied = []
  for (const name of readdirSync(from)) {
    const target = join(to, name)
    if (existsSync(target)) continue
    cpSync(join(from, name), target, {
      recursive: true,
      force: false,
      // Imported symlinks could make later private writes escape the home.
      filter: (source) => !lstatSync(source).isSymbolicLink(),
    })
    if (existsSync(target)) copied.push(name)
  }
  return copied.length > 0 ? { from, to, copied } : null
}

/**
 * The roster, in the one place both halves look.
 *
 * `CONSENSFLOW_HOME` used to mean two different directories: the manager read
 * it as its state root while the payload read it as the roster root — so
 * setting it split the machine in half. `cf agent list` showed your agents and
 * the session hook said "none configured", because each was reading a
 * different file. Both now agree: when it is set, everything ConsensFlow owns
 * lives under it; when it is not, the roster stays at `~/.consensflow`, which
 * is where the payload has always kept it.
 */
export function rosterHome(env) {
  const override = env?.CONSENSFLOW_HOME
  if (typeof override === 'string' && override.length > 0) return override
  return join(env?.HOME ?? homedir(), '.consensflow')
}

export function rosterPath(env) {
  return join(rosterHome(env), 'agents.json')
}

/**
 * What the roster was called before the vocabulary settled (2026-08-21):
 * `participants.json`, with a `participants` key. A machine that has one keeps
 * working — it is read as-is, and the next write lands in the new file.
 */
function legacyRosterPath(env) {
  return join(rosterHome(env), 'participants.json')
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

function saveDocument(document, env, benchmarks = readBenchmarkCache(rosterHome(env))) {
  for (const row of document.agents)
    row.profile = withBenchmarks(row, agentProfile(row), benchmarks)
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
    ...(row.profile ? { profile: row.profile } : {}),
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

/** Refresh display metadata only; never change the saved model or custom fields. */
export function refreshAgentProfiles(env, benchmarks = readBenchmarkCache(rosterHome(env))) {
  const document = loadDocument(env)
  if (
    document.agents.some(
      (row) =>
        JSON.stringify(row.profile) !==
        JSON.stringify(withBenchmarks(row, agentProfile(row), benchmarks)),
    )
  ) {
    saveDocument(document, env, benchmarks)
  }
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
  validateKimiEffort(input)
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
  // the image route takes a prompt, not a thinking level.
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
  validateKimiEffort(row)
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
 * Re-resolves preset-backed agents against the catalog. Every field the preset
 * owns moves — kind, model, effort/thinking, skillsPolicy, and since
 * 2026-08-27 the description, because a label naming the wrong model is what
 * the skill table shows a lead. A row with no `preset` is never touched.
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
