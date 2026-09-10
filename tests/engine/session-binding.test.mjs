import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { discoverSessionWithEvidence, harnessTurns } from '../../hosts/lib/harness-transcript.js'
import { createWindowSeed, formatLaunchMarker } from '../../hosts/lib/packets.js'
import { acceptsBinding, bindEvidence } from '../../hosts/lib/session-binding.js'

/**
 * Phase 2, TEST-PANE-21: a native session binds to a lead or a worker only
 * with launch-unique evidence — an id we preallocated, an id the harness
 * reported on our stream, or the launch nonce in the first user turn.
 */
async function withStores(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-bind-'))
  try {
    return await fn({ HOME: dir, XDG_DATA_HOME: path.join(dir, '.local', 'share') })
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

const write = async (file, text) => {
  await mkdir(path.dirname(file), { recursive: true })
  await writeFile(file, text, 'utf8')
}

test('session binding: the seed carries the launch nonce on its first line', async () => {
  const seed = createWindowSeed({ task: 'Review the retry path.', nonce: 'abc123' })

  assert.equal(seed.split('\n')[0], '[consensflow launch abc123]')
  assert.ok(seed.includes('Review the retry path.'))

  const bare = createWindowSeed({ task: 'Review the retry path.' })
  assert.ok(!bare.includes('[consensflow launch'), 'no nonce, no marker')
  assert.equal(bare, 'Review the retry path.')
})

test('session binding: formatLaunchMarker round-trips the nonce', async () => {
  assert.equal(formatLaunchMarker('abc123'), '[consensflow launch abc123]')
})

test('session binding: claude binds on the preallocated id', async () => {
  const launch = {
    nonce: null,
    preallocatedId: '4ea93ea2-dd16-442d-879e-3a14d48b48a9',
    reportedId: null,
  }

  assert.deepEqual(
    bindEvidence('claude-code', { sessionId: '4ea93ea2-dd16-442d-879e-3a14d48b48a9' }, launch),
    { bound: true, evidence: 'preallocated', generation: null },
  )
  const other = bindEvidence('claude-code', { sessionId: 'somebody-elses-session' }, launch)
  assert.equal(other.bound, false)
  assert.match(other.reason, /unbound/)
})

test('session binding: pi binds on the preallocated id', async () => {
  const launch = {
    nonce: null,
    preallocatedId: '019eac84-de84-74e9-b402-61918f2eaf6c',
    reportedId: null,
  }

  assert.deepEqual(
    bindEvidence('pi', { sessionId: '019eac84-de84-74e9-b402-61918f2eaf6c' }, launch),
    { bound: true, evidence: 'preallocated', generation: null },
  )
  const missing = bindEvidence('pi', { sessionId: '019eac84-de84-74e9-b402-61918f2eaf6c' }, {})
  assert.equal(missing.bound, false)
  assert.match(missing.reason, /unbound/)
})

test('session binding: codex binds when the first user turn carries the nonce', async () => {
  const launch = { nonce: 'n-codex-1', preallocatedId: null, reportedId: null }

  assert.deepEqual(
    bindEvidence(
      'codex',
      {
        sessionId: '01a0773d-6bdb-76e0-a09e-37b330d87d60',
        turn: '[consensflow launch n-codex-1]\nTell me a joke.',
      },
      launch,
    ),
    { bound: true, evidence: 'nonce', generation: null },
  )
})

test('session binding: task text alone never binds', async () => {
  // Two candidates in one directory, identical task text, different nonces.
  const task = 'Tell me a joke.'
  const a = bindEvidence(
    'codex',
    { sessionId: 'session-a', turn: `[consensflow launch nonce-a]\n${task}` },
    { nonce: 'nonce-a' },
  )
  const b = bindEvidence(
    'codex',
    { sessionId: 'session-b', turn: `[consensflow launch nonce-b]\n${task}` },
    { nonce: 'nonce-b' },
  )
  assert.deepEqual(a, { bound: true, evidence: 'nonce', generation: null })
  assert.deepEqual(b, { bound: true, evidence: 'nonce', generation: null })

  const crossed = bindEvidence(
    'codex',
    { sessionId: 'session-b', turn: `[consensflow launch nonce-b]\n${task}` },
    { nonce: 'nonce-a' },
  )
  assert.equal(crossed.bound, false)
  assert.match(crossed.reason, /unbound/)
})

test('session binding: no nonce means unbound, with a reason', async () => {
  for (const kind of ['codex', 'opencode', 'kimi']) {
    const decision = bindEvidence(kind, { sessionId: 'ses_x', turn: 'Tell me a joke.' }, {})
    assert.deepEqual(decision.bound, false)
    assert.match(decision.reason, /unbound/, kind)
  }
})

test('session binding: a reported id binds on every harness', async () => {
  for (const kind of ['codex', 'opencode', 'kimi', 'claude-code', 'pi']) {
    assert.deepEqual(
      bindEvidence(kind, { sessionId: 'ses_reported' }, { reportedId: 'ses_reported' }),
      { bound: true, evidence: 'reported', generation: null },
      kind,
    )
  }
  const stranger = bindEvidence('kimi', { sessionId: 'ses_other' }, { reportedId: 'ses_reported' })
  assert.equal(stranger.bound, false)
})

test('session binding: a session replaced in place yields replaced', async () => {
  // The native session changed underneath the pane (`/new`, `/resume`, a
  // fork): the binding is dead even though the candidate still carries our
  // nonce — and `alive` is not part of the condition.
  const decision = bindEvidence(
    'codex',
    {
      sessionId: 'session-old',
      turn: '[consensflow launch nonce-9]\nTell me a joke.',
      currentSessionId: 'session-new',
    },
    { nonce: 'nonce-9', reportedId: 'session-old', generation: 2 },
  )
  assert.deepEqual(decision, {
    bound: false,
    replaced: true,
    reason: 'replaced: the live pane now shows a different native session',
    generation: 2,
  })
})

test('session binding: acceptsBinding is the predicate the store will call', async () => {
  assert.equal(acceptsBinding('preallocated'), true)
  assert.equal(acceptsBinding('reported'), true)
  assert.equal(acceptsBinding('nonce'), true)
  assert.equal(acceptsBinding(undefined), false)
  assert.equal(acceptsBinding(null), false)
  assert.equal(acceptsBinding(''), false)
  assert.equal(acceptsBinding('unbound'), false)
})

test('session binding: the reader strips the marker line from what it shows', async () => {
  await withStores(async (env) => {
    const id = '01a03068-3530-7173-a123-009f15591007'
    await write(
      path.join(
        env.HOME,
        '.codex',
        'sessions',
        '2026',
        '09',
        '06',
        `rollout-2026-09-06T18-01-49-${id}.jsonl`,
      ),
      [
        // Copied 2026-09-06 from a real codex 0.153.4 rollout on this
        // machine: `session_meta` first, then `response_item` turns whose
        // user text sits in `payload.content[].text`.
        JSON.stringify({
          type: 'session_meta',
          payload: { cwd: '/work/here', timestamp: '2026-09-06T15:01:49.213Z' },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            type: 'message',
            role: 'user',
            content: [
              { type: 'input_text', text: '[consensflow launch strip-me]\nTell me a joke.' },
            ],
          },
        }),
      ].join('\n'),
    )

    assert.deepEqual(await harnessTurns('codex', id, env), [
      { role: 'user', text: 'Tell me a joke.' },
    ])
  })
})

test('session binding: codex discovery returns the session with our nonce', async () => {
  await withStores(async (env) => {
    const dir = path.join(env.HOME, '.codex', 'sessions', '2026', '09', '06')
    const stranger = '01a048fa-9c3d-7941-9860-00000000them'
    const ours = '01a048fa-9c3d-7941-9860-00000000ours'
    const iso = (offset) => new Date(Date.now() + offset).toISOString()
    await write(
      path.join(dir, `rollout-x-${stranger}.jsonl`),
      [
        JSON.stringify({
          type: 'session_meta',
          payload: { cwd: '/work/here', timestamp: iso(-1000) },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            type: 'message',
            role: 'user',
            content: [{ type: 'input_text', text: 'Tell me a joke.' }],
          },
        }),
      ].join('\n'),
    )
    await write(
      path.join(dir, `rollout-x-${ours}.jsonl`),
      [
        JSON.stringify({ type: 'session_meta', payload: { cwd: '/work/here', timestamp: iso(0) } }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            type: 'message',
            role: 'user',
            content: [
              { type: 'input_text', text: '[consensflow launch own-nonce]\nTell me a joke.' },
            ],
          },
        }),
      ].join('\n'),
    )

    const since = Date.now() - 60_000
    assert.deepEqual(
      await discoverSessionWithEvidence('codex', '/work/here', since, env, { nonce: 'own-nonce' }),
      {
        sessionId: ours,
        evidence: 'nonce',
        turn: '[consensflow launch own-nonce]\nTell me a joke.',
      },
    )
    assert.equal(
      await discoverSessionWithEvidence('codex', '/work/here', since, env, { nonce: 'nobody' }),
      null,
    )
    assert.equal(
      await discoverSessionWithEvidence('codex', '/work/here', since, env),
      null,
      'no nonce, never bound',
    )
    assert.deepEqual(
      await discoverSessionWithEvidence('claude-code', '/work/here', since, env, {
        preallocatedId: 'minted-id',
      }),
      null,
      'no file carries the minted id in this store',
    )
    await write(path.join(env.HOME, '.claude', 'projects', '-work-here', 'minted-id.jsonl'), '')
    assert.deepEqual(
      await discoverSessionWithEvidence('claude-code', '/work/here', since, env, {
        preallocatedId: 'minted-id',
      }),
      { sessionId: 'minted-id', evidence: 'preallocated' },
    )
    assert.deepEqual(
      await discoverSessionWithEvidence('kimi', '/work/here', since, env, { reportedId: ours }),
      { sessionId: ours, evidence: 'reported' },
    )
    assert.equal(await discoverSessionWithEvidence('image', '/work/here', since, env), null)
  })
})

test('session binding: opencode binds on the nonce in the frozen file layout too', async () => {
  // A machine that has not run opencode since the 2026-01-06 migration still
  // keeps the JSON tree: `session/<proj>/<id>.json` plus `message/` and `part/`.
  await withStores(async (env) => {
    const storage = path.join(env.XDG_DATA_HOME, 'opencode', 'storage')
    const writeSession = async (id, directory, created) => {
      await write(
        path.join(storage, 'session', 'proj-hash', `${id}.json`),
        JSON.stringify({ id, directory, time: { created } }),
      )
    }
    const writeTurn = async (sessionId, text) => {
      const msgId = `msg_${sessionId}`
      await write(
        path.join(storage, 'message', sessionId, `${msgId}.json`),
        JSON.stringify({ id: msgId, role: 'user', time: { created: 1 } }),
      )
      await write(
        path.join(storage, 'part', msgId, 'prt_1.json'),
        JSON.stringify({ type: 'text', text }),
      )
    }
    await writeSession('ses_old', '/work/here', 100)
    await writeTurn('ses_old', 'Tell me a joke.')
    await writeSession('ses_new', '/work/here', 800)
    await writeTurn('ses_new', '[consensflow launch files-nonce]\nTell me a joke.')

    assert.deepEqual(
      await discoverSessionWithEvidence('opencode', '/work/here', 500, env, {
        nonce: 'files-nonce',
      }),
      {
        sessionId: 'ses_new',
        evidence: 'nonce',
        turn: '[consensflow launch files-nonce]\nTell me a joke.',
      },
    )
    assert.equal(
      await discoverSessionWithEvidence('opencode', '/work/here', 500, env, { nonce: 'nope' }),
      null,
    )
  })
})

test('session binding: a marker buried in the turn never binds', async () => {
  // The marker is non-secret and sits verbatim in a file every agent in that
  // directory can read — so only the opening line counts, never a quote
  // thirty lines down or inside a fenced block.
  const buried = [
    ...Array.from({ length: 30 }, (_, i) => `padding line ${i}`),
    '[consensflow launch deep-nonce]',
    'Tell me a joke.',
  ].join('\n')
  assert.deepEqual(
    bindEvidence('codex', { sessionId: 's', turn: buried }, { nonce: 'deep-nonce' }).bound,
    false,
  )
  const fenced = '```\n[consensflow launch fenced-nonce]\n```\nTell me a joke.'
  const quoted = bindEvidence('codex', { sessionId: 's', turn: fenced }, { nonce: 'fenced-nonce' })
  assert.equal(quoted.bound, false)
  assert.match(quoted.reason, /unbound/)
})

test('session binding: the marker binds after injected blocks', async () => {
  const turn =
    '<skills_instructions>\nx\n</skills_instructions>\n[consensflow launch n]\nTell me a joke.'
  assert.deepEqual(bindEvidence('codex', { sessionId: 's', turn: turn }, { nonce: 'n' }), {
    bound: true,
    evidence: 'nonce',
    generation: null,
  })
})

test('session binding: every decision carries the generation it was made at', async () => {
  assert.deepEqual(bindEvidence('pi', { sessionId: 'p' }, { preallocatedId: 'p', generation: 3 }), {
    bound: true,
    evidence: 'preallocated',
    generation: 3,
  })
  const unbound = bindEvidence('kimi', { sessionId: 'k' }, { generation: 3 })
  assert.deepEqual(unbound, {
    bound: false,
    reason: 'unbound: no launch evidence (preallocated id, reported id, or nonce)',
    generation: 3,
  })
})

test('session binding: codex discovery reads a real rollout head, seed after preamble', async () => {
  // Built 2026-09-06 from a real codex 0.153.4 rollout head on this machine:
  // `session_meta` with its real envelope, developer turns, then a user turn
  // carrying workspace instructions (never our seed), then the seeded turn.
  // The first user turn is somebody else talking — binding must look on.
  await withStores(async (env) => {
    const dir = path.join(env.HOME, '.codex', 'sessions', '2026', '09', '06')
    const id = '01a0773d-6bdb-76e0-a09e-37b330d87d60'
    const file = path.join(dir, `rollout-2026-09-06T15-01-49-${id}.jsonl`)
    const userTurn = (text) =>
      JSON.stringify({
        type: 'response_item',
        payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text }] },
      })
    // The envelope below is the real one; only the clock is fresh, so the
    // session counts as created since.
    const now = new Date().toISOString()
    await write(
      file,
      [
        JSON.stringify({
          timestamp: now,
          ordinal: 0,
          type: 'session_meta',
          payload: {
            session_id: id,
            id,
            timestamp: now,
            cwd: '/work/here',
            originator: 'codex-tui',
            cli_version: '0.153.4',
            source: 'cli',
            thread_source: 'user',
          },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            type: 'message',
            role: 'developer',
            content: [{ type: 'input_text', text: 'You are Codex. Collaborate until done.' }],
          },
        }),
        userTurn('# AGENTS.md instructions\n\nWork iteratively and verify claims as you go.'),
        // An attributed tag is somebody talking, not injected context: the
        // injector writes bare tags, and this one carries attributes.
        userTurn('<div class="chat">tags are escaping wrong, why?</div>'),
        userTurn('[consensflow launch real-nonce]\nTell me a joke.'),
      ].join('\n'),
    )

    const since = Date.now() - 60_000
    const found = await discoverSessionWithEvidence('codex', '/work/here', since, env, {
      nonce: 'real-nonce',
    })
    assert.deepEqual(found, {
      sessionId: '01a0773d-6bdb-76e0-a09e-37b330d87d60',
      evidence: 'nonce',
      turn: '[consensflow launch real-nonce]\nTell me a joke.',
    })
    assert.equal(
      await discoverSessionWithEvidence('codex', '/work/here', since, env, { nonce: 'nope' }),
      null,
    )
    const turns = await harnessTurns('codex', id, env)
    assert.ok(
      turns.some((t) => t.text === '<div class="chat">tags are escaping wrong, why?</div>'),
      'an attributed tag is shown, not stripped',
    )
  })
})

test('session binding: codex discovery ignores a quoted marker and stops at five turns', async () => {
  await withStores(async (env) => {
    const dir = path.join(env.HOME, '.codex', 'sessions', '2026', '09', '06')
    const id = '01a048fa-9c3d-7941-9860-00000000late'
    const userTurn = (text) =>
      JSON.stringify({
        type: 'response_item',
        payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text }] },
      })
    const lines = [
      JSON.stringify({
        type: 'session_meta',
        payload: { cwd: '/work/here', timestamp: new Date().toISOString() },
      }),
      userTurn('quoting the launch: ```\n[consensflow launch late-nonce]\n```'),
      ...['one', 'two', 'three', 'four', 'five'].map((w) => userTurn(`follow-up ${w}`)),
      userTurn('[consensflow launch late-nonce]\nthe real seed, sixth'),
    ]
    await write(path.join(dir, `rollout-x-${id}.jsonl`), lines.join('\n'))

    // The fence quote is not an opening line, and the genuine marker is the
    // sixth user turn — past the first five. Neither binds.
    assert.equal(
      await discoverSessionWithEvidence('codex', '/work/here', Date.now() - 60_000, env, {
        nonce: 'late-nonce',
      }),
      null,
    )
  })
})

test('session binding: preallocated binds only when the harness holds the file', async () => {
  await withStores(async (env) => {
    const claudeId = '4ea93ea2-dd16-442d-879e-3a14d48b48a9'
    const piId = '019eac84-de84-74e9-b402-61918f2eaf6c'
    const since = Date.now() - 60_000
    assert.equal(
      await discoverSessionWithEvidence('claude-code', '/work/here', since, env, {
        preallocatedId: claudeId,
      }),
      null,
      'no file carries the minted id — claude may have rejected it',
    )
    assert.equal(
      await discoverSessionWithEvidence('pi', '/work/here', since, env, { preallocatedId: piId }),
      null,
    )
    await write(path.join(env.HOME, '.claude', 'projects', '-work-here', `${claudeId}.jsonl`), '')
    await write(
      path.join(
        env.HOME,
        '.pi',
        'agent',
        'sessions',
        '--work-here--',
        `2026-09-06T00-00-00-000Z_${piId}.jsonl`,
      ),
      '',
    )
    assert.deepEqual(
      await discoverSessionWithEvidence('claude-code', '/work/here', since, env, {
        preallocatedId: claudeId,
      }),
      { sessionId: claudeId, evidence: 'preallocated' },
    )
    assert.deepEqual(
      await discoverSessionWithEvidence('pi', '/work/here', since, env, { preallocatedId: piId }),
      { sessionId: piId, evidence: 'preallocated' },
    )
  })
})

test('session binding: preallocated wins over reported for claude and pi', async () => {
  // bindEvidence and discovery must agree: with both present, the minted id
  // decides, so discovery never hands the store an id the store refuses.
  await withStores(async (env) => {
    const minted = '4ea93ea2-dd16-442d-879e-3a14d48b48a9'
    await write(path.join(env.HOME, '.claude', 'projects', '-work-here', `${minted}.jsonl`), '')
    assert.deepEqual(
      await discoverSessionWithEvidence('claude-code', '/work/here', Date.now() - 60_000, env, {
        preallocatedId: minted,
        reportedId: 'stranger-reported',
      }),
      { sessionId: minted, evidence: 'preallocated' },
    )
  })
  const storeSide = bindEvidence(
    'claude-code',
    { sessionId: 'stranger-reported' },
    { preallocatedId: '4ea93ea2-dd16-442d-879e-3a14d48b48a9', reportedId: 'stranger-reported' },
  )
  assert.equal(storeSide.bound, false)
})

test('session binding: the store binds on the matching turn discovery returns', async () => {
  // E1: discovery holds the matching turn when it returns, and the store
  // checks THAT turn — the AGENTS.md preamble alone never binds.
  const seedTurn = '[consensflow launch real-nonce]\nTell me a joke.'
  assert.deepEqual(
    bindEvidence('codex', { sessionId: 's', turn: seedTurn }, { nonce: 'real-nonce' }),
    {
      bound: true,
      evidence: 'nonce',
      generation: null,
    },
  )
  const preamble = '# AGENTS.md instructions\n\nWork iteratively and verify claims as you go.'
  const alone = bindEvidence('codex', { sessionId: 's', turn: preamble }, { nonce: 'real-nonce' })
  assert.equal(alone.bound, false)
  assert.match(alone.reason, /unbound/)
})

test('session binding: reported equal to the minted id binds when the file is absent', async () => {
  // E2, case equal: no file carries the minted id, but the harness reported
  // that same id on our stream — both entry points accept it as reported.
  await withStores(async (env) => {
    const minted = '4ea93ea2-dd16-442d-879e-3a14d48b48a9'
    const since = Date.now() - 60_000
    assert.deepEqual(
      await discoverSessionWithEvidence('claude-code', '/work/here', since, env, {
        preallocatedId: minted,
        reportedId: minted,
      }),
      { sessionId: minted, evidence: 'reported' },
    )
  })
  assert.deepEqual(
    bindEvidence(
      'claude-code',
      { sessionId: '4ea93ea2-dd16-442d-879e-3a14d48b48a9' },
      {
        preallocatedId: '4ea93ea2-dd16-442d-879e-3a14d48b48a9',
        reportedId: '4ea93ea2-dd16-442d-879e-3a14d48b48a9',
      },
    ),
    { bound: true, evidence: 'reported', generation: null },
  )
})

test('session binding: a reported id differing from the minted id is refused', async () => {
  // E2, case differ: both entry points refuse — discovery returns null, the
  // store returns unbound — for the same inputs.
  await withStores(async (env) => {
    const minted = '4ea93ea2-dd16-442d-879e-3a14d48b48a9'
    const since = Date.now() - 60_000
    assert.equal(
      await discoverSessionWithEvidence('claude-code', '/work/here', since, env, {
        preallocatedId: minted,
        reportedId: 'stranger-reported',
      }),
      null,
    )
  })
  const storeSide = bindEvidence(
    'pi',
    { sessionId: 'stranger-reported' },
    {
      preallocatedId: '019eac84-de84-74e9-b402-61918f2eaf6c',
      reportedId: 'stranger-reported',
    },
  )
  assert.equal(storeSide.bound, false)
  assert.match(storeSide.reason, /unbound/)
})

test('quiet Codex binding accepts only launch-owned native session metadata', () => {
  const launch = { nonce: 'quiet-launch', originator: 'consensflow-quiet-launch', generation: 4 }
  const sessionId = '00000000-1111-4222-8333-444444444444'
  const sessionMeta = {
    id: sessionId,
    originator: launch.originator,
    source: 'cli',
    cli_version: 'future-version',
    thread_source: 'user',
  }
  assert.deepEqual(bindEvidence('codex', { sessionId, sessionMeta }, launch), {
    bound: true,
    evidence: 'nonce',
    generation: 4,
  })
  for (const changed of [
    { thread_source: 'subagent' },
    { originator: 'consensflow-another-launch' },
    { id: 'someone-else' },
    { source: { subagent: {} } },
    { forked_from_id: 'parent' },
    { parent_thread_id: 'parent' },
  ]) {
    assert.equal(
      bindEvidence(
        'codex',
        {
          sessionId,
          sessionMeta: { ...sessionMeta, ...changed },
          turn: '[consensflow launch quiet-launch]',
        },
        launch,
      ).bound,
      false,
      'a matching user turn cannot replace the metadata evidence',
    )
  }
  assert.equal(
    bindEvidence(
      'codex',
      {
        sessionId,
        turn: '[consensflow launch quiet-launch]',
      },
      launch,
    ).bound,
    false,
  )
  assert.equal(bindEvidence('opencode', { sessionId, sessionMeta }, launch).bound, false)
})

test('quiet Codex discovery separates concurrent launches and refuses ambiguous metadata', async () => {
  await withStores(async (env) => {
    const root = path.join(env.HOME, '.codex', 'sessions')
    const since = Date.now() - 1000
    const cwd = '/work/quiet'
    const launch = { nonce: 'quiet-launch', originator: 'consensflow-quiet-launch' }
    const own = '00000000-1111-4222-8333-444444444444'
    const other = '00000000-1111-4222-8333-555555555555'
    const fork = '00000000-1111-4222-8333-666666666666'
    const metadata = (id, more = {}) => ({
      id,
      originator: launch.originator,
      source: 'cli',
      cli_version: '0.153.4',
      thread_source: 'user',
      cwd,
      timestamp: new Date().toISOString(),
      ...more,
    })
    const save = async (id, meta, turn) =>
      write(
        path.join(root, `rollout-x-${id}.jsonl`),
        JSON.stringify({ type: 'session_meta', payload: meta }) +
          '\n' +
          (turn
            ? `${JSON.stringify({
                type: 'event_msg',
                payload: { type: 'user_message', message: turn },
              })}\n`
            : ''),
      )
    await save(
      other,
      metadata(other, { originator: 'consensflow-other-launch' }),
      '[consensflow launch quiet-launch]',
    )
    await save(fork, metadata(fork, { forked_from_id: own }), '[consensflow launch quiet-launch]')
    assert.equal(await discoverSessionWithEvidence('codex', cwd, since, env, launch), null)
    const ours = metadata(own)
    await save(own, ours)
    assert.deepEqual(await discoverSessionWithEvidence('codex', cwd, since, env, launch), {
      sessionId: own,
      evidence: 'nonce',
      sessionMeta: ours,
    })
    assert.equal(
      await discoverSessionWithEvidence('codex', '/another-folder', since, env, launch),
      null,
    )
    assert.equal(
      await discoverSessionWithEvidence('codex', cwd, Date.now() + 1000, env, launch),
      null,
    )
    await save(other, metadata(other))
    assert.equal(
      await discoverSessionWithEvidence('codex', cwd, since, env, launch),
      null,
      'two native roots carrying one launch are ambiguous, never choose the newest',
    )
  })
})

test('quiet Codex launch discovery accepts an alias of the exact workspace', async () => {
  await withStores(async env => {
    const actual = path.join(env.HOME, 'actual-workspace')
    const alias = path.join(env.HOME, 'workspace-alias')
    await mkdir(actual)
    const { symlink } = await import('node:fs/promises')
    await symlink(actual, alias)
    const launch = { nonce: 'alias-launch', originator: 'consensflow-alias-launch' }
    const id = '00000000-1111-4222-8333-444444444444'
    await write(path.join(env.HOME, '.codex', 'sessions', `rollout-x-${id}.jsonl`), JSON.stringify({
      type: 'session_meta', payload: { id, originator: launch.originator, source: 'cli', thread_source: 'user', cwd: actual, timestamp: new Date().toISOString() }
    }) + '\n')
    const found = await discoverSessionWithEvidence('codex', alias, Date.now() - 1000, env, launch)
    assert.equal(found?.sessionId, id)
  })
})
