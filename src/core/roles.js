import { readFileSync } from 'node:fs'
import { teamTable, workTierList } from '../skill.js'

/**
 * The instructions each window of the new core starts with, one text per role
 * (`skill/core/<role>.md`). The lead also gets the rules for choosing the tier
 * that does the work, with the team it has.
 */
const ROLES = ['lead', 'advisor', 'worker', 'reviewer', 'designer']
const text = (name) => readFileSync(new URL(`../../skill/core/${name}.md`, import.meta.url), 'utf8')

export function roleInstructions(role, team) {
  if (!ROLES.includes(role)) throw new Error(`no role instructions for ${role}`)
  const base = text(role)
  if (role !== 'lead') return base
  return (
    base +
    text('coordinating').replace('{{tiers}}', workTierList()).replace('{{team}}', teamTable(team))
  )
}
