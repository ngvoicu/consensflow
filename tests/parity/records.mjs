/**
 * `npm run parity:records`: the conversations in this machine's harness
 * stores, each read by Node's readers and then by the Rust readers, held to
 * the same readings.
 *
 * Node's half is here. It copies the newest conversations of each harness
 * into a snapshot: each file a harness's reader would open for one, with its
 * time of writing, at its place, and each SQLite store whole (`VACUUM INTO`,
 * read-only, from a store in WAL mode alone, whose writers it never holds
 * up). The snapshot's environment is this one re-rooted: the same variables,
 * each naming its place in the snapshot. Both halves read the copies, so
 * nothing a harness writes meanwhile comes between them.
 *
 * Once every copy is made, Node reads each conversation from the snapshot
 * twice, with a reader kept between the looks as the daemon keeps one, and
 * once from the live stores. A live reading that differs from the copy's is
 * counted as written since, where what the copy came from changed (a file,
 * one made since, a reader's folder); where nothing did, the copy missed
 * something, and the run fails. It writes both looks' digests, one
 * conversation a line, into the snapshot: the items' ids, roles,
 * completeness and times, and each text's UTF-16 length and SHA-256, never
 * the text, with how long the live read took. Then it runs the Rust half
 * (`crates/cf-harness/tests/parity.rs`, told the file in CF_PARITY_RECORDS),
 * which reads each conversation from the same snapshot and compares, and
 * times its own reads of the live stores beside Node's.
 *
 * The live stores are the ones this environment names (HOME, CLAUDE_CONFIG_DIR,
 * CODEX_HOME, PI_CODING_AGENT_*, XDG_DATA_HOME, APPDATA, LOCALAPPDATA,
 * OPENCODE_*, and Pi's evidence, CF_DELIVERY_*), read and never written to
 * (though SQLite may make a store's `-wal` and `-shm` beside it, where it has
 * none); a place named relative to the working folder is refused. Devin's wire logs
 * are ConsensFlow's own: only with `--with-wires` are they read, from its
 * home. The snapshot holds the texts: it is removed, unless the halves
 * differ, when it is kept for the difference to be read again.
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
import { home } from '../../hosts/lib/completion/shared.js'
import { answers, cachedAnswers } from '../../hosts/lib/completion.js'
import { devinFolders, opencodeStores, piSessionDir } from '../../src/harnesses.js'
import { refuseTailoredCollation } from '../goldens/records/tables.mjs'

const REPO = path.join(import.meta.dirname, '..', '..')

/**
 * The variables that name where a harness keeps its record, and what an
 * empty value is to the code that reads each: a path (`??`), or none (`||`,
 * `if`). A value that is not a place (`OS`, the launch's id) is kept.
 */
const PLACES = {
  HOME: 'path',
  USERPROFILE: 'path',
  CLAUDE_CONFIG_DIR: 'path',
  CODEX_HOME: 'path',
  PI_CODING_AGENT_DIR: 'none',
  PI_CODING_AGENT_SESSION_DIR: 'none',
  XDG_DATA_HOME: 'path',
  APPDATA: 'path',
  LOCALAPPDATA: 'none',
  OPENCODE_DB: 'none',
  OPENCODE_DATA: 'none',
  CONSENSFLOW_HOME: 'path',
  CF_DELIVERY_SETTLED: 'path',
}
const VALUES = ['OS', 'CF_DELIVERY_LAUNCH_ID']
/** Pi's own variables, which it reads a `~` in as the home (`piPath`). */
const TILDE = new Set(['PI_CODING_AGENT_DIR', 'PI_CODING_AGENT_SESSION_DIR'])
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/
/** How deep a harness's locator looks for a file (`findFile`). */
const DEPTH = 6

// The readers order ids as ICU's root collation does, as the goldens hold,
// and read a reset that names no zone in the one `Intl` names.
refuseTailoredCollation()
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
if (zone === undefined) throw new Error('this process names no zone: set TZ to one Intl knows')
const { values } = parseArgs({
  options: {
    limit: { type: 'string', default: '25' },
    'with-wires': { type: 'boolean', default: false },
  },
})
const limit = Number(values.limit)

const present = Object.fromEntries(
  [...Object.keys(PLACES), ...VALUES]
    .filter((name) => process.env[name] !== undefined)
    .map((name) => [name, process.env[name]]),
)
/**
 * Whether the snapshot re-roots `name`'s `value`: a place, as the code that
 * reads it reads it. An empty one that is none to that code, and a `~` Pi
 * reads as the home, are kept; a place named by the working folder cannot be.
 */
function reRoots(name, value) {
  if (!(name in PLACES)) return false
  if (value === '' && PLACES[name] === 'none') return false
  const tilde = value === '~' || value.startsWith('~/') || value.startsWith(`~${path.sep}`)
  if (TILDE.has(name) && tilde) return false
  if (!path.isAbsolute(value)) {
    throw new Error(
      `${name}=${JSON.stringify(value)} names a place by the working folder: name one whole`,
    )
  }
  return true
}
for (const [name, value] of Object.entries(present)) reRoots(name, value)

const snapshot = fs.mkdtempSync(path.join(os.tmpdir(), 'consensflow-parity-'))
const DIGESTS = path.join(snapshot, 'digests.jsonl')
// ConsensFlow's own home, read for Devin's wire logs: an empty one unless asked.
const live = values['with-wires']
  ? present
  : { ...present, CONSENSFLOW_HOME: path.join(snapshot, 'no-wires') }
const env = Object.fromEntries(
  Object.entries(live).map(([name, value]) => [
    name,
    reRoots(name, value) ? path.join(snapshot, name) : value,
  ]),
)

/**
 * What the copies came from, by the conversation (`kind`, `session`) or the
 * harness they were made for: each a stamp to take again, and the one taken
 * when the copy was made.
 */
const sources = new Map()
function source(key, stamp) {
  sources.set(key, [...(sources.get(key) ?? []), [stamp, stamp()]])
}
/** A file's size and time of writing: none where it is not, so that one made since is seen. */
const fileStamp = (file) => () => {
  try {
    const { size, mtimeNs } = fs.statSync(file, { bigint: true })
    return `${size} ${mtimeNs}`
  } catch {
    return 'none'
  }
}
/** Whether nothing the copies for any of `keys` came from changed since. */
const unchanged = (...keys) =>
  keys.every((key) => (sources.get(key) ?? []).every(([stamp, was]) => stamp() === was))
/** The places copied to: a place two copies name (Pi's sessions in Claude's folder) is copied once. */
const copied = new Set()

/** Throws unless `to` is in the snapshot: a copy never writes a live store. */
function inSnapshot(to) {
  if (!path.resolve(to).startsWith(snapshot + path.sep)) {
    throw new Error(`${to} is outside the snapshot`)
  }
}

/** `x` moved `steps` doubles up or down. */
function nudged(x, steps) {
  const double = new Float64Array([x])
  new BigInt64Array(double.buffer)[0] += BigInt(steps)
  return double[0]
}

/**
 * `file` copied to `to` for `key`, with its time of writing as both halves
 * read it, `mtimeMs` to the last bit: Pi's reader reads it, and a copy
 * stamped now is never quiet. Windows' copy keeps it; elsewhere the time is
 * set, in seconds, the double nearest it and its neighbours tried in turn.
 */
function copy(key, file, to) {
  inSnapshot(to)
  source(key, fileStamp(file))
  if (copied.has(to)) return
  copied.add(to)
  fs.mkdirSync(path.dirname(to), { recursive: true })
  fs.copyFileSync(file, to)
  const want = fs.statSync(file).mtimeMs
  const { atimeNs, mtimeNs } = fs.statSync(file, { bigint: true })
  const seconds = Number(mtimeNs) / 1e9
  for (const steps of [0, 1, -1, 2, -2, 3, -3, 4, -4]) {
    if (fs.statSync(to).mtimeMs === want) return
    fs.utimesSync(to, Number(atimeNs) / 1e9, nudged(seconds, steps))
  }
  if (fs.statSync(to).mtimeMs !== want) throw new Error(`${to} cannot be given ${file}'s time`)
}

/** The files under `root`, `depth` folders down at most, whose names `test` takes, as `findFile` walks. */
function files(root, test, depth = DEPTH) {
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
      if (entry.isFile() && test(entry.name)) found.push(full)
      else if (entry.isDirectory() && left > 0) walk(full, left - 1)
    }
  }
  walk(root, depth)
  return found
}

/** The `.jsonl` files under `root`, `depth` folders down at most, newest first. */
function newest(root, depth) {
  // A file gone since its folder was read is passed over.
  return files(root, (name) => name.endsWith('.jsonl'), depth)
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

/**
 * Each file under `from` a locator's `test` takes, copied for `key` to its
 * place under `to`; which files those are is a source too, so that one
 * made since is seen.
 */
function copyMatching(key, from, to, test) {
  source(key, () => files(from, test).sort().join('\n'))
  for (const file of files(from, test)) copy(key, file, path.join(to, path.relative(from, file)))
}

/** A store's rows, read-only: `sql`'s answer, or none for a store that cannot be read. */
function rows(file, sql) {
  let db
  try {
    db = new DatabaseSync(file, { readOnly: true })
    return db.prepare(sql).all()
  } catch {
    return []
  } finally {
    db?.close()
  }
}

/**
 * The store `file`, whole as one read of it saw it, written to `to` for
 * `key`; nothing where none is. Why it cannot be copied, where it cannot: a
 * store not in WAL mode, whose writers a read would hold up.
 */
function copyStore(key, file, to) {
  if (!fs.existsSync(file)) return null
  inSnapshot(to)
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const mode = db.prepare('pragma journal_mode').get().journal_mode
    if (mode !== 'wal') return `${file} is in ${mode} mode, not WAL`
    source(key, fileStamp(file))
    source(key, fileStamp(`${file}-wal`))
    fs.mkdirSync(path.dirname(to), { recursive: true })
    db.prepare('vacuum into ?').run(to)
    return null
  } finally {
    db.close()
  }
}

/** Claude's and Codex's folders, as their locators find them (`claudeTranscript`, `codexTranscript`). */
const claudeRoot = (env) => env.CLAUDE_CONFIG_DIR ?? path.join(home(env), '.claude')
const codexRoot = (env) => env.CODEX_HOME ?? path.join(home(env), '.codex')
/** The transcript a JSONL harness's locator takes for a session, by its kind. */
const TRANSCRIPTS = { 'claude-code': claudeTranscript, codex: codexTranscript, pi: piTranscript }

/**
 * Each harness: its conversations in the live stores, newest first, and
 * their copying into the snapshot: none, or why the harness is left out.
 */
const HARNESSES = {
  'claude-code': {
    sessions: () =>
      newest(path.join(claudeRoot(live), 'projects'), 1)
        .map((file) => path.basename(file, '.jsonl'))
        .filter((name) => UUID.test(name)),
    copy: (sessions) => {
      for (const session of sessions) {
        copyMatching(
          `claude-code\n${session}`,
          path.join(claudeRoot(live), 'projects'),
          path.join(claudeRoot(env), 'projects'),
          (name) => name === `${session}.jsonl`,
        )
      }
      return null
    },
  },
  codex: {
    sessions: () =>
      newest(path.join(codexRoot(live), 'sessions'), DEPTH)
        .map((file) => path.basename(file).match(UUID)?.[0])
        .filter(Boolean),
    copy: (sessions) => {
      for (const session of sessions) {
        copyMatching(
          `codex\n${session}`,
          path.join(codexRoot(live), 'sessions'),
          path.join(codexRoot(env), 'sessions'),
          (name) => name.includes(session),
        )
      }
      return null
    },
  },
  pi: {
    sessions: () =>
      newest(piSessionDir(live), DEPTH)
        .map((file) => path.basename(file).match(UUID)?.[0])
        .filter(Boolean),
    copy: (sessions) => {
      for (const session of sessions) {
        copyMatching(`pi\n${session}`, piSessionDir(live), piSessionDir(env), (name) =>
          name.includes(session),
        )
      }
      // The extension's evidence of the launch the environment names (`piSettlementEvidence`).
      const launch = live.CF_DELIVERY_LAUNCH_ID
      if (live.CF_DELIVERY_SETTLED !== undefined && launch !== undefined) {
        for (const name of [`${launch}.json`, `${launch}.working.json`]) {
          const file = path.join(live.CF_DELIVERY_SETTLED, name)
          if (fs.existsSync(file)) copy('pi', file, path.join(env.CF_DELIVERY_SETTLED, name))
          else source('pi', fileStamp(file))
        }
      }
      return null
    },
  },
  opencode: {
    sessions: () =>
      opencodeStores(live).flatMap((file) =>
        rows(file, 'select id from session order by time_updated desc').map((row) => row.id),
      ),
    // Each store to its place, as far down OpenCode's list in the snapshot.
    copy: () => {
      const places = opencodeStores(env)
      for (const [index, file] of opencodeStores(live).entries()) {
        const refused = copyStore('opencode', file, places[index])
        if (refused !== null) return refused
      }
      return null
    },
  },
  devin: {
    sessions: () =>
      rows(
        path.join(devinFolders(live).data, 'cli', 'sessions.db'),
        'select id from sessions order by rowid desc',
      ).map((row) => row.id),
    copy: () => {
      const store = (env) => path.join(devinFolders(env).data, 'cli', 'sessions.db')
      const refused = copyStore('devin', store(live), store(env))
      if (refused !== null) return refused
      // Each launch's wire log, as Devin's reader opens it (`readWires`).
      const wires = (env) =>
        path.join(
          env.CONSENSFLOW_HOME ?? path.join(home(env), '.consensflow'),
          'integrations',
          'devin',
        )
      const launches = () =>
        fs.existsSync(wires(live))
          ? fs
              .readdirSync(wires(live), { withFileTypes: true })
              .filter((entry) => entry.isDirectory())
              .map((entry) => entry.name)
          : []
      source('devin', () => launches().sort().join('\n'))
      for (const name of launches()) {
        const file = path.join(wires(live), name, 'wire.jsonl')
        if (fs.existsSync(file)) copy('devin', file, path.join(wires(env), name, 'wire.jsonl'))
        else source('devin', fileStamp(file))
      }
      return null
    },
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
      // An item's time, or none where it holds none: undefined is not null.
      item.at === undefined ? [] : [item.at],
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

/** The size of the transcript the snapshot holds of `session`; none for a store. */
async function transcriptSize(kind, session) {
  const file = await TRANSCRIPTS[kind]?.(session, env)
  return file ? fs.statSync(file).size : null
}

// One instant for every reading, the Rust half's too: Pi's quiet reads the clock.
const now = Date.now()
Date.now = () => now
const lines = []
let kept = false
try {
  // Every copy first: a later copy never changes what a reading read.
  const picked = []
  for (const [kind, harness] of Object.entries(HARNESSES)) {
    const sessions = [...new Set(harness.sessions())].slice(0, limit)
    const refused = harness.copy(sessions)
    if (refused === null) picked.push([kind, sessions])
    else console.log(`${kind}: left out: ${refused}`)
  }
  for (const [kind, sessions] of picked) {
    let written = 0
    for (const session of sessions) {
      const read = cachedAnswers()
      const first = digest(await read(kind, session, env))
      const again = digest(await read(kind, session, env))
      const started = performance.now()
      const there = digest(await answers(kind, session, live))
      const ms = performance.now() - started
      if (JSON.stringify(there) !== JSON.stringify(first)) {
        if (unchanged(kind, `${kind}\n${session}`)) {
          throw new Error(
            `${kind} ${session}: the copy reads otherwise, though nothing was written`,
          )
        }
        written += 1
      }
      const size = await transcriptSize(kind, session)
      lines.push(
        JSON.stringify({ kind, session, now, zone, env, live, ms, size, reading: first, again }),
      )
    }
    console.log(`${kind}: ${sessions.length} conversations, ${written} written since copied`)
  }
  fs.writeFileSync(DIGESTS, `${lines.join('\n')}\n`)
  console.log(`${lines.length} digests in ${DIGESTS}`)

  // Optimised, as the daemon is built: the Rust half times its reads.
  const rust = spawnSync(
    'cargo',
    ['test', '--release', '-p', 'cf-harness', '--test', 'parity', '--', '--ignored', '--nocapture'],
    { cwd: REPO, stdio: 'inherit', env: { ...process.env, CF_PARITY_RECORDS: DIGESTS } },
  )
  process.exitCode = rust.status ?? 1
  kept = process.exitCode !== 0
} finally {
  if (kept) console.log(`the snapshot is kept, to read the difference again: ${snapshot}`)
  else fs.rmSync(snapshot, { recursive: true, force: true })
}
