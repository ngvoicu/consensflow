import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

/**
 * The roster: one JSON file of named participants. Every function takes the
 * environment explicitly — nothing here reads process.env — so tests can pass
 * a throwaway home and be certain the real one is never touched.
 */

export const RUNTIMES = ['claude', 'codex', 'pi', 'opencode']
export const PERMISSIONS = ['workspace-write', 'full-auto']

const NAME_PATTERN = /^[a-z][a-z0-9-]*$/

export function configRoot(env) {
  if (typeof env?.CONSENSFLOW_HOME === 'string' && env.CONSENSFLOW_HOME.length > 0) {
    return env.CONSENSFLOW_HOME
  }
  const xdg = env?.XDG_CONFIG_HOME
  const base =
    typeof xdg === 'string' && xdg.length > 0 ? xdg : join(env?.HOME ?? homedir(), '.config')
  return join(base, 'consensflow')
}

function rosterPath(env) {
  return join(configRoot(env), 'participants.json')
}

export function loadRoster(env) {
  try {
    const parsed = JSON.parse(readFileSync(rosterPath(env), 'utf8'))
    return { participants: Array.isArray(parsed.participants) ? parsed.participants : [] }
  } catch {
    return { participants: [] }
  }
}

export function saveRoster(roster, env) {
  mkdirSync(configRoot(env), { recursive: true })
  writeFileSync(rosterPath(env), `${JSON.stringify(roster, null, 2)}\n`)
}

export function listParticipants(env) {
  return loadRoster(env).participants
}

function validate(input) {
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
  validate(input)
  const roster = loadRoster(env)
  if (roster.participants.some((p) => p.name === input.name)) {
    throw new Error(`a participant named ${input.name} already exists`)
  }
  const now = new Date().toISOString()
  const participant = {
    name: input.name,
    runtime: input.runtime,
    model: input.model,
    ...(input.effort ? { effort: input.effort } : {}),
    permission: input.permission ?? 'workspace-write',
    ...(input.description ? { description: input.description } : {}),
    createdAt: now,
    updatedAt: now,
  }
  roster.participants.push(participant)
  saveRoster(roster, env)
  return participant
}

function findIndex(roster, name) {
  const index = roster.participants.findIndex((p) => p.name === name)
  if (index === -1) throw new Error(`no participant named ${name}`)
  return index
}

export function editParticipant(name, patch, env) {
  const roster = loadRoster(env)
  const index = findIndex(roster, name)
  const merged = {
    ...roster.participants[index],
    ...patch,
    name,
    updatedAt: new Date().toISOString(),
  }
  if (patch.effort === '' || patch.effort === null) delete merged.effort
  validate(merged)
  roster.participants[index] = merged
  saveRoster(roster, env)
  return merged
}

export function removeParticipant(name, env) {
  const roster = loadRoster(env)
  const index = findIndex(roster, name)
  roster.participants.splice(index, 1)
  saveRoster(roster, env)
}

const V1_KIND_TO_RUNTIME = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
}

/** Best-effort effort lookup in a v1 presets.js: a text scan, never an import. */
function presetEfforts(presetsPath) {
  let source
  try {
    source = readFileSync(presetsPath, 'utf8')
  } catch {
    return {}
  }
  const efforts = {}
  for (const block of source.split(/\n\s*\{/)) {
    const id = /id:\s*["']([a-z0-9-]+)["']/.exec(block)
    const effort = /effort:\s*["']([a-z]+)["']/.exec(block)
    if (id && effort) efforts[id[1]] = effort[1]
  }
  return efforts
}

/**
 * Imports the v1 roster (`~/.consensflow/participants.json`). Efforts come
 * from the v1 `thinking` field (pi) or, when a v1 presets.js is provided,
 * from the participant's preset. Unsupported kinds are reported, not dropped
 * silently. Re-importing updates in place.
 */
export function importV1({ v1Path, presetsPath }, env) {
  const raw = readFileSync(v1Path, 'utf8')
  const v1 = JSON.parse(raw)
  const efforts = presetsPath ? presetEfforts(presetsPath) : {}

  const roster = loadRoster(env)
  const imported = []
  const skipped = []

  for (const p of v1.participants ?? []) {
    const runtime = V1_KIND_TO_RUNTIME[p.kind]
    if (runtime === undefined) {
      skipped.push({ name: p.id, reason: `unsupported kind ${p.kind}` })
      continue
    }
    const effort = p.thinking ?? efforts[p.preset ?? p.id]
    const now = new Date().toISOString()
    const participant = {
      name: p.id,
      runtime,
      model: p.model,
      ...(effort ? { effort } : {}),
      permission: p.toolsPolicy === 'full-auto' ? 'full-auto' : 'workspace-write',
      ...(p.description ? { description: p.description } : {}),
      createdAt: now,
      updatedAt: now,
    }
    validate(participant)
    const existing = roster.participants.findIndex((x) => x.name === participant.name)
    if (existing === -1) roster.participants.push(participant)
    else roster.participants[existing] = { ...roster.participants[existing], ...participant }
    imported.push(participant)
  }

  saveRoster(roster, env)
  return { imported, skipped }
}
