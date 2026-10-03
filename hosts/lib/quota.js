/**
 * What a harness's own record says about its quota, in the dispatcher's
 * terms: `{state, usedPercent?, resetsAt?}`, or null when the record says
 * nothing. Codex reports its usage ahead of time; Claude Code, OpenCode, Pi
 * and Devin only say so once a request is refused (a 429, or a 402 for spent
 * credit), so for them the
 * daemon learns at the first refusal. OpenCode says it only in its window's
 * live status, never in its store, since it waits to retry the request.
 */

const LOW_PERCENT = 95
/** A reset's units by their first letter: "2 days", "3hr 4min", "35 minutes", "4h 30m". */
const UNIT_MS = { w: 604_800_000, d: 86_400_000, h: 3_600_000, m: 60_000, s: 1_000 }
const MONTHS = ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec']

/** "Resets in 3 days", "Resets in 3hr 4min": the time that names, from `atMs`; null otherwise. */
function resetIn(text, atMs) {
  const span = /resets?\s+in\s+((?:\d+\s*[a-z]+[\s,]*(?:and\s+)?)+)/i.exec(text)
  if (span === null) return null
  let ms = 0
  for (const [, count, unit] of span[1].matchAll(/(\d+)\s*([a-z]+)/gi)) {
    const each = UNIT_MS[unit[0].toLowerCase()]
    if (each === undefined) return null
    ms += Number(count) * each
  }
  return new Date(atMs + ms).toISOString()
}

/** The instant a wall clock in `timeZone` reads that time; Date.UTC's overflow carries a day past the month's end. */
function zonedTime(year, month, day, hour, minute, timeZone) {
  const wanted = Date.UTC(year, month, day, hour, minute)
  const shown = new Intl.DateTimeFormat('en-US', {
    timeZone,
    hourCycle: 'h23',
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
  })
  let instant = wanted
  // Twice: once for the zone's offset, once more if that guess crossed a change of it.
  for (let pass = 0; pass < 2; pass += 1) {
    const part = Object.fromEntries(
      shown.formatToParts(instant).map(({ type, value }) => [type, Number(value)]),
    )
    instant += wanted - Date.UTC(part.year, part.month - 1, part.day, part.hour, part.minute)
  }
  return instant
}

/**
 * Claude's own words, "resets 7:30pm (Europe/Bucharest)" or "resets Sep 29
 * at 11am (Europe/Bucharest)": the next time that wall clock reads so, after
 * `atMs`, in the zone it names (this machine's when it names none); null
 * when the words are not of that shape or the zone is unknown.
 */
function resetAt(text, atMs) {
  const match =
    /resets?\s+(?:([a-z]{3})[a-z]*\s+(\d{1,2})\s+at\s+)?(\d{1,2})(?::(\d{2}))?\s*([ap]m)\b(?:\s*\(([^)]+)\))?/i.exec(
      text,
    )
  if (match === null) return null
  const [, monthName, dayOfMonth, hourText, minuteText, meridiem, zone] = match
  const hour = (Number(hourText) % 12) + (meridiem.toLowerCase() === 'pm' ? 12 : 0)
  const minute = Number(minuteText ?? 0)
  try {
    const timeZone = zone ?? Intl.DateTimeFormat().resolvedOptions().timeZone
    const today = Object.fromEntries(
      new Intl.DateTimeFormat('en-US', { timeZone, year: 'numeric', month: 'numeric', day: 'numeric' })
        .formatToParts(atMs)
        .map(({ type, value }) => [type, Number(value)]),
    )
    if (monthName !== undefined) {
      const month = MONTHS.indexOf(monthName.toLowerCase())
      if (month === -1) return null
      let at = zonedTime(today.year, month, Number(dayOfMonth), hour, minute, timeZone)
      // A date gone by more than a day is next year's.
      if (at < atMs - UNIT_MS.d) at = zonedTime(today.year + 1, month, Number(dayOfMonth), hour, minute, timeZone)
      return new Date(at).toISOString()
    }
    let at = zonedTime(today.year, today.month - 1, today.day, hour, minute, timeZone)
    if (at <= atMs) at = zonedTime(today.year, today.month - 1, today.day + 1, hour, minute, timeZone)
    return new Date(at).toISOString()
  } catch {
    return null
  }
}

/** When a refusal says the quota comes back, from `atMs`: in a span, or at a time; null when it does not say. */
function namedReset(text, atMs) {
  if (!Number.isFinite(atMs)) return null
  const words = String(text ?? '')
  return resetIn(words, atMs) ?? resetAt(words, atMs)
}

/**
 * The statuses that mean an account takes no more for now: a rate or usage
 * limit (429), or its credit spent (402, OpenRouter's "requires more
 * credits").
 */
const QUOTA_STATUSES = new Set([402, 429])

/** Whether a provider's status is a refusal for quota. */
export const quotaStatus = (status) => QUOTA_STATUSES.has(Number(status))

/**
 * Whether a provider's error text opens on a quota status: "429: …", or
 * "OpenAI API error (429): …", Pi's two shapes (seen on Pi, 2026-09).
 */
export function refusedForQuota(text) {
  const match = /^(?:[^(:\n]*\()?(\d{3})\b/.exec(String(text ?? ''))
  return match !== null && quotaStatus(match[1])
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
    resetsAt: namedReset(text, atMs),
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

/** OpenCode's words for a limit it waits out, in a retry's message or reason. */
const OPENCODE_LIMIT = /limit|usage|quota|too many requests/i
/** OpenCode's own name for a spent free tier, in a retry's action. */
const OPENCODE_SPENT = /free_tier_limit|quota|usage/i
/** A retry sooner than this is backoff, not the limit's reset. */
const BACKOFF_MS = 60_000

/**
 * OpenCode waiting to retry a refused request (`{type: 'retry', message,
 * action, next}`, probed on OpenCode 1.18.31): a spent quota when its action
 * names one (`free_tier_limit`), or when the words say a limit and the retry
 * is due only at a reset a minute or more away. A retry due in seconds is
 * backoff on a rate limit or an overloaded provider: the window is working,
 * and taking its task away would waste what it did. Anything else is not a
 * quota.
 */
export function opencodeRetryQuota(status, nowMs) {
  if (status?.type !== 'retry') return null
  const next = Number(status.next)
  const soon = Number.isFinite(next) && next - nowMs < BACKOFF_MS
  const spent = OPENCODE_SPENT.test(status.action?.reason ?? '')
  const words = OPENCODE_LIMIT.test(`${status.message ?? ''} ${status.action?.reason ?? ''}`)
  if (!spent && (!words || soon)) return null
  return {
    state: 'exhausted',
    at: null,
    resetsAt: Number.isFinite(next) && !soon ? new Date(next).toISOString() : null,
  }
}

/** Devin's words for a refusal in its own messages. */
export const DEVIN_REFUSAL = /rate limit|usage limit|quota exhausted/i
