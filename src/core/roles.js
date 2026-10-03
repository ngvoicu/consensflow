import { readFileSync } from 'node:fs'
import { teamTable, workTierList } from '../skill.js'

/**
 * The instructions each window the daemon opens starts with, one text per role.
 * The chief's is `skill/core/chief.md`, with the work tiers and the staff it
 * has filled in. Every member's is the shared text of `skill/core/staff.md`
 * with the role's own parts, so a rule all members keep is written once.
 */
const MEMBERS = ['advisor', 'worker', 'reviewer', 'designer']
const text = (name) => readFileSync(new URL(`../../skill/core/${name}.md`, import.meta.url), 'utf8')

/** A project's staff as its chief reads it: each member's name, roles and tier, sessions left out. */
export function staffOf(project) {
  return project.participants
    .filter(
      (member) => member.role !== 'chief' && member.agent !== null && member.memberId === null,
    )
    .map((member) => ({ name: member.handle, roles: member.roles, workTier: member.tier }))
}

/** `staff.md`'s shared text, each `{{slot}}` filled from the part under `<!-- role: slot -->`. */
function memberText(role) {
  const [head, ...parts] = text('staff').split(/^<!-- (\w+): (\w+) -->\n/m)
  const own = { role }
  for (let at = 0; at < parts.length; at += 3) {
    if (parts[at] === role) own[parts[at + 1]] = parts[at + 2].replace(/\n+$/, '')
  }
  return head
    .replace(/^<!--[\s\S]*?-->\n/, '')
    .replace(/\n+$/, '\n')
    .replace(/\{\{(\w+)\}\}/g, (_, slot) => {
      if (own[slot] === undefined) throw new Error(`staff.md has no ${slot} for the ${role} role`)
      return own[slot]
    })
}

export function roleInstructions(role, staff, { cf } = {}) {
  if (role !== 'chief' && !MEMBERS.includes(role)) {
    throw new Error(`no role instructions for ${role}`)
  }
  // A shell that re-reads the user's profile can find another `cf` first:
  // another ConsensFlow's, or the Cloud Foundry CLI (a Devin worker's `cf`
  // was the live app's older one, 2026-09-26). The full path always works.
  const where =
    cf === undefined
      ? ''
      : `\n## This window's cf\n\nHere \`cf\` is ${cf}. If \`cf\` says a command is unknown, or answers as another program, another \`cf\` comes first on this shell's PATH: run ${cf} instead.\n`
  if (role !== 'chief') return memberText(role) + where
  // Evals only: a chief measured without ConsensFlow's card gets this file's
  // text instead of all of it (tiers, staff and its cf too), so nothing tells
  // it the board exists. Nothing in the app sets it.
  const card = process.env.CONSENSFLOW_EVAL_CHIEF_CARD
  if (card) return readFileSync(card, 'utf8')
  return (
    text('chief').replace('{{tiers}}', workTierList()).replace('{{staff}}', teamTable(staff)) +
    where
  )
}
