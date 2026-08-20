import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { detectAgents } from './agents.js'
import { hostStatus, installHost, uninstallHost } from './hosts.js'
import { installSkill, uninstallSkills } from './install.js'
import { configRoot, listParticipants } from './roster.js'
import { generateSkill } from './skill.js'

/**
 * One ConsensFlow path per machine.
 *
 * Three modes, mutually exclusive by construction:
 * - `claude`     — the Claude Code integration, which hands a participant the
 *                  live conversation. Only Claude Code can consult.
 * - `pi`         — the same, inside pi. Only pi can consult.
 * - `standalone` — the generated skill on every coding agent found. Every
 *                  agent can consult; none of them gets the conversation.
 *
 * Switching modes removes what the previous mode installed, so two paths can
 * never be live at once. It removes only what ConsensFlow installed: an
 * integration someone put there another way is reported and left alone.
 *
 * The cost of a host mode is real — in `claude` mode, codex and opencode have
 * no ConsensFlow at all — so every entry point states it rather than letting
 * it be discovered later.
 */

export const MODES = ['claude', 'pi', 'standalone']

function statePath(env) {
  return join(configRoot(env), 'mode.json')
}

export function currentMode(env) {
  try {
    const mode = JSON.parse(readFileSync(statePath(env), 'utf8')).mode
    return MODES.includes(mode) ? mode : null
  } catch {
    return null
  }
}

function rememberMode(mode, env) {
  mkdirSync(configRoot(env), { recursive: true })
  writeFileSync(
    statePath(env),
    `${JSON.stringify({ mode, at: new Date().toISOString() }, null, 2)}\n`,
  )
}

/** Removes the generated skill from every agent that has ours. */
function dropGeneratedSkill(env) {
  return uninstallSkills(env, { filter: (_path, recorded) => recorded.source === 'consensflow' })
}

function dropHost(host, env) {
  const status = hostStatus(env).find((entry) => entry.id === host)
  if (status?.installed !== true) return []
  uninstallHost(host, env)
  return [{ host, action: 'removed' }]
}

/**
 * What this mode means for the machine, in plain words — including who ends
 * up with nothing.
 */
export function modeReport(mode, env) {
  const agents = detectAgents(env).map((agent) => agent.id)
  if (mode === 'standalone') {
    return agents.length > 0
      ? [`every agent can consult: ${agents.join(', ')}`]
      : ['no coding agent found on PATH yet']
  }
  const others = agents.filter((agent) => agent !== mode)
  const lines = [`${mode} can consult, and gets your conversation as context`]
  if (others.length > 0) {
    lines.push(
      `${others.join(', ')}: no ConsensFlow in this mode — switch to standalone to include them`,
    )
  }
  return lines
}

export function applyMode(mode, env, options = {}) {
  if (!MODES.includes(mode)) {
    throw new Error(`unknown mode ${JSON.stringify(mode)}; expected ${MODES.join(', ')}`)
  }

  const changes = []
  if (mode === 'standalone') {
    for (const host of ['claude', 'pi']) changes.push(...dropHost(host, env))
    const participants = listParticipants(env)
    if (participants.length > 0) {
      // A foreign integration still owns its agent; skip that one and say so
      // rather than stacking a second skill on it.
      const targets = detectAgents(env).filter((agent) => agent.native !== true)
      changes.push(
        ...installSkill(
          {
            relPath: 'consensflow/SKILL.md',
            content: generateSkill(participants),
            source: 'consensflow',
          },
          env,
          { targets, force: options.force },
        ),
      )
    }
  } else {
    changes.push(...dropGeneratedSkill(env))
    for (const host of ['claude', 'pi']) {
      if (host !== mode) changes.push(...dropHost(host, env))
    }
    changes.push(installHost(mode, env, options))
  }

  rememberMode(mode, env)
  return { mode, changes, report: modeReport(mode, env) }
}
