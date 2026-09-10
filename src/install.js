import { existsSync, mkdirSync, readdirSync, rmdirSync, rmSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { detectHarnesses, knownHarnesses } from './harnesses.js'
import { fileState, loadManifest, saveManifest, sha256 } from './manifest.js'
import { preparePiExtension } from './pi-install.js'
import { configRoot, listAgents } from './roster.js'
import { generateSkill } from './skill.js'
import { installTerminalCommand, removeTerminalCommand, terminalRuntime } from './terminal.js'

/** Legacy install entry point now writes only ConsensFlow's private role document. */
export function installSkill({ content, source }, env) {
  if (source !== 'consensflow') return []
  const path = join(
    configRoot(env),
    'roles',
    'lead',
    '.claude',
    'skills',
    'consensflow-lead',
    'SKILL.md',
  )
  const manifest = loadManifest(env)
  const unchanged =
    manifest.files[path]?.sha256 === sha256(content) &&
    fileState(path, manifest.files[path]) === 'ok'
  if (!unchanged) {
    mkdirSync(dirname(path), { recursive: true, mode: 0o700 })
    writeFileSync(path, content, { mode: 0o600 })
  }
  manifest.files[path] = { sha256: sha256(content), source }
  saveManifest(manifest, env)
  return [{ harness: 'consensflow', path, action: unchanged ? 'unchanged' : 'installed' }]
}

/**
 * What is installed, counted the way a reader asks about it.
 *
 * The manifest counts FILES, because that is what ownership is tracked at — a
 * skill is a directory, and cmux-browser alone is eleven files. Reporting the
 * file count alone invited the obvious question ("312 skills?"), so this
 * reports both, and splits ours from the ones we install on cmux's behalf.
 */
export function skillsSummary(env) {
  const rows = skillsStatus(env)
  const dirs = [
    ...knownHarnesses(env),
    { id: 'consensflow', skillsDir: join(configRoot(env), 'roles', 'lead', '.claude', 'skills') },
  ]
  const skills = new Set()
  let ours = 0
  let cmux = 0
  let cmuxCommit = null

  for (const row of rows) {
    if (row.source === 'consensflow') ours += 1
    else {
      cmux += 1
      cmuxCommit = row.source
    }
    const dir = dirs.find((harness) => row.path.startsWith(`${harness.skillsDir}/`))
    if (dir === undefined) continue
    const name = row.path.slice(dir.skillsDir.length + 1).split('/')[0]
    skills.add(`${dir.id}/${name}`)
  }

  const harnesses = new Set([...skills].map((entry) => entry.split('/')[0]))
  return {
    files: rows.length,
    ours,
    cmux,
    cmuxCommit: cmuxCommit === null ? null : cmuxCommit.slice('cmux@'.length),
    skills: skills.size,
    harnesses: harnesses.size,
    perHarness: harnesses.size === 0 ? 0 : Math.round(skills.size / harnesses.size),
    drifted: rows.filter((row) => row.state === 'drifted').length,
    missing: rows.filter((row) => row.state === 'missing').length,
  }
}

/** One row per manifest-owned file: where it is and whether it is intact. */
export function skillsStatus(env) {
  const manifest = loadManifest(env)
  return Object.entries(manifest.files).map(([path, recorded]) => ({
    path,
    source: recorded.source,
    state: fileState(path, recorded),
  }))
}

/**
 * Removes every manifest-owned file, or the subset `options.filter` accepts.
 * A drifted file is refused without force (the user's edits are theirs);
 * anything not in the manifest is never touched. Emptied skill directories
 * are cleaned up.
 */
export function uninstallSkills(env, options = {}) {
  const manifest = loadManifest(env)
  const report = []

  for (const [path, recorded] of Object.entries(manifest.files)) {
    if (!resolve(path).startsWith(`${resolve(configRoot(env), 'roles')}/`)) {
      report.push({ path, action: 'manual-cleanup-required' })
      continue
    }
    // A filter narrows the sweep (retiring one skill from one host, say);
    // without it every owned file goes.
    if (options.filter !== undefined && !options.filter(path, recorded)) continue
    const state = fileState(path, recorded)
    if (state === 'drifted' && options.force !== true) {
      report.push({ path, action: 'refused-drifted' })
      continue
    }
    rmSync(path, { force: true })
    // Climb away every directory the removal emptied, stopping at the harness's
    // skills root — a skill is a directory tree, and leaving hollow shells
    // behind reads as "still installed" in every harness's skill picker.
    let parent = dirname(path)
    while (basename(parent) !== 'skills') {
      try {
        if (readdirSync(parent).length > 0) break
        rmdirSync(parent)
      } catch {
        break
      }
      parent = dirname(parent)
    }
    delete manifest.files[path]
    report.push({ path, action: state === 'missing' ? 'already-gone' : 'removed' })
  }

  saveManifest(manifest, env)
  return report
}

/** Every detected harness without its own ConsensFlow receives the same skill. */
export function scopeTargets(env, { all = false } = {}) {
  return detectHarnesses(env).filter((harness) => all || harness.native !== true)
}

/** Opening the standalone app claims its launcher and refreshes its installation. */
export function installEverywhere(env) {
  const changes = []
  const wiring = terminalRuntime(env)
  let command = wiring?.exists && wiring.mine ? 'ok' : 'claimed'
  const report = []
  try {
    installTerminalCommand(env)
  } catch (cause) {
    command = cause instanceof Error ? cause.message : String(cause)
    report.push(`The cf launcher could not be installed: ${command}`)
  }
  changes.push(
    ...installSkill({ content: generateSkill(listAgents(env)), source: 'consensflow' }, env),
  )
  return { changes, report, command, piExtension: preparePiExtension(env) }
}

const APP_ID = 'dev.ngvoicu.consensflow'

export function syncCmuxSkills() {
  return { report: [], notice: 'Global skills are managed manually.' }
}

export function turnOff(env, options = {}) {
  const changes = []
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

  return { changes }
}

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

export function resetPreview(env) {
  return { agents: listAgents(env).length, runs: countRuns(env) }
}

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
