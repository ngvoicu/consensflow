import { createHash, randomBytes } from 'node:crypto'
import { createServer } from 'node:http'
import { LedgerError } from '../ledger/index.js'

/**
 * The agents' door into the new core: what `cf` calls from inside a window.
 *
 * Every window gets its own bearer token, issued when the dispatcher opens it
 * and revoked when it closes, so a token names exactly one participant of one
 * project. The API decides who may do what (coordinators hand out tasks, only
 * the assignee finishes one, only the one asked answers), the ledger keeps the
 * state rules, and every write wakes the dispatcher.
 */

const MAX_BODY_BYTES = 2 * 1024 * 1024
const COORDINATORS = new Set(['lead', 'pm', 'human'])
const MEMBERS = new Set(['worker', 'advisor', 'reviewer'])
/** Whose pool a coordinator's tiered task draws from. */
const POOL_OF = { lead: 'worker', pm: 'advisor', human: 'worker' }
const TASK_ROUTE = /^\/api\/tasks\/(\d+)(?:\/(done|accept|reopen|cancel|review))?$/
const MESSAGE_ROUTE = /^\/api\/inbox\/(\d+)$/

const digest = (token) => createHash('sha256').update(token).digest('hex')

/** The tokens of the windows that are open now; only their digests are kept. */
export class Credentials {
  #byDigest = new Map()

  issue({ participant, project, generation }) {
    const token = randomBytes(32).toString('hex')
    this.#byDigest.set(digest(token), {
      participantId: participant.id,
      projectId: project.id,
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
    const { project, participant } = caller
    const at = `${request.method} ${url.pathname}`

    if (at === 'GET /api/whoami') {
      const task = ledger.activeTask(participant.id)
      return ok({
        project: { id: project.id, name: project.name, directory: project.directory },
        participant: { handle: participant.handle, role: participant.role },
        task: task === null ? null : summary(task),
      })
    }
    if (at === 'GET /api/team') {
      return ok({
        members: project.participants
          .filter((member) => member.agent !== null)
          .map((member) => {
            const row = roster(member.agent)
            return {
              handle: member.handle,
              role: member.role,
              tier: member.tier,
              tags: member.tags,
              harness: member.harness,
              model: row?.model ?? null,
              effort: row?.effort ?? null,
            }
          }),
      })
    }
    if (at === 'GET /api/tasks') {
      const board = ledger.board(project.id)
      return ok({
        open: board.open.map(summary),
        lanes: board.lanes.map(({ participant: owner, tasks }) => ({
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
      const to = body.self === true ? participant.handle : body.to
      const target = to === undefined ? undefined : memberByHandle(project, to)
      if (target !== undefined && MEMBERS.has(target.role)) {
        throw new Refusal(
          403,
          'name-a-tier',
          `@${to} is a ${target.role}: name a tier, not a member (cf task add --tier ${target.tier} "…")`,
        )
      }
      const created = ledger.createTask(project.id, {
        from: participant.handle,
        ...(to === undefined
          ? {
              pool: body.pool ?? POOL_OF[participant.role],
              tier: body.tier,
              tags: body.tags ?? [],
              purpose: body.purpose,
            }
          : { to }),
        body: body.body,
        title: body.title,
      })
      changed()
      return {
        status: 201,
        body: { task: summary(created.task), message: created.message?.id ?? null },
      }
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
        found.projectId !== project.id ||
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
      if (MEMBERS.has(memberByHandle(project, to)?.role)) {
        throw new Refusal(
          403,
          'not-addressable',
          `questions go to your coordinator or the human, not to @${to}`,
        )
      }
      const asked = ledger.ask(project.id, {
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

  async function taskRoute(request, { project, participant }, number, action) {
    const task = ledger.task(project.id, number)
    if (task === null) throw new Refusal(404, 'unknown-task', `no task T-${number} in this project`)
    if (action === undefined && request.method === 'GET') return ok({ task })
    if (request.method !== 'POST' || action === undefined) {
      throw new Refusal(404, 'unknown-route', 'no such task command')
    }
    const body = await readJson(request)
    if (action === 'done') {
      if (task.assignee !== participant.handle) {
        throw new Refusal(403, 'not-yours', `T-${number} is assigned to @${task.assignee}`)
      }
      const done = ledger.recordResult(project.id, number, { body: body.body })
      changed()
      return ok({ task: summary(done.task) })
    }
    if (action === 'review') {
      const reviewed = ledger.requestReview(project.id, number, { by: participant.handle })
      changed()
      return ok({ task: summary(reviewed) })
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
        ? ledger.acceptTask(project.id, number, { by })
        : action === 'cancel'
          ? ledger.cancelTask(project.id, number, { by })
          : ledger.reopenTask(project.id, number, { by, body: body.body }).task
    changed()
    return ok({ task: summary(moved) })
  }

  function memberByHandle(project, handle) {
    return ledger.project(project.id)?.participants.find((p) => p.handle === handle)
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
    const project = ledger.project(identity.projectId)
    const participant = project?.participants.find((p) => p.id === identity.participantId)
    if (participant === undefined) {
      throw new Refusal(
        401,
        'unauthorized',
        'this window belongs to a project that no longer exists',
      )
    }
    return { project, participant }
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
    kind: task.kind,
    requester: task.requester,
    assignee: task.assignee,
    pool: task.pool,
    tier: task.tier,
    tags: task.tags,
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
