/**
 * `npm run parity:records`: the conversations in this machine's harness
 * stores, each read by Node's readers and then by the Rust readers, held to
 * the same readings.
 *
 * Node's half is here. It copies the newest conversations of each harness
 * into a snapshot: each file its harness's locator would take for one, with
 * its time of writing, at its place, and each SQLite store whole (`VACUUM
 * INTO`, read-only, from a store in WAL mode alone, whose writers it never
 * holds up). The snapshot's environment is this one re-rooted: the same
 * variables, each naming a place in the snapshot. Both halves read the
 * copies, so nothing a harness writes meanwhile comes between them.
 *
 * Node reads each conversation from the snapshot twice, with a reader kept
 * between the looks as the daemon keeps one, and once from the live
 * stores: a live reading that differs from the copy's is counted (a
 * conversation written since it was copied), and a copy that lost a record
 * fails the run. It writes both looks' digests, one conversation a line, to
 * `<tmpdir>/consensflow-parity-records.jsonl`: the items' ids, roles,
 * completeness and times, and each text's UTF-16 length and SHA-256, never
 * the text, with how long the live read took. Then it runs the Rust half
 * (`crates/cf-harness/tests/parity.rs`, told the file in CF_PARITY_RECORDS),
 * which reads each conversation from the same snapshot and compares, and
 * times its own reads of the live stores beside Node's.
 *
 * The live stores are the ones this environment names (HOME, CLAUDE_CONFIG_DIR,
 * CODEX_HOME, PI_CODING_AGENT_*, XDG_DATA_HOME, APPDATA, LOCALAPPDATA,
 * OPENCODE_*), read and never written. Devin's wire logs are ConsensFlow's
 * own: only with `--with-wires` are they read, from its home. The snapshot
 * holds the texts: it is removed, unless the halves differ, when it is kept
 * for the difference to be read again, and its place printed.
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
const OUTPUT = path.join(os.tmpdir(), 'consensflow-parity-records.jsonl')

/** The variables that say where a harness keeps its record. */
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
/** How deep a harness's locator looks for a file (`findFile`). */
const DEPTH = 6
/** How a reading begins that found no record: a copy that reads so, where the live store holds one, is broken. */
const LOST = ['unreadable: no ', 'unreadable: missing Devin session']

// The readers order ids as ICU's root collation does, as the goldens hold.
refuseTailoredCollation()
const { values } = parseArgs({
  options: {
    limit: { type: 'string', default: '25' },
    'with-wires': { type: 'boolean', default: false },
  },
})
const limit = Number(values.limit)
const snapshot = fs.mkdtempSync(path.join(os.tmpdir(), 'consensflow-parity-'))

const present = Object.fromEntries(
  NAMES.filter((name) => process.env[name] !== undefined).map((name) => [name, process.env[name]]),
)
// ConsensFlow's own home, read for Devin's wire logs: an empty one unless asked.
const live = values['with-wires']
  ? present
  : { ...present, CONSENSFLOW_HOME: path.join(snapshot, 'no-wires') }
// The snapshot's environment: each place this one names, re-rooted. A path
// under `~` follows the re-rooted home; an empty one is none, as it was.
const env = Object.fromEntries(
  Object.entries(live).map(([name, value]) => [
    name,
    name === 'OS' || value === '' || value.startsWith('~') ? value : path.join(snapshot, name),
  ]),
)

/** The files under `root`, `depth` folders down at most, whose names `test` takes. */
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

/** `file` copied to `to` with its times, to the microsecond: Pi's reader reads them. */
function copy(file, to) {
  fs.mkdirSync(path.dirname(to), { recursive: true })
  fs.copyFileSync(file, to)
  const { atimeNs, mtimeNs } = fs.statSync(file, { bigint: true })
  fs.utimesSync(to, Number(atimeNs) / 1e9, Number(mtimeNs) / 1e9)
}

/** Each file under `from` a locator's `test` takes, copied to its place under `to`. */
function copyMatching(from, to, test) {
  for (const file of files(from, test)) copy(file, path.join(to, path.relative(from, file)))
}

/**
 * The store `file`, whole as one read of it saw it, written to `to`; nothing
 * where none is. Why it cannot be copied, where it cannot: a store not in WAL
 * mode, whose writers a read would hold up.
 */
function copyStore(file, to) {
  if (!fs.existsSync(file)) return null
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const mode = db.prepare('pragma journal_mode').get().journal_mode
    if (mode !== 'wal') return `${file} is in ${mode} mode, not WAL`
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

/** The size of the transcript the snapshot holds of `session`; none for a store. */
async function transcriptSize(kind, session) {
  const file = await TRANSCRIPTS[kind]?.(session, env)
  return file ? fs.statSync(file).size : null
}

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
        copyMatching(piSessionDir(live), piSessionDir(env), (name) => name.includes(session))
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
        const refused = copyStore(file, places[index])
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
      const refused = copyStore(store(live), store(env))
      if (refused !== null) return refused
      const wires = (env) =>
        path.join(
          env.CONSENSFLOW_HOME ?? path.join(home(env), '.consensflow'),
          'integrations',
          'devin',
        )
      copyMatching(wires(live), wires(env), (name) => name === 'wire.jsonl')
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

// One instant for every reading, the Rust half's too: Pi's quiet reads the
// clock. And the zone a reset that names none is read in.
const now = Date.now()
Date.now = () => now
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
const lines = []
let kept = false
try {
  for (const [kind, harness] of Object.entries(HARNESSES)) {
    const sessions = [...new Set(harness.sessions())].slice(0, limit)
    const refused = harness.copy(sessions)
    if (refused !== null) {
      console.log(`${kind}: left out: ${refused}`)
      continue
    }
    let drifted = 0
    for (const session of sessions) {
      const read = cachedAnswers()
      const first = digest(await read(kind, session, env))
      const again = digest(await read(kind, session, env))
      const started = performance.now()
      const there = digest(await answers(kind, session, live))
      const ms = performance.now() - started
      if (JSON.stringify(there) !== JSON.stringify(first)) {
        if (!there.unknown && first.unknown && LOST.some((lost) => first.reason.startsWith(lost))) {
          throw new Error(`${kind} ${session}: the copy lost its record (${first.reason})`)
        }
        drifted += 1
      }
      const size = await transcriptSize(kind, session)
      lines.push(
        JSON.stringify({ kind, session, now, zone, env, live, ms, size, reading: first, again }),
      )
    }
    console.log(
      `${kind}: ${sessions.length} conversations, ${drifted} read otherwise live (written since)`,
    )
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
  kept = process.exitCode !== 0
} finally {
  if (kept) console.log(`the snapshot is kept, to read the difference again: ${snapshot}`)
  else fs.rmSync(snapshot, { recursive: true, force: true })
}
