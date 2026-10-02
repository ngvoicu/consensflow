import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { AGENT_PRESETS, agentProfile, validateWorkTier } from '../hosts/lib/presets.js'

/**
 * The roster: every catalog agent, exactly as the catalog has it, and the
 * agents defined by hand. `agents.json` keeps only the latter, in full. A
 * catalog agent is never edited or removed: a different setting is a custom
 * agent under a name of its own. Rows an older build saved from the catalog
 * are the catalog's again on read, and `normalizeRoster` drops them at
 * daemon start. Reads map native kind/thinking fields to the app's
 * harness/effort view.
 *
 * Every function takes the environment explicitly — nothing reads
 * process.env — so tests run against throwaway homes.
 */

// `image` is a harness in the sense that matters here: it is what runs the
// agent. There is no CLI behind it — image generation is reached through the Codex
// login — but the roster, the catalog and `cf run` treat it like any other, so
// @pygmalion works wherever the rest do.
export const HARNESSES = ['claude', 'codex', 'pi', 'opencode', 'devin', 'image']

const NAME_PATTERN = /^[a-z][a-z0-9-]*$/
const KIND_TO_HARNESS = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
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

/**
 * The file as the human left it, or undefined when there is none. Only a
 * missing file is an empty roster: one that cannot be read or parsed (a hand
 * edit's trailing comma) is said to whoever reads it, and nothing is saved
 * over it, since the next write would have erased every agent in it.
 */
function readRoster(env) {
  for (const path of [rosterPath(env), legacyRosterPath(env)]) {
    let text
    try {
      text = readFileSync(path, 'utf8')
    } catch (error) {
      if (error.code === 'ENOENT') continue
      throw unreadable(path, `cannot be read (${error.code ?? error.message})`)
    }
    let parsed
    try {
      parsed = JSON.parse(text)
    } catch {
      throw unreadable(path, 'is not valid JSON')
    }
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed))
      throw unreadable(path, 'is not an agents file')
    return parsed
  }
  return undefined
}

const unreadable = (path, why) =>
  new Error(
    `Your agents file ${path} ${why}: fix it or move it away. ConsensFlow left it as it is.`,
  )

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

/** Written whole or not at all: a write cut short (a crash, a full disk) leaves the previous file. */
function saveDocument(document, env) {
  for (const row of document.agents) for (const field of STALE_FIELDS) delete row[field]
  const path = rosterPath(env)
  mkdirSync(dirname(path), { recursive: true })
  const temporary = `${path}.${process.pid}.tmp`
  try {
    writeFileSync(temporary, `${JSON.stringify(document, null, 2)}\n`)
    renameSync(temporary, path)
  } catch (error) {
    rmSync(temporary, { force: true })
    throw error
  }
}

const CATALOG_BY_PRESET = new Map(AGENT_PRESETS.map((preset) => [preset.preset, preset]))
const CATALOG_BY_ID = new Map(AGENT_PRESETS.map((preset) => [preset.id, preset]))
/** The pi runner reads `thinking`; every other runner reads `effort`. */
const effortKey = (kind) => (kind === 'pi' ? 'thinking' : 'effort')
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

/**
 * The catalog entry a stored row is a copy of, if it is one: by the
 * provenance it carries, else by a matching name and harness (a copy an
 * older build saved). A custom row that took a catalog name on another
 * harness is its own agent, and hides the entry.
 */
function entryOf(row) {
  const byPreset = row.preset === undefined ? undefined : CATALOG_BY_PRESET.get(row.preset)
  if (byPreset !== undefined && byPreset.id === row.id) return byPreset
  const byId = CATALOG_BY_ID.get(row.id)
  return byId !== undefined && byId.kind === row.kind ? byId : undefined
}

/**
 * The roster as the app sees it, in the file's shape: every catalog agent
 * as the catalog has it, then the agents defined by hand (marked `custom`).
 * A stored copy of a catalog entry is ignored; a custom row with a catalog
 * name hides that entry.
 */
function rows(document) {
  const custom = document.agents.filter((row) => entryOf(row) === undefined)
  const hidden = new Set(custom.map((row) => row.id))
  return [
    ...AGENT_PRESETS.filter((preset) => !hidden.has(preset.id)).map(catalogRow),
    ...custom.map((row) => ({ ...row, custom: true })),
  ]
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
    profile: agentProfile(row),
    ...(harness === undefined ? { unsupported: true } : {}),
  }
}

/**
 * The row the launcher runs, in the stored shape (`kind`, `thinking`): the
 * catalog entry, or the custom row. The runner and the packet builder speak
 * that shape, so `listAgents()` output would drop the fields they run on.
 */
export function agentRow(name, env) {
  const wanted = String(name ?? '').replace(/^@/, '')
  return rows(loadDocument(env)).find((row) => row.id === wanted)
}

/** What the human chose about the roster, kept in the file beside their own agents. */
const PREFERENCE_KEYS = ['ownHarnessOnly']
const preferencesOf = (document) => ({
  ownHarnessOnly: document.preferences?.ownHarnessOnly === true,
})
export function preferences(env) {
  return preferencesOf(loadDocument(env))
}
export function setPreferences(patch, env) {
  const document = loadDocument(env)
  const next = preferencesOf(document)
  for (const [key, value] of Object.entries(patch ?? {})) {
    if (!PREFERENCE_KEYS.includes(key)) throw new Error(`no preference named ${key}`)
    if (typeof value !== 'boolean') throw new Error(`${key} is on or off`)
    next[key] = value
  }
  saveDocument({ ...document, preferences: next }, env)
  return next
}
/**
 * Claude and OpenAI models reached through Pi or OpenCode are hidden when the
 * human keeps them to their own harnesses; a member already on one still runs.
 */
const RELAYED = new Set(['pi', 'opencode'])
const hides = (prefs, view) =>
  prefs.ownHarnessOnly && RELAYED.has(view.harness) && /^(claude|gpt)-/.test(view.profile.modelKey)
export function listAgents(env) {
  const document = loadDocument(env)
  const prefs = preferencesOf(document)
  return rows(document).map((row) => {
    const view = toView(row)
    return hides(prefs, view) ? { ...view, hidden: true } : view
  })
}

/**
 * Folds what older builds wrote into the shape the file keeps now: a copy
 * of a catalog entry goes (the catalog has it), and so does stored display
 * data. Says whether the file changed.
 */
export function normalizeRoster(env) {
  const before = JSON.stringify(loadDocument(env))
  const document = loadDocument(env)
  document.agents = document.agents.filter((row) => {
    for (const field of STALE_FIELDS) delete row[field]
    return entryOf(row) === undefined
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
    throw new Error(`${input.name} is a catalog agent: pick another name for your own`)
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

/** Edits an agent defined by hand, in place. A catalog agent is the catalog's. */
export function editAgent(name, patch, env) {
  validateWorkTier(patch.workTier)
  const document = loadDocument(env)
  const stored = document.agents.find((row) => row.id === name)
  const entry = CATALOG_BY_ID.get(name)
  if (entry !== undefined && (stored === undefined || entryOf(stored) === entry)) {
    throw new Error(
      `${name} is a catalog agent and stays as the catalog has it: define your own with the settings you want`,
    )
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

/** Removes an agent defined by hand; a catalog agent is the catalog's. */
export function removeAgent(name, env) {
  const document = loadDocument(env)
  const stored = document.agents.find((row) => row.id === name)
  const entry = CATALOG_BY_ID.get(name)
  if (entry !== undefined && (stored === undefined || entryOf(stored) === entry)) {
    throw new Error(`${name} is a catalog agent: it is not yours to remove`)
  }
  if (stored === undefined) throw new Error(`no agent named ${name}`)
  document.agents = document.agents.filter((row) => row !== stored)
  saveDocument(document, env)
}
