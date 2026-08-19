import { readFileSync } from 'node:fs'
import { installSkill } from './install.js'
import { loadManifest, saveManifest, sha256 } from './manifest.js'
import { listParticipants, RUNTIMES, rosterPath } from './roster.js'
import { generateSkill } from './skill.js'

/**
 * The one place the roster becomes the installed skill.
 *
 * Two callers, two policies:
 * - `refreshInstalledSkill` — the mutation path (cf participant …, cf ui):
 *   installs on the first participant, regenerates on every change after.
 * - `healSkillIfStale` — the read path (any other cf invocation): the roster
 *   file is shared with consensflow-cc and consensflow-pi, which write it
 *   without telling v3. The manifest remembers the roster hash each
 *   generation used; when the file has moved on, the skill is regenerated —
 *   but only where it is already installed, so an uninstall stays uninstalled.
 */

export function refreshInstalledSkill(env) {
  const participants = listParticipants(env)
  if (!participants.some((p) => RUNTIMES.includes(p.runtime))) return
  installSkill(
    {
      relPath: 'consensflow/SKILL.md',
      content: generateSkill(participants),
      source: 'consensflow',
    },
    env,
  )
  recordRosterSha(env)
}

export function healSkillIfStale(env) {
  const manifest = loadManifest(env)
  const installed = Object.values(manifest.files).some((file) => file.source === 'consensflow')
  if (!installed) return

  let currentSha
  try {
    currentSha = sha256(readFileSync(rosterPath(env)))
  } catch {
    return
  }
  if (currentSha === manifest.rosterSha) return
  refreshInstalledSkill(env)
}

function recordRosterSha(env) {
  const manifest = loadManifest(env)
  try {
    manifest.rosterSha = sha256(readFileSync(rosterPath(env)))
  } catch {
    return
  }
  saveManifest(manifest, env)
}
