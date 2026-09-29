import { WORK_TIERS } from '../hosts/lib/presets.js'

/** The project staff as the chief reads it: one row per member with its roles and tier. */
export function teamTable(members) {
  const cell = (value) =>
    String(value ?? '')
      .replace(/\|/g, '\\|')
      .replace(/[\r\n]+/g, ' ')
  // Name, roles and tier, nothing else: the chief names a tier and never picks
  // a member, so it needs no model, route or description here.
  const rows = members.map(
    (member) =>
      `| ${[member.name, member.roles.join(', '), WORK_TIERS[member.workTier].label]
        .map(cell)
        .join(' | ')} |`,
  )
  return rows.length
    ? ['| Member | Roles | Work tier |', '|---|---|---|', ...rows].join('\n')
    : 'Nobody is on the staff yet: only the human adds members, in the app. Ask them here in your terminal for the members your work needs, and do not create agents as a side effect.'
}

/** The saved work tiers, one line each. */
export function workTierList() {
  return Object.values(WORK_TIERS)
    .map((tier) => `- ${tier.label}: ${tier.description}`)
    .join('\n')
}
