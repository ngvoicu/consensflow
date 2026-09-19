import { readFileSync } from 'node:fs'
import { teamTable, workTierList } from '../skill.js'

/**
 * The instructions each window of the new core starts with, one text per role
 * (`skill/core/<role>.md`). Coordinators (lead, PM) also get the rules for
 * choosing who does the work and for cross-model review, with the session team
 * they choose from: the lead's workers and reviewers, the PM's advisors.
 */
const ROLES = ['lead', 'pm', 'advisor', 'worker', 'reviewer']
const COORDINATORS = { lead: 'workers', pm: 'advisors' }
const text = (name) => readFileSync(new URL(`../../skill/core/${name}.md`, import.meta.url), 'utf8')

export function roleInstructions(role, team) {
  if (!ROLES.includes(role)) throw new Error(`no role instructions for ${role}`)
  const base = text(role)
  const noun = COORDINATORS[role]
  if (noun === undefined) return base
  return (
    base +
    text('coordinating')
      .replaceAll('{{noun}}', noun)
      .replace('{{tiers}}', workTierList())
      .replace('{{team}}', teamTable(team, noun))
  )
}
