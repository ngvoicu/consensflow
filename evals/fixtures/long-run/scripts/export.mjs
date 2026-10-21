/**
 * `npm run export -- --csv`: the journal as CSV, into exports/journal.csv, the
 * entries the books could read (the skipped are said on the error stream).
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

import { config } from '../src/config.mjs'
import { journalCsv } from '../src/export.mjs'
import { load } from '../src/index.mjs'

const asked = process.argv.slice(2)
if (!asked.includes('--csv')) {
  console.error('usage: npm run export -- --csv')
  process.exit(2)
}
const books = load(config.data)
mkdirSync(config.exports, { recursive: true })
const file = join(config.exports, 'journal.csv')
writeFileSync(file, journalCsv(books.entries))
console.log(`wrote ${books.entries.length} entries to ${file}`)
if (books.skipped.length > 0) console.error(`skipped ${books.skipped.length} invalid entries`)
