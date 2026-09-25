import { createHash, randomBytes } from 'node:crypto'
import { createServer } from 'node:http'
import { LedgerError } from '../ledger/index.js'

/**
 * The agents' door into the new core: what `cf` calls from inside a window.
 *
 * Every window gets its own bearer token, issued when the dispatcher opens it
 * and revoked when it closes, so a token names exactly one participant of one
 * project. The API decides who may do what (the lead hands out tasks, only the
 * assignee finishes one, only the one asked answers), the ledger keeps the
 * state rules, and every write wakes the dispatcher.
 */

const MAX_BODY_BYTES = 2 * 1024 * 1024
const TASK_ROUTE =
  /^\/api\/tasks\/(\d+)(?:\/(done|accept|reopen|cancel|pause|resume|tell|transcript))?$/
const MESSAGE_ROUTE = /^\/api\/inbox\/(\d+)$/
/** A task in one of these states has a window a `tell` reaches (a queued one has none yet). */
const WINDOWED = new Set(['working', 'waiting', 'paused'])
const QUESTION_ROUTE = /^\/api\/questions\/(\d+)$/
/** The longest one poll for an answer may hold; a door polls again. */
const MAX_WAIT_MS = 25_000
const POLL_MS = 250
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

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

export async function startApi({
  ledger,
  credentials,
  changed = () => {},
  roster = () => null,
  ui = null,
}) {
  const server = createServer((request, reply) => {
    handle(request).then(
      ({ status, body, html }) => send(reply, status, body, html),
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
    // The human's screens, under the app's UI token, before any window's.
    const screen = ui === null ? null : await ui.handle(request, url)
    if (screen !== null) return screen
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
          .filter((member) => member.agent !== null && member.memberId === null)
          .map((member) => {
            const row = roster(member.agent)
            return {
              handle: member.handle,
              role: member.role,
              roles: member.roles,
              tier: member.tier,
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
      if (participant.role !== 'lead') {
        throw new Refusal(
          403,
          'not-a-coordinator',
          'members do not hand out tasks: ask your lead instead (cf ask)',
        )
      }
      const body = await readJson(request)
      // The board is the only channel between agents: no task is given by
      // name. A follow-up that needs the context of the window that did T-n
      // goes back to that window (`after`); the lead's own later step is its
      // own (`self`); everything else is fresh work for a tier of worker,
      // advice from a tier of advisor, a review from a tier of reviewer, or
      // an image from the designer.
      const pool =
        body.design === true
          ? 'designer'
          : body.advice === true
            ? 'advisor'
            : body.review === true
              ? 'reviewer'
              : 'worker'
      const created = ledger.createTask(project.id, {
        from: participant.handle,
        ...(body.after !== undefined
          ? { after: Number(body.after) }
          : body.self === true
            ? { to: participant.handle }
            : { pool, tier: body.tier, purpose: body.purpose }),
        body: body.body,
        ...(body.needs === undefined ? {} : { needs: body.needs }),
        ...(body.before === undefined ? {} : { before: body.before }),
      })
      changed()
      // With human approval required, the brief waits for the human before it
      // moves; `cf` says so, and the lead knows a quiet board is a waiting one.
      return {
        status: 201,
        body: {
          task: summary(created.task),
          message: created.message?.id ?? null,
          gated: ledger.project(project.id).gate,
        },
      }
    }
    const task = TASK_ROUTE.exec(url.pathname)
    if (task !== null) return taskRoute(request, caller, Number(task[1]), task[2], url)
    if (at === 'GET /api/inbox') {
      return ok({ messages: ledger.inbox(participant.id).map(messageSummary) })
    }
    const message = MESSAGE_ROUTE.exec(url.pathname)
    if (message !== null && request.method === 'GET') {
      const found = ledger.message(Number(message[1]))
      // What still waits for the human is not yet the recipient's to read.
      if (
        found === null ||
        found.projectId !== project.id ||
        (found.recipient !== participant.handle && found.sender !== participant.handle) ||
        (found.state === 'gated' && found.recipient === participant.handle)
      ) {
        throw new Refusal(404, 'unknown-message', `no message m-${message[1]} for you`)
      }
      return ok({ message: found })
    }
    if (at === 'POST /api/questions') {
      const body = await readJson(request)
      const active = ledger.activeTask(participant.id, { queued: true })
      // The lead's question goes to the human; a member's to whoever gave its task.
      const to = active?.requester ?? (participant.role === 'lead' ? 'human' : 'lead')
      const asked = ledger.ask(project.id, {
        from: participant.handle,
        to,
        task: active?.number,
        body: body.body,
        ...(body.questions === undefined ? {} : { questions: body.questions }),
      })
      changed()
      return { status: 201, body: { message: messageSummary(asked) } }
    }
    if (at === 'POST /api/notes') {
      const body = await readJson(request)
      const active = ledger.activeTask(participant.id, { queued: true })
      // The lead's note goes to the human; a member's to whoever gave its task.
      const to = active?.requester ?? (participant.role === 'lead' ? 'human' : 'lead')
      const noted = ledger.note(project.id, {
        from: participant.handle,
        to,
        task: active?.number,
        body: body.body,
      })
      changed()
      return { status: 201, body: { message: messageSummary(noted) } }
    }
    // A door waiting for the answer to the question it put on the board.
    const question = QUESTION_ROUTE.exec(url.pathname)
    if (question !== null && request.method === 'GET') {
      const asked = ledger.message(Number(question[1]))
      if (asked === null || asked.kind !== 'question' || asked.projectId !== project.id) {
        throw new Refusal(404, 'unknown-message', `no question m-${question[1]}`)
      }
      if (asked.sender !== participant.handle) {
        throw new Refusal(403, 'not-your-question', `m-${asked.id} was asked by @${asked.sender}`)
      }
      const wait = Math.min(MAX_WAIT_MS, Math.max(0, Number(url.searchParams.get('wait')) || 0))
      const until = Date.now() + wait
      let answer = ledger.answerTo(asked.id)
      while (answer === null && Date.now() < until) {
        await sleep(Math.min(POLL_MS, until - Date.now()))
        answer = ledger.answerTo(asked.id)
      }
      return ok({
        question: messageSummary(asked),
        answer:
          answer === null
            ? null
            : { id: answer.id, from: answer.sender, body: answer.body, choices: answer.choices },
      })
    }
    if (at === 'POST /api/answers') {
      const body = await readJson(request)
      const answer = ledger.answer(body.question, {
        from: participant.handle,
        body: body.body,
        ...(body.choices === undefined ? {} : { choices: body.choices }),
      })
      changed()
      return { status: 201, body: { message: messageSummary(answer) } }
    }
    throw new Refusal(404, 'unknown-route', `no such command: ${at}`)
  }

  async function taskRoute(request, { project, participant }, number, action, url) {
    const task = ledger.task(project.id, number)
    if (task === null) throw new Refusal(404, 'unknown-task', `no task T-${number} in this project`)
    if (action === undefined && request.method === 'GET') {
      // The thread as far as the human has let it go: a gated message waits unseen.
      return ok({ task: { ...task, messages: task.messages.filter((m) => m.state !== 'gated') } })
    }
    // What the task's window did so far, from ConsensFlow's own copy: the
    // last items, for whoever gave the task.
    if (action === 'transcript' && request.method === 'GET') {
      if (participant.role !== 'lead' && task.requester !== participant.handle) {
        throw new Refusal(
          403,
          'not-a-coordinator',
          `only the lead or @${task.requester} may read what T-${number}'s window did`,
        )
      }
      const last = Math.min(50, Math.max(1, Number(url.searchParams.get('last')) || 10))
      return ok(ledger.transcript(project.id, number, { limit: last }))
    }
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
    if (participant.role !== 'lead' && task.requester !== participant.handle) {
      throw new Refusal(
        403,
        'not-a-coordinator',
        `only the lead or @${task.requester} may ${action} T-${number}`,
      )
    }
    if (action === 'tell') {
      // Stop the task and put this to its window: the agent is interrupted as
      // for any pause, reads the question once idle, and its answer comes
      // back as a message; the lead resumes the task with its words.
      if (task.assignee === null || !WINDOWED.has(task.state)) {
        throw new Refusal(
          409,
          'no-window',
          `T-${number} has no window to tell: it is ${task.state}`,
        )
      }
      if (task.state !== 'paused') ledger.pauseTask(project.id, number, { by: participant.handle })
      const told = ledger.ask(project.id, {
        from: participant.handle,
        to: task.assignee,
        task: number,
        body: body.body,
        urgent: true,
      })
      changed()
      return ok({ message: messageSummary(told), task: summary(ledger.task(project.id, number)) })
    }
    const by = participant.handle
    const moved =
      action === 'accept'
        ? ledger.acceptTask(project.id, number, { by })
        : action === 'cancel'
          ? ledger.cancelTask(project.id, number, { by })
          : action === 'pause'
            ? ledger.pauseTask(project.id, number, { by })
            : action === 'resume'
              ? ledger.resumeTask(project.id, number, { by, body: body.body }).task
              : ledger.reopenTask(project.id, number, { by, body: body.body }).task
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
    needs: task.needs,
    blockedBy: task.blockedBy,
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
    questions: message.questions,
    choices: message.choices,
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

function send(reply, status, body, html) {
  if (html !== undefined) {
    reply.writeHead(status, { 'content-type': 'text/html; charset=utf-8' })
    reply.end(html)
    return
  }
  if (body === undefined) {
    reply.writeHead(status)
    reply.end()
    return
  }
  reply.writeHead(status, { 'content-type': 'application/json' })
  reply.end(JSON.stringify(body))
}
