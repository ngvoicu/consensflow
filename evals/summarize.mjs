#!/usr/bin/env node
/**
 * Every report in evals/reports/, one line each: scenario, chief, staff,
 * how many expectations held, the numbers behind them. Reports are kept:
 * a run is evidence, and a later run beside it is the comparison.
 *
 *   npm run eval:summary
 */
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPORTS = join(dirname(fileURLToPath(import.meta.url)), 'reports')
const rows = readdirSync(REPORTS)
  .filter((name) => name.endsWith('.json'))
  .sort()
  .map((name) => ({ name, ...JSON.parse(readFileSync(join(REPORTS, name), 'utf8')) }))
if (rows.length === 0) {
  process.stdout.write('no reports yet\n')
  process.exit(0)
}
const line = (r) => {
  const m = r.metrics
  const held = r.checks.filter((c) => c.ok).length
  return [
    r.name.slice(0, 19),
    r.scenario,
    `chief ${r.chief ?? 'claude'} (${r.model})`,
    `staff ${(r.staff ?? ['claude']).join('+')}`,
    `${held}/${r.checks.length}`,
    `tasks ${m.tasks.length} par ${m.parallel} adv ${m.advice} rev ${m.reviews} q ${m.questionsToHuman.length} n ${m.notesToHuman.length} edits ${m.chiefEdits}`,
    `${Math.round(r.seconds / 60)} min`,
  ].join(' · ')
}
for (const row of rows) process.stdout.write(`${line(row)}\n`)
