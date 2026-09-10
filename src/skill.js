import { readFileSync } from 'node:fs'
import { HARNESSES } from './roster.js'

export function agentCommand(agent) {
  return `cf run @${agent.name} "<task>"`
}

/** Only the app lead receives this document; roster discovery stays explicit. */
export function generateSkill(agents) {
  const base = readFileSync(
    new URL('../skill/roles/consensflow-lead/SKILL.md', import.meta.url),
    'utf8',
  )
  const supported = agents.filter((agent) => HARNESSES.includes(agent.harness))
  if (supported.length === 0) return base
  const cell = (value) =>
    String(value ?? '')
      .replace(/\|/g, '\\|')
      .replace(/[\r\n]+/g, ' ')
  const rows = supported.map(
    (agent) =>
      `| ${cell(agent.name)} | ${cell(agent.harness)} | ${cell(agent.model)} | ${cell(agent.description)} |`,
  )
  return `${base}\n## Available workers\n\n| Agent | Harness | Model | Purpose |\n|---|---|---|---|\n${rows.join('\n')}\n`
}
