/**
 * One HTTP exchange with the API, written down where the server sees it: the
 * request as it arrived (its method, target, `Authorization` and body bytes),
 * the answer as it left (status, content type, body bytes), the kicks the
 * handler made, what its ledger calls drew and logged, and the calls it made
 * on the test's stand-ins. Nothing is put in front of the server: the
 * request's chunks are copied as whoever reads them takes them, and the
 * response's methods are wrapped, so the server answers as it would.
 */
import * as session from './session.mjs'
import { wireOf } from './wire.mjs'

/** An answer with these statuses has no body on the wire, whatever the handler wrote. */
const BODYLESS = new Set([204, 205, 304])

/** Bytes as a run's streams are held: their text (empty for none), or `{ base64 }` when they are not UTF-8. */
export function text(chunks) {
  const all = Buffer.concat(chunks)
  const read = all.toString('utf8')
  return Buffer.from(read, 'utf8').equals(all) ? read : { base64: all.toString('base64') }
}

/** Bytes as a request or an answer holds its body: `body`, null for none, or `bodyBase64` when they are not UTF-8. */
function bytes(chunks) {
  if (Buffer.concat(chunks).length === 0) return { body: null }
  const body = text(chunks)
  return typeof body === 'string' ? { body } : { bodyBase64: body.base64 }
}

/** The header `name` in a `writeHead` call's arguments. */
function headerOf(args, name) {
  for (const arg of args) {
    if (arg === null || typeof arg !== 'object' || Array.isArray(arg)) continue
    for (const [key, value] of Object.entries(arg))
      if (key.toLowerCase() === name) return String(value)
  }
  return null
}

/** The `cf` run an exchange of a `cf` client belongs to: the one in flight with that token, or the only one. */
function runOf(request) {
  const token = (request.headers.authorization ?? '').replace(/^Bearer /, '')
  const flying = session.runsInFlight()
  const own = flying.find((interval) => interval.step.env?.CONSENSFLOW_TOKEN === token)
  return (own ?? (flying.length === 1 ? flying[0] : undefined))?.step.id
}

/**
 * An exchange begun: the request is read and the frame the handler runs in is
 * made, or null when no test is recording. The exchange is a step where it
 * began; one that other steps overlapped is marked when it ends.
 */
export function watch(request, reply) {
  if (session.recording() === null) return null
  const world = session.worldBefore()
  const id = session.next('exchange')
  const wire = wireOf(request)
  const client = (request.headers['user-agent'] ?? '').startsWith('ureq/') ? 'cf' : 'test'
  const run = client === 'cf' ? runOf(request) : undefined
  const step = {
    kind: 'exchange',
    id,
    client,
    ...(run === undefined ? {} : { run }),
    request: {
      method: request.method,
      target: request.url,
      authorization: request.headers.authorization ?? null,
      contentType: request.headers['content-type'] ?? null,
    },
    response: null,
    kicks: 0,
    clock: [],
    names: [],
    events: [],
    seams: [],
  }
  const interval = session.begin(step, (other) => run !== undefined && other.run === run)
  // The body is what the connection carried, read when the test ends: a client may still be sending.
  session.later(() => {
    wire?.wire.end()
    const sent = wire?.wire.bodies[wire.index]
    Object.assign(
      step.request,
      bytes(sent?.chunks ?? []),
      sent !== undefined && !sent.whole ? { truncated: sent.declared } : {},
    )
  })

  const sent = { status: null, contentType: null, chunks: [] }
  const { writeHead, write, end } = reply
  reply.writeHead = function (status, ...rest) {
    sent.status = status
    sent.contentType = headerOf(rest, 'content-type')
    return writeHead.call(this, status, ...rest)
  }
  reply.write = function (chunk, ...rest) {
    if (typeof chunk === 'string' || chunk instanceof Uint8Array)
      sent.chunks.push(Buffer.from(chunk))
    return write.call(this, chunk, ...rest)
  }
  reply.end = function (chunk, ...rest) {
    if (typeof chunk === 'string' || chunk instanceof Uint8Array)
      sent.chunks.push(Buffer.from(chunk))
    return end.call(this, chunk, ...rest)
  }
  let over = false
  const finish = (answered) => {
    if (over) return
    over = true
    if (answered) {
      const status = sent.status ?? reply.statusCode
      const bodyless = BODYLESS.has(status) || request.method === 'HEAD'
      step.response = {
        status,
        contentType: sent.contentType ?? reply.getHeader('content-type') ?? null,
        ...bytes(bodyless ? [] : sent.chunks),
      }
    } else step.aborted = true
    session.worldAfter(world, step)
    session.end(interval, { exchange: id, ...(run === undefined ? {} : { run }) })
  }
  reply.once('finish', () => finish(true))
  reply.once('close', () => finish(false))
  return { kind: 'exchange', target: step }
}
