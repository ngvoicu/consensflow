// ConsensFlow's Pi extension driven through a published Pi's own loop, offline: Pi's SDK session
// (`createAgentSession`), its loader (ConsensFlow's extension from the command line's paths, a
// person's extension from the settings), its retry, compaction and session file, with Pi's own
// stand-in provider (`fauxProvider`) for the model. No login, no model endpoint, no Pi home: HOME
// and Pi's folder are the run's own (the brain's `architecture/pi-turn-state.md`, §8).
//
//   sh tests/pi-loop/get-pi.sh 1.6.1 <fresh folder>
//   HOME=<another fresh folder> node tests/pi-loop/drive.mjs <that Pi's folder> <fresh output folder> [case …]
//   (both folders outside the account's home: under TMPDIR)
//
// Each case asserts what the word (`settled/<launch>.state.json`) says at each of Pi's events and
// over time, and what the inbox consumer does. `--record <folder>` also keeps, for the cases that
// name moments, the session file and the word at each, for the reader's fixtures
// (`crates/cf-harness/tests/fixtures/pi-turn-state`).
import assert from 'node:assert/strict'
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { userInfo } from 'node:os'
import { join, resolve, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const args = process.argv.slice(2)
const recordAt = args.includes('--record') ? resolve(args[args.indexOf('--record') + 1]) : null
const plain = args.filter((arg, at) => arg !== '--record' && args[at - 1] !== '--record')
const [piRoot, outRoot, ...only] = plain.map((arg, at) => (at < 2 ? resolve(arg) : arg))
if (!piRoot || !outRoot) throw new Error('usage: drive.mjs <pi folder> <fresh output folder> [case …]')
// Pi reads its own folder under HOME (~/.pi) for what a session is not given, and walks up from
// a session's folder for a project's resources: never the person's. HOME is the run's, and the
// run's folders lie outside the account's home (TMPDIR's), with Pi's discovery of skills,
// context files, prompts and themes turned off besides.
const account = resolve(userInfo().homedir)
const inside = (path) => resolve(path) === account || resolve(path).startsWith(`${account}/`)
if (!process.env.HOME || resolve(process.env.HOME) === account) {
  throw new Error("run with HOME set to a folder of the run, not the account's own")
}
if (inside(outRoot) || inside(process.env.HOME)) {
  throw new Error("run's folders must lie outside the account's home (under TMPDIR, say)")
}
mkdirSync(outRoot)

const packageDir = join(piRoot, 'node_modules/@earendil-works/pi-coding-agent')
const nested = (name) => {
  const flat = join(piRoot, 'node_modules/@earendil-works', name)
  return existsSync(flat) ? flat : join(packageDir, 'node_modules/@earendil-works', name)
}
const pc = await import(pathToFileURL(join(packageDir, 'dist/index.js')).href)
const ai = await import(pathToFileURL(join(nested('pi-ai'), 'dist/index.js')).href)
const VERSION = pc.VERSION
const [major, minor] = VERSION.split('.').map(Number)
// Pi marks a run from the first line of `_startRun` from 1.2.0; before it, at its first request.
const marksEarly = major > 1 || (major === 1 && minor >= 2)
// Pi's `agent_settled` carries its abort flag from 1.1.0; before it, the word's flag is null.
const flags = major > 1 || (major === 1 && minor >= 1)
const flag = (value) => (flags ? value : null)

const wait = (ms) => new Promise((done) => setTimeout(done, ms))
const gate = () => {
  let open
  const shut = new Promise((done) => {
    open = done
  })
  return { shut, open }
}

const POLL_MS = 40
/** How long one case may take before it is given up as failed. */
const CASE_LIMIT_MS = 60_000
const RUN_EVENTS = new Set(['before_agent_start', 'agent_start', 'turn_start', 'agent_end', 'agent_before_settle'])

/** A person's extension's own entries, inside the open turn and after its end: the second form. */
const EXTRAS = {
  turn_start: (_e, _ctx, pi) => pi.appendEntry('keeper', { count: 1 }),
  agent_settled: (_e, _ctx, pi) => {
    pi.appendEntry('keeper', { count: 2 })
    pi.sendMessage({ customType: 'keeper', content: 'Kept the cache warm.', display: true })
  },
}
const LAUNCH = 'launch-loop'

/**
 * A Pi session with ConsensFlow's extension and a person's: its folder, the word's file, what the
 * person's extension saw at each hook, and the word sampled every few milliseconds.
 */
// Each case runs in two forms: plain, and with a person's extension's own entries.
let form = 'plain'
async function open(base, { person = {}, ours = {}, retry = true, tokensPerSecond, resume, manager, start } = {}) {
  const name = `${base}-${form}`
  const extras = form === 'extras'
  const dir = join(outRoot, name)
  const sessions = join(dir, 'sessions')
  const settled = ours.settled ?? join(dir, 'settled')
  const inbox = join(dir, 'inbox')
  const ack = join(dir, 'ack')
  for (const folder of [dir, sessions, inbox, ack]) mkdirSync(folder, { recursive: true })
  const loop = {
    wordFile: join(settled, `${LAUNCH}.state.json`),
    seen: [],
    person,
    extras: extras ? EXTRAS : undefined,
    ours: {
      inbox,
      ack,
      quarantine: join(dir, 'quarantine'),
      settled,
      expired: join(dir, 'expired'),
      launchId: LAUNCH,
      pollMs: POLL_MS,
      logger: { error: (...said) => loop.logs.push(said.join(' ')) },
      ...ours,
    },
    logs: [],
  }
  globalThis.__piLoop = loop
  const faux = ai.fauxProvider({
    provider: 'stand-in',
    models: [{ id: 'stand-in-model', contextWindow: 200_000 }],
    ...(tokensPerSecond ? { tokensPerSecond, tokenSize: { min: 1, max: 1 } } : {}),
  })
  const settingsManager = pc.SettingsManager.inMemory({
    retry: { enabled: retry, maxRetries: 2, baseDelayMs: 30, maxAgentDelayMs: 60_000 },
    compaction: { enabled: false, keepRecentTokens: 1 },
    extensions: [join(here, 'person.mjs')],
  })
  const loader = new pc.DefaultResourceLoader({
    cwd: dir,
    agentDir: join(dir, 'agent'),
    settingsManager,
    additionalExtensionPaths: [join(here, 'ours.mjs')],
    noSkills: true,
    noContextFiles: true,
    noPromptTemplates: true,
    noThemes: true,
  })
  await loader.reload()
  const { session } = await pc.createAgentSession({
    cwd: dir,
    agentDir: join(dir, 'agent'),
    model: faux.getModel(),
    settingsManager,
    resourceLoader: loader,
    ...(start ? { sessionStartEvent: start } : {}),
    sessionManager: manager ?? (resume ? pc.SessionManager.open(resume, sessions) : pc.SessionManager.create(dir, sessions)),
  })
  // The TUI's mode, with a screen that draws nothing: ConsensFlow's consumer sends only there.
  const ui = new Proxy({}, { get: (_target, key) => (key === 'then' ? undefined : () => undefined) })
  await session.bindExtensions({ mode: 'tui', uiContext: ui })
  session.agent.streamFunction = faux.provider.streamSimple
  // A login is Pi's own business: the stand-in needs none.
  session._modelRuntime.hasConfiguredAuth = () => true
  const word = () => {
    try {
      return JSON.parse(readFileSync(loop.wordFile, 'utf8'))
    } catch {
      return null
    }
  }
  const states = []
  const sampler = setInterval(() => {
    const now = word()?.state ?? null
    if (states.at(-1) !== now) states.push(now)
  }, 5)
  const file = () => {
    const names = readdirSync(sessions).filter((n) => n.endsWith('.jsonl'))
    return names.length === 1 ? join(sessions, names[0]) : null
  }
  const rows = () => {
    const at = file()
    return at === null ? [] : readFileSync(at, 'utf8').trim().split('\n').map((row) => JSON.parse(row))
  }
  const until = async (what, limit = 5_000) => {
    const deadline = Date.now() + limit
    while (!what()) {
      if (Date.now() > deadline) throw new Error(`${name}: timed out; word ${JSON.stringify(word())}`)
      await wait(5)
    }
  }
  const moments = []
  const moment = (label, expect) => {
    moments.push({ label, word: word(), rows: rows(), expect })
  }
  const close = async () => {
    clearInterval(sampler)
    // Whatever the case: at every hook of a run in the person's extension, the word says `working`.
    for (const seen of loop.seen) {
      if (RUN_EVENTS.has(seen.event)) assert.equal(seen.state, 'working', `${name}: ${seen.event} read ${seen.state}`)
    }
    if (recordAt !== null && moments.length > 0) {
      for (const [at, kept] of moments.entries()) {
        const folder = join(recordAt, name, `${String(at + 1).padStart(2, '0')}-${kept.label}`)
        mkdirSync(folder, { recursive: true })
        // Kept with no path of this machine's: Pi's folder and the run's are named, not spelled.
        const scrub = (text) => text.replaceAll(packageDir, '<pi>').replaceAll(piRoot, '<pi>').replaceAll(outRoot, '<run>')
        if (kept.rows.length > 0) {
          // Pi's system message (its prompt and tools, kilobytes each) kept in shape only: the
          // reader reads it as an entry a request can follow, and nothing of what it says.
          const short = (row) =>
            row.message?.role === 'system'
              ? { ...row, message: { ...row.message, sections: {}, toolsAdded: (row.message.toolsAdded ?? []).map((t) => ({ name: t.name })) } }
              : row
          const text = scrub(kept.rows.map((r) => `${JSON.stringify(short(r))}\n`).join(''))
          if (text.includes(userInfo().username)) throw new Error(`${name}: a path of this machine's is left in the record`)
          writeFileSync(join(folder, 'hazy-ridge.jsonl'), text)
        }
        if (kept.word !== null) writeFileSync(join(folder, `${LAUNCH}.state.json`), `${JSON.stringify(kept.word)}\n`)
        writeFileSync(join(folder, 'expect.json'), `${JSON.stringify(kept.expect ?? {})}\n`)
      }
    }
    await session.dispose?.()
  }
  return { session, faux, loop, word, states, rows, file, until, moment, close, inbox, ack }
}

/** A message from ConsensFlow, as the channel writes one into the inbox. */
function envelope(at, session, text, number = 1) {
  const id = `m-${String(number).padStart(32, '0')}`
  writeFileSync(
    join(at.inbox, `${id}.json`),
    `${JSON.stringify({ id, type: 'message', launchId: LAUNCH, session, text, expiresAt: Date.now() + 30_000 })}\n`,
  )
  return id
}
const acked = (at, id) => {
  try {
    return JSON.parse(readFileSync(join(at.ack, `${id}.json`), 'utf8'))
  } catch {
    return null
  }
}

const settles = async (at) => at.until(() => at.word()?.state === 'rest' && at.session.isIdle)

const CASES = {
  /** A run that ends in its answer: `working` from its first hook, the end written at
   * `agent_settled` with the state as it was, `rest` by the next poll. */
  async stop() {
    const at = await open('stop')
    await settles(at)
    at.moment('new-session', { at: 'rest', items: 0 })
    at.faux.setResponses([ai.fauxAssistantMessage('Done.')])
    await at.session.prompt('Run the long script')
    const begin = at.loop.seen.find((s) => s.event === 'before_agent_start')
    assert.equal(begin.state, 'working', 'working before a later extension runs')
    const settledSeen = at.loop.seen.find((s) => s.event === 'agent_settled')
    assert.equal(settledSeen.state, 'working', 'the state stays as it was inside agent_settled')
    assert.equal(settledSeen.ended, 1)
    assert.equal(at.word().ends.at(-1).aborted, flag(false))
    await settles(at)
    await wait(4 * POLL_MS)
    at.moment('answered', { at: 'rest', ended: 'stop' })
    assert.deepEqual(at.states.filter((s) => s !== null).slice(-2), ['working', 'rest'])
    assert.ok(!at.states.includes('busy') || at.states.indexOf('busy') < at.states.indexOf('working'))
    await at.close()
  },

  /** An Escape while the model writes its final text, and while it writes a tool call's
   * arguments: the end at once, aborted, the call never run. The field pauses. */
  async escape() {
    for (const point of ['text', 'call']) {
      const at = await open(`escape-${point}`, { tokensPerSecond: 40 })
      await settles(at)
      const long = 'I will start by reading the files one at a time and then '.repeat(20)
      at.faux.setResponses([
        point === 'text'
          ? ai.fauxAssistantMessage(long)
          : ai.fauxAssistantMessage([
              ai.fauxText('Running the script now.'),
              ai.fauxToolCall('bash', { command: `python3 - <<'PY'\n${'x = 1\n'.repeat(200)}` }, { id: 'c1' }),
            ]),
      ])
      const run = at.session.prompt('Run the long script')
      await at.until(() => at.word()?.state === 'working')
      await wait(150)
      await at.session.abort()
      await run
      assert.equal(at.word().ends.at(-1).aborted, flag(true))
      await settles(at)
      at.moment('interrupted', { at: 'rest', ended: 'aborted' })
      await at.close()
    }
  },

  /** A provider's error Pi retries: `working` through the failed step, its omission, the backoff
   * and the second request held long; a final error is the run's end. Cancelling the backoff
   * (Escape in the retry's wait) ends it with Pi's flag false: the omission names the step. */
  async retry() {
    const at = await open('retry')
    await settles(at)
    const held = gate()
    at.faux.setResponses([
      ai.fauxAssistantMessage([ai.fauxText('Running the script now.'), ai.fauxToolCall('bash', { command: 'ls' }, { id: 'c1' })], {
        stopReason: 'error',
        errorMessage: '503 Service Unavailable: overloaded',
      }),
      async () => {
        await held.shut
        return ai.fauxAssistantMessage('Retry finished.')
      },
    ])
    const run = at.session.prompt('Run the long script')
    await at.until(() => at.faux.state.callCount === 2)
    await wait(10 * POLL_MS)
    assert.equal(at.word().state, 'working', 'a retry at work')
    at.moment('second-request', { at: 'work' })
    held.open()
    await run
    await settles(at)
    at.moment('retried', { at: 'rest', ended: 'stop', words: 'Retry finished.' })
    assert.equal(at.word().ended, 1, 'one run')
    await at.close()

    const cancel = await open('cancel-backoff', {})
    await settles(cancel)
    cancel.faux.setResponses([
      ai.fauxAssistantMessage('Partial text', { stopReason: 'error', errorMessage: '503 Service Unavailable: overloaded' }),
      ai.fauxAssistantMessage('never'),
    ])
    cancel.session.settingsManager.getRetrySettings = () => ({ enabled: true, maxRetries: 2, baseDelayMs: 2_000, maxAgentDelayMs: 60_000 })
    const cancelled = cancel.session.prompt('Run the long script')
    await cancel.until(() => cancel.rows().some((row) => row.type === 'context_edit') || cancel.session.isRetrying)
    await wait(3 * POLL_MS)
    assert.equal(cancel.word().state, 'working', 'working in the backoff')
    cancel.session.abortRetry()
    await cancelled
    await settles(cancel)
    cancel.moment('cancelled', { at: 'rest', ended: 'aborted' })
    assert.equal(cancel.word().ends.at(-1).aborted, flag(false), "Pi's flag is false for a cancelled backoff")
    await cancel.close()

    const final = await open('final-error', { retry: false })
    await settles(final)
    final.faux.setResponses([ai.fauxAssistantMessage('Partial text', { stopReason: 'error', errorMessage: '400 Invalid request.' })])
    await final.session.prompt('Run the long script')
    await settles(final)
    final.moment('failed', { at: 'rest', ended: 'failed', failure: '400 Invalid request.' })
    await final.close()
  },

  /** D1: a prompt someone submitted waits in a person's `input` handler: `busy`, never `rest`, and
   * no message of ConsensFlow's goes in ahead of it; let go after the hold if no run begins. */
  async held_prompt() {
    const hold = gate()
    const at = await open('held-prompt', {
      person: { input: async () => (await hold.shut, undefined) },
      ours: { inHandMs: 400 },
    })
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('Answered the human.'), ai.fauxAssistantMessage('Answered ConsensFlow.')])
    const human = at.session.prompt('The human typed this')
    await at.until(() => at.word()?.state === 'busy')
    const sessionId = at.session.sessionManager.getSessionId()
    const id = envelope(at, sessionId, 'ConsensFlow words')
    await at.until(() => acked(at, id) !== null)
    assert.equal(acked(at, id).admitted, false, 'refused before send while a prompt is in hand')
    assert.equal(acked(at, id).reason, 'chief busy')
    assert.ok(!at.states.slice(at.states.indexOf('busy')).includes('rest'), 'never rest while it is held')
    hold.open()
    await human
    await settles(at)
    const users = at.rows().filter((r) => r.message?.role === 'user').map((r) => r.message.content[0].text)
    assert.deepEqual(users, ['The human typed this'])

    // A prompt another extension handled: no run begins; let go after the hold.
    at.loop.person.input = () => ({ action: 'handled' })
    await at.session.prompt('handled elsewhere')
    assert.equal(at.word().state, 'busy')
    await settles(at)
    await at.close()
  },

  /** D2: a compaction at rest by a person's extension, its `session_compact` handler held: `busy`,
   * never `working`; Pi idle and the handler done, `rest`. */
  async compaction_at_rest() {
    const held = gate()
    const at = await open('compaction-at-rest', {
      person: { session_compact: async () => (await held.shut, undefined) },
    })
    await settles(at)
    at.faux.setResponses([
      ai.fauxAssistantMessage('First part done. '.repeat(50)),
      ai.fauxAssistantMessage('Second part done. '.repeat(50)),
      ...Array.from({ length: 4 }, () => ai.fauxAssistantMessage('The summary.')),
    ])
    await at.session.prompt('Run the long script')
    await at.session.prompt('And the second part')
    await settles(at)
    const states = at.states.length
    const compaction = at.session.compact().catch((cause) => cause)
    await at.until(() => at.word()?.state === 'busy', 2_000).catch(async (cause) => {
      console.error('compaction:', await compaction, JSON.stringify(at.loop.seen.slice(-4)))
      throw cause
    })
    await wait(5 * POLL_MS)
    assert.ok(!at.states.slice(states).includes('working'), 'a compaction is no run')
    at.moment('compaction-held', { at: 'neither' })
    held.open()
    await compaction
    await settles(at)
    at.moment('compacted', { at: 'rest', ended: 'stop' })
    assert.ok(!at.states.slice(states).includes('working'))
    await at.close()
  },

  /** D3: a run's end, then a run a person's extension triggers from `agent_settled`, deferred by
   * Pi, with no poll between: the first end is in the word's ends while it says `working`. */
  async triggered_after_settle() {
    let once = false
    const at = await open('triggered-after-settle', {
      person: {
        agent_settled: async (_e, _ctx, pi) => {
          if (once) return
          once = true
          pi.sendMessage({ customType: 'keeper', content: 'Continue automatically', display: false }, { triggerTurn: true })
        },
      },
      ours: { pollMs: 60_000 },
    })
    await settles(at).catch(() => {})
    at.faux.setResponses([ai.fauxAssistantMessage('First answer done.'), ai.fauxAssistantMessage('Automatically continued.')])
    await at.session.prompt('Run the long script')
    await at.until(() => at.word()?.ended === 2)
    const word = at.word()
    assert.deepEqual(word.ends.map((e) => e.n), [1, 2])
    assert.notEqual(word.ends[0].leaf, word.ends[1].leaf)
    // The second run's first hook in the person's extension (before 1.2.0 a triggered run has no
    // `before_agent_start`: it begins at `agent_start`).
    const settledAt = at.loop.seen.findIndex((s) => s.event === 'agent_settled')
    const second = at.loop.seen
      .slice(settledAt + 1)
      .find((s) => ['before_agent_start', 'agent_start'].includes(s.event))
    assert.ok(second, `a second run: ${JSON.stringify(at.loop.seen.map((s) => s.event))}`)
    assert.equal(second.state, 'working', 'the triggered run is working from our first hook')
    assert.equal(second.ended, 1)
    at.moment('two-ends', { at: 'work', ends: [1, 2] })
    await at.close()
  },

  /** (i): on a Pi that marks a run only at its first request, a slow `before_agent_start` of a
   * person's extension reads `working` for three timer passes and 3 s; on 1.2.0 and later, as
   * long as it takes. */
  async slow_start() {
    const at = await open('slow-start', {
      person: { before_agent_start: async () => (await wait(2 * POLL_MS + POLL_MS / 2), undefined) },
    })
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('Done.')])
    await at.session.prompt('Run the long script')
    await settles(at)
    assert.equal(at.word().ended, 1, `one run on ${VERSION}: ${JSON.stringify(at.word().ends)}`)
    const slower = await open('slower-start', {
      person: { before_agent_start: async () => (await wait(6 * POLL_MS), undefined) },
    })
    await settles(slower)
    slower.faux.setResponses([ai.fauxAssistantMessage('Done.')])
    await slower.session.prompt('Run the long script')
    await settles(slower)
    // A hold under 3 s is no dropped run on any Pi (R4): one end, Pi's own (its flag from
    // 1.1.0). A hold past it: `start_held_past_the_allowance`, `board_start_held`.
    assert.equal(slower.word().ended, 1, `${VERSION}: ${JSON.stringify(slower.word().ends)}`)
    if (VERSION !== '0.87.0') assert.equal(slower.word().ends[0].aborted, false)
    await at.close()
    await slower.close()
  },


  /** Finding 7: a provider's own `aborted` (the local signal clear) with a queued follow-up, and
   * with an omission-only continuation from a person's `agent_before_settle`: one run, `working`
   * through the second request. */
  async continuation() {
    for (const how of ['follow-up', 'omission']) {
      let asked = false
      const at = await open(`continuation-${how}`, {
        person:
          how === 'omission'
            ? {
                agent_before_settle: (_e, ctx) => {
                  if (asked) return undefined
                  asked = true
                  return {
                    entries: [{ type: 'context_edit', targetId: ctx.sessionManager.getLeafId(), replacement: null }],
                    continue: true,
                  }
                },
              }
            : {},
      })
      await settles(at)
      const held = gate()
      at.faux.setResponses([
        async () => {
          if (how === 'follow-up') await at.session.prompt('Previously queued words', { streamingBehavior: 'followUp' })
          return ai.fauxAssistantMessage('The current answer', { stopReason: 'aborted', errorMessage: 'Remote request cancelled.' })
        },
        async () => {
          await held.shut
          return ai.fauxAssistantMessage('Automatically continued.')
        },
      ])
      const run = at.session.prompt('Run the long script')
      const second = await Promise.race([
        at.until(() => at.faux.state.callCount === 2).then(() => true),
        run.then(() => false),
      ])
      if (second) {
        await wait(4 * POLL_MS)
        assert.equal(at.word().state, 'working', `${how}: the same run goes on`)
        assert.equal(at.word().ended, 0)
        at.moment('second-request', { at: 'work' })
        held.open()
      }
      await run
      await settles(at)
      // However this Pi goes on (or not), every run that ended is in the word once.
      const settledCount = at.loop.seen.filter((s) => s.event === 'agent_settled').length
      assert.equal(at.word().ended, settledCount)
      at.moment('continued', { at: 'rest' })
      notes.push(`${how} (${form}): the second request came in the same run: ${second}`)
      await at.close()
    }
  },

  /** Finding 8: a retry cancelled in its backoff, then a run a person's extension triggers from
   * `agent_settled`: `working` from our first hook while the file still ends at the omission. */
  async cancelled_then_triggered() {
    let once = false
    const at = await open('cancelled-then-triggered', {
      person: {
        agent_settled: (_e, _ctx, pi) => {
          if (once) return
          once = true
          pi.sendMessage({ customType: 'keeper', content: 'Continue automatically', display: false }, { triggerTurn: true })
        },
      },
    })
    await settles(at)
    const held = gate()
    at.faux.setResponses([
      ai.fauxAssistantMessage('Partial text', { stopReason: 'error', errorMessage: '503 Service Unavailable: overloaded' }),
      async () => {
        await held.shut
        return ai.fauxAssistantMessage('Automatically continued.')
      },
    ])
    at.session.settingsManager.getRetrySettings = () => ({ enabled: true, maxRetries: 2, baseDelayMs: 2_000, maxAgentDelayMs: 60_000 })
    const run = at.session.prompt('Run the long script')
    await at.until(() => at.session.isRetrying)
    at.session.abortRetry()
    await at.until(() => at.word()?.ended === 1 && at.faux.state.callCount === 2)
    assert.equal(at.word().state, 'working')
    at.moment('triggered-after-cancel', { at: 'work', ends: [1] })
    held.open()
    await run
    await settles(at)
    await at.close()
  },

  /** Also: this extension's own compaction for the project's size, asked by the pass that writes
   * `rest`, its word written before that `rest`; `busy` while it compacts; `rest` after. */
  async own_compaction() {
    const at = await open('own-compaction', { ours: { compaction: 10 } })
    const seenWithWord = []
    const compactingFile = join(at.loop.ours.settled, `${LAUNCH}.compacting.json`)
    const sampler = setInterval(() => {
      const now = `${at.word()?.state ?? null}${existsSync(compactingFile) ? '+compacting' : ''}`
      if (seenWithWord.at(-1) !== now) seenWithWord.push(now)
    }, 2)
    await settles(at)
    const summary = gate()
    // Pi asks the stand-in for answers and, compacting, for summaries (and some versions for more,
    // such as a title): each request is answered by what it asks for.
    const summaries = { asked: 0 }
    const answer = async (context) => {
      const asked = JSON.stringify(context?.systemPrompt ?? '') + JSON.stringify(context?.messages?.at(-1) ?? '')
      if (/summar/i.test(asked)) {
        summaries.asked += 1
        if (summaries.asked > 2) await summary.shut
        return ai.fauxAssistantMessage('The summary.')
      }
      return ai.fauxAssistantMessage('A part done. '.repeat(50))
    }
    at.faux.setResponses(Array.from({ length: 20 }, () => answer))
    // The first rest asks too (its summaries are not held); the second's is held.
    await at.session.prompt('Run the long script')
    await settles(at)
    await at.until(() => !existsSync(compactingFile))
    seenWithWord.length = 0
    await at.session.prompt('And the second part')
    await at.until(() => existsSync(compactingFile))
    await at.until(() => at.word()?.state === 'busy')
    await wait(4 * POLL_MS)
    assert.equal(at.word().state, 'busy', 'our own compaction is no run')
    assert.ok(existsSync(compactingFile))
    summary.open()
    await at.until(() => at.rows().some((row) => row.type === 'compaction'), 5_000)
    await settles(at)
    await at.until(() => !existsSync(compactingFile))
    clearInterval(sampler)
    // Between the run's end and the compaction, the word said `rest` only with the compaction's
    // word already written (the unit test holds the order exactly).
    const ended = seenWithWord.findIndex((s, i) => i > 0 && !s.startsWith('working') && seenWithWord[i - 1].startsWith('working'))
    const before = seenWithWord.slice(ended, seenWithWord.indexOf('busy+compacting', ended))
    assert.ok(!before.includes('rest'), JSON.stringify(seenWithWord))
    assert.ok(!seenWithWord.slice(ended).some((s) => s.startsWith('working')), JSON.stringify(seenWithWord))
    await at.close()
  },

  /** Also: a resume onto a leaf of each kind (an answer, an interrupt, a cut turn whose user
   * message has no step): `rest` at once. */
  async resume() {
    const at = await open('resume-source', { tokensPerSecond: 40 })
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('Done.'), ai.fauxAssistantMessage('long words '.repeat(200))])
    await at.session.prompt('First')
    const run = at.session.prompt('Second')
    await at.until(() => at.faux.state.callCount === 2)
    await wait(100)
    await at.session.abort()
    await run
    await settles(at)
    const source = at.file()
    const text = readFileSync(source, 'utf8')
    await at.close()
    const leaves = {
      stop: text.split('\n').filter(Boolean).slice(0, -2).join('\n') + '\n',
      aborted: text,
    }
    const rows = text.trim().split('\n').map((row) => JSON.parse(row))
    const last = rows.at(-1)
    leaves.user =
      text +
      `${JSON.stringify({ type: 'message', id: 'cut00001', parentId: last.id, timestamp: new Date().toISOString(), message: { role: 'user', content: [{ type: 'text', text: 'A turn the quit cut' }], timestamp: Date.now() } })}\n`
    for (const [kind, body] of Object.entries(leaves)) {
      const folder = join(outRoot, `resume-${kind}-${form}-file`)
      mkdirSync(folder)
      const file = join(folder, 'hazy-ridge.jsonl')
      writeFileSync(file, body)
      const again = await open(`resume-${kind}`, { resume: file })
      await again.until(() => again.word() !== null)
      assert.equal(again.word().state, 'rest', `${kind}: rest at the resume`)
      assert.equal(again.word().ended, 0)
      again.moment(`resumed-${kind}`, { at: 'rest' })
      await again.close()
    }
  },


  /** R3: Pi's own `/reload`, a new session and a switch back, each a new instance of the extension
   * in the same launch: the counts go on from the launch's, never back down. */
  async reload() {
    const at = await open('reload')
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('First run'), ai.fauxAssistantMessage('Second run')])
    await at.session.prompt('First')
    await settles(at)
    const before = at.word()
    await at.session.reload()
    await at.until(() => at.word() !== null)
    const after = at.word()
    notes.push(`reload (${form}): events ${JSON.stringify(at.loop.seen.map((s) => s.event).slice(-4))}, ended ${before.ended} → ${after.ended}`)
    assert.equal(after.launchId, before.launchId)
    assert.equal(after.sessionId, before.sessionId)
    assert.ok(after.ended >= before.ended, `the count went down across /reload: ${before.ended} → ${after.ended}`)
    assert.deepEqual(after.ends.map((e) => e.n), before.ends.map((e) => e.n), 'the unread ends of the same session stay')
    at.faux.setResponses([ai.fauxAssistantMessage('Second run')])
    await at.session.prompt('Second')
    await settles(at)
    assert.equal(at.word().ended, before.ended + 1)
    await at.close()
  },

  async switch_session() {
    const first = await open('switch')
    await settles(first)
    first.faux.setResponses([ai.fauxAssistantMessage('first answer')])
    await first.session.prompt('first run')
    await settles(first)
    const original = first.word()
    const services = { cwd: first.session.sessionManager.getCwd(), agentDir: join(outRoot, `runtime-agent-${form}`) }
    let serial = 0
    let active
    const runtime = new pc.AgentSessionRuntime(first.session, services, async (options) => {
      active = await open(`switch-replacement-${++serial}`, {
        manager: options.sessionManager,
        start: options.sessionStartEvent,
        ours: { settled: dirname(first.loop.wordFile) },
      })
      return { session: active.session, services }
    })
    const previous = first.file()
    await runtime.newSession()
    await first.until(() => first.word()?.sessionId !== original.sessionId)
    const fresh = first.word()
    await runtime.switchSession(previous)
    await first.until(() => first.word()?.sessionId === original.sessionId)
    const back = first.word()
    notes.push(`switch (${form}): ended ${original.ended} → new session ${fresh.ended} → back ${back.ended}`)
    assert.ok(fresh.ended >= original.ended && back.ended >= original.ended, `counts went down: ${original.ended}, ${fresh.ended}, ${back.ended}`)
    assert.deepEqual(fresh.ends, [], 'a new session has no ends of its own')
    await active?.close()
    await first.close()
  },

  /** R4: inbox events while a person's `before_agent_start` holds the start of a run: never a
   * dropped run on any Pi; and a board message is not sent ahead of the person's prompt. */
  async watcher_events() {
    const release = gate()
    let entered = false
    const at = await open('watcher-events', {
      ours: { pollMs: 1000 },
      person: { before_agent_start: async () => { if (!entered) { entered = true; await release.shut } } },
    })
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('Answer one'), ai.fauxAssistantMessage('Answer two')])
    const pending = at.session.prompt('human request')
    await at.until(() => entered)
    for (let i = 0; i < 6; i += 1) {
      writeFileSync(join(at.inbox, 'pending.tmp'), String(i))
      await wait(30)
    }
    const id = envelope(at, at.session.sessionManager.getSessionId(), 'board request')
    await wait(900)
    assert.equal(at.word().state, 'working', `a run held in its start for under a second: ${JSON.stringify(at.word())}`)
    assert.equal(at.word().ended, 0)
    assert.equal(acked(at, id), null, 'no board message goes in while the run is held')
    release.open()
    await pending
    await settles(at)
    const users = at.rows().filter((r) => r.message?.role === 'user').map((r) => JSON.stringify(r.message.content))
    assert.ok(users[0].includes('human request'), `the person's prompt first: ${users}`)
    await at.close()
  },

  /** R4's limit: on a Pi that marks a run only at its first request, a person's start handler
   * held past three seconds and three passes of the timer reads as a dropped run (nothing else
   * tells the two apart there); what then happens. On 1.2.0 and later the run stays on. */
  async start_held_past_the_allowance() {
    const release = gate()
    let entered = false
    const at = await open('start-held', {
      ours: { pollMs: 200 },
      person: { before_agent_start: async () => { if (!entered) { entered = true; await release.shut } } },
    })
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('Answer one'), ai.fauxAssistantMessage('Answer two')])
    const pending = at.session.prompt('human request')
    await at.until(() => entered)
    const id = envelope(at, at.session.sessionManager.getSessionId(), 'board request')
    await wait(3_600)
    const during = { state: at.word().state, ended: at.word().ended, ack: acked(at, id)?.admitted ?? null }
    release.open()
    await pending
    await settles(at)
    const users = at.rows().filter((r) => r.message?.role === 'user').map((r) => JSON.stringify(r.message.content))
    notes.push(`start held 3.6 s (${form}): during ${JSON.stringify(during)}; user messages in order ${users.join(' | ')}`)
    if (marksEarly) assert.deepEqual([during.state, during.ended], ['working', 0])
    // Before 1.2.0 (the stated limit): the held run is taken as dropped (end 1), the board's
    // message is sent and answered (end 2), and the person's run, let go, is a run of its own
    // (end 3): a person sees the board's answer before the answer to what they typed.
    else notes.push(`older Pi, start held 3.6 s (${form}): ends ${JSON.stringify(at.word().ends.map((e) => [e.n, e.aborted]))}`)
    await at.close()
  },

  /** R4's limit with a board message: a person's `before_agent_start` holds the board's own run
   * 3.6 s. Notes where the run's one end is, against the board's user message and its answer. */
  async board_start_held() {
    const release = gate()
    let held = false
    const at = await open('board-start-held', {
      ours: { pollMs: 200 },
      person: { before_agent_start: async () => { held = true; await release.shut } },
    })
    await settles(at)
    at.faux.setResponses([ai.fauxAssistantMessage('Board answer')])
    const id = envelope(at, at.session.sessionManager.getSessionId(), 'board request')
    await at.until(() => held)
    await wait(3_600)
    const during = { state: at.word().state, ended: at.word().ended, ack: acked(at, id)?.admitted ?? null }
    release.open()
    await at.until(() => at.rows().some((r) => r.message?.role === 'assistant'))
    await settles(at)
    const rows = at.rows()
    const user = rows.find((r) => r.message?.role === 'user')?.id
    const answer = rows.find((r) => r.message?.role === 'assistant')?.id
    const word = at.word()
    notes.push(`board start held 3.6 s (${form}): during ${JSON.stringify(during)}; ends ${JSON.stringify(word.ends)}; user ${user}, answer ${answer}; word ${word.state} at ${word.leaf}`)
    at.moment('answered', { at: 'rest', ended: 'stop', words: 'Board answer' })
    await at.close()
  },

  /** R2 / finding 7: a person's `/tree` navigation back to an earlier answer, labelled: the word's
   * leaf is on that branch; recorded for the reader, which reads the end along it. */
  async tree_navigation() {
    const b = await open('tree', { retry: false })
    await settles(b)
    b.faux.setResponses([
      ai.fauxAssistantMessage('First answer'),
      ai.fauxAssistantMessage('Partial answer', { stopReason: 'error', errorMessage: 'Final provider error' }),
    ])
    await b.session.prompt('First prompt')
    await settles(b)
    const good = b.rows().find((r) => r.message?.role === 'assistant').id
    await b.session.prompt('Second prompt')
    await settles(b)
    await b.session.navigateTree(good, { label: 'Kept answer' })
    await b.until(() => b.word()?.leaf === b.session.sessionManager.getLeafId())
    await wait(4 * POLL_MS)
    b.moment('navigated', { at: 'rest', ended: 'stop', words: 'First answer' })
    await b.close()
  },

  /** D5: a new session that was never written: `rest`, a leaf named, no file. A message goes in. */
  async new_session() {
    const at = await open('new-session')
    await settles(at)
    assert.equal(at.file(), null, 'Pi writes no file before a conversation')
    assert.notEqual(at.word().leaf, null)
    at.moment('no-file', { at: 'rest', items: 0 })
    at.faux.setResponses([ai.fauxAssistantMessage('Got it.')])
    const id = envelope(at, at.session.sessionManager.getSessionId(), 'Your task')
    await at.until(() => acked(at, id) !== null)
    assert.equal(acked(at, id).admitted, true)
    await settles(at)
    await at.close()
  },
}

const chosen = only.length > 0 ? only : Object.keys(CASES)
const results = []
const notes = []
for (const name of chosen) {
  for (const each of ['plain', 'extras']) {
    form = each
    const started = Date.now()
    try {
      let limit
      await Promise.race([
        CASES[name](),
        new Promise((_, fail) => {
          limit = setTimeout(() => fail(new Error(`${name} ${form}: no end in ${CASE_LIMIT_MS} ms`)), CASE_LIMIT_MS)
        }),
      ]).finally(() => clearTimeout(limit))
      results.push({ case: name, form, ok: true, ms: Date.now() - started })
    } catch (cause) {
      results.push({ case: name, form, ok: false, error: cause?.stack ?? String(cause) })
    }
  }
}
console.log(JSON.stringify({ pi: VERSION, results, notes }, null, 1))
process.exit(results.every((r) => r.ok) ? 0 : 1)
