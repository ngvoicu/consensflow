/**
 * The trace of the dispatcher test that runs now: what the engine asked of
 * what it was given, in the order it asked, and the database it left. A
 * file's tests run one at a time in its process, so one trace is current.
 *
 * The launch ids a test draws are a stream of its own, the n-th
 * `00000000-0000-4000-8000-00000000000n`, so the fake agents' conversations,
 * named after them, are the same at every recording, and the Rust engine's
 * tests draw the same.
 */
import crypto from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import { basename, join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'

const OUT = process.env.CF_ENGINE_TRACES

/** The test that runs now, what it recorded and the folders its ledgers were in; null between tests. */
let current = null
/** How many launch ids the test drew. */
let drawn = 0
/** How many tests of this file were written. */
let written = 0

crypto.randomUUID = () => {
  drawn += 1
  return `00000000-0000-4000-8000-${String(drawn).padStart(12, '0')}`
}
syncBuiltinESMExports()

/** A test begins: its own trace, and the first launch id again. */
export function begin(test) {
  drawn = 0
  current = { test, events: [], finals: [], folders: new Set() }
}

/** What the engine did, in its place in the order. */
export function record(event) {
  current?.events.push(event)
}

/** A ledger of the test's opened in `folder`: its path is written «dir». */
export function opened(folder) {
  current?.folders.add(folder)
}

/** A ledger of the test's closed: the database it left. */
export function closed(file) {
  current?.finals.push(dump(file))
}

/** The test ended: its trace is written, its temporary folders as «dir». */
export function end() {
  const trace = current
  current = null
  if (trace === null || !OUT) return
  written += 1
  const name = `${basename(process.argv[1] ?? 'unknown').replace(/\.test\.mjs$/, '')}-${String(written).padStart(3, '0')}`
  let text = JSON.stringify({ test: trace.test, events: trace.events, finals: trace.finals })
  for (const folder of trace.folders) {
    text = text.replaceAll(JSON.stringify(folder).slice(1, -1), '«dir»')
  }
  mkdirSync(OUT, { recursive: true })
  writeFileSync(join(OUT, `${name}.json`), `${text}\n`)
}

/** The database a ledger left, exactly: each table's rows in rowid order, each value as SQLite quotes it. */
function dump(file) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const tables = {}
    for (const { name } of db
      .prepare(`SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name`)
      .all()) {
      const columns = db
        .prepare(`PRAGMA table_info("${name}")`)
        .all()
        .map((column) => column.name)
      const quoted = columns.map((column) => `quote("${column}")`).join(', ')
      tables[name] = {
        columns,
        rows: db
          .prepare(`SELECT ${quoted} FROM "${name}" ORDER BY rowid`)
          .all()
          .map((row) => Object.values(row)),
      }
    }
    return tables
  } finally {
    db.close()
  }
}
