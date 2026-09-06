import assert from 'node:assert/strict'
import test from 'node:test'
import { effectivePolicy, leadMaySet, recordLeadPreference } from '../../hosts/lib/policy.js'

/**
 * Phase 3, TEST-PANE-27: the effective delivery policy for one pane, and who
 * it came from. Precedence, from the Decision Log (2026-09-06): tab human
 * `manual` veto > pane human setting > tab human `auto` > lead preference >
 * default `auto`. `inherit` at pane scope falls through. The result names its
 * source because the pane title shows it (`name · @agent · policy (source)`).
 *
 * The human's settings live on the tab and the pane — `src/store.js`
 * `policySet` writes only those, from the page only. The lead's own
 * preference lives on the conversation row as `notifyPreference`, and
 * `cf run --notify` may write nothing else.
 */

const UNSET = undefined

/**
 * Every tab × pane × lead combination, with the answer written out as a
 * literal. Deliberately NOT computed from a second precedence walk: a walk
 * that misunderstands the rule the same way the module does would pass.
 */
const TABLE = [
  // tab unset, pane unset — the lead is heard, then the default
  [UNSET, UNSET, UNSET, 'auto', 'default'],
  [UNSET, UNSET, 'auto', 'auto', 'lead'],
  [UNSET, UNSET, 'manual', 'manual', 'lead'],
  // tab unset, pane inherit — falls through exactly like unset
  [UNSET, 'inherit', UNSET, 'auto', 'default'],
  [UNSET, 'inherit', 'auto', 'auto', 'lead'],
  [UNSET, 'inherit', 'manual', 'manual', 'lead'],
  // tab unset, pane set — the human at pane scope beats the lead
  [UNSET, 'auto', UNSET, 'auto', 'pane-human'],
  [UNSET, 'auto', 'auto', 'auto', 'pane-human'],
  [UNSET, 'auto', 'manual', 'auto', 'pane-human'],
  [UNSET, 'manual', UNSET, 'manual', 'pane-human'],
  [UNSET, 'manual', 'auto', 'manual', 'pane-human'],
  [UNSET, 'manual', 'manual', 'manual', 'pane-human'],
  // tab auto — beats the lead, loses to a pane setting
  ['auto', UNSET, UNSET, 'auto', 'tab-human'],
  ['auto', UNSET, 'auto', 'auto', 'tab-human'],
  ['auto', UNSET, 'manual', 'auto', 'tab-human'],
  ['auto', 'inherit', UNSET, 'auto', 'tab-human'],
  ['auto', 'inherit', 'auto', 'auto', 'tab-human'],
  ['auto', 'inherit', 'manual', 'auto', 'tab-human'],
  ['auto', 'auto', UNSET, 'auto', 'pane-human'],
  ['auto', 'auto', 'auto', 'auto', 'pane-human'],
  ['auto', 'auto', 'manual', 'auto', 'pane-human'],
  ['auto', 'manual', UNSET, 'manual', 'pane-human'],
  ['auto', 'manual', 'auto', 'manual', 'pane-human'],
  ['auto', 'manual', 'manual', 'manual', 'pane-human'],
  // tab manual — the veto: nothing under it is heard
  ['manual', UNSET, UNSET, 'manual', 'tab-human'],
  ['manual', UNSET, 'auto', 'manual', 'tab-human'],
  ['manual', UNSET, 'manual', 'manual', 'tab-human'],
  ['manual', 'inherit', UNSET, 'manual', 'tab-human'],
  ['manual', 'inherit', 'auto', 'manual', 'tab-human'],
  ['manual', 'inherit', 'manual', 'manual', 'tab-human'],
  ['manual', 'auto', UNSET, 'manual', 'tab-human'],
  ['manual', 'auto', 'auto', 'manual', 'tab-human'],
  ['manual', 'auto', 'manual', 'manual', 'tab-human'],
  ['manual', 'manual', UNSET, 'manual', 'tab-human'],
  ['manual', 'manual', 'auto', 'manual', 'tab-human'],
  ['manual', 'manual', 'manual', 'manual', 'tab-human'],
]

test('policy: every tab x pane x lead combination resolves to its documented mode and source', () => {
  assert.equal(TABLE.length, 36, 'three tab values x four pane values x three lead values')
  for (const [tabPolicy, panePolicy, lead, mode, source] of TABLE) {
    const actual = effectivePolicy(
      { id: 't-1', policy: tabPolicy },
      { id: 'p-2', policy: panePolicy },
      { notifyPreference: lead },
    )
    assert.deepEqual(actual, { mode, source }, `tab=${tabPolicy} pane=${panePolicy} lead=${lead}`)
  }
})

test('policy: a tab human manual vetoes a pane set to auto and a lead asking for auto', () => {
  const actual = effectivePolicy(
    { policy: 'manual' },
    { policy: 'auto' },
    { notifyPreference: 'auto' },
  )
  assert.deepEqual(actual, { mode: 'manual', source: 'tab-human' })
})

test('policy: the pane human setting beats a tab auto and the lead', () => {
  const actual = effectivePolicy(
    { policy: 'auto' },
    { policy: 'manual' },
    { notifyPreference: 'auto' },
  )
  assert.deepEqual(actual, { mode: 'manual', source: 'pane-human' })
})

test('policy: inherit at pane scope falls through to the tab', () => {
  assert.deepEqual(effectivePolicy({ policy: 'auto' }, { policy: 'inherit' }, {}), {
    mode: 'auto',
    source: 'tab-human',
  })
})

test('policy: inherit falls through past an unset tab to the lead, and then to the default', () => {
  assert.deepEqual(effectivePolicy({}, { policy: 'inherit' }, { notifyPreference: 'manual' }), {
    mode: 'manual',
    source: 'lead',
  })
  assert.deepEqual(effectivePolicy({}, { policy: 'inherit' }, {}), {
    mode: 'auto',
    source: 'default',
  })
})

test('policy: the default is auto when nobody said anything, including with no arguments', () => {
  assert.deepEqual(effectivePolicy(), { mode: 'auto', source: 'default' })
  assert.deepEqual(effectivePolicy(null, null, null), { mode: 'auto', source: 'default' })
})

test('policy: all four sources are reachable, so the pane title can always name one', () => {
  const sources = new Set(
    TABLE.map(
      ([tabPolicy, panePolicy, lead]) =>
        effectivePolicy({ policy: tabPolicy }, { policy: panePolicy }, { notifyPreference: lead })
          .source,
    ),
  )
  assert.deepEqual([...sources].sort(), ['default', 'lead', 'pane-human', 'tab-human'])
})

test('policy: anything that is not exactly auto or manual is unset at every scope', () => {
  // A value the store would never write must not be mistaken for a setting;
  // `inherit` at TAB scope is not in the tab vocabulary and falls through too.
  for (const junk of [null, '', 'AUTO', 'Manual', 'sometimes', 'inherit', 0, true, {}]) {
    assert.deepEqual(
      effectivePolicy({ policy: junk }, { policy: junk }, { notifyPreference: junk }),
      { mode: 'auto', source: 'default' },
      `${JSON.stringify(junk)} is not a policy`,
    )
  }
})

test('policy: the lead is heard only through notifyPreference — never through a human field', () => {
  // `cf run --notify` writes `notifyPreference`. A row that also carries
  // human-looking fields (a stale shape, or a lead that tried) changes
  // nothing: those scopes live on the tab and the pane.
  const row = {
    notifyPreference: 'auto',
    policy: 'manual',
    notify: 'manual',
    notifySetBy: 'human',
  }
  assert.deepEqual(effectivePolicy({}, {}, row), { mode: 'auto', source: 'lead' })
  assert.deepEqual(effectivePolicy({}, {}, { policy: 'manual', notify: 'manual' }), {
    mode: 'auto',
    source: 'default',
  })
})

test('policy: deciding never mutates the tab, the pane or the row', () => {
  const tab = Object.freeze({ id: 't-1', policy: 'auto' })
  const pane = Object.freeze({ id: 'p-2', policy: 'inherit' })
  const row = Object.freeze({ notifyPreference: 'manual' })
  assert.deepEqual(effectivePolicy(tab, pane, row), { mode: 'auto', source: 'tab-human' })
})

test('policy: leadMaySet lets cf run --notify write notifyPreference and name nothing else', () => {
  assert.deepEqual(leadMaySet({ notifyPreference: 'auto' }), {
    allowed: true,
    field: 'notifyPreference',
  })
  assert.deepEqual(leadMaySet({}), { allowed: true, field: 'notifyPreference' })
})

test('policy: leadMaySet names notifyPreference even on a row carrying human fields', () => {
  const decision = leadMaySet({ policy: 'manual', notify: 'auto', notifySetBy: 'human' })
  assert.equal(decision.allowed, true)
  assert.equal(decision.field, 'notifyPreference')
  assert.equal(decision.field === 'policy', false, 'the human scopes are the tab and the pane')
})

test('policy: leadMaySet refuses when there is no conversation row to record a preference on', () => {
  for (const missing of [undefined, null, 'nyx-coral-lane', 7]) {
    const decision = leadMaySet(missing)
    assert.equal(decision.allowed, false, `${JSON.stringify(missing)} is not a row`)
    assert.equal(decision.field, null)
    assert.ok(decision.reason.length > 0)
  }
})

// --- finding 6: exercise the write, not just the permission ------------------

/** Which fields actually differ between two records. */
function changedKeys(before, after) {
  const keys = new Set([...Object.keys(before), ...Object.keys(after)])
  return [...keys].filter((key) => before[key] !== after[key]).sort()
}

test('policy: cf run --notify writes notifyPreference and nothing else on the row', () => {
  const row = Object.freeze({ name: 'nyx-coral-lane', agent: 'nyx', notifyPreference: 'auto' })
  const written = recordLeadPreference(row, 'manual')

  assert.equal(written.ok, true)
  assert.equal(written.field, 'notifyPreference')
  assert.equal(written.row.notifyPreference, 'manual')
  assert.deepEqual(changedKeys(row, written.row), ['notifyPreference'])
  assert.equal(row.notifyPreference, 'auto', 'the row handed in is never mutated')
})

test('policy: the write preserves the tab and pane policy the human set', () => {
  const tab = Object.freeze({ id: 't-1', policy: 'auto' })
  const pane = Object.freeze({ id: 'p-2', policy: 'manual' })
  const row = Object.freeze({ notifyPreference: 'auto' })

  const written = recordLeadPreference(row, 'manual')
  assert.equal(written.ok, true)
  assert.deepEqual(tab, { id: 't-1', policy: 'auto' }, 'tab scope untouched')
  assert.deepEqual(pane, { id: 'p-2', policy: 'manual' }, 'pane scope untouched')

  // And the human still wins where precedence says so.
  assert.deepEqual(effectivePolicy(tab, pane, written.row), {
    mode: 'manual',
    source: 'pane-human',
  })
  assert.deepEqual(effectivePolicy({ policy: 'manual' }, {}, written.row), {
    mode: 'manual',
    source: 'tab-human',
  })
  assert.deepEqual(effectivePolicy({ policy: 'auto' }, {}, written.row), {
    mode: 'auto',
    source: 'tab-human',
  })
  // Only where nobody human spoke is the lead heard.
  assert.deepEqual(effectivePolicy({}, {}, written.row), { mode: 'manual', source: 'lead' })
})

test('policy: the write never touches a human-looking field the row happens to carry', () => {
  const decoy = Object.freeze({ policy: 'manual', notify: 'manual', notifySetBy: 'human' })
  const written = recordLeadPreference(decoy, 'auto')

  assert.equal(written.ok, true)
  assert.deepEqual(changedKeys(decoy, written.row), ['notifyPreference'])
  assert.equal(written.row.policy, 'manual')
  assert.equal(written.row.notify, 'manual')
  assert.equal(written.row.notifySetBy, 'human')
})

test('policy: only auto and manual may be recorded — everything else is refused unchanged', () => {
  const row = Object.freeze({ notifyPreference: 'auto' })
  for (const value of [undefined, null, '', 'inherit', 'AUTO', 'sometimes', 0, true]) {
    const written = recordLeadPreference(row, value)
    assert.equal(written.ok, false, `${JSON.stringify(value)} is not a preference`)
    assert.equal(written.row.notifyPreference, 'auto', 'a refused write changes nothing')
    assert.ok(written.reason.length > 0)
  }
})

test('policy: recording the value already there is allowed and changes nothing', () => {
  const row = Object.freeze({ notifyPreference: 'auto', name: 'nyx-coral-lane' })
  const written = recordLeadPreference(row, 'auto')
  assert.equal(written.ok, true)
  assert.deepEqual(changedKeys(row, written.row), [])
})

test('policy: with no conversation row there is nothing to record a preference on', () => {
  for (const missing of [undefined, null, 'nyx-coral-lane', 7]) {
    const written = recordLeadPreference(missing, 'manual')
    assert.equal(written.ok, false, `${JSON.stringify(missing)} is not a row`)
    assert.equal(written.row, null)
    assert.ok(written.reason.length > 0)
  }
})
