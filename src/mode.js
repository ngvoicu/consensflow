import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { detectHarnesses, knownHarnesses } from './harnesses.js'
import { retireHostPayloads } from './host-payloads.js'
import { installSkill, uninstallSkills } from './install.js'
import { configRoot, listAgents } from './roster.js'
import { generateSkill } from './skill.js'
import { installTerminalCommand, removeTerminalCommand } from './terminal.js'

/**
 * One ConsensFlow path per machine.
 *
 * A mode is a scope over one generated skill — who on this machine can
 * consult, and nothing else:
 * - `claude` — only Claude Code gets the skill.
 * - `pi`     — only pi gets it.
 * - `cmux`   — every detected harness gets it (pi, cc, codex, opencode), plus
 *              cmux's own pane-control skills. Called `standalone` until
 *              2026-08-20; the old name still normalizes to this one.
 *
 * `claude` and `pi` used to be integrations rather than scopes, each with a
 * payload and a hand-written skill of its own, because a host integration
 * could hand an agent the live conversation. Nothing has stashed a
 * conversation since 2026-08-22, and what was left was a second copy of the
 * skill that could not name the agents on this roster. One skill now, one
 * install path, and the mode decides who gets it.
 *
 * Switching modes takes the skill back from whoever is no longer in scope, so
 * two paths can never be live at once. A harness that ships its own
 * ConsensFlow is left alone.
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
 * Take-back only, never installs — like the host payloads before it.
 *
 * ConsensFlow used to clone cmux's whole skills tree and install it into
 * every harness in cmux mode: 20 skills, ~300 files, of which 18 were docs
 * for developing cmux itself — billing, release, localization. A consulting
 * lead needs three pane commands and a way to find the pane again, and the
 * generated skill quotes all four. Shipping a fifth of a repo into four
 * skills budgets for that was the tail wagging the dog.
 *
 * So ConsensFlow ships ONE skill — its own — and this takes back what the
 * cloning era installed: every manifest-owned file whose source is a cmux
 * commit, plus the checkout cache. Drift is still sacred: a cmux skill the
 * user edited is refused without force, exactly like ours.
 */
export function syncCmuxSkills(env, options = {}) {
  const report = uninstallSkills(env, {
    force: options.force,
    filter: (_path, recorded) => recorded.source.startsWith('cmux@'),
  })
  const cache = join(configRoot(env), 'cache', 'cmux')
  if (existsSync(cache)) {
    rmSync(cache, { recursive: true, force: true })
    report.push({ action: 'removed', path: cache })
  }
  return { commit: null, report }
}

/**
 * The generated skill, in exactly the harnesses this mode covers.
 *
 * A mode is a scope and nothing more. It used to be more than that: `claude`
 * and `pi` installed a hand-written skill through a payload of their own,
 * which was justified while a host integration could hand an agent the live
 * conversation. Nothing has stashed a conversation since 2026-08-22, so all
 * that remained was a second, worse copy of the skill — worse because it is
 * written ahead of time and cannot name the agents on this roster, and the
 * roster names in the description are what make a harness reach for it.
 *
 * So there is one skill now, generated from the roster, and the mode decides
 * who gets it. Anyone outside the target set gives it back.
 */
/**
 * Who the current mode puts in scope — the single answer, for the mode switch
 * and for every roster mutation alike.
 *
 * `null` (no mode chosen) is nobody: installing into every harness found would
 * put ConsensFlow in Claude Code without anyone choosing Claude Code. Choosing
 * the path is what installs it.
 */
export function scopeTargets(env, options = {}) {
  const mode = options.mode ?? currentMode(env)
  if (mode === null) return []
  // A harness shipping its own ConsensFlow keeps it: two entries with one name
  // and one trigger would compete for the same skills budget. `all` overrides.
  const detected = detectHarnesses(env).filter(
    (harness) => options.all === true || harness.native !== true,
  )
  return mode === 'cmux' ? detected : detected.filter((harness) => harness.id === mode)
}

function syncGeneratedSkill(env, options = {}) {
  const mode = options.mode ?? currentMode(env)
  const targets = scopeTargets(env, { mode })
  // What this mode covers, whether or not the CLI resolved on this run. Only a
  // change of MODE should take a skill back — not a PATH that happened to be
  // narrower this time, which is what made opencode flap in and out.
  const keep = knownHarnesses(env)
    .filter((harness) => mode === 'cmux' || harness.id === mode)
    .map((harness) => harness.skillsDir)

  const report = uninstallSkills(env, {
    filter: (path, recorded) =>
      recorded.source === 'consensflow' && !keep.some((dir) => path.startsWith(dir)),
  })

  // Nothing to teach yet, or nobody to teach it to. `generateSkill` refuses an
  // empty roster, and the first `cf agent add` installs it everywhere it goes.
  const agents = listAgents(env)
  if (targets.length === 0 || agents.length === 0) return report

  return [
    ...report,
    ...installSkill(
      {
        relPath: 'consensflow/SKILL.md',
        content: generateSkill(agents, { mode }),
        source: 'consensflow',
      },
      env,
      { targets, force: options.force },
    ),
  ]
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
  const lines = [`${mode} can consult, via the generated skill`]
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
  changes.push(...retireHostPayloads(env))
  changes.push(...uninstallSkills(env, { force: options.force }))
  // The launcher is ours when it says so; someone else's `cf` is left alone.
  for (const path of removeTerminalCommand(env).removed) {
    changes.push({ action: 'removed', path })
  }
  // Off means off: the bookkeeping goes too, so nothing is left claiming
  // state that no longer exists. The roster is untouched — agents are
  // the user's, shared with anything else that reads them.
  const root = configRoot(env)
  for (const name of ['mode.json', 'hosts.json', 'skills-manifest.json']) {
    rmSync(join(root, name), { force: true })
  }
  rmSync(join(root, 'hosts'), { recursive: true, force: true })
  // Off means off, and a clone of someone else's repository is not state worth
  // keeping for a machine that has stopped consulting.
  rmSync(join(root, 'cache'), { recursive: true, force: true })
  try {
    if (readdirSync(root).length === 0) rmSync(root, { recursive: true, force: true })
  } catch {
    // Already gone, or something else lives there — either is fine.
  }

  return { mode: null, changes }
}

/**
 * Factory reset: the machine as if ConsensFlow had never been installed.
 *
 * `off` stops deliberately short of this. It keeps the roster, because agents
 * are the user's and the file is shared with anything else that reads it, and
 * it keeps `workspaces/`, where every run's packet, transcript and generated
 * image lives. Those are exactly what this removes, which is the whole
 * difference between the two buttons and the reason this one asks again:
 * a roster is typed by hand and a run artifact exists nowhere else.
 *
 * Drift is not honoured here, and that is on purpose. The rule exists so an
 * install never clobbers an edit by accident; this is the one operation whose
 * entire point is to leave nothing behind, and it is only reached by saying so.
 */
export function resetEverything(env, options = {}) {
  const removed = resetPreview(env)
  const outcome = turnOff(env, { ...options, force: true })
  // turnOff prunes the root only when it is already empty — and the roster and
  // the run artifacts are precisely what kept it from being.
  rmSync(configRoot(env), { recursive: true, force: true })

  // The desktop app's own data, which lives outside the root because the OS
  // decides where a bundle keeps it. Nothing else creates these directories —
  // they carry ConsensFlow's bundle identifier — so a reset that left them
  // would be leaving something behind.
  for (const dir of appDataDirs(env)) {
    if (!existsSync(dir)) continue
    rmSync(dir, { recursive: true, force: true })
    outcome.changes.push({ action: 'removed', path: dir })
  }

  return { ...outcome, removed }
}

/** The app bundle's identifier, and the per-OS directories the webview fills. */
const APP_ID = 'dev.ngvoicu.consensflow'

/**
 * Where the desktop app's own data lands, per platform.
 *
 * The `.app` itself is deliberately not here. An application deleting its own
 * bundle while running is a bad idea, and on macOS removing an application is
 * something the user does in Finder — not something a CLI should do behind
 * their back. The reset says so rather than doing it.
 */
function appDataDirs(env) {
  const home = env.HOME ?? env.USERPROFILE
  if (home === undefined) return []
  if ((env.OS ?? '').toLowerCase().includes('windows') || process.platform === 'win32') {
    const local = env.LOCALAPPDATA ?? join(home, 'AppData', 'Local')
    const roaming = env.APPDATA ?? join(home, 'AppData', 'Roaming')
    return [join(local, APP_ID), join(roaming, APP_ID)]
  }
  if (process.platform === 'darwin') {
    return [
      join(home, 'Library', 'Caches', APP_ID),
      join(home, 'Library', 'WebKit', APP_ID),
      join(home, 'Library', 'Application Support', APP_ID),
      join(home, 'Library', 'Saved Application State', `${APP_ID}.savedState`),
    ]
  }
  return [
    join(env.XDG_CACHE_HOME ?? join(home, '.cache'), APP_ID),
    join(env.XDG_DATA_HOME ?? join(home, '.local', 'share'), APP_ID),
    join(env.XDG_CONFIG_HOME ?? join(home, '.config'), APP_ID),
  ]
}

/**
 * What a reset would destroy, without destroying it — so `cf reset`'s refusal
 * and the page's dialog quote the same two numbers, from the same place.
 */
export function resetPreview(env) {
  return { agents: listAgents(env).length, runs: countRuns(env) }
}

/** Runs on disk, counted across every workspace this machine has spawned in. */
function countRuns(env) {
  const workspaces = join(configRoot(env), 'workspaces')
  let total = 0
  for (const workspace of safeReaddir(workspaces)) {
    total += safeReaddir(join(workspaces, workspace, 'runs')).length
  }
  return total
}

function safeReaddir(dir) {
  try {
    return readdirSync(dir)
  } catch {
    return []
  }
}

export function applyMode(rawMode, env, options = {}) {
  const mode = canonical(rawMode)
  if (!MODES.includes(mode)) {
    throw new Error(`unknown mode ${JSON.stringify(rawMode)}; expected ${MODES.join(', ')}`)
  }

  const changes = []
  // Every mode teaches the same line — `cf run @name "<task>"` — so `cf` has
  // to exist in every mode: it is part of the path, not an optional extra, and
  // `off` takes it back. Without it the skill's `cf run` lines name a command
  // the machine does not have.
  let launcherProblem
  try {
    installTerminalCommand(env)
  } catch (cause) {
    launcherProblem = cause instanceof Error ? cause.message : String(cause)
  }

  // Whatever an older version wired into a harness's own config — Claude
  // Code's command and hooks, pi's registered extension — is taken back before
  // anything is installed, in every mode. It has no counterpart now.
  changes.push(...retireHostPayloads(env))

  changes.push(...syncGeneratedSkill(env, { mode, force: options.force }))

  // A machine with no network still gets its mode — it is told what it
  // missed instead of losing the switch.
  const report = modeReport(mode, env)
  if (launcherProblem !== undefined) {
    report.push(
      `\`cf\` could not be placed on PATH (${launcherProblem}) — the skill's \`cf run\` lines will not work until it is`,
    )
  }
  // Nothing to fetch any more: the only cmux-skills work left is taking back
  // what the cloning era installed, and that never needs the network.
  changes.push(...syncCmuxSkills(env, { force: options.force }).report)

  rememberMode(mode, env)
  return { mode, changes, report }
}
