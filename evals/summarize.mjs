#!/usr/bin/env node
/**
 * Every report in evals/reports/, one line each on the console and one row
 * each in evals/RESULTS.md: scenario, chief, staff, how many expectations
 * held, the numbers behind them. Reports are kept: a run is evidence, and a
 * later run beside it is the comparison.
 *
 *   npm run eval:summary
 */
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { resultsTable, row } from './results.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPORTS = join(HERE, 'reports')
const rows = readdirSync(REPORTS)
  .filter((name) => name.endsWith('.json'))
  .sort()
  .map((name) => ({ name, ...JSON.parse(readFileSync(join(REPORTS, name), 'utf8')) }))
if (rows.length === 0) {
  process.stdout.write('no reports yet\n')
  process.exit(0)
}
for (const report of rows) process.stdout.write(`${row(report).join(' · ')}\n`)
writeFileSync(join(HERE, 'RESULTS.md'), resultsTable(rows))
process.stdout.write(`written: evals/RESULTS.md (${rows.length} rows)\n`)
