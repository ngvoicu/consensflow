import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { detectHarnesses } from './harnesses.js'
import { installSkill, skillsSummary, uninstallSkills } from './install.js'
import { loadManifest, saveManifest, sha256 } from './manifest.js'
import { currentMode, scopeTargets } from './mode.js'
import { HARNESSES, listAgents, rosterPath } from './roster.js'
import { generateSkill } from './skill.js'
import { installTerminalCommand, terminalRuntime } from './terminal.js'

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

/**
 * Installed skill files whose text this version would no longer write.
 *
 * A new ConsensFlow reaches the machine as a new app bundle, and the skill it
 * would generate can differ from the one already sitting in the harnesses —
 * the template changed. Nothing regenerated it: `refreshInstalledSkill` runs
 * on a roster mutation and `healSkillIfStale` compares the ROSTER hash, so an
 * upgrade with an untouched roster left every lead reading the previous
 * version's prose while `cf skills status` said `ok` and `cf doctor` counted
 * the files and called them ours. Both were true and neither was the answer.
 *
 * A file the user edited is NOT stale — it is theirs, and drift stays sacred.
 * This only reports; refreshing is `cf skills install` or the page's button,
 * because it writes into someone else's harness.
 */
export function staleSkills(env) {
  const agents = listAgents(env)
  if (!agents.some((p) => HARNESSES.includes(p.harness))) return []
  let content
  try {
    content = generateSkill(agents, { mode: currentMode(env) })
  } catch {
    return []
  }
  const manifest = loadManifest(env)
  return Object.entries(manifest.files)
    .filter(([, recorded]) => recorded.source === 'consensflow')
    .filter(([path, recorded]) => {
      let onDisk
      try {
        onDisk = readFileSync(path, 'utf8')
      } catch {
        return false
      }
      // Edited by the user, so not ours to call out of date.
      if (sha256(onDisk) !== recorded.sha256) return false
      return onDisk !== content
    })
    .map(([path]) => path)
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

/**
 * What opening the app puts right, without being asked.
 *
 * Both halves used to be buttons and nothing else, and both went wrong
 * silently. A new app brings a new skill template and rewrites nothing, so
 * every lead kept reading the previous version's prose until somebody noticed
 * a number on a panel (2026-08-27). The command is worse: the skill it
 * installs teaches `cf run @name`, so a machine with the skill and no launcher
 * has a product that answers nothing, and the page showed a mute Install
 * button beside it.
 *
 * Opening the app is a deliberate act — the same act as pressing the button —
 * so it now does what the buttons do, under three limits that keep it from
 * being a surprise:
 *
 * - nothing before a mode is chosen. An app opened on a machine that has not
 *   picked a path still installs nothing at all.
 * - the command is claimed outright. It names one ConsensFlow absolutely, and
 *   the app you opened is the one you want answering `cf run` — including when
 *   it currently names another install. This machine is meant to hold one.
 * - a skill file is rewritten even where it was edited, because the skill is
 *   generated from the roster rather than kept by the user, and an edited copy
 *   makes every agent reading it answer for a roster that has moved. A file we
 *   never installed is still refused: that one is somebody else's.
 *
 * Returns what it did, so the page can say it out loud. A write nobody
 * reported is exactly the kind of quiet this function exists to end.
 */
export function healOnOpen(env) {
  const mode = currentMode(env)
  if (mode === null) return { mode: null, command: 'no mode chosen', skills: 0 }

  const wiring = terminalRuntime(env)
  let command = wiring !== null && wiring.exists && wiring.mine ? 'ok' : 'claimed'
  if (command === 'claimed') {
    try {
      installTerminalCommand(env)
    } catch (cause) {
      command = cause instanceof Error ? cause.message : String(cause)
    }
  }

  // Behind this version, missing from a harness in scope, or edited — all
  // three are the installed skill not saying what this ConsensFlow says.
  const drifted = skillsSummary(env).drifted
  const behind = staleSkills(env).length + skillGaps(env).length + drifted
  if (behind > 0) refreshInstalledSkill(env)
  // `replaced` is counted apart from the rest on purpose: bringing a file up to
  // date is the app doing its job and needs no announcement — the panel already
  // shows where the command points and that nothing is behind. Overwriting an
  // edit is the one thing here a user could regret, so it is the one thing
  // said out loud.
  return { mode, command, skills: behind, replaced: drifted }
}
