import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, rmSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { configRoot } from './roster.js'

/**
 * Everything the host-payload era installed, and nothing else — take-back only.
 *
 * Until 2026-08-23 `claude` and `pi` were not scopes but integrations: each
 * copied a payload (its own CLI, its own hand-written skill, a shared engine)
 * into `~/.consensflow/hosts`, wrote a `/consensflow` command and hooks into
 * Claude Code's config, or registered an extension with pi. A mode is a scope
 * now and installs one generated skill, so none of that has a counterpart.
 *
 * It still has to be removed. A registered pi extension pointing at a deleted
 * checkout breaks pi at load; a `/consensflow` command that no version answers
 * is a command that fails; and the old skill file sits exactly where the
 * generated one belongs, where `installSkill` would refuse it as unowned and
 * quietly leave the machine with no skill at all.
 *
 * Nothing here writes. Every removal is guarded by a marker or by our own
 * bookkeeping, so a file someone else put at one of these names is left alone.
 */
export function retireHostPayloads(env) {
  const changes = []
  const root = configRoot(env)
  const statePath = join(root, 'hosts.json')
  const state = readJson(statePath)

  // pi first: while its checkout still exists, `pi remove` can find it.
  const piSource = state.hosts?.pi?.source
  if (piSource !== undefined) {
    const removed = spawnSync('pi', ['remove', piSource], { env, encoding: 'utf8' })
    changes.push({
      host: 'pi',
      action: removed.status === 0 ? 'extension-removed' : 'extension-remove-failed',
    })
  }

  // The files the claude integration recorded — its skill, and on a machine
  // old enough, its command. Recorded by us, so removed by us.
  for (const path of state.hosts?.claude?.files ?? []) {
    if (!existsSync(path)) continue
    rmSync(path, { force: true })
    pruneEmpty(dirname(path))
    changes.push({ host: 'claude', action: 'removed', path })
  }

  changes.push(...retireClaudeCommand(env))

  for (const name of ['claude', 'pi', 'lib']) {
    const payload = join(root, 'hosts', name)
    if (!existsSync(payload)) continue
    rmSync(payload, { recursive: true, force: true })
    changes.push({ action: 'removed', path: payload })
  }
  pruneEmpty(join(root, 'hosts'))

  if (existsSync(statePath)) {
    rmSync(statePath, { force: true })
    changes.push({ action: 'removed', path: statePath })
  }

  return changes
}

/** The front-matter line every command file we ever wrote carries. */
const COMMAND_TAG = 'description: "ConsensFlow:'

/**
 * The `/consensflow` command, and the world-readable copy of settings.json an
 * older version dropped beside the real one and never came back for. Claude
 * Code's settings themselves are not ours to write — a hook we left behind is
 * reported by `cf doctor`, not silently edited away.
 */
export function retireClaudeCommand(env) {
  const changes = []
  const claudeDir = env.CLAUDE_CONFIG_DIR ?? join(home(env), '.claude')

  const command = join(claudeDir, 'commands', 'consensflow.md')
  // Only ours: a file someone else put at this name is not ours to remove.
  if (existsSync(command) && readFileSync(command, 'utf8').includes(COMMAND_TAG)) {
    rmSync(command, { force: true })
    pruneEmpty(dirname(command))
    changes.push({ host: 'claude', action: 'removed', path: command })
  }

  const backup = join(claudeDir, 'settings.json.consensflow.bak')
  if (existsSync(backup)) {
    rmSync(backup, { force: true })
    changes.push({ host: 'claude', action: 'removed', path: backup })
  }

  return changes
}

/**
 * Hooks an older version wrote into Claude Code's settings, if any survive.
 *
 * ConsensFlow stopped writing hooks on 2026-08-22 and no longer writes this
 * file at all: it is Claude Code's, and the same rule that protects a skill
 * the user edited protects their settings. So a leftover is reported and left
 * — `cf doctor` names it, and removing it is one edit the user makes once.
 */
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

function pruneEmpty(dir) {
  try {
    if (readdirSync(dir).length === 0) rmSync(dir, { recursive: true, force: true })
  } catch {
    // Already gone, or someone else's files live there too.
  }
}
