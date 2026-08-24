#!/usr/bin/env node
import { parseArgs } from 'node:util'
import { leadSession, makeStage, runLead, threadFrom } from './harness.mjs'
import { SCENARIOS } from './scenarios.mjs'

/**
 * Does the skill actually change what a lead does?
 *
 * `npm test` checks what the skill SAYS. Nothing checked what a lead DOES
 * with it, so three behavioural failures in one day were each answered with
 * more prose and none of the fixes was ever measured. This spends real tokens
 * on a real lead to turn "is it better?" into a number.
 *
 * Leads are not deterministic, so a single pass proves little: `--repeat`
 * reports a rate per check, which is the unit a prose change can be judged in.
 */
const { values } = parseArgs({
  options: {
    lead: { type: 'string', default: 'claude' },
    repeat: { type: 'string', default: '1' },
    scenario: { type: 'string' },
    timeout: { type: 'string', default: '180' },
  },
})

const repeat = Number.parseInt(values.repeat, 10)
const timeoutMs = Number.parseInt(values.timeout, 10) * 1000
const chosen = values.scenario ? SCENARIOS.filter((s) => s.id === values.scenario) : SCENARIOS
if (chosen.length === 0) {
  console.error(
    `no scenario ${JSON.stringify(values.scenario)}; have: ${SCENARIOS.map((s) => s.id).join(', ')}`,
  )
  process.exit(2)
}

console.log(`lead: ${values.lead} · ${chosen.length} scenarios × ${repeat}`)
console.log('this spends real tokens on a real lead, and touches nothing real')
console.log('')

const tally = new Map()
const note = (scenario, check, ok) => {
  const key = `${scenario} ${check}`
  const row = tally.get(key) ?? { scenario, check, passed: 0, of: 0 }
  row.passed += ok ? 1 : 0
  row.of += 1
  tally.set(key, row)
}

for (const scenario of chosen) {
  for (let pass = 0; pass < repeat; pass += 1) {
    const label = repeat > 1 ? `${scenario.id} (${pass + 1}/${repeat})` : scenario.id
    const stage = makeStage()
    const nextTurn = leadSession(values.lead)
    let thread = null
    let broke = null
    const failures = []
    try {
      for (const turn of scenario.turns) {
        const before = stage.read().length
        const invocation = nextTurn(turn.say, thread)
        const result = await runLead(invocation, stage, timeoutMs)
        if (invocation.capturesThread) thread = threadFrom(result.stdout) ?? thread
        if (result.code !== 0) {
          broke = `the lead exited ${result.code}: ${(result.stderr || result.stdout).slice(0, 300)}`
          break
        }
        const log = stage.read().slice(before)
        for (const [check, holds] of turn.expect) {
          const ok = holds(log)
          note(scenario.id, check, ok)
          if (!ok) failures.push(check)
        }
      }
    } finally {
      stage.cleanup()
    }
    if (broke) {
      console.log(`  ${label}: could not run — ${broke}`)
      continue
    }
    console.log(
      failures.length === 0
        ? `  ${label}: all checks held`
        : `  ${label}: ${failures.join('; ')}`,
    )
  }
}

console.log('')
let missed = 0
for (const scenario of chosen) {
  const rows = [...tally.values()].filter((r) => r.scenario === scenario.id)
  if (rows.length === 0) continue
  console.log(scenario.id)
  for (const row of rows) {
    const rate = row.passed / row.of
    missed += row.of - row.passed
    const mark = rate === 1 ? '  ok  ' : rate === 0 ? ' FAIL ' : ' flaky'
    console.log(`  ${mark} ${String(row.passed).padStart(2)}/${row.of}  ${row.check}`)
  }
}
// A flaky check is a failing check: the user meets it on the run it misses.
process.exitCode = missed === 0 ? 0 : 1
