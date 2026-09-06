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
    // 180s was the default until 2026-09-02, and it reported a SIGKILL as a
    // scenario failure: three runs "failed" checks they hold comfortably when
    // given time, and one of them read as a prose regression until it was
    // measured against HEAD and came back identical. A measurement tool that
    // manufactures failures is worse than a slow one.
    timeout: { type: 'string', default: '420' },
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
    const stage = makeStage(scenario.stage)
    const nextTurn = leadSession(values.lead)
    let thread = null
    let broke = null
    const failures = []
    const evidence = []
    // Every turn's commands, kept so a miss on turn 3 can show turns 1 and
    // 2 as well: a lead that sends into a pane it never opened THIS turn
    // opened it (or two) on a turn with no checks, and that is where the
    // explanation was (2026-09-06).
    const turnLogs = []
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
        turnLogs.push({ say: turn.say, log })
        let missedHere = false
        for (const [check, holds] of turn.expect) {
          // What the lead DID, and what it then told the user: some failures are
          // only visible in the report — a lead that read the tail of a long
          // answer ran exactly the right command and still reported the wrong thing.
          const ok = holds(log, result.stdout)
          note(scenario.id, check, ok)
          if (!ok) {
            failures.push(check)
            missedHere = true
          }
        }
        // The stage is thrown away below, so a missed check is the last chance
        // to see WHAT the lead ran — a rate says a fix did not work, only the
        // commands say why (2026-09-05: two checks missed on a turn that ran
        // no `cmux send` at all, and nothing said whether it had asked the
        // user instead or sent something the check did not count).
        if (missedHere && evidence.length === 0) {
          for (const { say, log: ran } of turnLogs) {
            const mark = say === turn.say ? ' (missed)' : ''
            evidence.push(`    turn ${JSON.stringify(say.slice(0, 60))} ran${mark}:`)
            for (const line of ran) evidence.push(`      $ ${line}`)
            if (ran.length === 0) evidence.push('      (nothing)')
          }
        } else if (missedHere) {
          evidence.push(`    turn ${JSON.stringify(turn.say.slice(0, 60))} ran (missed):`)
          for (const line of log) evidence.push(`      $ ${line}`)
          if (log.length === 0) evidence.push('      (nothing)')
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
    for (const line of evidence) console.log(line)
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
