import assert from 'node:assert/strict'
import test from 'node:test'
import { leadReady } from '../../hosts/lib/readiness.js'

/**
 * Phase 3, TEST-PANE-25 — rewritten against the completion/readiness contract
 * the lead fixed after asteria's BLOCK.
 *
 * `completion.answers()` carries an aggregate settlement proof beside
 * `items` / `inFlight` / `cancelled` / `replaced` / `version`:
 *
 *   settlement = {
 *     state: 'settled' | 'in-flight' | 'unknown',
 *     provenance: 'native' | 'derived',
 *     cursor: <native total-order position of the boundary>,
 *     boundary: <the native record that proves it: task_complete, turn.ended,
 *                the post-turn hook>,
 *     evidence: { complete, openTools, queuedTurns, hooksInFlight },
 *   }
 *
 * A bare `settled: true` on the last item authorises NOTHING — a queued user
 * item or a completed tool result satisfies that boolean just as well as a
 * finished turn (the weak derivation hyperion disproved). Readiness is `ready`
 * only when the proof qualifies, the draft latch is EXPLICITLY clear, and a
 * valid decision epoch was supplied. Missing or legacy proof is `unknown`;
 * demonstrated ongoing work is `busy`.
 *
 * The decision carries the settlement cursor and its provenance so the
 * watcher (TEST-PANE-31) can require a NEWER qualified boundary after each
 * automated submission — re-reading the same snapshot is not freshness.
 */

/** A transcript item. Its `settled` boolean is deliberately NOT the proof. */
function turn(overrides = {}) {
  return {
    id: 'i-1',
    role: 'assistant',
    text: 'Done.',
    complete: true,
    settled: true,
    at: '2026-09-06T20:00:00.000Z',
    ...overrides,
  }
}

/** A qualified native settlement — the shape every positive starts from. */
function settlement({ evidence = {}, ...overrides } = {}) {
  return {
    state: 'settled',
    provenance: 'native',
    cursor: 42,
    boundary: 'turn.ended',
    evidence: {
      complete: true,
      openTools: [],
      queuedTurns: [],
      hooksInFlight: [],
      ...evidence,
    },
    ...overrides,
  }
}

/** A `completion.answers()` result that read the session fine. */
function answers({ items = [turn()], inFlight = false, ...rest } = {}) {
  return {
    items,
    inFlight,
    cancelled: false,
    replaced: false,
    version: '1',
    settlement: settlement(),
    ...rest,
  }
}

/** The whole qualified input: proof, an explicitly clear latch, a real epoch. */
function qualified(overrides = {}) {
  return { answers: answers(), draftLatched: false, epoch: 7, ...overrides }
}

// --- the qualified positives -------------------------------------------------

test('readiness: Pi can delegate composer admission without claiming its terminal latch is clear', () => {
  const input = qualified({ kind: 'pi', draftLatched: true, composerAuthority: 'pi-native-editor' })
  const decision = leadReady(input)
  assert.equal(decision.state, 'ready')
  assert.match(decision.reason, /editor admission checked by native Pi/)
  assert.doesNotMatch(decision.reason, /no draft latched/)
  assert.equal(leadReady({ ...input, composerAuthority: 'terminal' }).state, 'draft')
  assert.equal(leadReady({ ...input, kind: 'codex' }).state, 'unknown')
  assert.equal(leadReady({ ...input, draftLatched: undefined }).state, 'unknown')
  assert.equal(leadReady({ ...input, answers: answers({ inFlight: true }) }).state, 'busy')
})

test('native queue delivery preserves the composer without clearing its latch (TEST-PANE-109)', () => {
  for (const kind of ['claude-code', 'codex', 'opencode']) {
    const input = qualified({ kind, draftLatched: true, composerAuthority: 'native-queue' })
    assert.equal(leadReady(input).state, 'ready', kind)
    assert.equal(leadReady({ ...input, composerAuthority: 'terminal' }).state, 'draft')
    assert.equal(leadReady({ ...input, answers: answers({ inFlight: true }) }).state, 'busy')
    assert.equal(leadReady({ ...input, epoch: undefined }).state, 'unknown')
    assert.equal(leadReady({ ...input, answers: answers({ replaced: true }) }).state, 'unknown')
  }
  for (const kind of ['pi', 'kimi', 'unknown']) {
    assert.equal(
      leadReady(qualified({ kind, draftLatched: true, composerAuthority: 'native-queue' })).state,
      'unknown',
    )
  }
})

test('readiness: a native settlement, an explicitly clear latch and a valid epoch are ready', () => {
  const decision = leadReady(qualified())
  assert.equal(decision.state, 'ready')
  assert.equal(decision.epoch, 7)
  assert.equal(decision.cursor, 42)
  assert.equal(decision.provenance, 'native')
  assert.match(decision.reason, /settle/i)
})

test('readiness: a derived settlement with every condition satisfied is ready', () => {
  const decision = leadReady(
    qualified({
      answers: answers({
        settlement: settlement({ provenance: 'derived', boundary: 'post-turn-hook' }),
      }),
    }),
  )
  assert.equal(decision.state, 'ready')
  assert.equal(decision.provenance, 'derived')
  assert.equal(decision.cursor, 42)
})

// --- finding 1: a bare boolean is not a proof --------------------------------

test('readiness: a last item marked settled with NO settlement proof is unknown, not ready', () => {
  // The gap: this is exactly the snapshot the old module approved.
  const legacy = leadReady(qualified({ answers: answers({ settlement: undefined }) }))
  assert.equal(legacy.state, 'unknown')
  assert.equal(legacy.cursor, null)
  assert.equal(legacy.provenance, null)

  // The qualifying boundary for the same snapshot turns it into a positive.
  const proven = leadReady(qualified())
  assert.equal(proven.state, 'ready')
})

test('readiness: the item booleans are ignored — the settlement decides in both directions', () => {
  // Unsettled items, qualified proof: ready. Settled items, no proof: unknown.
  const provenDespiteItems = leadReady(
    qualified({ answers: answers({ items: [turn({ complete: false, settled: false })] }) }),
  )
  assert.equal(provenDespiteItems.state, 'ready')

  const unprovenDespiteItems = leadReady(
    qualified({ answers: answers({ items: [turn({ settled: true })], settlement: null }) }),
  )
  assert.equal(unprovenDespiteItems.state, 'unknown')
})

test('readiness: a native marker behind queued work is not the frontier — busy, not ready', () => {
  const decision = leadReady(
    qualified({
      answers: answers({ settlement: settlement({ evidence: { queuedTurns: ['u-9'] } }) }),
    }),
  )
  assert.equal(decision.state, 'busy')
  assert.equal(decision.reason, 'lead busy: a turn is queued behind the boundary')
})

test('readiness: demonstrated ongoing work is busy whatever the settlement state claims', () => {
  const cases = [
    [{ openTools: ['bash-1'] }, 'lead busy: a tool is still open'],
    [{ queuedTurns: ['u-9'] }, 'lead busy: a turn is queued behind the boundary'],
    [{ hooksInFlight: ['post-turn'] }, 'lead busy: a hook is still in flight'],
  ]
  for (const [evidence, reason] of cases) {
    for (const provenance of ['native', 'derived']) {
      const decision = leadReady(
        qualified({ answers: answers({ settlement: settlement({ provenance, evidence }) }) }),
      )
      assert.equal(decision.state, 'busy', `${provenance} ${reason}`)
      assert.equal(decision.reason, reason)
    }
  }
})

test('readiness: an in-flight settlement is busy and an unknown one is unknown', () => {
  const inFlight = leadReady(
    qualified({ answers: answers({ settlement: settlement({ state: 'in-flight' }) }) }),
  )
  assert.equal(inFlight.state, 'busy')
  assert.equal(inFlight.reason, 'lead busy')

  const unsure = leadReady(
    qualified({ answers: answers({ settlement: settlement({ state: 'unknown' }) }) }),
  )
  assert.equal(unsure.state, 'unknown')
})

test('readiness: inFlight on the result is demonstrated work even with no settlement at all', () => {
  const decision = leadReady(
    qualified({ answers: answers({ inFlight: true, settlement: undefined }) }),
  )
  assert.equal(decision.state, 'busy')
  assert.equal(decision.reason, 'lead busy')
})

test('readiness: a provenance that is neither native nor derived proves nothing', () => {
  for (const provenance of [undefined, null, 'unknown', 'inferred', '']) {
    const decision = leadReady(
      qualified({ answers: answers({ settlement: settlement({ provenance }) }) }),
    )
    assert.equal(decision.state, 'unknown', `provenance ${JSON.stringify(provenance)}`)
    assert.equal(decision.provenance, null)
  }
})

test('readiness: a settlement with no observed post-turn boundary proves nothing', () => {
  for (const provenance of ['native', 'derived']) {
    for (const boundary of [undefined, null, '']) {
      const decision = leadReady(
        qualified({ answers: answers({ settlement: settlement({ provenance, boundary }) }) }),
      )
      assert.equal(decision.state, 'unknown', `${provenance} boundary ${JSON.stringify(boundary)}`)
    }
  }
})

test('readiness: a settlement with no usable native cursor proves nothing', () => {
  for (const cursor of [undefined, null, -1, 1.5, '42', Number.NaN]) {
    const decision = leadReady(
      qualified({ answers: answers({ settlement: settlement({ cursor }) }) }),
    )
    assert.equal(decision.state, 'unknown', `cursor ${JSON.stringify(cursor)}`)
    assert.equal(decision.cursor, null)
  }
})

// --- finding 1: each derived condition removed on its own must be refused ----

test('readiness: derived settlement without complete is refused', () => {
  for (const complete of [false, undefined, null]) {
    const decision = leadReady(
      qualified({
        answers: answers({
          settlement: settlement({ provenance: 'derived', evidence: { complete } }),
        }),
      }),
    )
    assert.equal(decision.state, 'unknown', `complete ${JSON.stringify(complete)}`)
  }
  // …and satisfied, the same snapshot qualifies.
  const decision = leadReady(
    qualified({
      answers: answers({
        settlement: settlement({ provenance: 'derived', evidence: { complete: true } }),
      }),
    }),
  )
  assert.equal(decision.state, 'ready')
})

test('readiness: a derived Pi settlement is never automatic-ready without native evidence', () => {
  const decision = leadReady(
    qualified({
      kind: 'pi',
      answers: answers({
        settlement: settlement({ provenance: 'derived', boundary: 'session.quiet_window' }),
      }),
    }),
  )
  assert.equal(decision.state, 'unknown')
  assert.match(decision.reason, /Pi.*native settlement evidence/i)
})

test('readiness: a manual Pi delivery may use a derived settlement', () => {
  const decision = leadReady(
    qualified({
      kind: 'pi',
      purpose: 'manual',
      answers: answers({
        settlement: settlement({ provenance: 'derived', boundary: 'session.quiet_window' }),
      }),
    }),
  )
  assert.equal(decision.state, 'ready')
  assert.equal(decision.provenance, 'derived')
})

test('readiness: derived settlement with no evidence object at all is refused', () => {
  const bare = { state: 'settled', provenance: 'derived', cursor: 42, boundary: 'post-turn-hook' }
  const decision = leadReady(qualified({ answers: answers({ settlement: bare }) }))
  assert.equal(decision.state, 'unknown')
})

test('readiness: an evidence list that is not a list is missing proof, not an empty list', () => {
  for (const field of ['openTools', 'queuedTurns', 'hooksInFlight']) {
    const decision = leadReady(
      qualified({
        answers: answers({ settlement: settlement({ evidence: { [field]: 'none' } }) }),
      }),
    )
    assert.equal(decision.state, 'unknown', `${field} must be a list`)
  }
})

// --- finding 2: missing evidence never permits readiness ---------------------

test('readiness: the draft latch must be EXPLICITLY clear — a missing latch is unknown', () => {
  for (const draftLatched of [undefined, null, 0, '']) {
    const decision = leadReady(qualified({ draftLatched }))
    assert.equal(decision.state, 'unknown', `latch ${JSON.stringify(draftLatched)}`)
    assert.match(decision.reason, /latch/i)
  }
  assert.equal(leadReady(qualified({ draftLatched: false })).state, 'ready')
})

test('readiness: a latched draft is draft, and outranks a busy lead', () => {
  assert.equal(leadReady(qualified({ draftLatched: true })).reason, 'draft open')
  const alsoBusy = leadReady(
    qualified({
      draftLatched: true,
      answers: answers({ inFlight: true, settlement: settlement({ state: 'in-flight' }) }),
    }),
  )
  assert.equal(alsoBusy.state, 'draft')
  assert.equal(alsoBusy.reason, 'draft open')
})

test('readiness: a valid decision epoch is required — a missing or malformed one is unknown', () => {
  for (const epoch of [undefined, null, -1, 1.5, '7', Number.NaN, {}]) {
    const decision = leadReady(qualified({ epoch }))
    assert.equal(decision.state, 'unknown', `epoch ${JSON.stringify(epoch)}`)
    assert.equal(decision.epoch, null)
    assert.match(decision.reason, /epoch/i)
  }
  assert.equal(leadReady(qualified({ epoch: 0 })).state, 'ready', 'epoch 0 is a real epoch')
})

test('readiness: an empty transcript is never ready, even carrying a qualified settlement', () => {
  const decision = leadReady(qualified({ answers: answers({ items: [] }) }))
  assert.equal(decision.state, 'unknown')
  assert.match(decision.reason, /transcript/i)
})

// --- finding 3: the decision carries the native freshness boundary -----------

test('readiness: every decision reports cursor and provenance, null when unproven', () => {
  const cases = [
    ['ready', qualified()],
    ['draft', qualified({ draftLatched: true })],
    ['busy', qualified({ answers: answers({ settlement: settlement({ state: 'in-flight' }) }) })],
    ['unknown', qualified({ answers: { unknown: true, reason: 'unreadable' } })],
  ]
  for (const [state, input] of cases) {
    const decision = leadReady(input)
    assert.equal(decision.state, state)
    assert.deepEqual(Object.keys(decision).sort(), [
      'cursor',
      'epoch',
      'provenance',
      'reason',
      'state',
    ])
  }
  assert.equal(leadReady(cases[3][1]).cursor, null)
  assert.equal(leadReady(cases[3][1]).provenance, null)
})

test('readiness: freshness without adapter-owned cursors is unknown, never numerically guessed', () => {
  for (const sinceCursor of [41, 42, 43, 100]) {
    const decision = leadReady(qualified({ kind: 'pi', sinceCursor }))
    assert.equal(decision.state, 'unknown', `unbranded cursor ${sinceCursor}`)
    assert.equal(decision.cursor, 42, 'the observed boundary is still reported')
  }
})

test('readiness: a first delivery has no freshness cursor to compare', () => {
  assert.equal(leadReady(qualified({ sinceCursor: undefined })).state, 'ready')
  assert.equal(leadReady(qualified({ sinceCursor: null })).state, 'ready')
})

test('readiness: a malformed sinceCursor is missing evidence, never a free pass', () => {
  for (const sinceCursor of [-1, 1.5, '42', Number.NaN, {}]) {
    const decision = leadReady(qualified({ sinceCursor }))
    assert.equal(decision.state, 'unknown', `sinceCursor ${JSON.stringify(sinceCursor)}`)
  }
})

// --- unreadable, replaced, and the two inert inputs --------------------------

test('readiness: unknown when the completion model could not read the session, with its reason', () => {
  const decision = leadReady(
    qualified({ answers: { unknown: true, reason: 'unreadable: not valid JSONL' } }),
  )
  assert.equal(decision.state, 'unknown')
  assert.equal(decision.reason, 'unreadable: not valid JSONL')
})

test('readiness: unknown when the completion model reports a session replaced in place', () => {
  const decision = leadReady(qualified({ answers: answers({ replaced: true }) }))
  assert.equal(decision.state, 'unknown')
  assert.match(decision.reason, /replaced/i)
})

test('readiness: unknown when there is no completion result at all, or its items are not a list', () => {
  for (const bad of [undefined, null, 'nope', {}, { items: 'two', inFlight: false }]) {
    const decision = leadReady(qualified({ answers: bad }))
    assert.equal(decision.state, 'unknown', `${JSON.stringify(bad)} is not a completion result`)
    assert.ok(decision.reason.length > 0)
  }
})

test('readiness: a periodically redrawing idle pane is ready — silence is not a condition', () => {
  const decision = leadReady(qualified({ paneIdleMs: 500, humanDraftMs: 0 }))
  assert.equal(decision.state, 'ready')
})

test('readiness: a long silence never makes a working lead ready — paneIdleMs is never read', () => {
  const decision = leadReady(
    qualified({
      paneIdleMs: 600_000,
      answers: answers({ settlement: settlement({ evidence: { openTools: ['bash-1'] } }) }),
    }),
  )
  assert.equal(decision.state, 'busy')
})

test('readiness: an abandoned draft is still a draft — a latch is never cleared by time', () => {
  const decision = leadReady(
    qualified({ draftLatched: true, paneIdleMs: 600_000, humanDraftMs: 3_600_000 }),
  )
  assert.equal(decision.state, 'draft')
})

test('readiness: cancellation is settled or not by the completion model, never by readiness', () => {
  const proven = leadReady(qualified({ answers: answers({ cancelled: true }) }))
  assert.equal(proven.state, 'ready')

  const unproven = leadReady(
    qualified({
      answers: answers({ cancelled: true, settlement: settlement({ state: 'unknown' }) }),
    }),
  )
  assert.equal(unproven.state, 'unknown')
})

test('readiness: called with nothing at all, it is unknown rather than ready', () => {
  assert.equal(leadReady().state, 'unknown')
})

// --- finding 5: the decision identity, not just its shape --------------------

test('readiness: every state echoes ITS OWN epoch, cursor and provenance', () => {
  // Fixed fixtures let a constant pass: returning 7 for every epoch and 42
  // for every cursor survived the whole suite once. These vary both and
  // deliberately pair a 7 with a cursor that is not 42, and a 42 with an
  // epoch that is not 7.
  const shapes = [
    ['ready', (proof) => qualified({ answers: answers({ settlement: proof }) })],
    [
      'draft',
      (proof) => qualified({ draftLatched: true, answers: answers({ settlement: proof }) }),
    ],
    [
      'busy',
      (proof) => qualified({ answers: answers({ settlement: { ...proof, state: 'in-flight' } }) }),
    ],
    [
      'unknown',
      (proof) => qualified({ answers: answers({ settlement: { ...proof, boundary: '' } }) }),
    ],
  ]
  const pairs = [
    [0, 0, 'native'],
    [3, 17, 'derived'],
    [7, 1234, 'native'],
    [19, 42, 'derived'],
    [100, 7, 'native'],
  ]
  for (const [state, build] of shapes) {
    for (const [epoch, cursor, provenance] of pairs) {
      const input = build(settlement({ cursor, provenance }))
      const decision = leadReady({ ...input, epoch })
      assert.equal(decision.state, state, `${state} @ epoch ${epoch} cursor ${cursor}`)
      assert.equal(decision.epoch, epoch, `${state} echoes epoch ${epoch}, not another`)
      assert.equal(decision.cursor, cursor, `${state} echoes cursor ${cursor}, not another`)
      assert.equal(decision.provenance, provenance, `${state} echoes provenance ${provenance}`)
    }
  }
})

test('readiness: an unreadable session still echoes the epoch it was judged at, with no cursor', () => {
  for (const epoch of [0, 5, 88]) {
    const decision = leadReady(
      qualified({ epoch, answers: { unknown: true, reason: 'unreadable: not valid JSONL' } }),
    )
    assert.equal(decision.epoch, epoch)
    assert.equal(decision.cursor, null)
    assert.equal(decision.provenance, null)
  }
})

test('readiness: an unknown freshness comparison still reports the observed settlement boundary', () => {
  const decision = leadReady(
    qualified({
      kind: 'pi',
      sinceCursor: 500,
      answers: answers({ settlement: settlement({ cursor: 88 }) }),
    }),
  )
  assert.equal(decision.state, 'unknown')
  assert.equal(
    decision.cursor,
    88,
    'the boundary we saw, never the cursor we were compared against',
  )
})
