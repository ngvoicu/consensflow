import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { exhaustedQuota, quotaStatus, refusedForQuota } from '../hosts/lib/quota.js'

/**
 * What a harness's refusal says about its quota (`hosts/lib/quota.js`), on
 * the words harnesses really wrote: Claude Code's limits, Pi's provider
 * errors and OpenRouter's spent credit, as this machine's records held them
 * (2026-09/10).
 */
const at = Date.parse('2026-10-03T12:00:00.000Z')
const resets = (text, when = at) => exhaustedQuota(text, when).resetsAt

describe('a quota refusal', () => {
  it("reads Claude's reset at a time of day, in the zone it names, as the next time it comes", () => {
    // 15:00 in Bucharest, summer time (UTC+3).
    assert.equal(
      resets("You've hit your session limit · resets 7:30pm (Europe/Bucharest)"),
      '2026-10-03T16:30:00.000Z',
    )
    assert.equal(
      resets("You've hit your session limit · resets 1:30am (Europe/Bucharest)"),
      '2026-10-03T22:30:00.000Z',
      'past for today: tomorrow',
    )
    assert.equal(
      resets("You've hit your weekly limit · resets 11am (Europe/Bucharest)"),
      '2026-10-04T08:00:00.000Z',
    )
  })

  it("reads Claude's reset on a date, across a change of the zone's offset", () => {
    assert.equal(
      resets(
        "You've hit your weekly limit · resets Sep 29 at 11am (Europe/Bucharest)",
        Date.parse('2026-09-26T12:00:00.000Z'),
      ),
      '2026-09-29T08:00:00.000Z',
    )
    // Summer time ends on October 25: November is UTC+2.
    assert.equal(
      resets("You've hit your weekly limit · resets Nov 2 at 9am (Europe/Bucharest)"),
      '2026-11-02T07:00:00.000Z',
    )
  })

  it('reads a span, in any of the ways providers write one', () => {
    const hours = (n) => new Date(at + n * 3_600_000).toISOString()
    assert.equal(
      resets(
        '429: {"type":"GoUsageLimitError","message":"5-hour usage limit reached. Resets in 3hr 4min. To continue using this model, upgrade."}',
      ),
      new Date(at + (3 * 60 + 4) * 60_000).toISOString(),
    )
    assert.equal(resets('Weekly usage limit reached. Resets in 2 days.'), hours(48))
    assert.equal(resets("You've hit your limit. Resets in 2 hours."), hours(2))
    assert.equal(
      resets('Rate limit … reset in 35 minutes'),
      new Date(at + 35 * 60_000).toISOString(),
    )
  })

  it('names no reset where the words name none, or a zone nobody knows', () => {
    assert.equal(
      resets("You're out of usage credits. Run /usage-credits to keep using Fable 5.1."),
      null,
    )
    assert.equal(resets('resets 7pm (Mars/Olympus_Mons)'), null)
    assert.deepEqual(exhaustedQuota('limit', Number.NaN), {
      state: 'exhausted',
      at: null,
      resetsAt: null,
    })
  })

  it("tells a provider's quota refusal, a 429 or spent credit, from its other errors", () => {
    for (const text of [
      '429: {"message":"Provider returned error","code":429}',
      'OpenAI API error (429): {"type":"GoUsageLimitError","message":"Weekly usage limit reached. Resets in 2 days."}',
      'OpenAI API error (429): {"code":"rate_limit_exceeded","type":"rate_limit_error"}',
      '402: {"message":"This request requires more credits, or fewer max_tokens."}',
    ]) {
      assert.equal(refusedForQuota(text), true, text)
    }
    for (const text of [
      '500: provider down',
      '400: {"type":"server_error","message":"Upstream request failed"}',
      'OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401)',
      'Provider returned error',
      'Stream ended without finish_reason',
    ]) {
      assert.equal(refusedForQuota(text), false, text)
    }
    assert.deepEqual([quotaStatus(429), quotaStatus('402'), quotaStatus(503)], [true, true, false])
  })
})
