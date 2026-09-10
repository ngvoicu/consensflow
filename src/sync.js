import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { installEverywhere, installSkill } from './install.js'
import { loadManifest, sha256 } from './manifest.js'
import { configRoot, listAgents } from './roster.js'
import { generateSkill } from './skill.js'

export function skillTargets(env) {
  return [
    { id: 'consensflow', skillsDir: join(configRoot(env), 'roles', 'lead', '.claude', 'skills') },
  ]
}

// Old global documents are removed manually, never by opening/updating the app.
export function retireSkillFromNativeHosts() {
  return []
}

export function skillGaps(env) {
  const file = join(skillTargets(env)[0].skillsDir, 'consensflow-lead', 'SKILL.md')
  try {
    readFileSync(file)
    return []
  } catch {
    return ['consensflow-lead']
  }
}

export function staleSkills(env) {
  const content = generateSkill(listAgents(env))
  const root = `${join(configRoot(env), 'roles')}/`
  return Object.entries(loadManifest(env).files)
    .filter(([file, entry]) => file.startsWith(root) && entry.source === 'consensflow')
    .filter(([file, entry]) => {
      try {
        const text = readFileSync(file, 'utf8')
        return sha256(text) === entry.sha256 && text !== content
      } catch {
        return false
      }
    })
    .map(([file]) => file)
}

export function refreshInstalledSkill(env) {
  return installSkill({ content: generateSkill(listAgents(env)), source: 'consensflow' }, env)
}

export function healSkillIfStale(env) {
  if (staleSkills(env).length) refreshInstalledSkill(env)
}

export function healOnOpen(env) {
  const missing = skillGaps(env).length + staleSkills(env).length
  const outcome = installEverywhere(env)
  return { command: outcome.command, skills: missing, replaced: 0 }
}
