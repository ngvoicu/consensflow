import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import fsp, { chmod, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { workspaceKey } from '../hosts/lib/state.js'
import { Store } from '../src/store.js'
import { Tabs } from '../src/tabs.js'
import { tempEnv } from './helpers.mjs'

/**
 * The app-wide serialised store (TEST-PANE-11). Every test hands the state
 * root to the store as an explicit argument — the store never reads
 * process.env — so isolation is just a throwaway directory per test.
 */
async function withHome(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-store-'))
  try {
    return await fn(path.join(dir, 'consensflow'), dir)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

async function openStore(home) {
  const store = new Store(home)
  await store.open()
  return store
}

const AT = '2026-09-06T10:00:00.000Z'

/**
 * The launch a row currently holds, in the shape a controller write names
 * it. Every controller mutation carries this so the store can compare it
 * against the reservation of the moment, inside its own queue.
 */
async function currentLaunch(store, ws, name) {
  const reserved = (await store.readThreads(ws))[name]?.reserved
  if (reserved === undefined) return null
  return {
    launchId: reserved.launchId,
    tab: reserved.tab,
    pane: reserved.pane,
    generation: reserved.generation,
  }
}

/**
 * Seeds a tab record straight through the queue, the way `src/tabs.js` will
 * once it exists. Store tests exercise the queue with raw mutations on
 * purpose: the serialization is the clause under test, not tab semantics.
 */
async function seedTabRecord(store, directory, tabId, panes = []) {
  await store.mutate(directory, 'tab.create', async (io) => {
    const tabs = await io.readTabs()
    tabs.push({
      id: tabId,
      directory,
      closed: false,
      policy: 'auto',
      lead: { harness: 'pi', generation: 1, nativeSession: null },
      panes,
      createdAt: AT,
      updatedAt: AT,
    })
    await io.writeTabs(tabs)
  })
}

function workerPane(id, order) {
  return { id, kind: 'worker', conversation: null, generation: 1, order }
}

async function collectFiles(dir, into = []) {
  let entries
  try {
    entries = await readdir(dir, { withFileTypes: true })
  } catch {
    return into
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) await collectFiles(full, into)
    else into.push(full)
  }
  return into
}

// --- the instance lock ------------------------------------------------------

test('store: open claims <root>/app/instance.lock with this pid and start time', async () => {
  await withHome(async (home) => {
    const store = await openStore(home)

    const lockPath = path.join(home, 'app', 'instance.lock')
    const [first, second] = (await readFile(lockPath, 'utf8')).split('\n')
    const note = JSON.parse(first)
    assert.equal(note.pid, process.pid, 'the note names the living instance')
    assert.ok(Number.isInteger(note.startedAt) && note.startedAt > 0)
    assert.equal(
      second,
      'ownership is the kernel lock; this file is diagnostics only',
      'the file says what it is, so nobody reads it as authority',
    )

    // Closing releases the kernel's lock, which is what ownership IS — the
    // file stays behind as a note. Unlinking it would let a successor
    // create and lock a different inode while someone still held this one,
    // so the release is proved the only way that means anything: the next
    // instance gets in.
    await store.close()
    assert.equal(typeof (await readFile(lockPath, 'utf8')), 'string', 'the note is left behind')
    const successor = await openStore(home)
    await successor.close()
  })
})

test('store: a second Store on the same root is refused', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const first = await openStore(home)

    const second = new Store(home)
    await assert.rejects(
      () => second.open(),
      /holds/,
      'one app instance owns a state root — the second never gets a queue',
    )
    await assert.rejects(
      () => second.mutate(ws, 'sent.record', async () => {}),
      /not open/,
      'an unopened store must refuse mutations, not silently work without the lock',
    )

    // The refusal of the second changed nothing for the first.
    await first.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })
    assert.equal(Object.keys(await first.readThreads(ws)).length, 1)

    await first.close()
    await assert.rejects(
      () => first.mutate(ws, 'sent.record', async () => {}),
      /not open/,
      'a closed store is not a queue anymore',
    )

    const third = new Store(home)
    await third.open()
    await third.close()
  })
})

// --- cross-process lock ownership --------------------------------------------
// The lock is the one thing two app instances share without a queue, and it
// is the kernel's: these tests run the contenders as real OS processes, the
// way two launches meet.

const STORE_URL = new URL('../src/store.js', import.meta.url).href

/**
 * Runs one contender child: it opens the root, reports the exact claim it
 * published AS SOON AS it holds it, holds (for `holdMs`, or until
 * `holdFile` appears), then closes. Always exits 0 — the verdict is the
 * result file, so a crash cannot deadlock the parent.
 *
 * `writeName` makes it attempt one mutation before closing — the point of
 * owning a root — and `env`/`execArgv` stay as seams for future lock tests.
 */
function spawnContender(
  home,
  { holdMs = 0, resultFile, holdFile = '', writeName = '', writeDir = '', execArgv = [], env = {} },
) {
  const script = [
    `import { access, readFile, rename, writeFile } from 'node:fs/promises';`,
    `import { Store } from ${JSON.stringify(STORE_URL)};`,
    `const [home, holdMs, resultFile, lockPath, holdFile, writeName, writeDir] = process.argv.slice(1);`,
    `const sleep = (ms) => new Promise((r) => setTimeout(r, ms));`,
    // tmp+rename, so a parent polling the file never reads half a report.
    `const report = async (body) => { await writeFile(resultFile + '.tmp', JSON.stringify(body), 'utf8'); await rename(resultFile + '.tmp', resultFile); };`,
    `try {`,
    `  const store = new Store(home);`,
    `  await store.open();`,
    `  const claim = JSON.parse((await readFile(lockPath, 'utf8')).split('\\n')[0]);`,
    `  await report({ opened: true, pid: process.pid, claim });`,
    `  if (holdFile.length > 0) { for (;;) { try { await access(holdFile); break; } catch { await sleep(10); } } }`,
    `  else { await sleep(Number(holdMs)); }`,
    `  let wrote = null;`,
    `  if (writeName.length > 0) {`,
    `    wrote = await store`,
    `      .conversationCreate(writeDir, { name: writeName, agent: 'nyx', kind: 'pi' })`,
    `      .then(() => 'ok', (error) => 'refused: ' + String(error?.message ?? error));`,
    `  }`,
    `  await report({ opened: true, pid: process.pid, claim, wrote });`,
    `  await store.close();`,
    `  await report({ opened: true, pid: process.pid, claim, wrote, closed: true });`,
    `} catch (error) {`,
    `  await report({ opened: false, pid: process.pid, error: String(error?.message ?? error) });`,
    `}`,
  ].join('\n')
  const lockPath = path.join(home, 'app', 'instance.lock')
  return spawn(
    process.execPath,
    [
      ...execArgv,
      '-e',
      script,
      home,
      String(holdMs),
      resultFile,
      lockPath,
      holdFile,
      writeName,
      writeDir,
    ],
    { stdio: 'ignore', env: { ...process.env, ...env } },
  )
}

/** Resolves when the child exits, whatever it exited with. */
function exited(proc, label = 'a contender') {
  return new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error(`${label} never exited`)), 30000)
    proc.on('error', reject)
    proc.on('close', () => {
      clearTimeout(deadline)
      resolve()
    })
  })
}

/** Polls until `check()` answers truthy, or gives up loudly. */
async function until(check, what, timeoutMs = 20000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    if (await check()) return
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`)
    await new Promise((r) => setTimeout(r, 10))
  }
}

const readClaim = async (lockPath) => {
  try {
    return JSON.parse((await readFile(lockPath, 'utf8')).split('\n')[0])
  } catch {
    return null
  }
}

async function runContenders(home, count, { holdMs = 0 } = {}) {
  const dir = path.join(home, 'contenders')
  await mkdir(dir, { recursive: true })
  const children = []
  for (let i = 0; i < count; i += 1) {
    children.push({ tag: i, resultFile: path.join(dir, `result-${i}.json`) })
  }
  const procs = children.map((c) => spawnContender(home, { holdMs, resultFile: c.resultFile }))
  try {
    await Promise.all(procs.map((proc) => exited(proc)))
  } finally {
    for (const proc of procs) {
      try {
        proc.kill('SIGKILL')
      } catch {
        // Already gone — the result file says what happened.
      }
    }
  }
  return Promise.all(
    children.map(async (c) => ({
      tag: c.tag,
      ...JSON.parse(await readFile(c.resultFile, 'utf8')),
    })),
  )
}

test('store: simultaneous acquisition across processes admits exactly one owner', async () => {
  await withHome(async (home) => {
    // Three contenders at once; the winner holds the root long enough that
    // every loser meets a held lock.
    const outcomes = await runContenders(home, 3, { holdMs: 1500 })
    const winners = outcomes.filter((o) => o.opened)
    assert.equal(winners.length, 1, `exactly one owner, got ${JSON.stringify(outcomes)}`)
    for (const loser of outcomes.filter((o) => !o.opened)) {
      assert.match(loser.error, /another|holds|claim/i, 'losers are refused, not crashed')
    }

    // The note names the winner — diagnostics, but they should still be
    // the winner's, and readable while it holds.
    const winner = winners[0]
    assert.equal(winner.claim.pid, winner.pid)

    // The lock file is all that is left: no staging file, no tombstone, no
    // shard — there is no publication protocol left to leave debris.
    assert.deepEqual(await readdir(path.join(home, 'app')), ['instance.lock'])
  })
})

test('store: a live claim refuses a separate process', async () => {
  await withHome(async (home) => {
    const store = await openStore(home)
    const outcomes = await runContenders(home, 1)
    assert.equal(outcomes.length, 1)
    assert.equal(outcomes[0].opened, false)
    assert.match(outcomes[0].error, /another|holds|claim/i)
    await store.close()
  })
})

// --- ONE app-wide queue -----------------------------------------------------

test('store: a queued mutation blocks later mutations on other files until it finishes', async () => {
  await withHome(async (home, dir) => {
    const wsA = path.join(dir, 'ws-a')
    const wsB = path.join(dir, 'ws-b')
    const store = await openStore(home)

    // A gate inside a threads.json mutation, with a tabs.json mutation
    // queued behind it: retained-data tests cannot tell interleaving from
    // serialisation, but an execution log can.
    let releaseGate
    const gate = new Promise((resolve) => {
      releaseGate = resolve
    })
    const order = []
    const first = store.mutate(wsA, 'conversation.create', async (io) => {
      order.push('threads-start')
      await gate
      const threads = await io.readThreads()
      threads.witness = { kind: 'pi' }
      await io.writeThreads(threads)
      order.push('threads-end')
    })
    const second = store.mutate(wsB, 'tab.create', async (io) => {
      order.push('tabs-run')
      const tabs = await io.readTabs()
      tabs.push({ id: 't-behind', directory: wsB, panes: [] })
      await io.writeTabs(tabs)
    })
    // The second mutation is queued, not running, while the gate holds.
    await new Promise((r) => setTimeout(r, 50))
    assert.deepEqual(order, ['threads-start'])
    releaseGate()
    await Promise.all([first, second])
    assert.deepEqual(order, ['threads-start', 'threads-end', 'tabs-run'])
    await store.close()
  })
})

test('store: a failed first write commits nothing anywhere', async () => {
  if (typeof process.getuid === 'function' && process.getuid() === 0) {
    return // read-only files do not refuse root; the refusal is the instrument
  }
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    const tabsBefore = await readFile(path.join(home, 'app', 'tabs.json'), 'utf8')

    // The workspace directory refuses writes: validation passes (tabs.json
    // reads fine), then the FIRST write fails — proving the reservation is
    // written before the pane link, and that no pane links without it.
    const wsKey = path.join(home, 'workspaces', workspaceKey(ws))
    await mkdir(wsKey, { recursive: true })
    await chmod(wsKey, 0o555)
    try {
      await assert.rejects(
        () =>
          store.conversationCreate(ws, {
            name: 'nyx-doomed-lane',
            agent: 'nyx',
            kind: 'codex',
            pane: { tab: 't-1', id: 'w-9', generation: 1 },
          }),
        /EACCES|EPERM|EROFS/i,
      )
      assert.deepEqual(
        Object.keys(await store.readThreads(ws)),
        [],
        'no witness row without its write',
      )
      assert.equal(
        await readFile(path.join(home, 'app', 'tabs.json'), 'utf8'),
        tabsBefore,
        'no pane links without the reservation write',
      )
    } finally {
      await chmod(wsKey, 0o755)
    }
    await store.close()
  })
})

test('store: close shuts admission, drains admitted work, then releases for a successor', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'sink', agent: 'nyx', kind: 'pi' })

    let releaseGate
    const gate = new Promise((resolve) => {
      releaseGate = resolve
    })
    const admitted = store.mutate(ws, 'sent.record', async (io) => {
      await gate
      const threads = await io.readThreads()
      threads.sink.sent = [{ text: 'gated' }]
      await io.writeThreads(threads)
    })

    // close() while a mutation is admitted: admission shuts synchronously,
    // concurrent closes share the one drain.
    const first = store.close()
    const second = store.close()
    assert.equal(first, second, 'concurrent closes share one drain')
    await assert.rejects(
      () => store.mutate(ws, 'sent.record', async () => {}),
      /not open/,
      'a job admitted after close starts must never outlive the lock',
    )

    releaseGate()
    await first
    await admitted

    // The successor observes the drained write: it could only acquire the
    // root after every admitted mutation landed.
    const next = await openStore(home)
    assert.deepEqual((await next.readThreads(ws)).sink.sent, [{ text: 'gated' }])
    await next.close()
  })
})

test('store: two overlapping mutations on one row both land', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })

    // Two read-modify-write cycles on the SAME row, started together: an
    // unsynchronised store interleaves them and loses one append; the queue
    // exists so that it cannot.
    await Promise.all([
      store.sentRecord(ws, { name: 'nyx-coral-lane', entry: { text: 'first question' } }),
      store.sentRecord(ws, { name: 'nyx-coral-lane', entry: { text: 'second question' } }),
    ])

    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.deepEqual(row.sent.map((entry) => entry.text).sort(), [
      'first question',
      'second question',
    ])
    await store.close()
  })
})

// --- unreadable state is never proof of absence --------------------------------

test('store: a corrupt tabs.json refuses to open and preserves every byte', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
    })
    await store.close()

    const tabsFile = path.join(home, 'app', 'tabs.json')
    const before = await readFile(tabsFile, 'utf8')
    await writeFile(tabsFile, '{corrupt!!', 'utf8')

    const reopened = new Store(home)
    await assert.rejects(() => reopened.open(), /tabs\.json/i)

    // The failed open released its claim and touched nothing: a second
    // attempt fails the same way, not on a stale lock, and the bytes —
    // corrupt as they are — are exactly what was there.
    await assert.rejects(() => new Store(home).open(), /tabs\.json/i)
    assert.equal(await readFile(tabsFile, 'utf8'), '{corrupt!!')

    // Restoring the file restores everything, including the reservation
    // recovery must never have released.
    await writeFile(tabsFile, before, 'utf8')
    const recovered = await openStore(home)
    assert.equal((await recovered.readThreads(ws))['nyx-coral-lane'].reserved.pane, 'w-7')
    await recovered.close()
  })
})

test('store: a corrupt threads.json fails the mutation and preserves bytes', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })

    const threadsFile = path.join(home, 'workspaces', workspaceKey(ws), 'threads.json')
    await writeFile(threadsFile, '[garbage', 'utf8')

    await assert.rejects(
      () => store.sentRecord(ws, { name: 'nyx-coral-lane', entry: { text: 'hello?' } }),
      /threads\.json/i,
      'an unreadable row is not an empty row the write may adopt',
    )
    assert.equal(await readFile(threadsFile, 'utf8'), '[garbage')
    await store.close()
  })
})

test('store: a rejected mutation rejects its caller and never poisons the queue', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })

    // A mutation that fails mid-queue: its caller sees the rejection…
    await assert.rejects(
      () =>
        store.mutate(ws, 'sent.record', async () => {
          throw new Error('boom mid-queue')
        }),
      /boom mid-queue/,
    )

    // …and everything enqueued after it still runs.
    await store.sentRecord(ws, { name: 'nyx-coral-lane', entry: { text: 'after the failure' } })
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.deepEqual(
      row.sent.map((entry) => entry.text),
      ['after the failure'],
    )
    await store.close()
  })
})

test('store: two overlapping mutations on different rows in one file both land', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)

    await Promise.all([
      store.conversationCreate(ws, { name: 'nyx-amber-moss', agent: 'nyx', kind: 'pi' }),
      store.conversationCreate(ws, { name: 'nyx-bubble-sky', agent: 'nyx', kind: 'pi' }),
    ])

    assert.deepEqual(Object.keys(await store.readThreads(ws)).sort(), [
      'nyx-amber-moss',
      'nyx-bubble-sky',
    ])
    await store.close()
  })
})

test('store: two tabs created in different directories both land', async () => {
  await withHome(async (home, dir) => {
    const store = await openStore(home)

    // Different cwds, one shared tabs.json. A per-workspace queue would let
    // these two interleave their read-modify-write and lose a tab; the queue
    // is ONE and app-wide.
    await Promise.all([
      store.mutate(path.join(dir, 'tabdir-a'), 'tab.create', async (io) => {
        const tabs = await io.readTabs()
        tabs.push({ id: 't-a', directory: path.join(dir, 'tabdir-a'), panes: [] })
        await io.writeTabs(tabs)
      }),
      store.mutate(path.join(dir, 'tabdir-b'), 'tab.create', async (io) => {
        const tabs = await io.readTabs()
        tabs.push({ id: 't-b', directory: path.join(dir, 'tabdir-b'), panes: [] })
        await io.writeTabs(tabs)
      }),
    ])

    assert.deepEqual((await store.readTabs()).map((tab) => tab.id).sort(), ['t-a', 't-b'])
    await store.close()
  })
})

test('store: 50 concurrent mixed mutations lose none', async () => {
  await withHome(async (home, dir) => {
    const wsA = path.join(dir, 'ws-a')
    const wsB = path.join(dir, 'ws-b')
    const store = await openStore(home)

    // Targets that exist before the storm, so every concurrent job mutates.
    await store.conversationCreate(wsA, { name: 'sink', agent: 'nyx', kind: 'pi' })
    await seedTabRecord(store, wsA, 't-mix', [
      workerPane('p-1', 0),
      workerPane('p-2', 1),
      workerPane('p-3', 2),
      workerPane('p-4', 3),
    ])

    const jobs = []
    for (let i = 1; i <= 10; i += 1) {
      jobs.push(
        store.conversationCreate(wsA, { name: `c-${i}`, agent: 'ares', kind: 'claude-code' }),
      )
    }
    for (let i = 1; i <= 5; i += 1) {
      jobs.push(store.conversationCreate(wsB, { name: `d-${i}`, agent: 'hera', kind: 'codex' }))
    }
    for (let i = 1; i <= 8; i += 1) {
      jobs.push(store.sentRecord(wsA, { name: 'sink', entry: { text: `q${i}` } }))
    }
    for (let i = 1; i <= 8; i += 1) {
      jobs.push(store.seenSet(wsA, { name: 'sink', items: [`item-${i}`], lead: 'tab:t-mix:1' }))
    }
    for (let i = 1; i <= 7; i += 1) {
      jobs.push(seedLegacy(store, wsA, { id: `del-${i}`, state: 'pending', answerId: `ans-${i}` }))
    }
    jobs.push(seedLegacy(store, wsA, { id: 'del-3', state: 'accepted' }))
    jobs.push(seedLegacy(store, wsA, { id: 'del-5', note: 're-planned' }))
    for (let i = 1; i <= 5; i += 1) {
      const directory = path.join(dir, `tabdir-${i}`)
      jobs.push(
        store.mutate(directory, 'tab.create', async (io) => {
          const tabs = await io.readTabs()
          tabs.push({ id: `t-raw-${i}`, directory, panes: [] })
          await io.writeTabs(tabs)
        }),
      )
    }
    jobs.push(store.policySet({ tab: 't-mix', value: 'manual' }))
    for (let i = 1; i <= 4; i += 1) {
      jobs.push(
        store.policySet({
          tab: 't-mix',
          pane: `p-${i}`,
          value: i % 2 === 0 ? 'manual' : 'inherit',
        }),
      )
    }
    assert.equal(jobs.length, 50, 'the storm is exactly the promised size')
    await Promise.all(jobs)

    const threadsA = await store.readThreads(wsA)
    assert.equal(Object.keys(threadsA).length, 11, 'sink plus ten conversations')
    assert.equal(threadsA.sink.sent.length, 8, 'every sent.record landed')
    assert.deepEqual(threadsA.sink.sent.map((entry) => entry.text).sort(), [
      'q1',
      'q2',
      'q3',
      'q4',
      'q5',
      'q6',
      'q7',
      'q8',
    ])
    assert.deepEqual(Object.keys(threadsA.sink.seen), ['tab:t-mix:1'])
    assert.equal(threadsA.sink.seen['tab:t-mix:1'].length, 8, 'every seen.set landed')
    assert.deepEqual(threadsA.sink.seen['tab:t-mix:1'].sort(), [
      'item-1',
      'item-2',
      'item-3',
      'item-4',
      'item-5',
      'item-6',
      'item-7',
      'item-8',
    ])
    assert.equal(Object.keys(await store.readThreads(wsB)).length, 5)

    const deliveries = await store.readDeliveries(wsA)
    assert.equal(Object.keys(deliveries).length, 7, 'seven distinct deliveries')
    assert.equal(deliveries['del-3'].state, 'accepted', 'the re-upsert merged')
    assert.equal(deliveries['del-3'].answerId, 'ans-3', 'and kept what it did not replace')
    assert.equal(deliveries['del-5'].note, 're-planned')
    assert.equal(deliveries['del-5'].state, 'pending')

    const tabs = await store.readTabs()
    assert.equal(tabs.length, 6, 'the seeded tab plus five concurrent creates')
    const mix = tabs.find((tab) => tab.id === 't-mix')
    assert.equal(mix.policy, 'manual')
    assert.deepEqual(
      mix.panes.map((pane) => pane.policy),
      ['inherit', 'manual', 'inherit', 'manual'],
      'every pane policy landed',
    )

    // Every write went tmp+rename: no shard survives the storm.
    const wsDir = path.join(home, 'workspaces', workspaceKey(wsA))
    assert.deepEqual((await readdir(wsDir)).sort(), ['deliveries.json', 'threads.json'])
    assert.deepEqual((await readdir(path.join(home, 'app'))).sort(), ['instance.lock', 'tabs.json'])
    await store.close()
  })
})

// --- the two-file reservation -----------------------------------------------

test('store: conversation.create validates the pane link before writing anything', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')

    const base = { agent: 'nyx', kind: 'codex' }
    // Unknown tab: refused, and no row survives the refusal.
    await assert.rejects(
      () =>
        store.conversationCreate(ws, {
          ...base,
          name: 'nyx-lost-lane',
          pane: { tab: 't-nope', id: 'p-1' },
        }),
      /no tab/,
    )
    assert.deepEqual(Object.keys(await store.readThreads(ws)), [])

    // A good link, then a generation conflict on the same pane.
    await store.conversationCreate(ws, {
      ...base,
      name: 'nyx-kept-lane',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
    })
    await assert.rejects(
      () =>
        store.conversationCreate(ws, {
          ...base,
          name: 'nyx-clash-lane',
          pane: { tab: 't-1', id: 'w-7', generation: 9 },
        }),
      /generation/,
    )
    // And an ownership conflict: the pane already serves another conversation.
    await assert.rejects(
      () =>
        store.conversationCreate(ws, {
          ...base,
          name: 'nyx-other-lane',
          pane: { tab: 't-1', id: 'w-7', generation: 1 },
        }),
      /already serves/,
    )

    const threads = await store.readThreads(ws)
    assert.deepEqual(Object.keys(threads).sort(), ['nyx-kept-lane'])
    assert.equal(threads['nyx-kept-lane'].reserved.generation, 1)
    await store.close()
  })
})

test('store: a failed pane-link write rolls the conversation row back', async () => {
  if (typeof process.getuid === 'function' && process.getuid() === 0) {
    return // read-only files do not refuse root; the refusal is the instrument
  }
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, { name: 'nyx-first-lane', agent: 'nyx', kind: 'codex' })

    // Fault injection without a code hook: the app directory refuses writes
    // while still reading fine, so validation passes, the first (workspace)
    // write lands, and only the second (pane-link) write fails.
    const appDir = path.join(home, 'app')
    await chmod(appDir, 0o555)
    try {
      await assert.rejects(
        () =>
          store.conversationCreate(ws, {
            name: 'nyx-doomed-lane',
            agent: 'nyx',
            kind: 'codex',
            pane: { tab: 't-1', id: 'w-9', generation: 1 },
          }),
        /EACCES|EPERM|EROFS/i,
      )
      const threads = await store.readThreads(ws)
      assert.deepEqual(
        Object.keys(threads),
        ['nyx-first-lane'],
        'the first file was restored inside the queue — no partial row survives',
      )
    } finally {
      await chmod(appDir, 0o755)
    }

    // The store itself is unpoisoned: later mutations still land.
    await store.conversationCreate(ws, { name: 'nyx-after-lane', agent: 'nyx', kind: 'codex' })
    assert.deepEqual(Object.keys(await store.readThreads(ws)).sort(), [
      'nyx-after-lane',
      'nyx-first-lane',
    ])
    await store.close()
  })
})

test('store: a reservation with no pane is released on restart', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')

    // `conversation.create` writes the reservation into threads.json FIRST,
    // then the pane into tabs.json — that window is the documented crash
    // case, and the recovery rule covers exactly it.
    const kept = await store.conversationCreate(ws, {
      name: 'nyx-kept-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
    })
    assert.equal(kept.reserved.pane, 'w-7')

    const tabsFile = path.join(home, 'app', 'tabs.json')
    const beforeTheCrash = await readFile(tabsFile, 'utf8')

    const lost = await store.conversationCreate(ws, {
      name: 'nyx-lost-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-8', generation: 1 },
    })
    assert.equal(lost.reserved.pane, 'w-8')

    // The crash: the tabs.json half of the second reservation never made it.
    await writeFile(tabsFile, beforeTheCrash, 'utf8')

    await store.close()
    const restarted = await openStore(home)
    const threads = await restarted.readThreads(ws)
    assert.equal(
      threads['nyx-kept-lane'].reserved.pane,
      'w-7',
      'a reservation whose pane exists survives the restart',
    )
    assert.equal(
      threads['nyx-lost-lane'].reserved,
      undefined,
      'a reservation with no pane is released',
    )
    await restarted.close()
  })
})

// --- reservation ops for existing conversations --------------------------------

test('store: reserve and release manage a launch on an existing conversation', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })

    // Reserving an unknown conversation is a caller bug, not an empty row.
    await assert.rejects(
      () => store.reserve(ws, { name: 'nyx-nope-lane', pane: { tab: 't-1', id: 'w-7' } }),
      /no conversation/,
    )

    const reserved = await store.reserve(ws, {
      name: 'nyx-coral-lane',
      pane: { tab: 't-1', id: 'w-7', generation: 3 },
      launch: { nonce: 'n-7f3a' },
    })
    assert.equal(reserved.reserved.pane, 'w-7')
    assert.equal(reserved.reserved.generation, 3)
    assert.equal(reserved.reserved.nonce, 'n-7f3a')
    assert.equal(typeof reserved.reserved.launchId, 'string')
    assert.ok(reserved.reserved.launchId.length > 0)

    // The launch identity is durable: rereads keep it, and the binding
    // records it alongside the evidence and generation.
    const reread = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(reread.reserved.launchId, reserved.reserved.launchId)
    const bound = await store.sessionBind(ws, {
      name: 'nyx-coral-lane',
      candidate: { sessionId: 'sess-live', turn: '[consensflow launch n-7f3a]\nship it' },
      expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
    })
    assert.equal(bound.binding.launchId, reserved.reserved.launchId)

    // A second reservation on a held conversation conflicts.
    await assert.rejects(
      () =>
        store.reserve(ws, {
          name: 'nyx-coral-lane',
          pane: { tab: 't-1', id: 'w-8', generation: 1 },
          launch: { nonce: 'n-other' },
        }),
      /already reserved/,
    )

    // Release drops the launch it names; binding afterwards has nothing to
    // decide on.
    const released = await store.release(ws, {
      name: 'nyx-coral-lane',
      launchId: reserved.reserved.launchId,
    })
    assert.equal(released.reserved, undefined)
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate: { sessionId: 's' },
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /launch/i,
    )
    await assert.rejects(
      () => store.release(ws, { name: 'nyx-coral-lane', launchId: reserved.reserved.launchId }),
      /no reservation/,
    )
    await store.close()
  })
})

test('store: a replaced session persists its invalidation instead of keeping the old binding', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
      launch: { nonce: 'n-7f3a' },
    })
    await store.sessionBind(ws, {
      name: 'nyx-coral-lane',
      candidate: { sessionId: 'sess-live', turn: '[consensflow launch n-7f3a]\nship it' },
      expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
    })

    // The native session was replaced in place (/new, /resume, a fork):
    // the bind is refused AND the old binding dies on disk with it.
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate: { sessionId: 'sess-live', currentSessionId: 'sess-new', alive: true },
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /replaced/i,
    )
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.sessionId, null, 'the replaced session no longer names a native session')
    assert.equal(row.binding, undefined, 'the old binding is gone, not kept')
    assert.match(row.replaced.reason, /replaced/i)
    assert.equal(row.replaced.previousSession, 'sess-live')
    assert.equal(typeof row.replaced.at, 'string')
    await store.close()
  })
})

test('store: seen marks are scoped to the app-owned lead identity', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })

    await assert.rejects(
      () => store.seenSet(ws, { name: 'nyx-coral-lane', items: ['item-1'] }),
      /lead/i,
      'a mark without its lead identity is nobody’s mark',
    )
    await assert.rejects(
      () => store.seenSet(ws, { name: 'nyx-nope-lane', items: ['item-1'], lead: 'tab:t-1:1' }),
      /no conversation/,
    )

    await store.seenSet(ws, {
      name: 'nyx-coral-lane',
      items: ['item-1', 'item-2'],
      lead: 'tab:t-1:1',
    })
    await store.seenSet(ws, {
      name: 'nyx-coral-lane',
      items: ['item-2', 'item-3'],
      lead: 'tab:t-1:2',
    })
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.deepEqual(row.seen['tab:t-1:1'], ['item-1', 'item-2'])
    assert.deepEqual(
      row.seen['tab:t-1:2'],
      ['item-2', 'item-3'],
      'generations keep their own marks',
    )
    await store.close()
  })
})

// --- session.bind: bindEvidence decides, the store records -------------------

test('store: session.bind binds only what bindEvidence accepts, and stamps the generation', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')

    // The launch record the store holds: the pane plus the evidence minted
    // before the launch. Binding decides against it, never against task text.
    const launched = await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 3 },
      launch: { nonce: 'n-7f3a' },
    })
    assert.equal(launched.reserved.nonce, 'n-7f3a')

    const bound = await store.sessionBind(ws, {
      name: 'nyx-coral-lane',
      candidate: { sessionId: 'sess-live', turn: '[consensflow launch n-7f3a]\nship it' },
      expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
    })
    assert.equal(bound.sessionId, 'sess-live')
    assert.equal(bound.binding.evidence, 'nonce')
    assert.equal(bound.binding.generation, 3, 'the binding records the generation it was made at')
    assert.equal(typeof bound.binding.at, 'string')

    // A candidate whose first turn carries no marker for THIS launch is refused.
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate: { sessionId: 'sess-other', turn: 'ship it' },
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /unbound/i,
    )

    // A session replaced in place never binds.
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate: { sessionId: 'sess-live', alive: true, currentSessionId: 'sess-new' },
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /replaced/i,
    )

    // Preallocated kinds bind their preallocated session and nothing else.
    await store.conversationCreate(ws, {
      name: 'nyx-amber-moss',
      agent: 'nyx',
      kind: 'pi',
      pane: { tab: 't-1', id: 'w-8', generation: 2 },
      launch: { preallocatedId: 'pi-named-session' },
    })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-amber-moss',
          candidate: { sessionId: 'someone-elses' },
          expect: await currentLaunch(store, ws, 'nyx-amber-moss'),
        }),
      /preallocated/i,
    )
    const preallocated = await store.sessionBind(ws, {
      name: 'nyx-amber-moss',
      candidate: { sessionId: 'pi-named-session' },
      expect: await currentLaunch(store, ws, 'nyx-amber-moss'),
    })
    assert.equal(preallocated.binding.evidence, 'preallocated')
    assert.equal(preallocated.binding.generation, 2)

    // The harness reporting the id on our stream binds too.
    await store.conversationCreate(ws, {
      name: 'nyx-bubble-sky',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-9', generation: 1 },
      launch: { reportedId: 'rep-1' },
    })
    const reported = await store.sessionBind(ws, {
      name: 'nyx-bubble-sky',
      candidate: { sessionId: 'rep-1' },
      expect: await currentLaunch(store, ws, 'nyx-bubble-sky'),
    })
    assert.equal(reported.binding.evidence, 'reported')

    // A row with no launch record has nothing to bind against.
    await store.conversationCreate(ws, { name: 'nyx-bare-lane', agent: 'nyx', kind: 'codex' })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-bare-lane',
          candidate: { sessionId: 'sess-x' },
          expect: await currentLaunch(store, ws, 'nyx-bare-lane'),
        }),
      /launch/i,
    )

    // A bare evidence word from the caller is never proof: the verdict comes
    // from bindEvidence against the stored launch record, not from a string
    // anyone can send.
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-bare-lane',
          candidate: { sessionId: 'sess-x', evidence: 'nonce' },
          expect: await currentLaunch(store, ws, 'nyx-bare-lane'),
        }),
      /launch/i,
      'no launch record, no bind — whatever the candidate claims',
    )
    await store.conversationCreate(ws, {
      name: 'nyx-word-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-10', generation: 1 },
      launch: { nonce: 'n-real' },
    })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-word-lane',
          candidate: { sessionId: 'sess-x', evidence: 'nonce' },
          expect: await currentLaunch(store, ws, 'nyx-word-lane'),
        }),
      /unbound/i,
      'the word "nonce" without the marker in the first turn binds nothing',
    )

    // The unbound refusals wrote nothing; the replaced refusal persisted
    // its invalidation (asserted in full by the dedicated test above).
    const threads = await store.readThreads(ws)
    assert.equal(threads['nyx-coral-lane'].sessionId, null)
    assert.equal(threads['nyx-coral-lane'].binding, undefined)
    assert.match(threads['nyx-coral-lane'].replaced.reason, /replaced/i)
    assert.equal(threads['nyx-amber-moss'].sessionId, 'pi-named-session')
    assert.equal(threads['nyx-bare-lane'].sessionId, null)
    await store.close()
  })
})

// --- failure branches ----------------------------------------------------------

test('store: unknown rows and malformed inputs fail without writing', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })

    await assert.rejects(
      () => store.sentRecord(ws, { name: 'nyx-nope-lane', entry: {} }),
      /no conversation/,
    )
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-nope-lane',
          candidate: {},
          expect: await currentLaunch(store, ws, 'nyx-nope-lane'),
        }),
      /no conversation/,
    )
    await assert.rejects(
      () => store.release(ws, { name: 'nyx-nope-lane', launchId: 'L1' }),
      /no conversation/,
    )
    await assert.rejects(
      () => store.seenSet(ws, { name: 'nyx-nope-lane', items: [], lead: 'tab:t-1:1' }),
      /no conversation/,
    )

    await assert.rejects(
      () => store.sentRecord(ws, { name: 'nyx-coral-lane', entry: 'nope' }),
      /object/,
    )
    await assert.rejects(
      () => store.seenSet(ws, { name: 'nyx-coral-lane', items: 'nope', lead: 'tab:t-1:1' }),
      /array/,
    )
    await assert.rejects(
      () => store.seenSet(ws, { name: 'nyx-coral-lane', items: [''], lead: 'tab:t-1:1' }),
      /required/,
    )
    await assert.rejects(
      () => store.conversationCreate(ws, { name: '', agent: 'nyx', kind: 'pi' }),
      /required/,
    )
    await assert.rejects(() => store.reserve(ws, { name: 'nyx-coral-lane' }), /pane must be/)
    await assert.rejects(() => store.policySet({ tab: 't-1', value: 'sometimes' }), /policy|tab/)

    // Untouched rows, and an upsert for an unknown delivery id is creation
    // by design — not an unknown-row failure.
    const created = await seedLegacy(store, ws, { id: 'del-new', state: 'pending' })
    assert.equal(created.id, 'del-new')
    const threads = await store.readThreads(ws)
    assert.deepEqual(Object.keys(threads), ['nyx-coral-lane'])
    assert.deepEqual(threads['nyx-coral-lane'].sent, [])
    await store.close()
  })
})

// --- policy.set --------------------------------------------------------------

test('store: policy.set writes the human policy at tab and pane scope', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('p-1', 0)])

    const tab = await store.policySet({ tab: 't-1', value: 'manual' })
    assert.equal(tab.policy, 'manual', 'a tab human manual vetoes everything under it')

    const after = await store.policySet({ tab: 't-1', pane: 'p-1', value: 'inherit' })
    assert.equal(after.panes.find((pane) => pane.id === 'p-1').policy, 'inherit')

    await assert.rejects(() => store.policySet({ tab: 't-1', value: 'sometimes' }), /policy/i)
    await assert.rejects(
      () => store.policySet({ tab: 't-1', pane: 'p-1', value: 'sometimes' }),
      /policy/i,
    )
    await assert.rejects(() => store.policySet({ tab: 'never', value: 'manual' }), /tab/i)
    await store.close()
  })
})

// --- harness stores stay read-only -------------------------------------------

test('store: harness stores are never opened for writing', async () => {
  const { root, env, cleanup } = tempEnv()
  try {
    // Pre-create the harness homes with a sentinel each: untouched means the
    // sentinel bytes are intact AND nothing new appeared beside them.
    const harnessHomes = [
      env.CLAUDE_CONFIG_DIR,
      env.CODEX_HOME,
      env.XDG_CONFIG_HOME,
      path.join(env.HOME, '.pi'),
      path.join(env.HOME, '.kimi-code'),
    ]
    await Promise.all(
      harnessHomes.map(async (dir) => {
        await mkdir(dir, { recursive: true })
        await writeFile(path.join(dir, 'sentinel'), 'harness-owned')
      }),
    )

    // Read-only harness homes AND sentinels: any write-capable open
    // (create, truncate, tmp+rename, or overwriting the sentinel itself)
    // fails outright, so a passing op pass proves the store never opened
    // these directories for writing — not merely that their contents look
    // unchanged afterwards.
    const lockHarnessHomes = typeof process.getuid === 'function' && process.getuid() !== 0
    if (lockHarnessHomes) {
      await Promise.all(
        harnessHomes.map(async (dir) => {
          await chmod(path.join(dir, 'sentinel'), 0o444)
          await chmod(dir, 0o555)
        }),
      )
    }

    const store = new Store(env.CONSENSFLOW_HOME)
    await store.open()
    const ws = path.join(root, 'ws')
    await seedTabRecord(store, ws, 't-1', [workerPane('p-1', 0)])

    // A pass over every named op: none of them has any reason to touch a
    // harness home, and this is the test that notices when one does.
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      pane: { tab: 't-1', id: 'p-1', generation: 1 },
      launch: { nonce: 'n-abc' },
    })
    await store.sessionBind(ws, {
      name: 'nyx-coral-lane',
      candidate: { sessionId: 'sess-1', turn: '[consensflow launch n-abc]\nship it' },
      expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
    })
    await store.sentRecord(ws, {
      name: 'nyx-coral-lane',
      entry: { text: 'how is it going?' },
      expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
    })
    await store.seenSet(ws, { name: 'nyx-coral-lane', items: ['item-1'], lead: 'tab:t-1:1' })
    await seedLegacy(store, ws, { id: 'del-1', state: 'pending', answerId: 'ans-1' })
    await store.policySet({ tab: 't-1', value: 'manual' })
    await store.mutate(ws, 'tab.suspend', async (io) => {
      const tabs = await io.readTabs()
      tabs[0].closed = true
      await io.writeTabs(tabs)
    })
    await store.close()

    if (lockHarnessHomes) {
      await Promise.all(
        harnessHomes.map(async (dir) => {
          await chmod(dir, 0o755)
          await chmod(path.join(dir, 'sentinel'), 0o644)
        }),
      )
    }

    for (const harnessHome of harnessHomes) {
      assert.deepEqual(await readdir(harnessHome), ['sentinel'], `${harnessHome} was written`)
      assert.equal(await readFile(path.join(harnessHome, 'sentinel'), 'utf8'), 'harness-owned')
    }

    // And nothing landed anywhere outside the store's own root, either.
    const strays = (await collectFiles(root)).filter(
      (file) => !file.startsWith(env.CONSENSFLOW_HOME) && path.basename(file) !== 'sentinel',
    )
    assert.deepEqual(strays, [], 'a write landed outside the state root')
  } finally {
    cleanup()
  }
})

// --- lifecycle boundaries (S2) ------------------------------------------------

/**
 * Parks the release of the lock — the close of the descriptor the kernel's
 * lock hangs on — and lets it go on demand. That is the window a second
 * close() must be handed the outstanding drain in: admission has shut,
 * `#opened` is already false, and the root is still held.
 */
function parkLockRelease(lockPath) {
  const realOpen = fsp.open
  let announce
  let release
  const arrived = new Promise((resolve) => {
    announce = resolve
  })
  const go = new Promise((resolve) => {
    release = resolve
  })
  let armed = true
  fsp.open = async (target, ...rest) => {
    const handle = await realOpen(target, ...rest)
    if (String(target) !== lockPath) return handle
    const realClose = handle.close.bind(handle)
    handle.close = async () => {
      if (armed) {
        armed = false
        announce()
        await go
      }
      return realClose()
    }
    return handle
  }
  return {
    arrived,
    release: () => release(),
    restore: () => {
      fsp.open = realOpen
    },
  }
}

test('store: a close during the release shares the one outstanding drain', async () => {
  await withHome(async (home) => {
    const lockPath = path.join(home, 'app', 'instance.lock')
    // Armed before the root is taken, so the park wraps the descriptor this
    // store will hold.
    const park = parkLockRelease(lockPath)
    try {
      const store = await openStore(home)
      const first = store.close()
      await park.arrived
      const second = store.close()
      assert.equal(second, first, 'the outstanding close is the only drain')

      let settled = false
      const watched = second.then(() => {
        settled = true
      })
      await new Promise((r) => setTimeout(r, 50))
      assert.equal(settled, false, 'no close resolves while the root is still held')

      park.release()
      await Promise.all([first, second, watched])
    } finally {
      park.restore()
    }
    // Released exactly once, and the proof is that the next instance gets in.
    const successor = await openStore(home)
    await successor.close()
  })
})

test('store: a store reopens after a completed close', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'pi' })
    await store.close()

    // The same instance, reopened: recovery runs on the reclaimed root, so
    // the lifecycle state must be reset before it, not after.
    await store.open()
    await store.sentRecord(ws, { name: 'nyx-coral-lane', entry: { text: 'after the reopen' } })
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.deepEqual(
      row.sent.map((entry) => entry.text),
      ['after the reopen'],
    )
    await store.close()
  })
})

// --- unreadable state is never proof of absence: recovery too (S5) ------------

test('store: a recovery that cannot read workspaces refuses and releases the claim', async () => {
  if (typeof process.getuid === 'function' && process.getuid() === 0) {
    return // an unreadable directory does not refuse root; the refusal is the instrument
  }
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const lockPath = path.join(home, 'app', 'instance.lock')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
    })
    await store.close()

    // An unreadable workspaces/ is not an empty workspaces/: skipping
    // recovery here would silently keep reservations recovery must judge.
    const workspaces = path.join(home, 'workspaces')
    await chmod(workspaces, 0o000)
    try {
      await assert.rejects(() => new Store(home).open(), /EACCES|EPERM|permission|workspaces/i)
    } finally {
      await chmod(workspaces, 0o755)
    }
    // The failed open let go of the lock: the proof is that the next
    // instance gets in, not that a file went away.
    assert.equal(typeof (await readFile(lockPath, 'utf8')), 'string')

    const reopened = await openStore(home)
    assert.equal((await reopened.readThreads(ws))['nyx-coral-lane'].reserved.pane, 'w-7')
    await reopened.close()
  })
})

// --- a release names the launch it releases ----------------------------------

test('store: a stale release for an ended launch leaves the newer reservation intact', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })

    await store.reserve(ws, {
      name: 'nyx-coral-lane',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
      launch: { launchId: 'L1', nonce: 'n-1' },
    })
    await store.release(ws, { name: 'nyx-coral-lane', launchId: 'L1' })
    await store.reserve(ws, {
      name: 'nyx-coral-lane',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
      launch: { launchId: 'L2', nonce: 'n-2' },
    })

    // L1's delayed cleanup arrives after L2 reserved the same conversation:
    // it names a launch that is over, and must take nothing with it.
    await assert.rejects(
      () => store.release(ws, { name: 'nyx-coral-lane', launchId: 'L1' }),
      /L1|launch/,
    )
    assert.equal(
      (await store.readThreads(ws))['nyx-coral-lane'].reserved.launchId,
      'L2',
      'the newer launch keeps its reservation',
    )

    // A release that names no launch is not a release: it is the guard gone.
    await assert.rejects(() => store.release(ws, { name: 'nyx-coral-lane' }), /launch id/i)
    await store.release(ws, { name: 'nyx-coral-lane', launchId: 'L2' })
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved, undefined)
    await store.close()
  })
})

// --- the lock under an adversarial schedule (S1, S6) --------------------------

/**
 * A shim the contender loads with `--import`: it parks the FIRST fs call
 * that removes the lock NAME — `rm` under a read-then-unlink reclaim,
 * `rename` under a take-aside one — and waits for the parent's go-ahead.
 * That is the exact window a reclaimer sits in after its final read, and
 * it is the only way to schedule two real processes into it.
 */
/** A `ps` on PATH that refuses one pid and answers for every other. */
// --- a failed compensation is not a failure to walk away from (S4) -----------

/** Fails every write under `home` after the first `allowed` ones. */
function failWritesAfter(home, allowed) {
  const realWriteFile = fsp.writeFile
  let seen = 0
  fsp.writeFile = async (file, ...rest) => {
    if (String(file).startsWith(home)) {
      seen += 1
      if (seen > allowed) {
        const error = new Error(`ENOSPC: no space left on device, write '${file}'`)
        error.code = 'ENOSPC'
        throw error
      }
    }
    return realWriteFile(file, ...rest)
  }
  return {
    restore: () => {
      fsp.writeFile = realWriteFile
    },
  }
}

test('store: a failed compensation degrades the store until it is recovered', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, { name: 'nyx-first-lane', agent: 'nyx', kind: 'codex' })

    // The pane-link write fails AND the rollback that would have undone the
    // reservation fails with it: the row is committed without its pane, and
    // nothing the store can do inside this mutation puts that right.
    const fault = failWritesAfter(home, 1)
    let queued
    try {
      const rejected = store.conversationCreate(ws, {
        name: 'nyx-doomed-lane',
        agent: 'nyx',
        kind: 'codex',
        pane: { tab: 't-1', id: 'w-7', generation: 1 },
      })
      queued = store.sentRecord(ws, {
        name: 'nyx-first-lane',
        entry: { text: 'queued behind the failure' },
      })
      const failure = await rejected.then(
        () => null,
        (error) => error,
      )
      assert.ok(failure !== null, 'the double failure reaches its caller')
      assert.match(failure.message, /degraded/i)
      assert.match(failure.message, /ENOSPC/, 'the write failure is named')
      assert.equal(failure.cause?.code, 'ENOSPC', 'the write failure is preserved')
      assert.equal(failure.compensation?.code, 'ENOSPC', 'and so is the failure to undo it')
    } finally {
      fault.restore()
    }

    // A mutation already queued behind the failure refuses too: it would
    // have run against a row the store knows is half-written.
    await assert.rejects(() => queued, /degraded/i)
    // And so does a new one, though the disk is writable again.
    await assert.rejects(
      () => store.sentRecord(ws, { name: 'nyx-first-lane', entry: { text: 'new' } }),
      /degraded/i,
    )
    assert.equal(
      (await store.readThreads(ws))['nyx-doomed-lane'].reserved.pane,
      'w-7',
      'the partial state is exactly what recovery is for',
    )
    assert.equal(store.degraded, true, 'and the store says so')

    const recovered = await store.recover()
    assert.equal(store.degraded, false)
    assert.equal(recovered.reservationsReleased.length, 1)
    assert.match(recovered.reservationsReleased[0], /nyx-doomed-lane/)
    assert.equal(
      (await store.readThreads(ws))['nyx-doomed-lane'].reserved,
      undefined,
      'the reservation with no pane is released, by the documented rule',
    )

    // Recovered means open for business again.
    await store.sentRecord(ws, { name: 'nyx-first-lane', entry: { text: 'after the recovery' } })
    assert.deepEqual(
      (await store.readThreads(ws))['nyx-first-lane'].sent.map((entry) => entry.text),
      ['after the recovery'],
    )
    await store.close()
  })
})

// --- one allocator for every pane-creation path (T1) -------------------------

test('store: a reservation may not take a pane identity that lives elsewhere', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await seedTabRecord(store, ws, 't-2')
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
    })
    await store.conversationCreate(ws, { name: 'nyx-bubble-sky', agent: 'nyx', kind: 'codex' })

    // Pane identity is app-wide: a second tab naming a live pane of the
    // first is a collision, never a second pane — through either writer.
    await assert.rejects(
      () =>
        store.conversationCreate(ws, {
          name: 'nyx-amber-moss',
          agent: 'nyx',
          kind: 'codex',
          pane: { tab: 't-2', id: 'w-7', generation: 1 },
        }),
      /w-7/,
    )
    await assert.rejects(
      () =>
        store.reserve(ws, {
          name: 'nyx-bubble-sky',
          pane: { tab: 't-2', id: 'w-7', generation: 1 },
        }),
      /w-7/,
    )
    // The minted namespace belongs to the allocator: no writer invents an
    // identity inside it, however free it looks.
    await assert.rejects(
      () =>
        store.reserve(ws, {
          name: 'nyx-bubble-sky',
          pane: { tab: 't-2', id: 'p-3', generation: 1 },
        }),
      /allocator|minted/i,
    )

    const tabs = await store.readTabs()
    assert.deepEqual(tabs.find((tab) => tab.id === 't-2').panes, [], 'nothing landed in t-2')
    assert.deepEqual(Object.keys(await store.readThreads(ws)).sort(), [
      'nyx-bubble-sky',
      'nyx-coral-lane',
    ])
    await store.close()
  })
})

test('store: a pane a reservation creates is issued by the same allocator', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })
    await store.reserve(ws, {
      name: 'nyx-coral-lane',
      pane: { tab: 't-1', id: 'w-9', generation: 1 },
      launch: { launchId: 'L1', nonce: 'n-1' },
    })

    // The identity this writer issued is recorded like any other: the tab
    // store may not hand the same one out again at the same generation.
    const tabs = new Tabs(store)
    await assert.rejects(
      () => tabs.addPane('t-1', { id: 'w-9', kind: 'shell', generation: 1 }),
      /duplicate pane id/,
    )
    await store.close()
  })
})

// --- the checks that had no test of their own (S3) ---------------------------

test('store: a reservation may not name a tab that lives in another directory', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const other = path.join(dir, 'other')
    const store = await openStore(home)
    await seedTabRecord(store, other, 't-other', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })

    // The tab is real and the pane is free — but the tab is another
    // directory's, and a conversation in this workspace has no business
    // reserving a pane over there. Through either writer.
    await assert.rejects(
      () =>
        store.reserve(ws, {
          name: 'nyx-coral-lane',
          pane: { tab: 't-other', id: 'w-7', generation: 1 },
        }),
      /lives in|not this workspace/,
    )
    await assert.rejects(
      () =>
        store.conversationCreate(ws, {
          name: 'nyx-amber-moss',
          agent: 'nyx',
          kind: 'codex',
          pane: { tab: 't-other', id: 'w-8', generation: 1 },
        }),
      /lives in|not this workspace/,
    )

    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved, undefined)
    const foreign = (await store.readTabs()).find((tab) => tab.id === 't-other')
    assert.deepEqual(
      foreign.panes.map((pane) => [pane.id, pane.conversation]),
      [['w-7', null]],
      'the foreign tab kept its panes and gained none',
    )
    await store.close()
  })
})

test('store: a bind is refused when the reserved pane no longer serves the conversation', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
      launch: { nonce: 'n-7f3a' },
    })
    const candidate = { sessionId: 'sess-live', turn: '[consensflow launch n-7f3a]\nship it' }

    // Rehomed: the launch evidence still checks out, the pane no longer
    // does — it serves another conversation now, and a binding made against
    // it would authorise writes into someone else's pane.
    await store.mutate(ws, 'tab.rehome', async (io) => {
      const tabs = await io.readTabs()
      tabs[0].panes[0].conversation = 'nyx-amber-moss'
      await io.writeTabs(tabs)
    })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate,
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /serves|no pane/,
    )
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].sessionId, null)

    // Replaced: same pane id, new generation — a different pane, and the
    // reservation names the one that is gone.
    await store.mutate(ws, 'tab.replace', async (io) => {
      const tabs = await io.readTabs()
      tabs[0].panes[0].conversation = 'nyx-coral-lane'
      tabs[0].panes[0].generation = 2
      await io.writeTabs(tabs)
    })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate,
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /no pane|generation/,
    )
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.sessionId, null, 'nothing bound')
    assert.equal(row.binding, undefined)
    await store.close()
  })
})

test('store: restart recovery releases a reservation whose pane serves someone else', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
    })

    // The pane key still matches the reservation exactly — same tab, same
    // id, same generation — but it serves another conversation. A matching
    // key is not the reservation's pane.
    await store.mutate(ws, 'tab.rehome', async (io) => {
      const tabs = await io.readTabs()
      tabs[0].panes[0].conversation = 'nyx-amber-moss'
      await io.writeTabs(tabs)
    })
    await store.close()

    const restarted = await openStore(home)
    assert.equal(
      (await restarted.readThreads(ws))['nyx-coral-lane'].reserved,
      undefined,
      'a pane that serves someone else never keeps this reservation alive',
    )
    await restarted.close()
  })
})

// --- a bind needs a pane that still serves this conversation (round 3) -------

test('store: a bind is refused when the reserved pane serves nobody at all', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])
    await store.conversationCreate(ws, {
      name: 'nyx-coral-lane',
      agent: 'nyx',
      kind: 'codex',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
      launch: { nonce: 'n-7f3a' },
    })
    const candidate = { sessionId: 'sess-live', turn: '[consensflow launch n-7f3a]\nship it' }

    // A queued unlink cleared the link: the pane serves nobody now, and an
    // unlinked pane is not this conversation's pane. The evidence is still
    // good — the target is not.
    await store.mutate(ws, 'tab.unlink', async (io) => {
      const tabs = await io.readTabs()
      tabs[0].panes[0].conversation = null
      await io.writeTabs(tabs)
    })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate,
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /serves|no pane/,
      'a null link is not a link',
    )
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].sessionId, null)

    // And an absent link is the same thing said differently.
    await store.mutate(ws, 'tab.strip', async (io) => {
      const tabs = await io.readTabs()
      delete tabs[0].panes[0].conversation
      await io.writeTabs(tabs)
    })
    await assert.rejects(
      async () =>
        store.sessionBind(ws, {
          name: 'nyx-coral-lane',
          candidate,
          expect: await currentLaunch(store, ws, 'nyx-coral-lane'),
        }),
      /serves|no pane/,
      'a missing link is not a link either',
    )
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.sessionId, null, 'nothing bound')
    assert.equal(row.binding, undefined)
    await store.close()
  })
})

test('store: a conversation named __proto__ is a row, never the prototype', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)

    // Conversation names come from the roster, not from this module, and
    // `__proto__` is a name like any other to a map that has no prototype.
    // On an ordinary object it would read as Object.prototype — a row that
    // "already exists", and a `sent` list pushed onto every object alive.
    // Read back through a computed key: spelling it as a member access
    // would go through the very accessor this test exists to keep out.
    const name = '__proto__'
    await store.conversationCreate(ws, { name, agent: 'nyx', kind: 'pi' })
    await store.sentRecord(ws, { name, entry: { text: 'to the row' } })

    const threads = await store.readThreads(ws)
    assert.ok(Object.hasOwn(threads, name), 'the row is an own key')
    assert.deepEqual(
      threads[name].sent.map((entry) => entry.text),
      ['to the row'],
    )
    assert.equal({}.sent, undefined, 'nothing leaked onto every object in the process')
    assert.equal({}.agent, undefined)

    // And an unknown row stays unknown, whatever the language says every
    // object inherits.
    await assert.rejects(
      () => store.sentRecord(ws, { name: 'toString', entry: {} }),
      /no conversation/,
    )
    await store.close()
  })
})

// --- ownership is the kernel's, not the file's -------------------------------

test('store: the claim file cannot let a second instance in, whatever it says', async () => {
  await withHome(async (home) => {
    const store = await openStore(home)
    const lockPath = path.join(home, 'app', 'instance.lock')

    // A provably dead pid, written straight over the live owner's claim.
    // To any design that READS the file to decide who owns the root, this
    // is the whole attack — and every interleaving asteria has built for
    // three rounds needed exactly this: bytes that authorise entry.
    const dead = spawnSync(process.execPath, ['-e', 'process.exit(0)'])
    assert.equal(dead.status, 0)
    await writeFile(lockPath, `${JSON.stringify({ pid: dead.pid, startedAt: 0 })}\n`, 'utf8')

    await assert.rejects(
      () => new Store(home).open(),
      /holds/,
      'the bytes are diagnostics: they decide nothing',
    )
    const outcomes = await runContenders(home, 1)
    assert.equal(outcomes[0].opened, false, 'and they decide nothing across processes either')

    // Emptying the file changes nothing either — there is no content a
    // contender can produce that hands it a held root.
    await writeFile(lockPath, '', 'utf8')
    await assert.rejects(() => new Store(home).open(), /holds/)

    // And there is no door marked reclaim: the interleaving cannot be
    // constructed because the API offers no way to take a held root.
    const surface = [
      ...Object.getOwnPropertyNames(Store.prototype),
      ...Object.keys(await import('../src/store.js')),
    ]
    for (const name of surface) {
      assert.doesNotMatch(name, /reclaim|steal|break|force|takeover/i, `${name} takes a root`)
    }
    await store.close()
  })
})

test('store: a process that dies holding the root releases it', async () => {
  await withHome(async (home) => {
    const lockPath = path.join(home, 'app', 'instance.lock')
    const resultFile = path.join(home, 'holder.json')
    // A hold file that never appears: the only way out of this child is death.
    const holder = spawnContender(home, { resultFile, holdFile: path.join(home, 'never') })
    try {
      await until(
        async () => (await readClaim(lockPath))?.pid === holder.pid,
        'the child to hold the root',
      )
      await assert.rejects(
        () => new Store(home).open(),
        /holds/,
        'a live holder keeps everyone out',
      )

      // The crash. Nothing runs in that process to tidy up, and nothing
      // here inspects what it left behind — the kernel drops the lock when
      // the process dies, which is the whole stale-lock problem, gone.
      holder.kill('SIGKILL')
      await exited(holder, 'the killed holder')

      const successor = await openStore(home)
      assert.equal(
        (await readClaim(lockPath))?.pid,
        process.pid,
        'the new owner overwrote the note: it never names a process that is gone',
      )
      await successor.close()
    } finally {
      try {
        holder.kill('SIGKILL')
      } catch {
        // Already gone.
      }
    }
  })
})

test('store: a platform without O_EXLOCK is refused, never run unlocked', async () => {
  await withHome(async (home) => {
    // The flag is a BSD number. Elsewhere it is not an error — it is bits
    // the kernel ignores, so the open succeeds and the store runs with no
    // lock at all and no sign of it. Refusing is the only honest answer
    // until the packaging work brings a lock for those platforms.
    const real = process.platform
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    try {
      await assert.rejects(() => new Store(home).open(), /O_EXLOCK/)
    } finally {
      Object.defineProperty(process, 'platform', { value: real, configurable: true })
    }

    // The refusal left nothing behind: the root is still there to be taken.
    const store = await openStore(home)
    await store.close()
  })
})

/** Makes one FileHandle step fail for the lock file, the way a full disk would. */
function failLockNote(lockPath, step) {
  const realOpen = fsp.open
  fsp.open = async (target, ...rest) => {
    const handle = await realOpen(target, ...rest)
    if (String(target) !== lockPath) return handle
    handle[step] = async () => {
      const error = new Error(`ENOSPC: no space left on device, ${step} '${lockPath}'`)
      error.code = 'ENOSPC'
      throw error
    }
    return handle
  }
  return {
    restore: () => {
      fsp.open = realOpen
    },
  }
}

test('store: an initialisation that fails after acquiring gives the lock back', async () => {
  await withHome(async (home) => {
    const lockPath = path.join(home, 'app', 'instance.lock')
    // Held so nothing is garbage-collected: a FileHandle closes itself when
    // collected, which would release the lock for the wrong reason and let
    // this test pass on a store that leaked it.
    const abandoned = []

    for (const step of ['truncate', 'write']) {
      const fault = failLockNote(lockPath, step)
      const failed = new Store(home)
      abandoned.push(failed)
      try {
        await assert.rejects(() => failed.open(), /ENOSPC/, `${step} must reach the caller`)
      } finally {
        fault.restore()
      }

      // The failed instance is never closed — it cannot be, it never became
      // open. The descriptor it took is what holds the root, so unless the
      // failure gave it back, nobody else gets in for the life of this
      // process. A successor in this very process is the strict test: the
      // kernel refuses a second open here exactly as it does across two.
      const successor = await openStore(home)
      assert.equal(
        (await readClaim(lockPath))?.pid,
        process.pid,
        `the root was free after ${step} failed`,
      )
      await successor.close()
    }
    assert.equal(abandoned.length, 2)
  })
})

test('store: a reservation resolves, and only for the launch it names', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })

    await assert.rejects(
      () => store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' }),
      /no reservation/,
    )

    const reserved = await store.reserve(ws, {
      name: 'nyx-coral-lane',
      pane: { tab: 't-1', id: 'w-7', generation: 1 },
      launch: { nonce: 'n-7f3a', opId: 'op-1' },
    })
    const { launchId } = reserved.reserved
    // The operation that reserved it is durable: a restart still knows
    // which request this launch belongs to.
    assert.equal(reserved.reserved.opId, 'op-1')
    assert.equal(reserved.reserved.resolvedAt, undefined, 'a fresh reservation is unresolved')

    // A stale resolve from an ended launch never resolves the live one.
    await assert.rejects(
      () => store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-other', outcome: 'opened' }),
      /not l-other/,
    )
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved.resolvedAt, undefined)

    const resolved = await store.resolve(ws, {
      name: 'nyx-coral-lane',
      launchId,
      outcome: 'opened',
    })
    assert.equal(resolved.reserved.outcome, 'opened')
    assert.equal(typeof resolved.reserved.resolvedAt, 'string')
    assert.equal(resolved.reserved.launchId, launchId)
    assert.equal(resolved.reserved.opId, 'op-1')

    // Resolving twice is the same answer, not a second event: a retry that
    // crossed the first answer must not restamp the record.
    const again = await store.resolve(ws, { name: 'nyx-coral-lane', launchId, outcome: 'opened' })
    assert.equal(again.reserved.resolvedAt, resolved.reserved.resolvedAt)

    // Release still takes only its own launch, resolved or not.
    await assert.rejects(
      () => store.release(ws, { name: 'nyx-coral-lane', launchId: 'l-other' }),
      /stale release/,
    )
    const released = await store.release(ws, { name: 'nyx-coral-lane', launchId })
    assert.equal(released.reserved, undefined)
    await store.close()
  })
})

test('store: a lead preference and a controller progress note land on the row', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })

    const noted = await store.leadPreferenceSet(ws, { name: 'nyx-coral-lane', value: 'manual' })
    assert.equal(noted.notifyPreference, 'manual')
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].notifyPreference, 'manual')
    // The lead writes through `recordLeadPreference` and reaches exactly one
    // field: the human's scopes are the tab and the pane, which it cannot name.
    await assert.rejects(
      () => store.leadPreferenceSet(ws, { name: 'nyx-coral-lane', value: 'sometimes' }),
      /not a delivery preference/,
    )
    await assert.rejects(
      () => store.leadPreferenceSet(ws, { name: 'nyx-nope-lane', value: 'auto' }),
      /no conversation/,
    )

    const progressed = await store.progressSet(ws, {
      name: 'nyx-coral-lane',
      progress: { state: 'running', detail: 'turn 1' },
    })
    assert.equal(progressed.progress.state, 'running')
    assert.equal(progressed.progress.detail, 'turn 1')
    assert.equal(typeof progressed.progress.at, 'string')
    // The latest note replaces the last one; progress is a state, not a log.
    const later = await store.progressSet(ws, {
      name: 'nyx-coral-lane',
      progress: { state: 'settled' },
    })
    assert.equal(later.progress.state, 'settled')
    assert.equal(later.progress.detail, undefined)
    await assert.rejects(
      () => store.progressSet(ws, { name: 'nyx-coral-lane', progress: { detail: 'no state' } }),
      /state/,
    )
    await store.close()
  })
})

test('store: delivery ids are minted app-wide and never come round again', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1', [workerPane('w-7', 0)])

    // Filename-safe by construction: a delivery id becomes
    // `<workspace>/deliveries/<id>.md`, so the namespace is `d-` and digits
    // and nothing else — no slash, no dot, no colon, nothing a shell or a
    // path would read as structure.
    const first = await store.allocateDeliveryId()
    assert.match(first, /^d-\d+$/)
    assert.equal(first, path.basename(first), 'a delivery id is one path segment')

    // The queue is what makes it atomic: fifty at once are fifty ids.
    const minted = await Promise.all(Array.from({ length: 50 }, () => store.allocateDeliveryId()))
    assert.equal(new Set(minted).size, 50, 'no id was handed out twice')
    assert.equal(minted.includes(first), false)
    for (const id of minted) assert.match(id, /^d-\d+$/)

    // The counter lives in the pane allocator's own file and is written
    // with it, so neither can lose the other's progress.
    const envelope = JSON.parse(await readFile(path.join(home, 'app', 'tabs.json'), 'utf8'))
    assert.equal(envelope.nextDelivery, 52)
    assert.equal(envelope.nextPane, 1, 'minting a delivery id burns no pane identity')
    assert.equal(envelope.tabs.length, 1)

    // A record can be written under a minted id like any other.
    await seedLegacy(store, ws, { id: first, targetSession: 'nyx-coral-lane' })
    assert.equal((await store.readDeliveries(ws))[first].targetSession, 'nyx-coral-lane')

    // Across a restart the counter carries on. An id that came round again
    // would overwrite an immutable delivery file that is already on disk.
    await store.close()
    const restarted = await openStore(home)
    const afterRestart = await restarted.allocateDeliveryId()
    assert.equal(new Set([...minted, first]).has(afterRestart), false)
    assert.equal(afterRestart, 'd-52')

    // A pane minted afterwards still gets its own counter's next id.
    const tabs = new Tabs(restarted)
    const created = await tabs.create(path.join(dir, 'other'), 'pi')
    assert.match((await tabs.get(created.id)).panes[0].id, /^p-\d+$/)
    await restarted.close()
  })
})

test('store: a corrupt delivery counter refuses, and a missing one starts at one', async () => {
  await withHome(async (home, dir) => {
    const file = path.join(home, 'app', 'tabs.json')
    const store = await openStore(home)
    await seedTabRecord(store, path.join(dir, 'ws'), 't-1')
    await store.close()

    // A store written before this allocator existed has issued no delivery
    // ids at all, so beginning at one repeats nothing.
    const envelope = JSON.parse(await readFile(file, 'utf8'))
    delete envelope.nextDelivery
    await writeFile(file, `${JSON.stringify(envelope, null, 2)}\n`)
    const upgraded = await openStore(home)
    assert.equal(await upgraded.allocateDeliveryId(), 'd-1')
    await upgraded.close()

    // A counter we cannot read is NOT a counter of zero. Starting over
    // would mint ids that name delivery files already written and
    // immutable on disk, so it refuses and keeps every byte.
    for (const broken of [0, -3, 'seven', 1.5]) {
      const corrupt = JSON.parse(await readFile(file, 'utf8'))
      corrupt.nextDelivery = broken
      const bytes = `${JSON.stringify(corrupt, null, 2)}\n`
      await writeFile(file, bytes)
      await assert.rejects(() => openStore(home), /delivery counter/, JSON.stringify(broken))
      assert.equal(await readFile(file, 'utf8'), bytes, 'the bytes are preserved')
    }
  })
})

test('store: worker admission saves native lead context for every lead harness', async () => {
  await withHome(async (home, dir) => {
    const store = await openStore(home)
    const ws = path.join(dir, 'ws')
    for (const kind of ['claude-code', 'codex', 'opencode', 'pi']) {
      const tab = `t-${kind}`
      await seedTabRecord(store, ws, tab)
      await store.mutate(ws, 'test.lead-context', async (io) => {
        const tabs = await io.readTabs()
        const lead = tabs.find((row) => row.id === tab).lead
        lead.harness = kind
        lead.nativeSession = `${kind}-native`
        await io.writeTabs(tabs)
      })
      const admitted = await store.admit(ws, {
        name: `worker-${kind}`,
        tab,
        agent: 'gefjon',
        kind: 'opencode',
        lead: `tab:${tab}:1`,
        launch: { launchId: `launch-${kind}`, nonce: `launch-${kind}` },
      })
      assert.deepEqual(admitted.row.leadContext, { kind, session: `${kind}-native` })
    }
    await store.close()
  })
})

test('store: admission decides and reserves in ONE queued operation', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await seedTabRecord(store, path.join(dir, 'ws'), 't-2')

    // A fresh conversation: admission mints the pane, creates the row and
    // takes the reservation, all in one step. Nothing outside the queue
    // ever sees the half of it.
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    assert.equal(first.outcome, 'reserved')
    assert.match(first.pane.id, /^p-\d+$/)
    assert.equal(first.pane.kind, 'worker')
    assert.equal(first.pane.conversation, 'nyx-coral-lane')
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.reserved.launchId, 'l-1')
    assert.equal(row.agent, 'nyx')
    assert.equal((await store.readTabs())[0].panes.at(-1).id, first.pane.id)

    // Unresolved: nothing may launch it again, whoever asks.
    await assert.rejects(
      () =>
        store.admit(ws, {
          name: 'nyx-coral-lane',
          tab: 't-1',
          launch: { launchId: 'l-2', opId: 'op-2', nonce: 'l-2' },
        }),
      /reserved/,
    )

    // Resolved and linked here: the answer is the live pane, not a launch.
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' })
    const live = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      launch: { launchId: 'l-3', opId: 'op-3', nonce: 'l-3' },
    })
    assert.equal(live.outcome, 'live')
    assert.equal(live.pane.id, first.pane.id)
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved.launchId, 'l-1')

    // Running under another tab: never taken, and never released from here.
    await assert.rejects(
      () =>
        store.admit(ws, {
          name: 'nyx-coral-lane',
          tab: 't-2',
          launch: { launchId: 'l-4', opId: 'op-4', nonce: 'l-4' },
        }),
      /another session/,
    )
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved.launchId, 'l-1')
    await store.close()
  })
})

test('store: concurrent admissions of one conversation produce ONE launch', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    await store.conversationCreate(ws, { name: 'nyx-coral-lane', agent: 'nyx', kind: 'codex' })

    // The interleaving that used to lose a launch: both callers read the
    // state, both decide there is no pane, both reserve. With the decision
    // INSIDE the queue there is no read to go stale.
    const attempts = await Promise.allSettled(
      Array.from({ length: 8 }, (_, at) =>
        store.admit(ws, {
          name: 'nyx-coral-lane',
          tab: 't-1',
          launch: { launchId: `l-${at}`, opId: `op-${at}`, nonce: `l-${at}` },
        }),
      ),
    )
    const reserved = attempts.filter(
      (attempt) => attempt.status === 'fulfilled' && attempt.value.outcome === 'reserved',
    )
    assert.equal(reserved.length, 1, 'exactly one caller took the launch')
    for (const attempt of attempts) {
      if (attempt === reserved[0]) continue
      assert.equal(attempt.status, 'rejected')
      assert.match(attempt.reason.message, /reserved/)
    }
    const tabs = await store.readTabs()
    assert.equal(
      tabs[0].panes.filter((pane) => pane.conversation === 'nyx-coral-lane').length,
      1,
      'and only one pane was ever minted for it',
    )
    await store.close()
  })
})

test('store: a stale reservation of this tab is released and replaced in one step', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' })

    // The pane is gone but the reservation was never released — a crash
    // between the two. Admission for the SAME tab replaces it; the row
    // keeps its history and its identity.
    await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
      keepReservation: true,
    })
    const again = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      launch: { launchId: 'l-9', opId: 'op-9', nonce: 'l-9' },
    })
    assert.equal(again.outcome, 'reserved')
    assert.notEqual(again.pane.id, first.pane.id)
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved.launchId, 'l-9')
    await store.close()
  })
})

test('store: a pane exit releases only the reservation that names that pane', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' })

    const released = await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
    })
    assert.equal(released.released, 'l-1')
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved, undefined)

    // The successor launch, in a new pane.
    const second = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      launch: { launchId: 'l-2', opId: 'op-2', nonce: 'l-2' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-2', outcome: 'opened' })

    // The FIRST pane's exit arriving late — a duplicate, or a slow event —
    // names a pane the reservation no longer holds. It takes nothing.
    const stale = await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
    })
    assert.equal(stale.released, null)
    assert.equal(stale.reason, 'stale')
    assert.equal(
      (await store.readThreads(ws))['nyx-coral-lane'].reserved.launchId,
      'l-2',
      'the successor keeps its reservation',
    )

    // The right pane at the WRONG generation is a different pane, and an
    // exit naming another tab is not this reservation's either.
    for (const wrong of [
      { tab: 't-1', pane: second.pane.id, generation: second.pane.generation + 1 },
      { tab: 't-9', pane: second.pane.id, generation: second.pane.generation },
    ]) {
      const ignored = await store.releaseExitedPane(ws, { name: 'nyx-coral-lane', ...wrong })
      assert.equal(ignored.released, null, JSON.stringify(wrong))
      assert.equal(ignored.reason, 'stale')
    }
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].reserved.launchId, 'l-2')
    await store.close()
  })
})

test('store: a controller write is refused unless it matches the CURRENT reservation', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' })
    const expect = {
      launchId: 'l-1',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
    }

    await store.sentRecord(ws, { name: 'nyx-coral-lane', entry: { kind: 'seed' }, expect })
    await store.progressSet(ws, {
      name: 'nyx-coral-lane',
      progress: { state: 'running' },
      expect,
    })
    await store.sessionBind(ws, {
      name: 'nyx-coral-lane',
      candidate: { sessionId: 'sess-1', turn: '[consensflow launch l-1]\nwork' },
      expect,
    })

    // The launch ends and its replacement takes the conversation. The old
    // controller's write was admitted before that and lands now: it names a
    // launch that is over, and the store compares INSIDE its queue.
    await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
    })
    const second = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      launch: { launchId: 'l-2', opId: 'op-2', nonce: 'l-2' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-2', outcome: 'opened' })

    for (const [op, extra] of [
      ['sentRecord', { entry: { kind: 'stale' } }],
      ['progressSet', { progress: { state: 'stale' } }],
      ['sessionBind', { candidate: { sessionId: 'sess-stale' } }],
    ]) {
      await assert.rejects(
        () => store[op](ws, { name: 'nyx-coral-lane', ...extra, expect }),
        /launch l-1 is over|not the current reservation/,
        op,
      )
    }
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.sent.length, 1, 'the ended launch appended nothing')
    assert.equal(row.progress.state, 'running')
    assert.equal(row.sessionId, 'sess-1')

    // The replacement's own controller writes fine.
    await store.sentRecord(ws, {
      name: 'nyx-coral-lane',
      entry: { kind: 'seed' },
      expect: {
        launchId: 'l-2',
        tab: 't-1',
        pane: second.pane.id,
        generation: second.pane.generation,
      },
    })
    assert.equal((await store.readThreads(ws))['nyx-coral-lane'].sent.length, 2)
    await store.close()
  })
})

test('store: the delivery counter takes only a positive safe integer', async () => {
  await withHome(async (home, dir) => {
    const file = path.join(home, 'app', 'tabs.json')
    const store = await openStore(home)
    await seedTabRecord(store, path.join(dir, 'ws'), 't-1')
    await store.close()

    // `null` is a value somebody wrote, not an absent field: reading it as
    // one would start over at d-1 and mint ids that name delivery files
    // already on disk. 2^53 and 1e21 are integers JavaScript cannot count
    // in: the first repeats an id, the second prints `d-1e+21`.
    for (const broken of [null, 2 ** 53, 1e21, Number.MAX_SAFE_INTEGER + 2, 0, -1, 'x', 1.5]) {
      const envelope = JSON.parse(await readFile(file, 'utf8'))
      envelope.nextDelivery = broken
      const bytes = `${JSON.stringify(envelope, null, 2)}\n`
      await writeFile(file, bytes)
      await assert.rejects(() => openStore(home), /delivery counter/, JSON.stringify(broken))
      assert.equal(await readFile(file, 'utf8'), bytes, `bytes preserved for ${broken}`)
    }

    // At the ceiling the allocator refuses rather than mint an id it cannot
    // count past.
    const envelope = JSON.parse(await readFile(file, 'utf8'))
    envelope.nextDelivery = Number.MAX_SAFE_INTEGER
    await writeFile(file, `${JSON.stringify(envelope, null, 2)}\n`)
    const store2 = await openStore(home)
    await assert.rejects(() => store2.allocateDeliveryId(), /delivery counter/)
    await store2.close()
  })
})

test('store: reopening on a bound session re-stamps the binding, never leaves it stale', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' })
    await store.sessionBind(ws, {
      name: 'nyx-coral-lane',
      candidate: { sessionId: 'rollout-7', turn: '[consensflow launch l-1]\nwork' },
      expect: {
        launchId: 'l-1',
        tab: 't-1',
        pane: first.pane.id,
        generation: first.pane.generation,
      },
    })
    const bound = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(bound.binding.generation, first.pane.generation)
    assert.equal(bound.binding.launchId, 'l-1')

    // The pane ends and the conversation is reopened ON that session. The
    // binding a reader sees must never name the pane that is gone: a
    // decision leaning on a stale generation is a decision for a pane
    // nothing is running in.
    await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
    })
    const again = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      launch: (row) => ({ launchId: 'l-2', opId: 'op-2', reportedId: row.sessionId }),
    })
    const reopened = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(reopened.sessionId, 'rollout-7', 'the session it resumes is the one it had')
    assert.equal(reopened.binding.launchId, 'l-2', 'the binding belongs to the launch resuming it')
    assert.equal(
      reopened.binding.generation,
      again.pane.generation,
      'and to the pane now carrying it',
    )
    assert.equal(reopened.binding.evidence, 'reported')
    assert.notEqual(again.pane.generation, undefined)

    // A launch that cannot vouch for the session the row names leaves no
    // binding behind at all, rather than one naming a pane that is gone.
    await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: again.pane.id,
      generation: again.pane.generation,
    })
    await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      launch: { launchId: 'l-3', opId: 'op-3', nonce: 'l-3' },
    })
    const unvouched = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(unvouched.binding, undefined, 'no binding rather than a stale one')
    assert.equal(unvouched.sessionId, 'rollout-7', 'the session it knows about is not forgotten')
    await store.close()
  })
})

test('store: a closed tab is refused, and is never evidence that its pane exited', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    const tabs = new Tabs(store)
    const created = await tabs.create(ws, 'codex')
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: created.id,
      agent: 'nyx',
      kind: 'codex',
      lead: created.leadId,
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    await store.resolve(ws, { name: 'nyx-coral-lane', launchId: 'l-1', outcome: 'opened' })

    // Asteria's interleaving: the caller took its snapshot while the tab
    // was open, the tab was suspended in between, and admission runs now.
    const snapshot = (await store.readTabs()).find((tab) => tab.id === created.id)
    assert.equal(snapshot.closed, false)
    await tabs.suspend(created.id)

    const panes = (await store.readTabs()).find((tab) => tab.id === created.id).panes.length
    await assert.rejects(
      () =>
        store.admit(ws, {
          name: 'nyx-coral-lane',
          tab: created.id,
          launch: { launchId: 'l-2', opId: 'op-2', nonce: 'l-2' },
        }),
      /closed/,
    )

    // A closed tab says nothing about whether its pane's process ended.
    // Only `pane.exit` releases, so the reservation stands and no second
    // pane was minted for the conversation.
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.reserved.launchId, 'l-1')
    assert.equal(row.reserved.pane, first.pane.id)
    assert.equal(
      (await store.readTabs()).find((tab) => tab.id === created.id).panes.length,
      panes,
      'and admission allocated nothing on its way to refusing',
    )
    await store.close()
  })
})

test('store: admission refuses a conversation that belongs to another agent', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    await seedTabRecord(store, ws, 't-1')
    const first = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      lead: 'tab:t-1:1',
      launch: { launchId: 'l-1', opId: 'op-1', nonce: 'l-1' },
    })
    await store.releaseExitedPane(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      pane: first.pane.id,
      generation: first.pane.generation,
    })

    // The identity is checked against the row this mutation read, so a
    // roster that changed between the caller's look and this write cannot
    // slip a different harness onto an existing conversation.
    for (const [agent, kind] of [
      ['zeus', 'codex'],
      ['nyx', 'claude-code'],
    ]) {
      await assert.rejects(
        () =>
          store.admit(ws, {
            name: 'nyx-coral-lane',
            tab: 't-1',
            agent,
            kind,
            launch: { launchId: 'l-x', opId: 'op-x', nonce: 'l-x' },
          }),
        /belongs to nyx \(codex\)/,
        `${agent} ${kind}`,
      )
    }
    const row = (await store.readThreads(ws))['nyx-coral-lane']
    assert.equal(row.reserved, undefined, 'and nothing was admitted on the way to refusing')

    const same = await store.admit(ws, {
      name: 'nyx-coral-lane',
      tab: 't-1',
      agent: 'nyx',
      kind: 'codex',
      launch: { launchId: 'l-2', opId: 'op-2', nonce: 'l-2' },
    })
    assert.equal(same.outcome, 'reserved')
    await store.close()
  })
})

test('the mutation queue announces every mutation it completes, once', async () => {
  await withHome(async (home, dir) => {
    const store = await openStore(home)
    const ws = path.join(dir, 'workspace')
    const seen = []
    const stop = store.onMutation((event) => seen.push(event))

    const tabs = new Tabs(store)
    const created = await tabs.create(ws, 'claude-code')

    // `tab.create` writes the envelope AND the tab list; the page is told
    // once, about the mutation, not once per file the mutation touched.
    assert.deepEqual(
      seen.map((event) => event.op),
      ['tab.create'],
    )
    assert.equal(seen[0].cwd, path.resolve(ws))

    // The listener runs after the write: what it reads is what the
    // mutation left, never the state before it.
    let observed = null
    const stopReader = store.onMutation(() => {
      observed = store.readTabs()
    })
    await tabs.addPane(created.id, { kind: 'worker' })
    assert.equal((await observed).find((tab) => tab.id === created.id).panes.length, 2)
    stopReader()

    // A mutation that failed changed nothing and says nothing.
    const before = seen.length
    await assert.rejects(() =>
      store.mutate(ws, 'test.fails', async () => {
        throw new Error('no')
      }),
    )
    assert.equal(seen.length, before, 'a failed mutation announces nothing')

    stop()
    await tabs.addPane(created.id, { kind: 'worker' })
    assert.equal(seen.length, before, 'and an unsubscribed listener hears nothing more')
    await store.close()
  })
})

test('a lead binds its native session only on the launch evidence it was given', async () => {
  await withHome(async (home, dir) => {
    const store = await openStore(home)
    const ws = path.join(dir, 'workspace')
    const tabs = new Tabs(store)
    const tab = await tabs.create(ws, 'claude-code')

    const admitted = await store.leadAdmit(ws, {
      tab: tab.id,
      launch: () => ({ launchId: 'l-lead-1', preallocatedId: 'ses-abc' }),
    })
    assert.equal(admitted.launchId, 'l-lead-1')
    assert.equal(admitted.nativeSession, null, 'nothing is bound by reserving')

    // The whole point of `bindEvidence`: a session id on its own is not
    // proof. Anyone who can reach this call could send one, and the id of a
    // conversation belonging to someone else is exactly what it would be.
    await assert.rejects(
      () => store.leadBind(ws, { tab: tab.id, candidate: { sessionId: 'someone-elses' } }),
      /leadBind refuses/,
    )
    assert.equal((await store.readTabs()).find((t) => t.id === tab.id).lead.nativeSession, null)

    const bound = await store.leadBind(ws, {
      tab: tab.id,
      candidate: { sessionId: 'ses-abc' },
      expect: { launchId: 'l-lead-1' },
    })
    assert.equal(bound.evidence, 'preallocated')
    const record = (await store.readTabs()).find((t) => t.id === tab.id)
    assert.equal(record.lead.nativeSession, 'ses-abc')
    assert.equal(record.lead.binding.launchId, 'l-lead-1')

    // A write naming a launch that is no longer the tab's own writes
    // nothing — the fence a worker's controller writes live behind.
    await assert.rejects(
      () =>
        store.leadBind(ws, {
          tab: tab.id,
          candidate: { sessionId: 'ses-abc' },
          expect: { launchId: 'l-lead-0' },
        }),
      /is over for the lead/,
    )

    // And a lead already bound offers no new identity on its next launch:
    // it is resumed on the one it has.
    await store.leadRelease(ws, { tab: tab.id, launchId: 'l-lead-1' })
    const again = await store.leadAdmit(ws, {
      tab: tab.id,
      launch: (row) => ({
        launchId: 'l-lead-2',
        ...(row.lead.nativeSession === null ? { preallocatedId: 'ses-new' } : {}),
      }),
    })
    assert.equal(again.nativeSession, 'ses-abc')
    assert.deepEqual(again.evidence, {})
    await store.close()
  })
})

test('a corrupted store file is a fault, not a refusal of the request', async () => {
  await withHome(async (home, dir) => {
    const store = await openStore(home)
    const ws = path.join(dir, 'workspace')
    const tabs = new Tabs(store)
    await tabs.create(ws, 'claude-code')

    // The store says no to a REQUEST with a typed refusal carrying a code.
    // Everything else it throws — a corrupt file, a bug, a full disk — is a
    // fault on this side, and nothing downstream may dress one up as a bad
    // request just because it happens to be a plain Error.
    const file = path.join(home, 'workspaces', workspaceKey(ws), 'threads.json')
    await mkdir(path.dirname(file), { recursive: true })
    await writeFile(file, 'not json at all')
    const corrupt = await store.sentRecord(ws, { name: 'anyone', entry: { kind: 'say' } }).then(
      () => null,
      (cause) => cause,
    )
    assert.notEqual(corrupt, null, 'a corrupt threads.json must not read as success')
    assert.equal(corrupt.code, undefined, 'and it carries no refusal code')
    assert.equal(corrupt.name, 'Error')
    assert.match(corrupt.message, /threads\.json/)
    // The instance lock is a real file handle. Leaving it to the collector
    // fails the whole FILE after every test in it has already passed, and
    // the report names no test at all.
    await store.close()
  })
})

test('a lead pane ending releases and suspends as ONE decision, or neither', async () => {
  await withHome(async (home, dir) => {
    const store = await openStore(home)
    const ws = path.join(dir, 'workspace')
    const tabs = new Tabs(store)
    const tab = await tabs.create(ws, 'claude-code')
    const pane = (await store.readTabs()).find((t) => t.id === tab.id).panes[0]
    await store.leadAdmit(ws, { tab: tab.id, launch: () => ({ launchId: 'l-1' }) })
    await store.leadResolve(ws, { tab: tab.id, launchId: 'l-1' })

    // The exit that matches: the launch is over and the tab has no window.
    const ended = await store.leadEnded(ws, {
      tab: tab.id,
      pane: pane.id,
      generation: pane.generation,
      launchId: 'l-1',
    })
    assert.equal(ended.ended, true)
    let record = (await store.readTabs()).find((t) => t.id === tab.id)
    assert.equal(record.closed, true)
    assert.equal(record.lead.reserved, undefined)

    // Now the tab is back, on a new generation with a launch of its own.
    await tabs.resume(tab.id)
    await store.leadAdmit(ws, { tab: tab.id, launch: () => ({ launchId: 'l-2' }) })

    // The late delivery of the FIRST exit. Releasing was already a no-op —
    // that reservation is gone — and suspending on the strength of it would
    // close a window the human just asked for. Both halves are one
    // decision, so a stale exit changes nothing at all.
    for (const stale of [
      { pane: pane.id, generation: pane.generation, launchId: 'l-1' },
      { pane: pane.id, generation: 2, launchId: 'l-1' },
      { pane: 'p-nope', generation: 2, launchId: 'l-2' },
    ]) {
      const answer = await store.leadEnded(ws, { tab: tab.id, ...stale })
      assert.equal(answer.ended, false, JSON.stringify(stale))
      record = (await store.readTabs()).find((t) => t.id === tab.id)
      assert.equal(record.closed, false, `closed by ${JSON.stringify(stale)}`)
      assert.equal(record.lead.reserved.launchId, 'l-2', 'and it kept its reservation')
    }
    await store.close()
  })
})

test('legacy sessions receive a persistent role name without losing their native identity', async () => {
  await withHome(async (home) => {
    let store = await openStore(home)
    const tabs = new Tabs(store)
    const created = await tabs.create('/tmp/project', 'codex')
    await store.mutate('/tmp/project', 'legacy.fixture', async (io) => {
      const rows = await io.readTabs()
      delete rows[0].roleName
      delete rows[0].role
      rows[0].lead.nativeSession = 'retained-native-session'
      await io.writeTabs(rows)
    })
    await store.close()
    store = await openStore(home)
    const first = (await store.readTabs())[0]
    assert.equal(first.role, 'lead')
    assert.match(first.roleName, /^[a-z]+-[a-z]+$/)
    assert.equal(first.id, created.id)
    assert.equal(first.lead.nativeSession, 'retained-native-session')
    await store.close()
    store = await openStore(home)
    assert.equal((await store.readTabs())[0].roleName, first.roleName)
    await store.close()
  })
})

test('restart restores legacy worker navigation only for the original surviving tab', async () => {
  await withHome(async (home) => {
    let store = await openStore(home)
    let tabs = new Tabs(store)
    const a = await tabs.create('/tmp/legacy-project', 'codex')
    const b = await tabs.create('/tmp/legacy-project', 'codex')
    const other = await tabs.create('/tmp/other-project', 'codex')
    const original = {
      'a-review': {
        agent: 'zeus',
        kind: 'claude-code',
        lead: `tab:${a.id}:1`,
        sessionId: 'native-a',
      },
      'b-review': { agent: 'diana', kind: 'codex', lead: `tab:${b.id}:1`, sessionId: 'native-b' },
      'deleted-session': {
        agent: 'zeus',
        kind: 'claude-code',
        lead: 'tab:t-999:1',
        sessionId: 'native-deleted',
      },
      'future-generation': {
        agent: 'zeus',
        kind: 'claude-code',
        lead: `tab:${a.id}:9`,
        sessionId: 'native-future',
      },
      'invalid-owner': {
        agent: 'zeus',
        kind: 'claude-code',
        lead: `tab:${a.id}:1extra`,
        sessionId: 'native-invalid',
      },
    }
    await store.mutate('/tmp/legacy-project', 'legacy.workers', async (io) =>
      io.writeThreads(original),
    )
    await store.close()
    store = await openStore(home)
    tabs = new Tabs(store)
    const restoredA = (await tabs.get(a.id)).panes.filter((p) => p.kind === 'worker')
    const restoredB = (await tabs.get(b.id)).panes.filter((p) => p.kind === 'worker')
    assert.deepEqual(
      restoredA.map((p) => p.conversation),
      ['a-review'],
    )
    assert.deepEqual(
      restoredB.map((p) => p.conversation),
      ['b-review'],
    )
    assert.notEqual(restoredA[0].id, restoredB[0].id)
    assert.equal(restoredA[0].closed, true)
    assert.equal(restoredA[0].alive, false)
    assert.equal((await tabs.get(other.id)).panes.length, 1)
    assert.deepEqual(
      { ...(await store.readThreads('/tmp/legacy-project')) },
      original,
      'native bindings and ownership stay unchanged',
    )
    await tabs.resume(a.id)
    const { Page } = await import('../src/page.js')
    const page = new Page({
      store,
      tabs,
      agents: { row: () => undefined, names: () => [] },
      env: { HOME: home, CONSENSFLOW_HOME: home },
    })
    const drawn = (await page.state()).tabs
      .find((t) => t.id === a.id)
      .panes.find((p) => p.kind === 'worker')
    assert.equal(drawn.alive, false, 'a recovered history row is not a running worker')
    assert.notEqual(drawn.starting, true)
    await store.close()
    store = await openStore(home)
    assert.deepEqual(
      (await store.readTabs()).find((t) => t.id === a.id).panes.filter((p) => p.kind === 'worker'),
      restoredA,
    )
    await store.close()
  })
})

test('permanent pane deletion fences admission and survives legacy recovery', async () => {
  await withHome(async (home) => {
    let store = await openStore(home)
    const tabs = new Tabs(store)
    const created = await tabs.create('/tmp/delete-pane-project', 'codex')
    const pane = await tabs.addPane(created.id, { kind: 'worker', conversation: 'old-review' })
    await store.mutate('/tmp/delete-pane-project', 'fixture', (io) =>
      io.writeThreads({
        'old-review': {
          agent: 'zeus',
          kind: 'codex',
          sessionId: 'native-kept',
          lead: `tab:${created.id}:1`,
        },
      }),
    )
    await assert.rejects(() => tabs.beginPaneDelete(created.id, pane.id, pane.generation + 1))
    await tabs.beginPaneDelete(created.id, pane.id, pane.generation)
    await assert.rejects(
      () => store.admit('/tmp/delete-pane-project', { name: 'old-review', tab: created.id }),
      /deleted from this session/,
    )
    await tabs.removePane(created.id, pane.id, pane.generation)
    await store.close()
    store = await openStore(home)
    assert.equal(
      (await store.readTabs())[0].panes.length,
      1,
      'deleted worker stays absent after restart',
    )
    assert.equal(
      (await store.readThreads('/tmp/delete-pane-project'))['old-review'].sessionId,
      'native-kept',
    )
    await store.close()
  })
})

test('store: closed advisors cannot be adopted by the lead and PM cannot adopt workers', async () => {
  await withHome(async (home, dir) => {
    const ws = path.join(dir, 'ws')
    const store = await openStore(home)
    try {
      await seedTabRecord(store, ws, 't-1')
      await seedTabRecord(store, ws, 't-pm')
      await store.mutate(ws, 'test.pm', async (io) => {
        const tabs = await io.readTabs()
        tabs.find((t) => t.id === 't-pm').role = 'pm'
        await io.writeTabs(tabs)
      })
      for (const [owner, other, role] of [
        ['t-1', 't-pm', 'worker'],
        ['t-pm', 't-1', 'advisor'],
      ]) {
        const name = `same-agent-${role}`
        const admitted = await store.admit(ws, {
          name,
          tab: owner,
          agent: 'zeus',
          kind: 'codex',
          lead: `tab:${owner}:1`,
          launch: { launchId: role, opId: role, nonce: role },
        })
        assert.equal(admitted.row.role, role)
        await store.resolve(ws, { name, launchId: role, outcome: 'opened' })
        await store.releaseExitedPane(ws, {
          name,
          tab: owner,
          pane: admitted.pane.id,
          generation: admitted.pane.generation,
        })
        await assert.rejects(store.admit(ws, { name, tab: other }), /another group/)
      }
    } finally {
      await store.close()
    }
  })
})

async function seedLegacy(store, cwd, record) {
  return store.mutate(cwd, 'test.legacy', async (io) => {
    const records = await io.readDeliveries()
    const merged = { ...records[record.id], ...record }
    records[record.id] = merged
    await io.writeDeliveries(records)
    return merged
  })
}
