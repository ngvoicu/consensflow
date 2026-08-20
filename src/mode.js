import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { detectAgents } from './agents.js'
import { installCmuxSkills } from './cmux-skills.js'
import { hostStatus, installHost, uninstallHost } from './hosts.js'
import { installSkill, uninstallSkills } from './install.js'
import { configRoot, listParticipants } from './roster.js'
import { generateSkill } from './skill.js'

/**
 * One ConsensFlow path per machine.
 *
 * Three modes, named after the three things they are, mutually exclusive by
 * construction:
 * - `claude` — the Claude Code integration, which hands a participant the
 *              live conversation. Only Claude Code can consult.
 * - `pi`     — the same, inside pi. Only pi can consult.
 * - `cmux`   — the generated skill across every coding agent (pi, cc, codex,
 *              opencode). Every agent can consult; none gets the
 *              conversation. Called `standalone` until 2026-08-20; the old
 *              name is still accepted and normalizes to this one.
 *
 * Switching modes removes what the previous mode installed, so two paths can
 * never be live at once. It removes only what ConsensFlow installed: an
 * integration someone put there another way is reported and left alone.
 *
 * The cost of a host mode is real — in `claude` mode, codex and opencode have
 * no ConsensFlow at all — so every entry point states it rather than letting
 * it be discovered later.
 */

export const MODES = ['claude', 'pi', 'cmux']

/** What `cmux` mode was called before it was named after what it covers. */
const ALIASES = { standalone: 'cmux' }

/** Every agent the cmux path can teach — the parenthetical the UI shows. */
const CMUX_AGENTS = ['pi', 'cc', 'codex', 'opencode']

export function modeLabel(mode) {
  return mode === 'cmux' ? `cmux (${CMUX_AGENTS.join(', ')})` : mode
}

function canonical(mode) {
  return ALIASES[mode] ?? mode
}

function statePath(env) {
  return join(configRoot(env), 'mode.json')
}

export function currentMode(env) {
  try {
    const mode = canonical(JSON.parse(readFileSync(statePath(env), 'utf8')).mode)
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
export function modeReport(rawMode, env) {
  const mode = canonical(rawMode)
  const agents = detectAgents(env).map((agent) => agent.id)
  if (mode === 'cmux') {
    return agents.length > 0
      ? [`every agent can consult: ${agents.join(', ')}`]
      : ['no coding agent found on PATH yet']
  }
  const others = agents.filter((agent) => agent !== mode)
  const lines = [`${mode} can consult, and gets your conversation as context`]
  if (others.length > 0) {
    lines.push(`${others.join(', ')}: no ConsensFlow in this mode — switch to cmux to include them`)
  }
  return lines
}

/**
 * ConsensFlow off: every file it installed removed, both host payloads gone,
 * no mode. The counterpart to choosing a path — and the only way to leave
 * no trace without uninstalling the app itself.
 */
export function turnOff(env, options = {}) {
  const changes = []
  for (const host of ['claude', 'pi']) changes.push(...dropHost(host, env))
  changes.push(...uninstallSkills(env, { force: options.force }))
  try {
    rmSync(statePath(env), { force: true })
  } catch {
    // Never written, or already gone.
  }
  return { mode: null, changes }
}

export function applyMode(rawMode, env, options = {}) {
  const mode = canonical(rawMode)
  if (!MODES.includes(mode)) {
    throw new Error(`unknown mode ${JSON.stringify(rawMode)}; expected ${MODES.join(', ')}`)
  }

  const changes = []
  if (mode === 'cmux') {
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

  // cmux's own skills (pane control) come with every mode: they teach the
  // agent to drive the terminal it lives in, which is useful whichever path
  // consults. A machine with no network still gets its mode — it is told
  // what it missed instead of losing the switch.
  const report = modeReport(mode, env)
  try {
    const cmux = installCmuxSkills(env, { force: options.force })
    changes.push(...cmux.report)
  } catch (cause) {
    report.push(
      `cmux skills were not fetched (${cause instanceof Error ? cause.message.split('(')[0].trim() : cause}) — run \`consensflow skills update\` when you are online`,
    )
  }

  rememberMode(mode, env)
  return { mode, changes, report }
}
