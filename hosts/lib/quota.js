/**
 * What a harness's own record says about its quota, in the dispatcher's
 * terms: `{state, usedPercent?, resetsAt?}`, or null when the record says
 * nothing. Codex reports its usage ahead of time; Claude Code, OpenCode, Pi
 * and Devin only say so once a request is refused (a 429), so for them the
 * daemon learns at the first refusal.
 */

const LOW_PERCENT = 95
const UNITS = { second: 1_000, minute: 60_000, hour: 3_600_000, day: 86_400_000 }

/** "Resets in 3 days", "reset in 35 minutes": the time that names, from `atMs`; null otherwise. */
function relativeReset(text, atMs) {
  const match = /resets?\s+in\s+(\d+)\s*(second|minute|hour|day)s?/i.exec(String(text ?? ''))
  if (match === null || !Number.isFinite(atMs)) return null
  return new Date(atMs + Number(match[1]) * UNITS[match[2].toLowerCase()]).toISOString()
}

/**
 * A refused request: exhausted, with the reset the text names when it names
 * one, and when it happened, so the daemon can tell an old refusal still in
 * the record from a new one.
 */
export function exhaustedQuota(text, atMs) {
  return {
    state: 'exhausted',
    at: Number.isFinite(atMs) ? new Date(atMs).toISOString() : null,
    resetsAt: relativeReset(text, atMs),
  }
}

/** Codex's `rate_limits` on a `token_count` event: the fullest window decides. */
export function codexQuota(limits) {
  const windows = [limits?.primary, limits?.secondary].filter(
    (window) => typeof window?.used_percent === 'number',
  )
  const fullest = windows.sort((a, b) => b.used_percent - a.used_percent)[0] ?? null
  const reached = limits?.rate_limit_reached_type != null
  const usedPercent = fullest?.used_percent ?? null
  return {
    state: reached ? 'exhausted' : usedPercent !== null && usedPercent >= LOW_PERCENT ? 'low' : 'ok',
    usedPercent,
    resetsAt:
      typeof fullest?.resets_at === 'number' ? new Date(fullest.resets_at * 1000).toISOString() : null,
  }
}

/** Devin's words for a refusal in its own messages. */
export const DEVIN_REFUSAL = /rate limit|usage limit|quota exhausted/i
