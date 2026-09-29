import { spawn } from 'node:child_process'
import { randomUUID, timingSafeEqual } from 'node:crypto'
import { once } from 'node:events'
import { chmod, mkdir, mkdtemp, rm, stat } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import WebSocket, { WebSocketServer } from 'ws'
import { runnable, terminate } from '../src/harnesses.js'
import { configRoot } from '../src/roster.js'
import { askTheBoard, boardClient, refusalReason } from './lib/question-door.js'
import { createReceiver } from './lib/receiver.js'

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const MAX_FRAME = 64 * 1024 * 1024
const refused = (error) => ({ ok: false, admitted: false, bytesWritten: 0, error })
const uncertain = () => ({ ok: false, admitted: null, error: 'uncertain' })

/** The broker owns the main chief identity and the last check before native admission. */
export async function startBroker({
  port,
  token,
  launchId,
  upstream,
  freshBypass = false,
  receiver: receiverOptions,
  board: boardOptions,
  questionWaitMs,
}) {
  if (
    !Number.isInteger(port) ||
    port < 0 ||
    port > 65535 ||
    typeof token !== 'string' ||
    token.length < 24 ||
    !/^[A-Za-z0-9._-]+$/.test(launchId ?? '')
  )
    throw new Error('Invalid Codex broker configuration')
  let selected = null
  let empty = false
  let owner = null
  let switching = false
  let revision = 0
  let closed = false
  let ready = false
  const idle = new Map()
  const observeStatus = (message) => {
    const params = message.params
    if (message.method === 'thread/status/changed')
      idle.set(params?.threadId, params?.status?.type === 'idle')
    if (message.method === 'turn/started') idle.set(params?.threadId, false)
    if (message.method === 'turn/completed') idle.set(params?.threadId, true)
  }
  const connections = new Set()
  const pending = new Map()
  // The question tool's door: the app-server asks the client; this broker is
  // the client, so it asks the board instead and answers the app-server with
  // what the board said. When nobody answers in time, or there is no board,
  // the request goes on to the TUI and its own dialog takes over.
  const board = boardClient(boardOptions)
  const control = new WebSocket(upstream, {
    maxPayload: MAX_FRAME,
    handshakeTimeout: 3000,
    perMessageDeflate: false,
  })
  connections.add(control)
  const clearControl = () => {
    ready = false
    for (const resolve of pending.values()) resolve(null)
    pending.clear()
  }
  control.on('error', clearControl)
  control.on('close', clearControl)
  control.on('message', (raw) => {
    let value
    try {
      value = JSON.parse(raw)
    } catch {
      control.terminate()
      return
    }
    pending.get(value.id)?.(value)
    observeStatus(value)
    if (value.method === 'turn/started' && value.params?.threadId === selected) empty = false
  })
  await once(control, 'open')
  const request = (method, params, expiresAt) =>
    new Promise((resolve) => {
      const id = randomUUID()
      const timer = setTimeout(() => finish(null), Math.max(1, expiresAt - Date.now()))
      const finish = (value) => {
        clearTimeout(timer)
        pending.delete(id)
        resolve(value)
      }
      pending.set(id, finish)
      control.send(JSON.stringify({ id, method, params }), (error) => {
        if (error) finish(null)
      })
    })
  const initialized = await request(
    'initialize',
    {
      clientInfo: { name: 'consensflow-delivery', version: '3.0.0' },
      capabilities: { experimentalApi: true },
    },
    Date.now() + 3000,
  )
  if (!initialized?.result) {
    control.terminate()
    throw new Error('Codex native server did not initialize')
  }
  control.send(JSON.stringify({ method: 'initialized' }))
  ready = true
  const receiver = receiverOptions
    ? createReceiver({
        ...receiverOptions,
        session: () => (!closed && ready && !switching ? selected : null),
        ready: () => !closed && ready && !switching && idle.get(selected) === true,
        insert: async (claim) => {
          if (
            closed ||
            !ready ||
            switching ||
            selected !== claim.receiver.session ||
            idle.get(selected) !== true
          )
            return refused('native-session-changed')
          idle.set(selected, false)
          empty = false
          const result = await request(
            'turn/start',
            {
              threadId: selected,
              input: [],
              toolOutput: { name: 'consensflow_inbox', namespace: null, output: claim.text },
            },
            Date.now() + 5000,
          )
          if (!result?.result) throw new Error('native insertion is unconfirmed')
          return { admitted: true }
        },
      })
    : null

  const authorized = (incoming) => {
    const actual = Buffer.from(incoming.headers.authorization ?? '')
    const expected = Buffer.from(`Bearer ${token}`)
    return actual.length === expected.length && timingSafeEqual(actual, expected)
  }
  const reply = (response, value, status = 200) => {
    response.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' })
    response.end(JSON.stringify(value))
  }
  const server = createServer(async (incoming, response) => {
    if (!authorized(incoming)) return reply(response, refused('unauthorized'), 401)
    if (incoming.method === 'GET' && incoming.url === '/session')
      return reply(response, {
        launchId,
        sessionId: switching ? null : selected,
        revision,
        empty: !switching && empty,
        // Whether a delivery would be taken now: the dispatcher holds a
        // message while this is false, instead of spending its attempts.
        available:
          !closed &&
          ready &&
          control.readyState === WebSocket.OPEN &&
          !switching &&
          selected !== null,
      })
    if (incoming.method !== 'POST' || incoming.url !== '/deliver')
      return reply(response, refused('invalid-record'), 404)
    let record
    try {
      const chunks = []
      let size = 0
      for await (const chunk of incoming) {
        size += chunk.length
        if (size > 128 * 1024) return reply(response, refused('invalid-record'), 413)
        chunks.push(chunk)
      }
      record = JSON.parse(Buffer.concat(chunks).toString('utf8'))
    } catch {
      return reply(response, refused('invalid-record'), 400)
    }
    if (
      record?.launchId !== launchId ||
      !UUID.test(record.sessionId ?? '') ||
      typeof record.text !== 'string' ||
      !record.text ||
      Buffer.byteLength(record.text) > 64 * 1024 ||
      !Number.isFinite(record.expiresAt)
    )
      return reply(response, refused('invalid-record'), 400)
    if (record.expiresAt <= Date.now()) return reply(response, refused('expired'))
    if (closed || !ready || control.readyState !== WebSocket.OPEN || switching || selected === null)
      return reply(response, refused('native-session-unavailable'))
    if (selected !== record.sessionId) return reply(response, refused('native-session-changed'))
    empty = false
    // An idle thread gets the message as its turn; Codex's queue drains only
    // when a running turn ends, and after an interrupt it kept a message for
    // good (a tell to a paused worker, 2026-09-26). While a turn runs, the
    // queue is right: the message goes in when the turn ends.
    const input = [{ type: 'text', text: record.text, text_elements: [] }]
    const deadline = Math.min(record.expiresAt, Date.now() + 3000)
    const idleNow = idle.get(selected) === true
    if (idleNow) idle.set(selected, false)
    // No await between comparing the selected main and forwarding this request.
    // A subsequent switch can only retire later submissions, never replay this one.
    let result = idleNow
      ? await request('turn/start', { threadId: selected, input }, deadline)
      : await request(
          'thread/queue/add',
          { threadId: selected, input, clientUserMessageId: randomUUID() },
          deadline,
        )
    // A turn the TUI started a moment before is an explicit refusal: queue it.
    if (idleNow && result?.error)
      result = await request(
        'thread/queue/add',
        { threadId: selected, input, clientUserMessageId: randomUUID() },
        deadline,
      )
    reply(response, result?.result ? { ok: true, admitted: true } : uncertain())
  })
  server.requestTimeout = 5000
  server.headersTimeout = 5000
  const websocket = new WebSocketServer({
    noServer: true,
    maxPayload: MAX_FRAME,
    perMessageDeflate: false,
  })
  server.on('upgrade', (incoming, socket, head) => {
    if (!authorized(incoming) || incoming.url !== '/' || closed) {
      socket.destroy()
      return
    }
    websocket.handleUpgrade(incoming, socket, head, (client) => {
      const native = new WebSocket(upstream, {
        maxPayload: MAX_FRAME,
        handshakeTimeout: 3000,
        perMessageDeflate: false,
      })
      connections.add(client)
      connections.add(native)
      const requests = new Map()
      const queued = []
      let queuedBytes = 0
      const retire = () => {
        if (owner === client) {
          selected = null
          empty = false
          owner = null
          switching = false
          revision++
        }
        connections.delete(client)
        connections.delete(native)
        client.terminate()
        native.terminate()
      }
      client.on('error', retire)
      native.on('error', retire)
      client.on('close', retire)
      native.on('close', retire)
      const forward = (destination, raw) => {
        if (
          destination.readyState !== WebSocket.OPEN ||
          destination.bufferedAmount + raw.length > MAX_FRAME
        ) {
          retire()
          return
        }
        destination.send(raw, { binary: false }, (error) => {
          if (error) retire()
        })
      }
      const fromTui = (raw) => {
        let message
        try {
          message = JSON.parse(raw)
        } catch {
          retire()
          return
        }
        const params = message.params ?? {}
        const mainStart =
          message.method === 'thread/start' &&
          params.ephemeral === false &&
          params.threadSource === 'user'
        const mainResume =
          message.method === 'thread/resume' && Array.isArray(params.runtimeWorkspaceRoots)
        const mainFork =
          message.method === 'thread/fork' &&
          params.ephemeral !== true &&
          params.threadSource === 'user' &&
          Array.isArray(params.runtimeWorkspaceRoots)
        if (mainStart || mainResume || mainFork) {
          const prior = switching ? null : selected
          const priorEmpty = !switching && empty
          owner = client
          selected = null
          empty = false
          switching = true
          revision++
          requests.set(message.id, { revision, prior, priorEmpty, mainResume })
          if (mainStart && freshBypass) {
            message.params = {
              ...params,
              approvalPolicy: 'never',
              sandbox: 'danger-full-access',
              permissions: null,
            }
            raw = Buffer.from(JSON.stringify(message))
          }
        }
        if (
          message.method === 'thread/settings/update' &&
          params.threadId === selected &&
          ['approvalPolicy', 'sandbox', 'permissions'].some((key) => Object.hasOwn(params, key))
        )
          requests.set(message.id, { permissionChange: true })
        if (
          ['turn/start', 'thread/queue/add'].includes(message.method) &&
          params.threadId === selected
        ) {
          empty = false
          idle.set(selected, false)
        }
        forward(native, raw)
      }
      client.on('message', (raw) => {
        if (native.readyState === WebSocket.CONNECTING) {
          queuedBytes += raw.length
          if (queuedBytes > MAX_FRAME) {
            retire()
            return
          }
          queued.push(raw)
        } else fromTui(raw)
      })
      native.on('open', () => {
        for (const raw of queued) fromTui(raw)
        queued.length = 0
        queuedBytes = 0
      })
      native.on('message', (raw) => {
        let message
        try {
          message = JSON.parse(raw)
        } catch {
          retire()
          return
        }
        const selection = requests.get(message.id)
        requests.delete(message.id)
        if (selection?.permissionChange && !message.error) freshBypass = false
        if (selection && owner === client && selection.revision === revision) {
          const candidate = message.result?.thread?.id
          selected = message.error
            ? selection.prior
            : UUID.test(candidate ?? '') && !message.result.readOnly
              ? candidate
              : null
          empty = message.error
            ? selection.priorEmpty
            : selected !== null &&
              message.result.thread.turns?.length === 0 &&
              message.result.thread.status?.type === 'idle'
          if (!message.error && selected && selection.mainResume) freshBypass = false
          if (!message.error && selected)
            idle.set(selected, message.result.thread.status?.type === 'idle')
          switching = false
        }
        observeStatus(message)
        if (message.method === 'turn/started' && message.params?.threadId === selected)
          empty = false
        if (message.method === 'item/tool/requestUserInput' && message.id !== undefined && board) {
          void holdQuestion(message, raw)
          return
        }
        forward(client, raw)
      })
      const holdQuestion = async (message, raw) => {
        const questions = message.params?.questions ?? []
        try {
          const asked = await askTheBoard(
            board,
            questions.map((q) => ({
              question: q.question,
              header: q.header,
              options: (q.options ?? []).map((o) => ({
                label: o.label,
                description: o.description,
              })),
              multiple: false,
            })),
            questionWaitMs === undefined ? {} : { waitMs: questionWaitMs },
          )
          if (asked.answer !== null) {
            const answers = Object.fromEntries(
              questions.map((q, at) => [q.id, { answers: asked.answer.choices[at] ?? [] }]),
            )
            forward(native, JSON.stringify({ id: message.id, result: { answers } }))
            return
          }
        } catch (cause) {
          // Refused: the model hears why, as the answer. Not reached: the
          // window asks instead.
          if (cause?.refused) {
            const reason = refusalReason(cause)
            const answers = Object.fromEntries(questions.map((q) => [q.id, { answers: [reason] }]))
            forward(native, JSON.stringify({ id: message.id, result: { answers } }))
            return
          }
        }
        forward(client, raw)
      }
    })
  })
  server.listen(port, '127.0.0.1')
  try {
    await once(server, 'listening')
  } catch (error) {
    control.terminate()
    throw error
  }
  receiver?.start()
  return {
    endpoint: `http://127.0.0.1:${server.address().port}`,
    receiver,
    async close() {
      closed = true
      await receiver?.stop().catch(() => {})
      selected = null
      ready = false
      clearControl()
      for (const socket of connections) socket.terminate()
      websocket.close()
      server.closeAllConnections()
      await new Promise((resolve) => server.close(resolve))
    },
  }
}

/** Remote resume forbids TUI permission overrides; native backend owns that config. */
export function codexProcessArguments(args) {
  const backend = []
  const tui = []
  for (let index = 0; index < args.length; index++) {
    const arg = args[index]
    if (arg === '--dangerously-bypass-approvals-and-sandbox') {
      backend.push('-c', 'approval_policy="never"', '-c', 'sandbox_mode="danger-full-access"')
    } else if (['-c', '--config', '--enable', '--disable'].includes(arg)) {
      const value = args[++index]
      if (typeof value !== 'string') throw new Error(`Missing Codex ${arg} value`)
      backend.push(arg, value)
      tui.push(arg, value)
    } else if (arg === '--model' || arg === '-m') {
      const value = args[++index]
      if (typeof value !== 'string') throw new Error('Missing Codex model')
      backend.push('-c', `model=${JSON.stringify(value)}`)
      tui.push(arg, value)
    } else tui.push(arg)
  }
  return { backend, tui }
}

/**
 * ConsensFlow's own variables, set explicitly in Codex's shell policy: a user
 * policy of `inherit = "core"` keeps only a handful of names, and a window
 * whose commands lose CONSENSFLOW_URL, _TOKEN and _NODE has a `cf` that
 * reaches nothing (a Codex chief, 2026-09-26). Everything else stays as the
 * user's policy says.
 */
export function consensflowShellEnvironment(env) {
  return Object.keys(env)
    .filter((name) => /^CONSENSFLOW_[A-Z0-9_]+$/.test(name) && typeof env[name] === 'string')
    .sort()
    .flatMap((name) => ['-c', `shell_environment_policy.set.${name}=${JSON.stringify(env[name])}`])
}

/** Whether a socket at `<prefix>XXXXXX/native.sock` fits sun_path (macOS: 104 bytes with the final NUL). */
const socketFits = (prefix) => Buffer.byteLength(join(`${prefix}XXXXXX`, 'native.sock')) < 104

/**
 * The socket's private directory: under the ConsensFlow home, or, when that
 * path is too long for a Unix socket (a home deep in a temporary tree), under
 * the user's own temporary directory. The socket is a runtime endpoint, not
 * state: it is removed with the session.
 */
export async function createSocketDirectory(env) {
  const homes = [join(configRoot(env), 'tmp'), join(env.TMPDIR ?? tmpdir(), 'consensflow')]
  const root = homes.find((candidate) => socketFits(join(candidate, 'codex-')))
  if (root === undefined) throw new Error('ConsensFlow home makes the Codex socket path too long')
  await mkdir(root, { recursive: true, mode: 0o700 })
  const directory = await mkdtemp(join(root, 'codex-'))
  await chmod(directory, 0o700)
  return directory
}

async function supervise(executable, args) {
  const configuration = JSON.parse(process.env.CF_CODEX_SESSION_BRIDGE ?? '{}')
  const split = codexProcessArguments(args)
  const directory = await createSocketDirectory(process.env)
  const socket = join(directory, 'native.sock')
  const env = { ...process.env }
  delete env.OPENAI_API_KEY
  const backendRun = runnable(
    executable,
    [
      ...split.backend,
      ...consensflowShellEnvironment(env),
      'app-server',
      '--listen',
      `unix://${socket}`,
    ],
    env,
  )
  const backend = spawn(backendRun.file, backendRun.args, {
    ...backendRun.options,
    env,
    stdio: ['ignore', 'ignore', 'pipe'],
  })
  let startupError = ''
  backend.stderr.on('data', (chunk) => {
    startupError = (startupError + chunk).slice(-4000)
  })
  backend.on('error', (error) => {
    startupError = error.message
  })
  let broker
  let tui
  const stop = () => {
    if (tui) terminate(tui, 'SIGTERM')
    terminate(backend, 'SIGTERM')
  }
  process.on('SIGTERM', stop)
  process.on('SIGINT', stop)
  try {
    const deadline = Date.now() + 15000
    while (!(await stat(socket).catch(() => null))?.isSocket()) {
      if (backend.exitCode !== null || backend.signalCode || Date.now() > deadline)
        throw new Error(`Codex server could not start: ${startupError}`)
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
    broker = await startBroker({
      ...configuration,
      receiver: env.CF_RESULT_RECEIVER ? { config: env.CF_RESULT_RECEIVER } : undefined,
      upstream: `ws+unix://${socket}`,
      freshBypass: args.includes('--dangerously-bypass-approvals-and-sandbox'),
    })
    const tuiRun = runnable(
      executable,
      [
        '--remote',
        broker.endpoint.replace('http:', 'ws:'),
        '--remote-auth-token-env',
        'CF_CODEX_TUI_TOKEN',
        ...split.tui,
      ],
      env,
    )
    tui = spawn(tuiRun.file, tuiRun.args, {
      ...tuiRun.options,
      env: { ...env, CF_CODEX_TUI_TOKEN: configuration.token },
      stdio: 'inherit',
    })
    backend.once('exit', () => {
      if (tui) terminate(tui, 'SIGTERM')
    })
    const [code] = await once(tui, 'exit')
    return code ?? 0
  } finally {
    stop()
    await broker?.close()
    if (backend.exitCode === null && !backend.signalCode) {
      const killer = setTimeout(() => terminate(backend, 'SIGKILL'), 1500)
      await once(backend, 'exit').catch(() => {})
      clearTimeout(killer)
    }
    process.off('SIGTERM', stop)
    process.off('SIGINT', stop)
    await rm(directory, { recursive: true, force: true })
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = await supervise(process.argv[2], process.argv.slice(3))
  } catch (error) {
    process.stderr.write(`ConsensFlow could not open Codex: ${error.message}\n`)
    process.exitCode = 1
  }
}
