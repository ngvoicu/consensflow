/**
 * `npm run parity:records`: the conversations in this machine's harness
 * stores, each read by Node's readers and then by the Rust readers, held to
 * the same readings.
 *
 * Node's half is here. It copies the newest conversations of each harness
 * into a snapshot, laid out as each harness lays out its own: a transcript
 * with its time of writing, a SQLite store whole (`VACUUM INTO`, read-only),
 * so that nothing a harness writes meanwhile comes between the two halves.
 * It reads each conversation from the snapshot twice with a reader kept
 * between the looks, as the daemon does, and writes the two readings, one
 * conversation a line, to `<tmpdir>/consensflow-parity-records.jsonl` as
 * digests: the items' ids, roles, completeness and times, and each text's
 * UTF-16 length and SHA-256, never the text, with how long the first look
 * took. Then it runs the Rust half (`crates/cf-harness/tests/parity.rs`,
 * told the file in CF_PARITY_RECORDS), which reads each conversation from
 * the same snapshot, compares, and times its looks beside Node's and a
 * locate in the live stores.
 *
 * The live stores are the ones this environment names (HOME, CLAUDE_CONFIG_DIR,
 * CODEX_HOME, PI_CODING_AGENT_*, XDG_DATA_HOME, APPDATA, LOCALAPPDATA,
 * OPENCODE_*), read and never written. Devin's wire logs are ConsensFlow's
 * own: only with `--with-wires` are they copied from its home.
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
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { devinFolders, opencodeStores, piSessionDir } from '../../src/harnesses.js'

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

const { values } = parseArgs({
  options: {
    limit: { type: 'string', default: '25' },
    'with-wires': { type: 'boolean', default: false },
  },
})
const limit = Number(values.limit)
const live = Object.fromEntries(
  NAMES.filter((name) => process.env[name] !== undefined).map((name) => [name, process.env[name]]),
)
const home = live.HOME ?? live.USERPROFILE ?? os.homedir()

// The snapshot, and the environment that names its copies alone.
const snapshot = fs.mkdtempSync(path.join(os.tmpdir(), 'consensflow-parity-'))
const at = (...parts) => path.join(snapshot, ...parts)
const env = {
  HOME: at('home'),
  USERPROFILE: at('home'),
  CLAUDE_CONFIG_DIR: at('claude'),
  CODEX_HOME: at('codex'),
  PI_CODING_AGENT_SESSION_DIR: at('pi'),
  XDG_DATA_HOME: at('data'),
  APPDATA: at('roaming'),
  LOCALAPPDATA: at('local'),
  CONSENSFLOW_HOME: at('consensflow'),
  ...(live.OS === undefined ? {} : { OS: live.OS }),
}
fs.mkdirSync(env.HOME, { recursive: true })
fs.mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })

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

/** `file` copied to `to`, with its time of writing: Pi's reader reads that. */
function copy(file, to) {
  fs.mkdirSync(path.dirname(to), { recursive: true })
  fs.copyFileSync(file, to)
  const { atime, mtime } = fs.statSync(file)
  fs.utimesSync(to, atime, mtime)
}

/** The store `file`, whole as one read of it saw it, written to `to`; nothing where none is. */
function copyStore(file, to) {
  if (!fs.existsSync(file)) return
  fs.mkdirSync(path.dirname(to), { recursive: true })
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    db.prepare('vacuum into ?').run(to)
  } finally {
    db.close()
  }
}

/** A transcript found under `from`, copied to its place under `to`; nothing where none was found. */
function copyUnder(file, from, to) {
  if (file !== null) copy(file, path.join(to, path.relative(from, file)))
}

/**
 * Each harness: its conversations in the live stores, newest first, and the
 * copying of the ones picked into the snapshot.
 */
const HARNESSES = {
  'claude-code': {
    sessions: () =>
      jsonl(path.join(live.CLAUDE_CONFIG_DIR ?? path.join(home, '.claude'), 'projects'), 1)
        .map((file) => path.basename(file, '.jsonl'))
        .filter((name) => UUID.test(name)),
    copy: async (sessions) => {
      const root = live.CLAUDE_CONFIG_DIR ?? path.join(home, '.claude')
      for (const session of sessions) {
        copyUnder(await claudeTranscript(session, live), root, env.CLAUDE_CONFIG_DIR)
      }
    },
  },
  codex: {
    sessions: () =>
      jsonl(path.join(live.CODEX_HOME ?? path.join(home, '.codex'), 'sessions'), 6)
        .map((file) => path.basename(file).match(UUID)?.[0])
        .filter(Boolean),
    copy: async (sessions) => {
      const root = live.CODEX_HOME ?? path.join(home, '.codex')
      for (const session of sessions) {
        copyUnder(await codexTranscript(session, live), root, env.CODEX_HOME)
      }
    },
  },
  pi: {
    sessions: () =>
      jsonl(piSessionDir(live), 6)
        .map((file) => path.basename(file).match(UUID)?.[0])
        .filter(Boolean),
    copy: async (sessions) => {
      for (const session of sessions) {
        const file = await piTranscript(session, live)
        copyUnder(file, piSessionDir(live), env.PI_CODING_AGENT_SESSION_DIR)
      }
    },
  },
  opencode: {
    sessions: () =>
      opencodeStores(live).flatMap((file) =>
        rows(file, 'select id from session order by time_updated desc').map((row) => row.id),
      ),
    // Each store in the snapshot's place as far down OpenCode's list.
    copy: async () => {
      const places = opencodeStores(env)
      opencodeStores(live).forEach((file, index) => {
        copyStore(file, places[index])
      })
    },
  },
  devin: {
    sessions: () =>
      rows(
        path.join(devinFolders(live).data, 'cli', 'sessions.db'),
        'select id from sessions order by rowid desc',
      ).map((row) => row.id),
    copy: async () => {
      copyStore(
        path.join(devinFolders(live).data, 'cli', 'sessions.db'),
        path.join(devinFolders(env).data, 'cli', 'sessions.db'),
      )
      if (!values['with-wires']) return
      const wires = (folder) => path.join(folder, 'integrations', 'devin')
      const from = wires(live.CONSENSFLOW_HOME ?? path.join(home, '.consensflow'))
      for (const launch of fs.existsSync(from) ? fs.readdirSync(from) : []) {
        const file = path.join(from, launch, 'wire.jsonl')
        if (fs.existsSync(file)) {
          copy(file, path.join(wires(env.CONSENSFLOW_HOME), launch, 'wire.jsonl'))
        }
      }
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
try {
  for (const [kind, harness] of Object.entries(HARNESSES)) {
    const sessions = [...new Set(harness.sessions())].slice(0, limit)
    await harness.copy(sessions)
    for (const session of sessions) {
      const read = cachedAnswers()
      const started = performance.now()
      const first = await read(kind, session, env)
      const ms = performance.now() - started
      const again = await read(kind, session, env)
      lines.push(
        JSON.stringify({
          kind,
          session,
          now,
          zone,
          env,
          live,
          ms,
          reading: digest(first),
          again: digest(again),
        }),
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
} finally {
  fs.rmSync(snapshot, { recursive: true, force: true })
}
