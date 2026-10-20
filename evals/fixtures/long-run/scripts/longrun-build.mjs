/**
 * `npm run build`: the report site, into dist/ (or the folder named after it): a
 * page for each month of the year, a page for each account, and an index. It says
 * each page as it is made, the entries it skipped, and how long it took.
 */
import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

import { config } from '../src/config.mjs'
import { monthName, monthsOf } from '../src/dates.mjs'
import { formatMoney } from '../src/format.mjs'
import { escape, links, page, table } from '../src/html.mjs'
import { load } from '../src/index.mjs'
import { accountReport, monthReport } from '../src/report.mjs'
import { pace } from './pace.mjs'

const out = process.argv[2] ?? config.dist
const started = Date.now()
const books = load(config.data)
const money = (cents) => formatMoney(cents, config.currency)

const pages = [
  ...monthsOf(config.year).map((month) => {
    const report = monthReport(books.entries, books.accounts, month, config.currency)
    const rows = report.rows.map((row) => [row.code, escape(row.name), row.kind, money(row.total)])
    return {
      file: `${month}.html`,
      title: monthName(month),
      body: `<p>Earned ${money(report.income)}, spent ${money(report.spending)}.</p>\n${table(['account', 'name', 'kind', 'total'], rows)}`,
    }
  }),
  ...[...books.accounts.keys()].map((code) => {
    const report = accountReport(books.entries, books.accounts, code, config.year, config.currency)
    const rows = report.months.map((month) => [monthName(month.month), money(month.total)])
    return {
      file: `account-${code}.html`,
      title: `${code} ${report.name}`,
      body: `<p>The year: ${money(report.total)}.</p>\n${table(['month', 'total'], rows)}`,
    }
  }),
]

rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })
copyFileSync(join('site', 'style.css'), join(out, 'style.css'))
for (const [at, made] of pages.entries()) {
  await pace('page')
  writeFileSync(join(out, made.file), page(made.title, made.body))
  console.log(`built ${made.file} (${at + 1}/${pages.length + 1})`)
}
const index = links(pages.map((made) => [made.file, made.title]))
writeFileSync(join(out, 'index.html'), page(`The books of ${config.year}`, index))
console.log(`built index.html (${pages.length + 1}/${pages.length + 1})`)
if (books.skipped.length > 0) {
  console.log(`skipped ${books.skipped.length} invalid entries:`)
  for (const skipped of books.skipped) {
    console.log(`  data/journal.csv line ${skipped.line}: ${skipped.errors.join('; ')}`)
  }
}
console.log(`Built ${pages.length + 1} pages in ${((Date.now() - started) / 1000).toFixed(1)} s`)
