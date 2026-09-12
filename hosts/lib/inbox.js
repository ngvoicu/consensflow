import { digest, envelope, partsFor } from './deliveries.js'

const CLAIM_MS = 10_000
const map = () => Object.create(null)
const text = (value, name) => {
  if (typeof value !== 'string' || !value.length) throw new Error(`${name} is required`)
  return value
}
const time = (value) => {
  if (!Number.isFinite(value)) throw new Error('time is required')
  return value
}

export const emptyInbox = () => ({ version: 1, results: map(), receivers: map() })

/** Logical answer identity never depends on the current destination conversation. */
export function indexResult(state, input) {
  const source = ['owner', 'conversation', 'kind', 'session', 'answerId'].map((key) =>
    text(input[key], key),
  )
  const key = JSON.stringify(source)
  if (typeof input.answer !== 'string') throw new Error('answer must be text')
  const answer = input.answer.toWellFormed()
  const existing = Object.values(state.results).find((result) => result.key === key)
  if (existing) {
    if (existing.answer !== answer) throw new Error('result body is immutable')
    return existing
  }
  if (Object.hasOwn(state.results, input.id)) throw new Error('result id already exists')
  const result = {
    id: input.id,
    key,
    owner: input.owner,
    conversation: input.conversation,
    agent: text(input.agent, 'agent'),
    kind: input.kind,
    session: input.session,
    answerId: input.answerId,
    answer,
    createdAt: time(input.now),
    claims: [],
  }
  const body = envelope(result)
  result.digest = digest(body)
  // Below Claude's hard 10,000-character hook persistence threshold, framing included.
  result.parts = partsFor(body, result.id, input.budget ?? { bytes: 8_000, lines: 900 })
  state.results[result.id] = result
  return result
}

/** Registration is a compare-and-swap; its launch authority is checked by the service. */
export function registerReceiver(state, input) {
  for (const key of ['owner', 'launch', 'pane', 'kind', 'session', 'lease']) text(input[key], key)
  if (!Number.isSafeInteger(input.generation) || input.generation < 1)
    throw new Error('receiver generation is required')
  const old = state.receivers[input.owner]
  if (
    old &&
    old.retiredAt === undefined &&
    ['launch', 'pane', 'generation', 'kind', 'session'].every((key) => old[key] === input[key])
  ) {
    old.observedAt = time(input.now)
    return structuredClone(old)
  }
  // Retrying an identical registration after a lost response is idempotent.
  // Changing native selection still requires the current lease.
  if (old && input.previous !== old.lease) throw new Error('receiver changed')
  if (old?.lease === input.lease) throw new Error('receiver lease must change')
  const receiver = {
    owner: input.owner,
    launch: input.launch,
    pane: input.pane,
    generation: input.generation,
    kind: input.kind,
    session: input.session,
    lease: input.lease,
    revision: (old?.revision ?? 0) + 1,
    observedAt: time(input.now),
  }
  state.receivers[input.owner] = receiver
  for (const result of Object.values(state.results)) {
    if (result.owner !== input.owner) continue
    for (const claim of result.claims) {
      if (claim.receiver.lease === receiver.lease) continue
      if (claim.state === 'claimed') claim.state = 'released'
      if (claim.state === 'submitting') claim.state = 'uncertain'
    }
  }
  return structuredClone(receiver)
}

function currentReceiver(state, owner, lease) {
  const receiver = state.receivers[owner]
  if (!receiver || receiver.lease !== lease || receiver.retiredAt !== undefined)
    throw new Error('receiver changed')
  return receiver
}

export function retireReceiver(state, { owner, lease, now }) {
  const receiver = currentReceiver(state, owner, lease)
  receiver.retiredAt = time(now)
  for (const result of Object.values(state.results)) {
    if (result.owner !== owner) continue
    for (const claim of result.claims) {
      if (claim.receiver.lease !== lease) continue
      if (claim.state === 'claimed') claim.state = 'released'
      if (claim.state === 'submitting') claim.state = 'uncertain'
    }
  }
  return structuredClone(receiver)
}

function coverage(result, lease) {
  return new Set(
    result.claims
      .filter((claim) => claim.state === 'received' && claim.receiver.lease === lease)
      .map((claim) => claim.part),
  )
}

export function resultStatus(result) {
  if (
    result.legacyReceived === true ||
    result.claims.some(
      (claim) => coverage(result, claim.receiver.lease).size === result.parts.length,
    )
  )
    return 'received'
  if (result.legacyUncertain === true || result.claims.some((claim) => claim.state === 'uncertain'))
    return 'uncertain'
  if (result.claims.some((claim) => ['claimed', 'submitting'].includes(claim.state)))
    return 'collecting'
  return result.cancelledAt === undefined ? 'waiting' : 'cancelled'
}

function claimText(result, claim) {
  return (
    `[consensflow receiver ${claim.id}]\n${result.parts[claim.part - 1].text}` +
    `[end of receiver ${claim.id}]\n`
  )
}

/** Serialized by Store. Only never-started claims can expire into retryable work. */
export function claimNext(state, { owner, lease, id, now, eligible = () => true, resultId } = {}) {
  const receiver = currentReceiver(state, owner, lease)
  time(now)
  if (typeof id !== 'string' || !/^[A-Za-z0-9_-]{1,100}$/.test(id))
    throw new Error('invalid claim id')
  const results = Object.values(state.results).sort((a, b) => a.createdAt - b.createdAt)
  if (results.some((result) => result.claims.some((claim) => claim.id === id)))
    throw new Error('claim id already exists')
  for (const result of results) {
    if (result.owner !== owner || (resultId !== undefined && result.id !== resultId)) continue
    for (const claim of result.claims) {
      if (claim.state === 'claimed' && now >= claim.expiresAt) claim.state = 'released'
    }
    if (resultStatus(result) !== 'waiting' || !eligible(result)) continue
    const received = coverage(result, lease)
    const part = result.parts.find((part) => !received.has(part.k))
    if (!part) continue
    const claim = {
      id,
      result: result.id,
      part: part.k,
      receiver: structuredClone(receiver),
      state: 'claimed',
      fetchedAt: now,
      expiresAt: now + CLAIM_MS,
    }
    result.claims.push(claim)
    return {
      ...structuredClone(claim),
      text: claimText(result, claim),
      totalParts: result.parts.length,
    }
  }
  return null
}

function findClaim(state, input) {
  const result = state.results[input.result]
  const claim = result?.claims.find((claim) => claim.id === input.claim)
  if (!claim) throw new Error('unknown result claim')
  return { result, claim }
}

/** Explicit tool reads may recover an uncertain answer; they never create another result. */
export function claimForRead(state, input) {
  const receiver = currentReceiver(state, input.owner, input.lease)
  const result = state.results[input.result]
  if (result?.owner !== input.owner) throw new Error('result does not belong to receiver')
  if (!Number.isSafeInteger(input.part) || !result.parts[input.part - 1])
    throw new Error('invalid result part')
  if (
    typeof input.id !== 'string' ||
    !/^[A-Za-z0-9_-]{1,100}$/.test(input.id) ||
    result.claims.some((claim) => claim.id === input.id)
  )
    throw new Error('invalid claim id')
  const claim = {
    id: input.id,
    result: result.id,
    part: input.part,
    receiver: structuredClone(receiver),
    state: 'submitting',
    manual: true,
    fetchedAt: time(input.now),
    startedAt: input.now,
  }
  result.claims.push(claim)
  return { ...structuredClone(claim), text: claimText(result, claim) }
}

export function beginInsertion(state, input) {
  currentReceiver(state, input.owner, input.lease)
  const { result, claim } = findClaim(state, input)
  if (result.owner !== input.owner || claim.receiver.lease !== input.lease)
    throw new Error('receiver changed')
  if (claim.state !== 'claimed' || time(input.now) >= claim.expiresAt)
    throw new Error('claim is no longer available for insertion')
  claim.state = 'submitting'
  claim.startedAt = input.now
  return structuredClone(claim)
}

export function releaseClaim(state, input) {
  const { claim } = findClaim(state, input)
  if (['received', 'released'].includes(claim.state)) return
  claim.state =
    claim.state === 'claimed' || (input.admitted === false && input.bytesWritten === 0)
      ? 'released'
      : 'uncertain'
  claim.finishedAt = time(input.now)
  if (typeof input.reason === 'string') claim.reason = input.reason
}

/** Native evidence may complete a retired claim, but never adopts a new receiver. */
export function observeReceipt(state, input) {
  const { result, claim } = findClaim(state, input)
  if (
    !['submitting', 'uncertain'].includes(claim.state) ||
    input.session !== claim.receiver.session ||
    input.kind !== claim.receiver.kind
  )
    return false
  const expected = claimText(result, claim)
  const evidence = input.items?.find(
    (item) =>
      typeof item.id === 'string' &&
      ['user', 'tool', 'custom'].includes(item.role) &&
      typeof item.text === 'string' &&
      item.text.includes(expected),
  )
  if (!evidence) return false
  claim.state = 'received'
  claim.receivedAt = time(input.now)
  claim.evidence = { id: evidence.id, role: evidence.role, digest: digest(expected) }
  return true
}
