import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { openLedger, SCHEMA_VERSION } from '../src/ledger/index.js'
import { clock, withDir } from './ledger-fixtures.mjs'

/**
 * The ledger as a file: the daemon is the only process that holds it, and a
 * process killed mid-write leaves the last committed state.
 */
const LEDGER = new URL('../src/ledger/index.js', import.meta.url).href

/** Runs a Node child that imports the ledger; resolves with its stdout. */
function child(source, { killAfterMs, readyLine } = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn(process.execPath, ['--input-type=module', '-e', source], {
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let out = ''
    let err = ''
    proc.stdout.on('data', (chunk) => {
      out += chunk
      if (readyLine && out.includes(readyLine) && killAfterMs !== undefined) {
        setTimeout(() => proc.kill('SIGKILL'), killAfterMs)
        readyLine = null
      }
    })
    proc.stderr.on('data', (chunk) => {
      err += chunk
    })
    proc.on('error', reject)
    proc.on('close', (code, signal) => resolve({ out, err, code, signal }))
  })
}

describe('opening the ledger', () => {
  it('creates the schema at the current version and keeps its data across a reopen', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const first = openLedger(file, { now: clock() })
      const created = first.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'opencode' },
      })
      first.close()

      const raw = new DatabaseSync(file, { readOnly: true })
      assert.equal(raw.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION)
      assert.equal(raw.prepare('PRAGMA journal_mode').get().journal_mode, 'wal')
      raw.close()

      const second = openLedger(file, { now: clock() })
      try {
        assert.deepEqual(
          second.projects().map((project) => [project.id, project.name, project.state]),
          [[created.id, 'app', 'open']],
        )
      } finally {
        second.close()
      }
    })
  })

  it('tells a trace every event as it is logged, with the time the ledger gave it', async () => {
    await withDir(async (dir) => {
      const entries = []
      const ledger = openLedger(path.join(dir, 'consensflow.db'), {
        now: clock(),
        trace: (entry) => entries.push(entry),
      })
      try {
        const project = ledger.createProject({
          directory: '/work/app',
          name: 'app',
          chief: { harness: 'pi' },
        })
        const logged = ledger.events(project.id)
        assert.deepEqual(entries[0], {
          at: logged[0].at,
          project: project.id,
          kind: 'project.created',
          data: { name: 'app', directory: '/work/app' },
        })
        assert.deepEqual(
          entries.map((entry) => [entry.at, entry.kind]),
          logged.map((event) => [event.at, event.kind]),
          'the trace is the event log, as it happens',
        )
      } finally {
        ledger.close()
      }
    })
  })

  it('refuses a second open while the first holds the file, in this process and in another', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const holder = openLedger(file)
      try {
        assert.throws(() => openLedger(file), { code: 'ledger-locked' })
        const other = await child(
          `import { openLedger, RESUME_WORDS } from ${JSON.stringify(LEDGER)}
           try { openLedger(${JSON.stringify(file)}); console.log('opened') }
           catch (error) { console.log(error.code) }`,
        )
        assert.equal(other.out.trim(), 'ledger-locked', other.err)
      } finally {
        holder.close()
      }
      openLedger(file).close()
    })
  })

  it('is free again once a holder is killed', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const killed = await child(
        `import { openLedger } from ${JSON.stringify(LEDGER)}
         openLedger(${JSON.stringify(file)}); console.log('ready'); setInterval(() => {}, 1000)`,
        { readyLine: 'ready', killAfterMs: 0 },
      )
      assert.equal(killed.signal, 'SIGKILL', killed.err)
      openLedger(file).close()
    })
  })

  it('refuses a database written by a newer ConsensFlow', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const raw = new DatabaseSync(file)
      raw.exec(`PRAGMA user_version = ${SCHEMA_VERSION + 1}`)
      raw.close()
      assert.throws(() => openLedger(file), { code: 'ledger-newer' })
    })
  })

  it('names a file that is not a ledger instead of failing somewhere inside SQLite', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      await writeFile(file, 'this is not a database, it is a text file '.repeat(200))
      assert.throws(() => openLedger(file), { code: 'ledger-unreadable' })
    })
  })

  it('keeps every task with its first message when the process is killed mid-write', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const crashed = await child(
        `import { openLedger } from ${JSON.stringify(LEDGER)}
         const ledger = openLedger(${JSON.stringify(file)})
         const project = ledger.createProject({ directory: '/w', name: 'w', chief: { harness: 'pi' } })
         ledger.addMember(project.id, { agent: 'zeus', harness: 'pi', role: 'worker', tier: 'standard' })
         console.log('ready')
         for (let n = 0; ; n++) ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'task ' + n })`,
        { readyLine: 'ready', killAfterMs: 150 },
      )
      assert.equal(crashed.signal, 'SIGKILL', crashed.err)
      const ledger = openLedger(file)
      try {
        assert.equal(ledger.integrity(), 'ok')
        const [project] = ledger.projects()
        const tasks = ledger.board(project.id).lanes.flatMap((lane) => lane.tasks)
        assert.ok(tasks.length > 0, 'the child wrote tasks before it was killed')
        for (const task of tasks) {
          const first = ledger
            .task(project.id, task.number)
            .messages.filter((m) => m.kind === 'task')
          assert.equal(first.length, 1, `T-${task.number} has exactly one task message`)
        }
      } finally {
        ledger.close()
      }
    })
  })
})
