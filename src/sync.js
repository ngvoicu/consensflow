import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { detectHarnesses } from './harnesses.js'
import { installSkill, uninstallSkills } from './install.js'
import { loadManifest, saveManifest, sha256 } from './manifest.js'
import { currentMode, scopeTargets } from './mode.js'
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
 * Where the generated skill belongs: exactly who the mode puts in scope.
 *
 * This used to answer "cmux mode, or nobody", which was right while `claude`
 * and `pi` were integrations installing a hand-written skill of their own.
 * They are scopes over this same skill now, so a roster edit in a host mode
 * has to reach that one harness — otherwise choosing `claude` and adding the
 * first agent leaves the machine with no skill anywhere.
 */
export function skillTargets(env, { all = false } = {}) {
  return scopeTargets(env, { all })
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

/**
 * Harnesses the mode puts in scope that are carrying no skill of ours.
 *
 * `installSkill` refuses a path it does not own and records nothing — correct,
 * because the file is someone else's. But the refusal is one row among
 * hundreds in an install report, so a harness can end up silently with no
 * skill while everything around it reports success. That is a positive check
 * of who SHOULD have it against who DOES, so it catches the gap however it
 * arose: a refusal, a hand-deleted file, an install that ran with a narrower
 * PATH than the one you have now.
 */
export function skillGaps(env) {
  const owned = new Set(
    Object.entries(loadManifest(env).files)
      .filter(([, recorded]) => recorded.source === 'consensflow')
      .map(([path]) => path),
  )
  if (listAgents(env).length === 0) return []
  return skillTargets(env)
    .filter((harness) => !owned.has(join(harness.skillsDir, 'consensflow', 'SKILL.md')))
    .map((harness) => harness.id)
}

export function refreshInstalledSkill(env) {
  const agents = listAgents(env)
  if (!agents.some((p) => HARNESSES.includes(p.harness))) return
  const targets = skillTargets(env)
  if (targets.length === 0) return
  installSkill(
    {
      relPath: 'consensflow/SKILL.md',
      content: generateSkill(agents, { mode: currentMode(env) }),
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
