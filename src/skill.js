import { agentProfile, WORK_TIERS } from '../hosts/lib/presets.js'

/** The project team as a coordinator reads it: one row per member with its tier and tags. */
export function teamTable(agents, noun) {
  const cell = (value) =>
    String(value ?? '')
      .replace(/\|/g, '\\|')
      .replace(/[\r\n]+/g, ' ')
  // Name, tier and tags, nothing else: a coordinator prefers with tags and
  // never picks a member, so it needs no model, route or description here.
  const rows = agents.map((agent) => {
    const profile = agentProfile(agent)
    return `| ${[
      agent.name,
      WORK_TIERS[profile.workTier].label,
      (agent.tags ?? profile.tags ?? profile.categories ?? []).join(', ') || 'none',
    ]
      .map(cell)
      .join(' | ')} |`
  })
  return rows.length
    ? ['| Member | Work tier | Tags |', '|---|---|---|', ...rows].join('\n')
    : `No saved ${noun} are available. Continue within your own role; do not create agents as a side effect.`
}

/** The saved work tiers, one line each. */
export function workTierList() {
  return Object.values(WORK_TIERS)
    .map((tier) => `- ${tier.label}: ${tier.description}`)
    .join('\n')
}
