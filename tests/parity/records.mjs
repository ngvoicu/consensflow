/**
 * `npm run parity:records`: the conversations in this machine's harness
 * stores, each read by Node's `answers` and then by the Rust readers, held
 * to the same reading.
 *
 * Node's half is here. It reads each conversation at one instant and writes
 * it, one a line, to `<tmpdir>/consensflow-parity-records.jsonl` as a
 * digest (the Rust half is told the file in CF_PARITY_RECORDS): its items' ids, roles, completeness and times, and each text's
 * UTF-16 length and SHA-256, never the text, with how long the read took.
 * Then it runs the Rust half (`crates/cf-harness/tests/parity.rs`), which
 * reads each again, compares, and times its reads beside Node's.
 *
 * The stores are the ones this environment names (HOME, CLAUDE_CONFIG_DIR,
 * CODEX_HOME, PI_CODING_AGENT_*, XDG_DATA_HOME, APPDATA, OPENCODE_*), read
 * and never written. Devin's wire logs are ConsensFlow's own: unless
 * `--with-wires`, CONSENSFLOW_HOME is an empty folder, so the live home is
 * never read. Each conversation is stamped before it is read (its file's
 * size and time, or what its rows add up to), for the Rust half to tell a
 * conversation written between the two reads from a difference.
 *
 *   npm run parity:records [-- --limit 25 --with-wires]
 */
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { performance } from 'node:perf_hooks'
import { DatabaseSync } from 'node:sqlite'
import { parseArgs } from 'node:util'
import { claudeTranscript } from '../../hosts/lib/completion/claude-code.js'
import { codexTranscript } from '../../hosts/lib/completion/codex.js'
import { piTranscript } from '../../hosts/lib/completion/pi.js'
import { answers } from '../../hosts/lib/completion.js'
import { devinFolders, opencodeStores, piSessionDir } from '../../src/harnesses.js'

const REPO = path.join(import.meta.dirname, '..', '..')
const OUTPUT = path.join(os.tmpdir(), 'consensflow-parity-records.jsonl')

/** The variables that say where a harness keeps its record: the Rust half reads with these alone. */
const NAMES = [
  'HOME',
  'USERPROFILE',
  'OS',
  'CLAUDE_CONFIG_DIR',
  'CODEX_HOME',
  'PI_CODING_AGENT_DIR',
  'PI_CODING_AGENT_SESSION_DIR',
  'XDG_DATA_HOME',
  'APPDATA',
  'LOCALAPPDATA',
  'OPENCODE_DB',
  'OPENCODE_DATA',
  'CONSENSFLOW_HOME',
]
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/

const { values } = parseArgs({
  options: {
    limit: { type: 'string', default: '25' },
    'with-wires': { type: 'boolean', default: false },
  },
})
const limit = Number(values.limit)
const env = Object.fromEntries(
  NAMES.filter((name) => process.env[name] !== undefined).map((name) => [name, process.env[name]]),
)
if (!values['with-wires']) {
  env.CONSENSFLOW_HOME = fs.mkdtempSync(path.join(os.tmpdir(), 'consensflow-parity-home-'))
}
const home = env.HOME ?? env.USERPROFILE ?? os.homedir()

/** The `.jsonl` files under `root`, `depth` folders down at most, newest first. */
function jsonl(root, depth) {
  const found = []
  const walk = (folder, left) => {
    let entries
    try {
      entries = fs.readdirSync(folder, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      const full = path.join(folder, entry.name)
      if (entry.isFile() && entry.name.endsWith('.jsonl')) found.push(full)
      else if (entry.isDirectory() && left > 0) walk(full, left - 1)
    }
  }
  walk(root, depth)
  // A file gone since its folder was read is passed over.
  return found
    .flatMap((file) => {
      try {
        return [{ file, mtimeMs: fs.statSync(file).mtimeMs }]
      } catch {
        return []
      }
    })
    .sort((left, right) => right.mtimeMs - left.mtimeMs)
    .map(({ file }) => file)
}

/** A store's rows, read-only: `sql`'s answer, or none for a store that cannot be read. */
function rows(file, sql, ...params) {
  let db
  try {
    db = new DatabaseSync(file, { readOnly: true })
    return db.prepare(sql).all(...params)
  } catch {
    return []
  } finally {
    db?.close()
  }
}

/** A transcript's stamp: its size and time of writing, or that it is gone; none where none was found. */
function fileStamp(file) {
  if (file === null) return null
  try {
    const { size, mtimeMs } = fs.statSync(file)
    return { file, size, mtimeMs }
  } catch {
    return { file, gone: true }
  }
}

/** A store's stamp: what `sql` adds up of the session's rows, `?1` the session. */
function storeStamp(file, sql, session) {
  return { file, sql, session, rows: rows(file, sql, session) }
}

const OPENCODE_STAMP = `select
  (select count(*) from message where session_id = ?1) as messages,
  (select max(time_updated) from message where session_id = ?1) as messageUpdated,
  (select total(length(data)) from message where session_id = ?1) as messageBytes,
  (select count(*) from part where session_id = ?1) as parts,
  (select max(time_updated) from part where session_id = ?1) as partUpdated,
  (select total(length(data)) from part where session_id = ?1) as partBytes,
  (select count(*) from event where aggregate_id = ?1) as events,
  (select max(seq) from event where aggregate_id = ?1) as lastEvent`
const DEVIN_STAMP = `select
  (select count(*) from message_nodes where session_id = ?1) as rows,
  (select max(row_id) from message_nodes where session_id = ?1) as lastRow,
  (select total(length(chat_message)) from message_nodes where session_id = ?1) as bytes,
  (select main_chain_id from sessions where id = ?1) as head`
const devinStore = () => path.join(devinFolders(env).data, 'cli', 'sessions.db')

/** Each harness: its conversations, newest first, and how one is stamped. */
const HARNESSES = {
  'claude-code': {
    sessions: () =>
      jsonl(path.join(env.CLAUDE_CONFIG_DIR ?? path.join(home, '.claude'), 'projects'), 1)
        .map((file) => path.basename(file, '.jsonl'))
        .filter((name) => UUID.test(name)),
    stamp: async (session) => fileStamp(await claudeTranscript(session, env)),
  },
  codex: {
    sessions: () =>
      jsonl(path.join(env.CODEX_HOME ?? path.join(home, '.codex'), 'sessions'), 6)
        .map((file) => path.basename(file).match(UUID)?.[0])
        .filter(Boolean),
    stamp: async (session) => fileStamp(await codexTranscript(session, env)),
  },
  pi: {
    sessions: () =>
      jsonl(piSessionDir(env), 6)
        .map((file) => path.basename(file).match(UUID)?.[0])
        .filter(Boolean),
    stamp: async (session) => fileStamp(await piTranscript(session, env)),
  },
  opencode: {
    sessions: () =>
      opencodeStores(env).flatMap((file) =>
        rows(file, 'select id from session order by time_updated desc').map((row) => row.id),
      ),
    stamp: async (session) =>
      opencodeStores(env).map((file) => storeStamp(file, OPENCODE_STAMP, session)),
  },
  devin: {
    sessions: () =>
      rows(devinStore(), 'select id from sessions order by rowid desc').map((row) => row.id),
    stamp: async (session) => storeStamp(devinStore(), DEVIN_STAMP, session),
  },
}

const ours = JSON.parse(
  fs.readFileSync(
    path.join(REPO, 'crates', 'cf-harness', 'tests', 'goldens', 'records', 'tables.json'),
    'utf8',
  ),
).reasons.ours

/** What the Rust half compares of a reading: never the text itself. */
function digest(reading) {
  if (reading.unknown) {
    const own = ours.some((prefix) => reading.reason.startsWith(prefix))
    return { unknown: true, reason: own ? reading.reason : 'unreadable: «platform»' }
  }
  return {
    items: reading.items.map((item) => [
      item.id,
      item.role,
      item.complete,
      item.at,
      item.text.length,
      createHash('sha256').update(item.text).digest('hex'),
      item.commentary === true,
    ]),
    inFlight: reading.inFlight,
    asking: reading.asking,
    failed: reading.failed,
    quota: reading.quota,
    settlement: reading.settlement.state,
  }
}

// One instant for every reading, the Rust half's too: Pi's quiet reads the
// clock. And the zone a reset that names none is read in.
const now = Date.now()
Date.now = () => now
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
const lines = []
for (const [kind, harness] of Object.entries(HARNESSES)) {
  const sessions = [...new Set(harness.sessions())].slice(0, limit)
  for (const session of sessions) {
    const stamp = await harness.stamp(session)
    const started = performance.now()
    const reading = await answers(kind, session, env)
    const ms = performance.now() - started
    lines.push(
      JSON.stringify({ kind, session, now, zone, env, stamp, ms, reading: digest(reading) }),
    )
  }
  console.log(`${kind}: ${sessions.length} conversations`)
}
fs.writeFileSync(OUTPUT, `${lines.join('\n')}\n`)
console.log(`${lines.length} digests in ${OUTPUT}`)

// Optimised, as the daemon is built: the Rust half times its reads.
const rust = spawnSync(
  'cargo',
  ['test', '--release', '-p', 'cf-harness', '--test', 'parity', '--', '--ignored', '--nocapture'],
  { cwd: REPO, stdio: 'inherit', env: { ...process.env, CF_PARITY_RECORDS: OUTPUT } },
)
process.exitCode = rust.status ?? 1
