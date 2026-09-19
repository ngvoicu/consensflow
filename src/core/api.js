import { createHash, randomBytes } from 'node:crypto'
import { createServer } from 'node:http'
import { LedgerError } from '../ledger/index.js'

/**
 * The agents' door into the new core: what `cf` calls from inside a window.
 *
 * Every window gets its own bearer token, issued when the dispatcher opens it
 * and revoked when it closes, so a token names exactly one participant of one
 * session. The API decides who may do what (coordinators hand out tasks, only
 * the assignee finishes one, only the one asked answers), the ledger keeps the
 * state rules, and every write wakes the dispatcher.
 */

const MAX_BODY_BYTES = 2 * 1024 * 1024
const COORDINATORS = new Set(['lead', 'pm', 'human'])
const TASK_ROUTE = /^\/api\/tasks\/(\d+)(?:\/(done|accept|reopen|cancel))?$/
const MESSAGE_ROUTE = /^\/api\/inbox\/(\d+)$/

const digest = (token) => createHash('sha256').update(token).digest('hex')

/** The tokens of the windows that are open now; only their digests are kept. */
export class Credentials {
  #byDigest = new Map()

  issue({ participant, session, generation }) {
    const token = randomBytes(32).toString('hex')
    this.#byDigest.set(digest(token), {
      participantId: participant.id,
      sessionId: session.id,
      generation,
    })
    return token
  }

  revoke(token) {
    if (typeof token === 'string') this.#byDigest.delete(digest(token))
  }

  resolve(token) {
    return typeof token === 'string' ? (this.#byDigest.get(digest(token)) ?? null) : null
  }
}

class Refusal extends Error {
  constructor(status, code, message) {
    super(message)
    this.status = status
    this.code = code
  }
}

export async function startApi({ ledger, credentials, changed = () => {}, roster = () => null }) {
  const server = createServer((request, reply) => {
    handle(request).then(
      ({ status, body }) => send(reply, status, body),
      (cause) => {
        if (cause instanceof Refusal || cause instanceof LedgerError) {
          send(reply, cause.status, { error: cause.code, message: cause.message })
        } else {
          send(reply, 500, { error: 'internal', message: cause?.message ?? String(cause) })
        }
      },
    )
  })

  async function handle(request) {
    const url = new URL(request.url, 'http://127.0.0.1')
    const caller = callerOf(request)
    const { session, participant } = caller
    const at = `${request.method} ${url.pathname}`

    if (at === 'GET /api/whoami') {
      const task = ledger.activeTask(participant.id)
      return ok({
        session: { id: session.id, name: session.name, directory: session.directory },
        participant: { handle: participant.handle, role: participant.role },
        task: task === null ? null : summary(task),
      })
    }
    if (at === 'GET /api/team') {
      return ok({
        members: session.participants
          .filter((member) => member.agent !== null)
          .map((member) => {
            const row = roster(member.agent)
            return {
              handle: member.handle,
              role: member.role,
              harness: member.harness,
              model: row?.model ?? null,
              effort: row?.effort ?? null,
            }
          }),
      })
    }
    if (at === 'GET /api/tasks') {
      return ok({
        lanes: ledger.board(session.id).lanes.map(({ participant: owner, tasks }) => ({
          handle: owner.handle,
          role: owner.role,
          tasks: tasks.map(summary),
        })),
      })
    }
    if (at === 'POST /api/tasks') {
      if (!COORDINATORS.has(participant.role)) {
        throw new Refusal(
          403,
          'not-a-coordinator',
          'workers do not hand out tasks: ask your lead instead (cf ask)',
        )
      }
      const body = await readJson(request)
      const created = ledger.createTask(session.id, {
        from: participant.handle,
        to: body.to,
        body: body.body,
        title: body.title,
      })
      changed()
      return { status: 201, body: { task: summary(created.task), message: created.message.id } }
    }
    const task = TASK_ROUTE.exec(url.pathname)
    if (task !== null) return taskRoute(request, caller, Number(task[1]), task[2])
    if (at === 'GET /api/inbox') {
      return ok({ messages: ledger.inbox(participant.id).map(messageSummary) })
    }
    const message = MESSAGE_ROUTE.exec(url.pathname)
    if (message !== null && request.method === 'GET') {
      const found = ledger.message(Number(message[1]))
      if (
        found === null ||
        found.sessionId !== session.id ||
        (found.recipient !== participant.handle && found.sender !== participant.handle)
      ) {
        throw new Refusal(404, 'unknown-message', `no message m-${message[1]} for you`)
      }
      return ok({ message: found })
    }
    if (at === 'POST /api/questions') {
      const body = await readJson(request)
      const active = ledger.activeTask(participant.id)
      const to = body.to ?? active?.requester ?? (participant.role === 'lead' ? 'human' : 'lead')
      const asked = ledger.ask(session.id, {
        from: participant.handle,
        to,
        task: body.task ?? active?.number,
        body: body.body,
      })
      changed()
      return { status: 201, body: { message: messageSummary(asked) } }
    }
    if (at === 'POST /api/answers') {
      const body = await readJson(request)
      const answer = ledger.answer(body.question, { from: participant.handle, body: body.body })
      changed()
      return { status: 201, body: { message: messageSummary(answer) } }
    }
    throw new Refusal(404, 'unknown-route', `no such command: ${at}`)
  }

  async function taskRoute(request, { session, participant }, number, action) {
    const task = ledger.task(session.id, number)
    if (task === null) throw new Refusal(404, 'unknown-task', `no task T-${number} in this session`)
    if (action === undefined && request.method === 'GET') return ok({ task })
    if (request.method !== 'POST' || action === undefined) {
      throw new Refusal(404, 'unknown-route', 'no such task command')
    }
    const body = await readJson(request)
    if (action === 'done') {
      if (task.assignee !== participant.handle) {
        throw new Refusal(403, 'not-yours', `T-${number} is assigned to @${task.assignee}`)
      }
      const done = ledger.recordResult(session.id, number, { body: body.body })
      changed()
      return ok({ task: summary(done.task) })
    }
    if (!COORDINATORS.has(participant.role) && task.requester !== participant.handle) {
      throw new Refusal(
        403,
        'not-a-coordinator',
        `only a coordinator or @${task.requester} may ${action} T-${number}`,
      )
    }
    const by = participant.handle
    const moved =
      action === 'accept'
        ? ledger.acceptTask(session.id, number, { by })
        : action === 'cancel'
          ? ledger.cancelTask(session.id, number, { by })
          : ledger.reopenTask(session.id, number, { by, body: body.body }).task
    changed()
    return ok({ task: summary(moved) })
  }

  function callerOf(request) {
    const header = request.headers.authorization ?? ''
    const token = header.startsWith('Bearer ') ? header.slice('Bearer '.length) : null
    const identity = credentials.resolve(token)
    if (identity === null) {
      throw new Refusal(
        401,
        'unauthorized',
        'this window has no ConsensFlow access (it may have closed)',
      )
    }
    const session = ledger.session(identity.sessionId)
    const participant = session?.participants.find((p) => p.id === identity.participantId)
    if (participant === undefined) {
      throw new Refusal(
        401,
        'unauthorized',
        'this window belongs to a session that no longer exists',
      )
    }
    return { session, participant }
  }

  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const { port } = server.address()
  return {
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise((resolve) => server.close(() => resolve())),
  }
}

function ok(body) {
  return { status: 200, body }
}

function summary(task) {
  return {
    number: task.number,
    title: task.title,
    state: task.state,
    requester: task.requester,
    assignee: task.assignee,
    updatedAt: task.updatedAt,
  }
}

function messageSummary(message) {
  return {
    id: message.id,
    kind: message.kind,
    state: message.state,
    sender: message.sender,
    recipient: message.recipient,
    task: message.taskNumber,
    preview: message.body.split('\n')[0].slice(0, 160),
    createdAt: message.createdAt,
  }
}

async function readJson(request) {
  const chunks = []
  let size = 0
  for await (const chunk of request) {
    size += chunk.length
    if (size > MAX_BODY_BYTES)
      throw new Refusal(413, 'too-large', 'the request is larger than 2 MB')
    chunks.push(chunk)
  }
  if (size === 0) return {}
  try {
    const value = JSON.parse(Buffer.concat(chunks).toString('utf8'))
    if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new Error()
    return value
  } catch {
    throw new Refusal(400, 'invalid-json', 'the request body must be a JSON object')
  }
}

function send(reply, status, body) {
  reply.writeHead(status, { 'content-type': 'application/json' })
  reply.end(JSON.stringify(body))
}
