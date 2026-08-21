import { readFileSync } from 'node:fs'
import { detectHarnesses } from './harnesses.js'
import { installSkill, uninstallSkills } from './install.js'
import { loadManifest, saveManifest, sha256 } from './manifest.js'
import { currentMode } from './mode.js'
import { HARNESSES, listAgents, rosterPath } from './roster.js'
import { generateSkill } from './skill.js'

/**
 * The one place the roster becomes the installed skill.
 *
 * Two callers, two policies:
 * - `refreshInstalledSkill` — the mutation path (cf agent …, cf ui):
 *   installs on the first agent, regenerates on every change after.
 * - `healSkillIfStale` — the read path (any other cf invocation): the roster
 *   file is shared with consensflow-cc and consensflow-pi, which write it
 *   without telling v3. The manifest remembers the roster hash each
 *   generation used; when the file has moved on, the skill is regenerated —
 *   but only where it is already installed, so an uninstall stays uninstalled.
 */

/**
 * Where the generated skill belongs: every detected harness except the ones
 * that already ship their own ConsensFlow. `all` installs regardless.
 */
export function skillTargets(env, { all = false } = {}) {
  // The generated skill belongs to cmux mode and nowhere else.
  //
  // A host mode means one harness consults and the rest do not, so a roster edit
  // must not quietly undo it. No mode at all means the same thing for a
  // different reason: nobody has chosen a path yet, and installing into every
  // harness found would put ConsensFlow in Claude Code without anyone choosing
  // Claude Code. Choosing the path is what installs it.
  const mode = currentMode(env)
  if (mode !== 'cmux') return []

  const harnesses = detectHarnesses(env)
  return all ? harnesses : harnesses.filter((harness) => harness.native !== true)
}

/**
 * Removes a generated skill we installed on a host that has since gained its
 * own ConsensFlow (the cc plugin, the pi extension) — otherwise an upgrade
 * would leave two same-named skills competing there forever. Only our own,
 * only unedited: a file the user changed is theirs and stays.
 */
export function retireSkillFromNativeHosts(env) {
  const nativeDirs = detectHarnesses(env)
    .filter((harness) => harness.native === true)
    .map((harness) => harness.skillsDir)
  if (nativeDirs.length === 0) return []

  return uninstallSkills(env, {
    filter: (path, recorded) =>
      recorded.source === 'consensflow' && nativeDirs.some((dir) => path.startsWith(dir)),
  }).map((row) => ({
    ...row,
    action: row.action === 'refused-drifted' ? 'kept-drifted' : 'retired',
  }))
}

export function refreshInstalledSkill(env) {
  const agents = listAgents(env)
  if (!agents.some((p) => HARNESSES.includes(p.harness))) return
  const targets = skillTargets(env)
  if (targets.length === 0) return
  installSkill(
    {
      relPath: 'consensflow/SKILL.md',
      content: generateSkill(agents),
      source: 'consensflow',
    },
    env,
    { targets: skillTargets(env) },
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
