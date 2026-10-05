/**
 * A value the engine handed one of its seams, or was handed, as a trace
 * writes it: as JSON holds it, what JSON cannot hold tagged, and the seams
 * themselves by name (`known`), never their insides.
 */

/** The seams' objects, each by the name a trace gives it ("$host", "$ledger"…). */
export const known = new WeakMap()

/** `value` as JSON holds it. */
export function encode(value, seen = new WeakSet()) {
  if (value === undefined) return { $undefined: true }
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value
  if (typeof value === 'number') return Number.isFinite(value) ? value : { $number: String(value) }
  if (typeof value === 'bigint') return { $bigint: String(value) }
  if (typeof value === 'function') return known.get(value) ?? { $fn: value.name || true }
  if (typeof value !== 'object') return { $other: String(value) }
  if (known.has(value)) return known.get(value)
  if (value instanceof Error) return failure(value)
  if (value instanceof Date) return { $date: value.toISOString() }
  if (seen.has(value)) return { $cycle: true }
  seen.add(value)
  try {
    if (value instanceof Promise) return { $promise: true }
    if (value instanceof Set) return { $set: [...value].map((item) => encode(item, seen)) }
    if (value instanceof Map) {
      return { $map: [...value].map(([key, item]) => [encode(key, seen), encode(item, seen)]) }
    }
    if (Array.isArray(value)) return value.map((item) => encode(item, seen))
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, encode(item, seen)]))
  } finally {
    seen.delete(value)
  }
}

/** A failure as a trace writes it: what a refusal says of itself. */
export function failure(cause) {
  return {
    $error: {
      name: cause?.name ?? null,
      code: cause?.code ?? null,
      status: cause?.status ?? null,
      message: String(cause?.message ?? cause),
    },
  }
}
