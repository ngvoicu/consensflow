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
import {
  AGENT_PRESETS,
  agentProfile,
  validateKimiEffort,
  validateWorkTier,
} from '../hosts/lib/presets.js'

/**
 * The roster: every catalog agent, with the human's overrides on it, and the
 * agents defined by hand. `agents.json` keeps only what is the human's: a
 * custom agent in full, and for a catalog agent the fields changed on it
 * (model, effort, tier, description), keyed by the catalog entry. Reads
 * merge the catalog with the file, so a release that moves an entry reaches
 * every field the human did not touch. Rows an older build saved from the
 * catalog in full read as overrides of what differs, and `normalizeRoster`
 * folds them at daemon start. Reads map native kind/thinking fields to the
 * app's harness/effort view.
 *
 * Every function takes the environment explicitly — nothing reads
 * process.env — so tests run against throwaway homes.
 */

// `image` is a harness in the sense that matters here: it is what runs the
// agent. There is no CLI behind it — image generation is reached through the Codex
// login — but the roster, the catalog and `cf run` treat it like any other, so
// @pygmalion works wherever the rest do.
export const HARNESSES = ['claude', 'codex', 'pi', 'opencode', 'kimi', 'devin', 'image']

const NAME_PATTERN = /^[a-z][a-z0-9-]*$/
const KIND_TO_HARNESS = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  kimi: 'kimi',
  devin: 'devin',
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
  devin: 'devin',
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
function rosterHome(env) {
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

/** Display data older builds wrote into the file; recomputed on read now, never stored again. */
const STALE_FIELDS = ['skillsPolicy', 'skillPaths', 'skills', 'skillPath', 'profile']

function saveDocument(document, env) {
  for (const row of document.agents) for (const field of STALE_FIELDS) delete row[field]
  const path = rosterPath(env)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(document, null, 2)}\n`)
}

const CATALOG_BY_PRESET = new Map(AGENT_PRESETS.map((preset) => [preset.preset, preset]))
const CATALOG_BY_ID = new Map(AGENT_PRESETS.map((preset) => [preset.id, preset]))
/** The pi runner reads `thinking`; every other runner reads `effort`. */
const effortKey = (kind) => (kind === 'pi' ? 'thinking' : 'effort')
/** What the human may change on a catalog agent; its harness is the catalog's. */
const overridable = (kind) => ['model', effortKey(kind), 'description', 'workTier']
const set = (value) => value !== undefined && value !== null && value !== ''

/** A catalog entry as the row the launcher runs when nothing on it is overridden. */
function catalogRow(preset) {
  const effort = preset.effort ?? preset.thinking
  return {
    id: preset.id,
    name: preset.name,
    kind: preset.kind,
    model: preset.model,
    ...(effort ? { [effortKey(preset.kind)]: effort } : {}),
    // The one-line label is what a roster row calls itself; the preset's
    // own description is the catalog card's paragraph.
    description: preset.label ?? preset.description,
    preset: preset.preset,
  }
}

/** The fields of a stored row that differ from its catalog entry: the human's overrides. */
function overridesOf(row, preset) {
  const base = catalogRow(preset)
  const own = {}
  for (const field of overridable(preset.kind)) {
    if (set(row[field]) && row[field] !== base[field]) own[field] = row[field]
  }
  return own
}

/**
 * The catalog entry a stored row overrides, if it is one: by the provenance
 * it carries, else by a matching name and harness (a copy an older build
 * saved, or a row released from its entry today). A custom row that took a
 * catalog name on another harness is its own agent.
 */
function entryOf(row) {
  const byPreset = row.preset === undefined ? undefined : CATALOG_BY_PRESET.get(row.preset)
  if (byPreset !== undefined && byPreset.id === row.id) return byPreset
  const byId = CATALOG_BY_ID.get(row.id)
  return byId !== undefined && byId.kind === row.kind ? byId : undefined
}

/**
 * The roster as the app sees it, in the file's shape: every catalog agent
 * with the human's overrides on it (marked `edited`), then the agents
 * defined by hand (marked `custom`). A custom row with a catalog name hides
 * that entry.
 */
function rows(document) {
  const overrides = new Map()
  const custom = []
  for (const row of document.agents) {
    const entry = entryOf(row)
    if (entry === undefined) custom.push(row)
    else overrides.set(entry.preset, row)
  }
  const hidden = new Set(custom.map((row) => row.id))
  const catalog = AGENT_PRESETS.filter((preset) => !hidden.has(preset.id)).map((preset) => {
    const stored = overrides.get(preset.preset)
    const own = stored === undefined ? {} : overridesOf(stored, preset)
    const row = { ...catalogRow(preset), ...own }
    return Object.keys(own).length > 0 ? { ...row, edited: true } : row
  })
  return [...catalog, ...custom.map((row) => ({ ...row, custom: true }))]
}

const effortOf = (row) => row[effortKey(row.kind)] ?? undefined

function toView(row) {
  const harness = KIND_TO_HARNESS[row.kind]
  return {
    name: row.id,
    harness: harness ?? row.kind,
    model: row.model,
    ...(row.workTier == null ? {} : { workTier: row.workTier }),
    ...(effortOf(row) ? { effort: effortOf(row) } : {}),
    ...(row.description ? { description: row.description } : {}),
    ...(row.preset ? { preset: row.preset } : {}),
    ...(row.custom ? { custom: true } : {}),
    ...(row.edited ? { edited: true } : {}),
    profile: agentProfile(row),
    ...(harness === undefined ? { unsupported: true } : {}),
  }
}

/**
 * The row the launcher runs, in the stored shape (`kind`, `thinking`): the
 * catalog entry with the human's overrides, or the custom row. The runner
 * and the packet builder speak that shape, so `listAgents()` output would
 * drop the fields they run on.
 */
export function agentRow(name, env) {
  const wanted = String(name ?? '').replace(/^@/, '')
  return rows(loadDocument(env)).find((row) => row.id === wanted)
}

export function listAgents(env) {
  return rows(loadDocument(env)).map(toView)
}

/**
 * Folds what older builds wrote into the shape the file keeps now: a full
 * copy of a catalog entry becomes the overrides that differ from it (or
 * nothing), and stored display data goes. Says whether the file changed.
 */
export function normalizeRoster(env) {
  const before = JSON.stringify(loadDocument(env))
  const document = loadDocument(env)
  document.agents = document.agents.flatMap((row) => {
    for (const field of STALE_FIELDS) delete row[field]
    const entry = entryOf(row)
    if (entry === undefined) return [row]
    const own = overridesOf(row, entry)
    if (Object.keys(own).length === 0) return []
    return [
      {
        id: entry.id,
        name: entry.name,
        kind: entry.kind,
        preset: entry.preset,
        ...(row.createdAt ? { createdAt: row.createdAt } : {}),
        ...(row.updatedAt ? { updatedAt: row.updatedAt } : {}),
        ...own,
      },
    ]
  })
  if (JSON.stringify(document) === before) return false
  saveDocument(document, env)
  return true
}

function validateAdd(input) {
  if (typeof input.name !== 'string' || !NAME_PATTERN.test(input.name)) {
    throw new Error(
      `agent names are lowercase [a-z0-9-] starting with a letter; got ${JSON.stringify(input.name)}`,
    )
  }
  if (CATALOG_BY_ID.has(input.name)) {
    throw new Error(`${input.name} is in the catalog already: edit it, or pick another name`)
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

/** An agent defined by hand: the catalog's agents are there already. */
export function addAgent(input, env) {
  validateAdd(input)
  validateKimiEffort(input)
  validateWorkTier(input.workTier)
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
    createdAt: now,
    updatedAt: now,
    model: input.model,
    ...(input.workTier == null ? {} : { workTier: input.workTier }),
    ...(input.effort ? { [effortKey(HARNESS_TO_KIND[input.harness])]: input.effort } : {}),
    ...(input.description ? { description: input.description } : {}),
  }
  document.agents.push(row)
  saveDocument(document, env)
  return toView({ ...row, custom: true })
}

/** The patch on a row of the file's shape: the effort under the key its kind reads. */
function applyPatch(row, patch) {
  if (patch.model !== undefined) {
    if (typeof patch.model !== 'string' || patch.model.length === 0) {
      throw new Error('an agent needs a model (any identifier its harness accepts)')
    }
    row.model = patch.model
  }
  if (patch.description !== undefined) row.description = patch.description
  if (patch.workTier !== undefined) {
    if (patch.workTier === null) delete row.workTier
    else row.workTier = patch.workTier
  }
  if (patch.effort !== undefined) {
    const key = effortKey(row.kind)
    if (patch.effort === '' || patch.effort === null) delete row[key]
    else row[key] = patch.effort
    // Never leave a stale value in the key this kind does not read.
    delete row[key === 'thinking' ? 'effort' : 'thinking']
  }
  validateKimiEffort(row)
}

function refuseEffortEdit(name, kind) {
  const supported = KIND_TO_HARNESS[kind] !== undefined
  // Two different refusals that used to be one: a kind this build cannot run
  // at all, and `image`, which it runs but which has no effort to set —
  // the image route takes a prompt, not a thinking level.
  throw new Error(
    supported
      ? `${name} is an image agent: it has no effort level — only its model and description can be edited`
      : `${name} is a ${kind} agent, which this build does not run; only its model and description can be edited here`,
  )
}

/**
 * Edits a catalog agent by storing only what now differs from its entry,
 * and a custom agent in place. A blank effort on a catalog agent means the
 * catalog's own.
 */
export function editAgent(name, patch, env) {
  validateWorkTier(patch.workTier)
  const document = loadDocument(env)
  const stored = document.agents.find((row) => row.id === name)
  const entry = CATALOG_BY_ID.get(name)
  if (entry !== undefined && (stored === undefined || entryOf(stored) === entry)) {
    if (patch.effort !== undefined && entry.kind === 'image') refuseEffortEdit(name, entry.kind)
    const current = {
      ...catalogRow(entry),
      ...(stored === undefined ? {} : overridesOf(stored, entry)),
    }
    applyPatch(current, patch)
    const own = overridesOf(current, entry)
    const now = new Date().toISOString()
    document.agents = document.agents.filter((row) => row !== stored)
    if (Object.keys(own).length > 0) {
      document.agents.push({
        id: entry.id,
        name: entry.name,
        kind: entry.kind,
        preset: entry.preset,
        createdAt: stored?.createdAt ?? now,
        updatedAt: now,
        ...own,
      })
    }
    saveDocument(document, env)
    return listAgents(env).find((agent) => agent.name === name)
  }
  if (stored === undefined) throw new Error(`no agent named ${name}`)
  if (
    patch.effort !== undefined &&
    (KIND_TO_HARNESS[stored.kind] === undefined || stored.kind === 'image')
  ) {
    refuseEffortEdit(name, stored.kind)
  }
  applyPatch(stored, patch)
  stored.updatedAt = new Date().toISOString()
  saveDocument(document, env)
  return toView({ ...stored, custom: true })
}

/** A catalog agent back as the catalog has it: its overrides go. */
export function resetAgent(name, env) {
  const document = loadDocument(env)
  const entry = CATALOG_BY_ID.get(name)
  const stored = document.agents.find((row) => row.id === name)
  if (entry === undefined || (stored !== undefined && entryOf(stored) !== entry)) {
    if (stored === undefined) throw new Error(`no agent named ${name}`)
    throw new Error(`${name} is your own agent: there is no catalog entry to reset it to`)
  }
  if (stored !== undefined) {
    document.agents = document.agents.filter((row) => row !== stored)
    saveDocument(document, env)
  }
  return listAgents(env).find((agent) => agent.name === name)
}

/** Removes an agent defined by hand; a catalog agent is reset, never removed. */
export function removeAgent(name, env) {
  const document = loadDocument(env)
  const stored = document.agents.find((row) => row.id === name)
  const entry = CATALOG_BY_ID.get(name)
  if (entry !== undefined && (stored === undefined || entryOf(stored) === entry)) {
    throw new Error(`${name} is in the catalog: reset it instead of removing it`)
  }
  if (stored === undefined) throw new Error(`no agent named ${name}`)
  document.agents = document.agents.filter((row) => row !== stored)
  saveDocument(document, env)
}
