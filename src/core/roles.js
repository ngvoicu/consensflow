import { readFileSync } from 'node:fs'
import { teamTable, workTierList } from '../skill.js'

/**
 * The instructions each window of the new core starts with, one text per role
 * (`skill/core/<role>.md`). The chief also gets the rules for choosing the tier
 * that does the work, with the staff it has.
 */
const ROLES = ['chief', 'advisor', 'worker', 'reviewer', 'designer']
const text = (name) => readFileSync(new URL(`../../skill/core/${name}.md`, import.meta.url), 'utf8')

export function roleInstructions(role, staff, { cf } = {}) {
  if (!ROLES.includes(role)) throw new Error(`no role instructions for ${role}`)
  const base = text(role)
  // A shell that re-reads the user's profile can find another `cf` first:
  // another ConsensFlow's, or the Cloud Foundry CLI (a Devin worker's `cf`
  // was the live app's older one, 2026-09-26). The full path always works.
  const where =
    cf === undefined
      ? ''
      : `\n## This window's cf\n\nHere \`cf\` is ${cf}. If \`cf\` says a command is unknown, or answers as another program, another \`cf\` comes first on this shell's PATH: run ${cf} instead.\n`
  if (role !== 'chief') return base + where
  return (
    base +
    text('coordinating')
      .replace('{{tiers}}', workTierList())
      .replace('{{staff}}', teamTable(staff)) +
    where
  )
}
