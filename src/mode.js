import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { installCmuxSkills } from './cmux-skills.js'
import { detectHarnesses } from './harnesses.js'
import { hostStatus, installHost, uninstallHost } from './hosts.js'
import { installSkill, uninstallSkills } from './install.js'
import { configRoot, listAgents } from './roster.js'
import { generateSkill } from './skill.js'

/**
 * One ConsensFlow path per machine.
 *
 * Three modes, named after the three things they are, mutually exclusive by
 * construction:
 * - `claude` — the Claude Code integration, which hands an agent the
 *              live conversation. Only Claude Code can consult.
 * - `pi`     — the same, inside pi. Only pi can consult.
 * - `cmux`   — the generated skill across every coding harness (pi, cc, codex,
 *              opencode). Every harness can consult; none gets the
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

/** Every harness the cmux path can teach — the parenthetical the UI shows. */
const CMUX_HARNESSES = ['pi', 'cc', 'codex', 'opencode']

export function modeLabel(mode) {
  return mode === 'cmux' ? `cmux (${CMUX_HARNESSES.join(', ')})` : mode
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

/**
 * cmux's own skills — pane control, browser surfaces, its settings — belong
 * to the cmux path and only to it.
 *
 * They have nothing to do with consulting: a Claude Code install runs its
 * agents as subprocesses and never touches a pane. So the mode named
 * after cmux carries them, and a host mode carries none. Anyone outside the
 * target set gives them back, because a rule that only ever added would
 * leave every harness it had once touched holding files it has no use for.
 */
export function syncCmuxSkills(env, options = {}) {
  const mode = options.mode ?? currentMode(env)
  const targets = mode === 'cmux' ? detectHarnesses(env) : []
  const keep = targets.map((harness) => harness.skillsDir)

  const report = uninstallSkills(env, {
    filter: (path, recorded) =>
      recorded.source.startsWith('cmux@') && !keep.some((dir) => path.startsWith(dir)),
  })
  // No target, no clone: a host mode should not reach the network for files
  // it has already decided nobody gets.
  if (targets.length === 0) return { commit: null, report }

  const cmux = installCmuxSkills(env, { force: options.force, targets })
  return { commit: cmux.commit, report: [...report, ...cmux.report] }
}

/** Removes the generated skill from every harness that has ours. */
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
  const harnesses = detectHarnesses(env).map((harness) => harness.id)
  if (mode === 'cmux') {
    return harnesses.length > 0
      ? [`every harness can consult: ${harnesses.join(', ')}`]
      : ['no coding harness found on PATH yet']
  }
  const others = harnesses.filter((harness) => harness !== mode)
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
  // Off means off: the bookkeeping goes too, so nothing is left claiming
  // state that no longer exists. The roster is untouched — agents are
  // the user's, shared with anything else that reads them.
  const root = configRoot(env)
  for (const name of ['mode.json', 'hosts.json', 'skills-manifest.json']) {
    rmSync(join(root, name), { force: true })
  }
  rmSync(join(root, 'hosts'), { recursive: true, force: true })
  try {
    if (readdirSync(root).length === 0) rmSync(root, { recursive: true, force: true })
  } catch {
    // Already gone, or something else lives there — either is fine.
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
    const agents = listAgents(env)
    if (agents.length > 0) {
      // A foreign integration still owns its harness; skip that one and say so
      // rather than stacking a second skill on it.
      const targets = detectHarnesses(env).filter((harness) => harness.native !== true)
      changes.push(
        ...installSkill(
          {
            relPath: 'consensflow/SKILL.md',
            content: generateSkill(agents),
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

  // A machine with no network still gets its mode — it is told what it
  // missed instead of losing the switch.
  const report = modeReport(mode, env)
  try {
    changes.push(...syncCmuxSkills(env, { mode, force: options.force }).report)
  } catch (cause) {
    report.push(
      `cmux skills were not fetched (${cause instanceof Error ? cause.message.split('(')[0].trim() : cause}) — run \`consensflow skills update\` when you are online`,
    )
  }

  rememberMode(mode, env)
  return { mode, changes, report }
}
