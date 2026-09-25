import { readFileSync } from 'node:fs'
import { teamTable, workTierList } from '../skill.js'

/**
 * The instructions each window of the new core starts with, one text per role
 * (`skill/core/<role>.md`). The chief also gets the rules for choosing the tier
 * that does the work, with the staff it has.
 */
const ROLES = ['chief', 'advisor', 'worker', 'reviewer', 'designer']
const text = (name) => readFileSync(new URL(`../../skill/core/${name}.md`, import.meta.url), 'utf8')

export function roleInstructions(role, staff) {
  if (!ROLES.includes(role)) throw new Error(`no role instructions for ${role}`)
  const base = text(role)
  if (role !== 'chief') return base
  return (
    base +
    text('coordinating').replace('{{tiers}}', workTierList()).replace('{{staff}}', teamTable(staff))
  )
}
