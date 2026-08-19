import { existsSync, mkdirSync, readdirSync, rmdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { detectAgents } from './agents.js'
import { fileState, loadManifest, saveManifest, sha256 } from './manifest.js'

/**
 * Installs one skill file into every detected agent's skills directory,
 * under the manifest's ownership rules:
 *
 * - a target we own and left unchanged is rewritten freely (that's an update);
 * - a target we own that the user edited is refused without force;
 * - a target we never wrote is refused without force, always;
 * - force backs nothing up silently — the refusal message is the preview.
 */
export function installSkill({ relPath, content, source }, env, options = {}) {
  const manifest = loadManifest(env)
  const report = []

  for (const agent of detectAgents(env)) {
    const path = join(agent.skillsDir, relPath)
    const owned = manifest.files[path]
    const existed = existsSync(path)

    if (existed) {
      if (owned === undefined && options.force !== true) {
        report.push({ agent: agent.id, path, action: 'refused-unowned' })
        continue
      }
      if (owned !== undefined && fileState(path, owned) === 'drifted' && options.force !== true) {
        report.push({ agent: agent.id, path, action: 'refused-drifted' })
        continue
      }
    }

    mkdirSync(dirname(path), { recursive: true })
    writeFileSync(path, content)
    manifest.files[path] = { sha256: sha256(content), source }
    report.push({ agent: agent.id, path, action: owned === undefined ? 'installed' : 'updated' })
  }

  saveManifest(manifest, env)
  return report
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
 * Removes every manifest-owned file. A drifted file is refused without force
 * (the user's edits are theirs); anything not in the manifest is never
 * touched. Emptied skill directories are cleaned up.
 */
export function uninstallSkills(env, options = {}) {
  const manifest = loadManifest(env)
  const report = []

  for (const [path, recorded] of Object.entries(manifest.files)) {
    const state = fileState(path, recorded)
    if (state === 'drifted' && options.force !== true) {
      report.push({ path, action: 'refused-drifted' })
      continue
    }
    rmSync(path, { force: true })
    try {
      if (readdirSync(dirname(path)).length === 0) rmdirSync(dirname(path))
    } catch {
      // The parent was already gone or not empty; either is fine.
    }
    delete manifest.files[path]
    report.push({ path, action: state === 'missing' ? 'already-gone' : 'removed' })
  }

  saveManifest(manifest, env)
  return report
}
