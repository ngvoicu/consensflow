/**
 * Plays a records scenario against Node's readers and writes down what each
 * look read: the oracle `crates/cf-harness` is held to.
 *
 * A scenario is data. Its steps write files and stores under a root, move
 * the clock, and look, and the Rust test plays the same steps against its
 * own readers. Everything a reader reads of time is the scenario's, so a run
 * on any machine records the same bytes:
 * - the clock is stubbed, and moves only by a step;
 * - every file step sets the file's mtime to the clock, one millisecond
 *   after the step before it.
 *
 * What a look read is written compactly:
 * - each item once, in `items`, and each reading once, in `readings`;
 * - a look names its reading, and when the cached reader handed back an
 *   object it handed back before, the look it handed it to (`sameAs`, and
 *   `quotaSameAs` for the quota).
 *
 * A reason that is ConsensFlow's own sentence is kept as it is. One that is
 * the platform's (an errno line, SQLite's words, V8's) is written
 * `unreadable: «platform»`, since only its prefix is promised.
 */
import { readFileSync } from 'node:fs'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { answers, cachedAnswers } from '../../../hosts/lib/completion.js'
import { FIXTURES } from './fixtures.mjs'

/** Where every scenario's clock starts: 2026-09-21T12:26:40.000Z. */
export const START = 1_790_000_000_000

/** The reasons that are ConsensFlow's own sentences: kept as they are. */
const OURS = [
  /^missing session id$/,
  /^missing explicit env argument$/,
  /^unknown kind: /,
  /^unreadable: no (claude session|codex rollout for|pi session|opencode store for|opencode session) /,
  /^unreadable: missing home in env$/,
  /^unreadable: missing native .+ id at record \d+$/,
  /^unreadable: malformed JSONL at record \d+$/,
  /^unreadable: empty (claude session|codex rollout for|pi session) /,
  /^unreadable: malformed OpenCode /,
  /^unreadable: missing OpenCode event /,
  /^unreadable: (missing Devin session|conflicting Devin completion evidence|cyclic Devin main chain|missing Devin main chain ancestor|invalid Devin message identity|unknown Devin message role)$/,
]

/** A reason as the golden keeps it. */
export function keptReason(reason) {
  if (OURS.some((pattern) => pattern.test(reason))) return reason
  if (!reason.startsWith('unreadable: ')) throw new Error(`a reason with no class: ${reason}`)
  return 'unreadable: «platform»'
}

/** `$ROOT/a/b` as a path under `root`, joined as this platform joins it. */
function resolve(root, value) {
  if (typeof value !== 'string' || !value.startsWith('$ROOT')) return value
  return path.join(root, ...value.slice('$ROOT'.length).split('/').filter(Boolean))
}

/** An env or options object with every `$ROOT` path made real, at any depth. */
function resolveAll(root, value) {
  if (Array.isArray(value)) return value.map((item) => resolveAll(root, item))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, resolveAll(root, item)]),
    )
  }
  return resolve(root, value)
}

/** `Date` as the readers read it: now is the scenario's clock. */
function stubbedDate(clock) {
  const RealDate = globalThis.Date
  return class extends RealDate {
    constructor(...args) {
      if (args.length === 0) super(clock.now)
      else super(...args)
    }
    static now() {
      return clock.now
    }
  }
}

/**
 * A fixture's lines, one of them in another's place: `{fixture, line,
 * text}`, the fixture's lines (its text with no final line break, split at
 * each line break) with line `line` written as `text`, each line ended by
 * a line break.
 */
function fixtureText({ fixture, line, text }) {
  const lines = readFileSync(`${FIXTURES}${fixture}`, 'utf8').trimEnd().split('\n')
  lines[line] = text
  return `${lines.join('\n')}\n`
}

/** Sets a file's times to `ms`, the clock's reading. */
async function stamp(file, ms) {
  await fs.utimes(file, ms / 1000, ms / 1000)
}

/** What one look read, interned into the scenario's tables. */
function intern(tables, reading) {
  const kept =
    reading.unknown === true
      ? { ...reading, reason: keptReason(reading.reason) }
      : {
          ...reading,
          items: reading.items.map((item) => {
            const key = JSON.stringify(item)
            let at = tables.itemIndex.get(key)
            if (at === undefined) {
              at = tables.items.length
              tables.items.push(item)
              tables.itemIndex.set(key, at)
            }
            return at
          }),
        }
  const key = JSON.stringify(kept)
  let at = tables.readingIndex.get(key)
  if (at === undefined) {
    at = tables.readings.length
    tables.readings.push(kept)
    tables.readingIndex.set(key, at)
  }
  return at
}

/**
 * Plays `scenario` in a fresh temporary root and returns it with what each
 * look read. The steps are those of `scenario.steps`, each one of:
 * - `{mkdir}`, `{write, text}`, `{append, text}`;
 * - `{write, fixture, line, text}`: a fixture with one line written anew;
 * - `{replace, text}`: written beside, then renamed over;
 * - `{remove}`, `{move, to}`;
 * - `{mtime, ago}`: the file's times set `ago` milliseconds before the clock;
 * - `{clock}`: the clock moved on that many milliseconds;
 * - `{db, open}`, `{db, exec}`, `{db, run, params}`, `{db, close}`: a writer
 *   connection, kept open across looks as a harness keeps its own;
 * - `{look: 'cached' | 'fresh' | 'both', kind, session, options}`, read
 *   with the scenario's env or the look's own `env`; a fresh look's
 *   `between` steps are played between OpenCode's two snapshot reads.
 * A path is `$ROOT/…`. The cached reader forgets a conversation unread for
 * `scenario.idle` milliseconds, ten minutes unless the scenario says.
 */
export async function play(scenario) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-records-golden-'))
  const clock = { now: START }
  const RealDate = globalThis.Date
  const dbs = new Map()
  const tables = { items: [], itemIndex: new Map(), readings: [], readingIndex: new Map() }
  const handedOut = new Map()
  const quotasHandedOut = new Map()
  const env = resolveAll(root, scenario.env)
  const steps = []
  globalThis.Date = stubbedDate(clock)
  // Made under the stubbed clock, so the cache's idle sweep reads it too.
  const cached = cachedAnswers(scenario.idle === undefined ? {} : { idleMs: scenario.idle })
  try {
    for (const step of scenario.steps) {
      steps.push(await playStep(step))
    }
  } finally {
    globalThis.Date = RealDate
    for (const db of dbs.values()) db.close()
    await fs.rm(root, { recursive: true, force: true })
  }
  return {
    name: scenario.name,
    env: scenario.env,
    ...(scenario.idle === undefined ? {} : { idle: scenario.idle }),
    steps,
    items: tables.items,
    readings: tables.readings,
  }

  async function playStep(step) {
    // A file step happens a millisecond after the step before it.
    const file = (relative) => resolve(root, relative)
    if (step.mkdir !== undefined) {
      await fs.mkdir(file(step.mkdir), { recursive: true })
    } else if (step.write !== undefined) {
      clock.now += 1
      await fs.writeFile(
        file(step.write),
        step.fixture === undefined ? step.text : fixtureText(step),
      )
      await stamp(file(step.write), clock.now)
    } else if (step.append !== undefined) {
      clock.now += 1
      await fs.appendFile(file(step.append), step.text)
      await stamp(file(step.append), clock.now)
    } else if (step.replace !== undefined) {
      clock.now += 1
      const beside = `${file(step.replace)}.new`
      await fs.writeFile(beside, step.text)
      await stamp(beside, clock.now)
      await fs.rename(beside, file(step.replace))
    } else if (step.remove !== undefined) {
      await fs.rm(file(step.remove), { recursive: true, force: true })
    } else if (step.move !== undefined) {
      await fs.rename(file(step.move), file(step.to))
    } else if (step.mtime !== undefined) {
      await stamp(file(step.mtime), clock.now - step.ago)
    } else if (step.clock !== undefined) {
      clock.now += step.clock
    } else if (step.db !== undefined) {
      if (step.open !== undefined) dbs.set(step.db, new DatabaseSync(file(step.open)))
      else if (step.exec !== undefined) dbs.get(step.db).exec(step.exec)
      else if (step.run !== undefined)
        dbs
          .get(step.db)
          .prepare(step.run)
          .run(...(step.params ?? []))
      else if (step.close !== undefined) {
        dbs.get(step.db).close()
        dbs.delete(step.db)
      } else throw new Error(`a db step with nothing to do: ${JSON.stringify(step)}`)
    } else if (step.look !== undefined) {
      return { ...step, ...(await look(step)) }
    } else {
      throw new Error(`a step with nothing to do: ${JSON.stringify(step)}`)
    }
    return step
  }

  async function look(step) {
    const options = resolveAll(root, step.options ?? {})
    const lookEnv = step.env === undefined ? env : resolveAll(root, step.env)
    const recorded = {}
    if (step.look === 'cached' || step.look === 'both') {
      const reading = await cached(step.kind, step.session, lookEnv, options)
      recorded.read = intern(tables, reading)
      // An object handed out before: the scheduler tells an old refusal from
      // a new one by the quota object it holds.
      if (handedOut.has(reading)) recorded.sameAs = handedOut.get(reading)
      else {
        handedOut.set(reading, steps.length)
        const quota = reading.quota ?? null
        if (quota !== null && quotasHandedOut.has(quota))
          recorded.quotaSameAs = quotasHandedOut.get(quota)
        else if (quota !== null) quotasHandedOut.set(quota, steps.length)
      }
    }
    if (step.look === 'fresh' || step.look === 'both') {
      const between =
        step.between === undefined
          ? {}
          : {
              async betweenOpenCodeSnapshotReads() {
                for (const inner of step.between) await playStep(inner)
              },
            }
      const reading = await answers(step.kind, step.session, lookEnv, { ...options, ...between })
      const at = intern(tables, reading)
      if (step.look === 'fresh' || at !== recorded.read) recorded.fresh = at
    }
    return recorded
  }
}
