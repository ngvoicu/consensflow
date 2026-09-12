import { mkdirSync, readdirSync, rmdirSync, rmSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { fileState, loadManifest, saveManifest, sha256 } from './manifest.js'
import { prepareOpenCodeExtension } from './opencode-install.js'
import { preparePiExtension } from './pi-install.js'
import { configRoot, listAgents } from './roster.js'
import { generateSkill } from './skill.js'
import { installTerminalCommand, terminalRuntime } from './terminal.js'

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
  return {
    changes,
    report,
    command,
    piExtension: preparePiExtension(env),
    opencodeExtension: prepareOpenCodeExtension(env),
  }
}
