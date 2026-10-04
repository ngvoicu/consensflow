/**
 * The ledger, recording: what each ledger a test opens was asked and what it
 * answered, with every clock reading, session name and logged event each call
 * took, and at its close the database it left. A record is the trace the Rust
 * ledger replays (`record.mjs` writes them).
 */
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import * as real from '../../../src/ledger/index.js'
import { sessionName } from '../../../src/ledger/names.js'

export * from '../../../src/ledger/index.js'

const OUT = process.env.CF_LEDGER_TRACES
const opened = new Map()
/** The records of ledgers not closed yet: one a test leaves open is written as such when the process ends. */
const open = new Set()
/** The files the open ledgers of this process hold. */
const held = new Set()
process.on('exit', () => {
  for (const record of open) write({ ...record, final: null, unclosed: true })
})

/**
 * Which test opened a ledger: its file (each test file runs in a process of
 * its own) and the line in it the opening came from, async frames included.
 */
function opener() {
  const file = process.argv[1] ?? 'unknown'
  const frame = new Error().stack.split('\n').find((line) => line.includes(file)) ?? ''
  return { file: basename(file), line: Number(/:(\d+):\d+\)?$/.exec(frame)?.[1] ?? 0) }
}

/** A value as JSON writes it, what JSON cannot hold tagged. */
function encode(value, callbacks) {
  if (value === undefined) return { $undefined: true }
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value
  if (typeof value === 'number') return Number.isFinite(value) ? value : { $number: String(value) }
  if (typeof value === 'bigint') return { $bigint: String(value) }
  if (typeof value === 'function') return callbacks.wrap(value)
  if (value instanceof Date) return { $date: value.toISOString() }
  if (value instanceof Set) return { $set: [...value].map((item) => encode(item, callbacks)) }
  if (value instanceof Map) {
    return {
      $map: [...value].map(([key, item]) => [encode(key, callbacks), encode(item, callbacks)]),
    }
  }
  if (Array.isArray(value)) return value.map((item) => encode(item, callbacks))
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [key, encode(item, callbacks)]),
  )
}

const failure = (cause) => ({
  $error: {
    name: cause?.name ?? null,
    code: cause?.code ?? null,
    status: cause?.status ?? null,
    message: String(cause?.message ?? cause),
  },
})

/** The database a ledger left, exactly: each table's rows in rowid order, each value as SQLite quotes it. */
function dump(file) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const tables = db
      .prepare(
        `SELECT name, sql FROM sqlite_master WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY name`,
      )
      .all()
    const rows = {}
    for (const { name } of db
      .prepare(`SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name`)
      .all()) {
      const columns = db
        .prepare(`PRAGMA table_info("${name}")`)
        .all()
        .map((column) => column.name)
      const quoted = columns.map((column) => `quote("${column}")`).join(', ')
      rows[name] = {
        columns,
        rows: db
          .prepare(`SELECT ${quoted} FROM "${name}" ORDER BY rowid`)
          .all()
          .map((row) => Object.values(row)),
      }
    }
    return {
      userVersion: db.prepare('PRAGMA user_version').get().user_version,
      schema: tables,
      tables: rows,
    }
  } finally {
    db.close()
  }
}

/**
 * A file there before the ledger opened it: its database, or its bytes when
 * it is none. Read from a copy: another ledger, the one a test checks the
 * lock against, may hold the file itself.
 */
function before(file) {
  const dir = mkdtempSync(join(tmpdir(), 'cf-ledger-before-'))
  try {
    const copy = join(dir, 'ledger.db')
    copyFileSync(file, copy)
    if (existsSync(`${file}-wal`)) copyFileSync(`${file}-wal`, `${copy}-wal`)
    try {
      return { database: dump(copy) }
    } catch {
      return { bytes: readFileSync(file).toString('base64') }
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

export function openLedger(file, options = {}) {
  const site = opener()
  const number = (opened.get(site.file) ?? 0) + 1
  opened.set(site.file, number)
  const record = { test: site, number, file, options: {}, calls: [] }
  let current = null
  const take = (list, value) => {
    if (current !== null) current[list].push(value)
    return value
  }
  // A home a test wrote before (an older schema, a file that is no ledger):
  // what the ledger found, for a replay to start from. Never one a ledger of
  // this process holds: closing any descriptor of a file drops all of the
  // process's POSIX locks on it, the holder's included, and the open is
  // refused anyway.
  if (held.has(file)) record.initial = { heldHere: true }
  else if (existsSync(file)) record.initial = before(file)
  const now = options.now ?? (() => new Date())
  const names = options.names ?? sessionName
  const trace = options.trace ?? (() => {})
  record.options = {
    now: options.now !== undefined,
    names: options.names !== undefined,
    trace: options.trace !== undefined,
  }
  let ledger
  try {
    ledger = real.openLedger(file, {
      now: () => {
        const at = now()
        take('clock', at.toISOString())
        return at
      },
      names: () => take('names', names()),
      trace: (event) => {
        take('events', JSON.parse(JSON.stringify(event)))
        return trace(event)
      },
    })
  } catch (cause) {
    record.openError = failure(cause).$error
    write(record)
    throw cause
  }
  open.add(record)
  held.add(file)
  return new Proxy(ledger, {
    get(target, property, receiver) {
      const value = Reflect.get(target, property, receiver)
      if (typeof value !== 'function' || typeof property !== 'string') return value
      return (...args) => {
        const callbacks = callbacksFor()
        const call = {
          method: property,
          args: args.map((arg) => encode(arg, callbacks)),
          clock: [],
          names: [],
          events: [],
          callbacks: callbacks.calls,
        }
        const outer = current
        if (outer === null) {
          record.calls.push(call)
          current = call
        }
        try {
          const result = value.apply(
            target,
            args.map((arg) => (typeof arg === 'function' ? callbacks.forward(arg) : arg)),
          )
          call.result = encode(result, callbacks)
          return result
        } catch (cause) {
          call.result = failure(cause)
          throw cause
        } finally {
          if (outer === null) current = null
          if (property === 'close' && outer === null) {
            open.delete(record)
            held.delete(file)
            record.final = dump(file)
            write(record)
          }
        }
      }
    },
  })
}

/** A call's function arguments (a tier for each member, say): what each was asked and answered. */
function callbacksFor() {
  const calls = []
  const ids = new Map()
  const callbacks = {
    calls,
    wrap: (fn) => {
      if (!ids.has(fn)) ids.set(fn, ids.size)
      return { $fn: ids.get(fn) }
    },
    forward: (fn) => {
      const id = callbacks.wrap(fn).$fn
      return (...args) => {
        const entry = { fn: id, args: args.map((arg) => encode(arg, callbacks)) }
        calls.push(entry)
        const result = fn(...args)
        entry.result = encode(result, callbacks)
        return result
      }
    },
  }
  return callbacks
}

/**
 * Writes a record, the ledger's own path, a test's temporary folder that
 * differs at every run, as «ledger» wherever it is (a refusal names it): a
 * replay puts its own back.
 */
function write(record) {
  if (!OUT) return
  mkdirSync(OUT, { recursive: true })
  const name = `${record.test.file.replace(/\.test\.mjs$/, '')}-${String(record.number).padStart(3, '0')}`
  const path = JSON.stringify(record.file).slice(1, -1)
  const text = JSON.stringify(record).replaceAll(path, '«ledger»')
  writeFileSync(join(OUT, `${name}.json`), `${text}\n`)
}
