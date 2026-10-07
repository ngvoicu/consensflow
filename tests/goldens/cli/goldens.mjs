/**
 * The legacy CLI's goldens, as Node answers them: what the standalone verbs
 * of Node's CLI (`src/cli.js`) do, which `crates/cf` is held to for step 4. Every file is
 * deterministic, so the unit suite holds the committed copies equal to what
 * this computes (tests/cli-goldens.test.mjs), and `npm run goldens:cli`
 * writes them again after a change to what they record. FORMAT.md says what
 * is in them.
 *
 * - `crates/cf/tests/goldens/cli.<platform>.json`: each scenario played
 *   (`world.mjs`), one set per platform, since a path is joined with its own
 *   separator and a stand-in CLI is a script or a `.cmd`. The Windows one is
 *   recorded on Windows.
 * - `crates/cf-base/tests/goldens/args.json`: what `util.parseArgs` answers
 *   for the words after each verb (`parse-args.mjs`), which hold on every
 *   system.
 */
import { adminScenarios } from './admin.mjs'
import { addScenarios } from './agent-add.mjs'
import { editScenarios, removeScenarios } from './agent-edit.mjs'
import { agentUsageScenarios, listScenarios } from './agent-list.mjs'
import { catalogScenarios } from './catalog.mjs'
import { helpScenarios, pipeScenarios } from './help.mjs'
import { CLOCK } from './instant.mjs'
import { parseGolden } from './parse-args.mjs'
import { play } from './world.mjs'

/** Every scenario, in the order they are recorded: a kept difference may be made of an earlier record. */
export function scenarios() {
  return [
    ...helpScenarios(),
    ...catalogScenarios(),
    ...listScenarios(),
    ...agentUsageScenarios(),
    ...addScenarios(),
    ...editScenarios(),
    ...removeScenarios(),
    ...adminScenarios(),
    ...pipeScenarios(),
  ]
}

/** The scenarios played, a record a line. */
async function played() {
  const records = new Map()
  const lines = []
  for (const scenario of scenarios()) {
    if (records.has(scenario.name)) throw new Error(`two scenarios are called ${scenario.name}`)
    const record = await play(scenario, (name) => records.get(name))
    records.set(scenario.name, record)
    lines.push(`    ${JSON.stringify(record)}`)
  }
  return `{\n  "format": 1,\n  "platform": ${JSON.stringify(process.platform)},\n  "clock": ${JSON.stringify(CLOCK)},\n  "cases": [\n${lines.join(',\n')}\n  ]\n}\n`
}

/** Every golden's text, by its path from the repository's root. */
export async function cliGoldens() {
  return {
    [`crates/cf/tests/goldens/cli.${process.platform}.json`]: await played(),
    'crates/cf-base/tests/goldens/args.json': parseGolden(),
  }
}
