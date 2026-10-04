import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { existsSync, readdirSync } from 'node:fs'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { createServer } from 'node:http'
import { createServer as createNetServer } from 'node:net'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { before, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { fakeNodeExecutable } from '../helpers.mjs'

/**
 * The Codex window's supervisor, `cf codex-session <codex> <args…>`, as a
 * window runs it, on a stand-in Codex: Codex's server on a private socket
 * (on Windows on loopback, for the window's own token), its TUI through the
 * broker, and nothing left behind once the window ends. The native `cf` npm
 * run build:cf built runs here; the broker's own tests are its crate's.
 */

const WINDOWS = process.platform === 'win32'
const CF = fileURLToPath(new URL(`../../bin/${WINDOWS ? 'cf.exe' : 'cf'}`, import.meta.url))
const REPO = fileURLToPath(new URL('../..', import.meta.url))
const A = '01a09094-938f-7fd1-a2d3-315cf92b4559'
const TOKEN = 'private-launch-token-1234567890'

before(() => {
  assert.ok(existsSync(CF), `missing built cf: ${CF}; build it with npm run build:cf`)
})

/**
 * A stand-in `codex` for the supervisor: given `app-server --listen …` it is
 * Codex's server, starting thread A when asked; given `--remote …` it is the
 * TUI, which starts a thread through the broker, reads the broker's /session
 * with its token, and exits 7 (or waits to be ended). Each run writes what it
 * was given, and what it saw, to CF_TEST_CODEX_LOG. CF_TEST_CODEX_BACKEND says
 * how the server goes: `fail` at once, `die` once a thread starts; with
 * CF_TEST_CODEX_QUESTION it asks its client a question when a thread starts.
 *
 * The server listens where it is told, as Codex's does: on a Unix socket
 * (`unix://<path>`), or on loopback (`ws://127.0.0.1:0`, Windows'), where it
 * takes a port, says which on its stderr, and lets in only a connection that
 * brings the token whose SHA-256 it was given.
 */
function fakeCodex(root) {
  return fakeNodeExecutable(
    join(root, 'codex'),
    `#!${process.execPath}
import { createHash } from 'node:crypto'
import { appendFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
const WebSocket = createRequire(${JSON.stringify(join(REPO, 'package.json'))})('ws')
const args = process.argv.slice(2)
const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined)
const record = (entry) =>
  appendFileSync(process.env.CF_TEST_CODEX_LOG, JSON.stringify({ pid: process.pid, ...entry }) + '\\n')
const how = process.env.CF_TEST_CODEX_BACKEND
if (args.includes('app-server')) {
  record({ run: 'server', args, apiKey: process.env.OPENAI_API_KEY ?? null })
  if (how === 'fail') {
    process.stderr.write('not logged in\\n')
    process.exitCode = 1
  } else {
    const listen = option('--listen')
    const loopback = listen.startsWith('ws://')
    const hash = option('--ws-token-sha256')
    const server = createServer()
    new WebSocket.WebSocketServer({
      server,
      // On loopback, only the token whose SHA-256 it was given: any other is turned away.
      verifyClient: ({ req }) => {
        if (!loopback) return true
        const token = /^Bearer (.+)$/.exec(req.headers.authorization ?? '')?.[1]
        return token !== undefined && createHash('sha256').update(token, 'utf8').digest('hex') === hash
      },
    }).on('connection', (socket) => {
      socket.on('message', (raw) => {
        const message = JSON.parse(raw)
        if (message.id === undefined) return
        const start = message.method === 'thread/start'
        if (start) record({ run: 'server', started: message.params })
        const result = start ? { thread: { id: ${JSON.stringify(A)}, turns: [], status: { type: 'idle' } } } : {}
        socket.send(JSON.stringify({ id: message.id, result }), () => {
          if (start && how === 'die') process.exit(0)
        })
        // Codex's question tool: asked of its client, which the broker answers from the board.
        if (start && process.env.CF_TEST_CODEX_QUESTION)
          socket.send(
            JSON.stringify({
              id: 'ask-1',
              method: 'item/tool/requestUserInput',
              params: {
                threadId: ${JSON.stringify(A)},
                questions: [
                  { id: 'q', header: 'Which', question: 'Which one?', options: [{ label: 'a', description: 'first' }] },
                ],
              },
            }),
          )
      })
    })
    if (loopback)
      server.listen(0, '127.0.0.1', () => {
        process.stderr.write(
          'codex app-server (WebSockets)\\n  listening on: ws://127.0.0.1:' + server.address().port + '\\n',
        )
      })
    else server.listen(listen.slice('unix://'.length))
  }
} else {
  const [, remote, , tokenName] = args
  const token = process.env[tokenName]
  record({ run: 'tui', args, token, apiKey: process.env.OPENAI_API_KEY ?? null })
  const socket = new WebSocket(remote, { headers: { authorization: 'Bearer ' + token } })
  socket.on('error', () => {})
  let last = 0
  const call = (method, params) =>
    new Promise((resolve) => {
      const id = ++last
      const answer = (raw) => {
        if (JSON.parse(raw).id !== id) return
        socket.off('message', answer)
        resolve()
      }
      socket.on('message', answer)
      socket.send(JSON.stringify({ id, method, params }))
    })
  socket.on('open', async () => {
    await call('initialize', { clientInfo: { name: 'codex-tui', version: 'test' } })
    await call('thread/start', { ephemeral: false, threadSource: 'user' })
    const session = await fetch(remote.replace('ws:', 'http:') + '/session', {
      headers: { authorization: 'Bearer ' + token },
    })
    record({ run: 'tui', session: await session.json() })
    if (process.env.CF_TEST_CODEX_TUI !== 'wait') process.exit(7)
  })
}
`,
  )
}

async function freePort() {
  const server = createNetServer()
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  await new Promise((resolve) => server.close(resolve))
  return port
}

const alive = (pid) => {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/**
 * The supervisor as a Codex window runs it: `cf codex-session <codex> <args>`,
 * with the session bridge the channel configured, in a window's environment.
 */
async function supervised(t, args, extraEnv = {}, executable = fakeCodex) {
  const root = await mkdtemp(join(tmpdir(), 'cf-cx-'))
  let child = null
  let exited = null
  // A test that ends early closes the window as the app would: the
  // supervisor ends Codex's processes, then their files go.
  t.after(async () => {
    if (child !== null && child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM')
      const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
      await exited
      clearTimeout(stuck)
    }
    await rm(root, { recursive: true, force: true })
  })
  const codex = executable(root)
  const port = await freePort()
  const log = join(root, 'codex.jsonl')
  const inherited = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !name.startsWith('CONSENSFLOW_')),
  )
  child = spawn(CF, ['codex-session', codex, ...args], {
    env: {
      ...inherited,
      CONSENSFLOW_HOME: root,
      CONSENSFLOW_URL: 'http://127.0.0.1:1',
      CONSENSFLOW_TOKEN: 'window-token',
      CF_CODEX_SESSION_BRIDGE: JSON.stringify({ launchId: 'launch-1', port, token: TOKEN }),
      CF_TEST_CODEX_LOG: log,
      OPENAI_API_KEY: 'sk-not-for-codex',
      ...extraEnv,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  exited = once(child, 'exit')
  let stderr = ''
  child.stderr.on('data', (chunk) => {
    stderr += chunk
  })
  const runs = async () =>
    (await readFile(log, 'utf8').catch(() => ''))
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line))
  const until = async (predicate) => {
    for (let i = 0; i < 400; i++) {
      const seen = await runs()
      if (predicate(seen)) return seen
      await new Promise((resolve) => setTimeout(resolve, 25))
    }
    assert.fail(`the supervised Codex never got there: ${stderr}`)
  }
  return {
    root,
    port,
    child,
    runs,
    until,
    stderr: () => stderr,
    exit: async () => {
      const [code, signal] = await exited
      return { code, signal }
    },
  }
}

/** The private folder of the socket Codex's server was told to listen on: Unix only. */
const socketFolder = (server) =>
  dirname(server.args[server.args.indexOf('--listen') + 1].slice('unix://'.length))

it('opens Codex as its server on a private socket and its TUI through the broker, and leaves nothing behind', async (t) => {
  const s = await supervised(t, [
    '--model',
    'native-model',
    '--dangerously-bypass-approvals-and-sandbox',
  ])
  assert.deepEqual(await s.exit(), { code: 7, signal: null }, s.stderr())
  const runs = await s.runs()
  const serverRuns = runs.filter((entry) => entry.run === 'server')
  const tuiRuns = runs.filter((entry) => entry.run === 'tui')
  // On Unix a socket in a private folder of the home; on Windows loopback, on
  // a port the server picks, for a token of the window's own, which the
  // server is given as its SHA-256.
  const socket = WINDOWS ? null : join(socketFolder(serverRuns[0]), 'native.sock')
  const hash = serverRuns[0].args.at(-1)
  if (WINDOWS) assert.match(hash, /^[0-9a-f]{64}$/)
  else assert.ok(socket.startsWith(join(s.root, 'tmp', 'codex-')), socket)
  // The server gets the model and the bypass as configuration, and
  // ConsensFlow's own variables set in its shell policy.
  assert.deepEqual(serverRuns[0].args, [
    '-c',
    'model="native-model"',
    '-c',
    'approval_policy="never"',
    '-c',
    'sandbox_mode="danger-full-access"',
    '-c',
    `shell_environment_policy.set.CONSENSFLOW_HOME=${JSON.stringify(s.root)}`,
    '-c',
    'shell_environment_policy.set.CONSENSFLOW_TOKEN="window-token"',
    '-c',
    'shell_environment_policy.set.CONSENSFLOW_URL="http://127.0.0.1:1"',
    'app-server',
    ...(WINDOWS
      ? ['--listen', 'ws://127.0.0.1:0', '--ws-auth', 'capability-token', '--ws-token-sha256', hash]
      : ['--listen', `unix://${socket}`]),
  ])
  // The TUI reaches the server only through the broker, its token in its
  // environment and never on its command line.
  assert.deepEqual(tuiRuns[0].args, [
    '--remote',
    `ws://127.0.0.1:${s.port}`,
    '--remote-auth-token-env',
    'CF_CODEX_TUI_TOKEN',
    '--model',
    'native-model',
  ])
  assert.equal(tuiRuns[0].token, TOKEN)
  assert.deepEqual(
    [serverRuns[0].apiKey, tuiRuns[0].apiKey],
    [null, null],
    'an OpenAI API key never reaches Codex',
  )
  // The fresh thread the TUI started is the broker's to deliver to, with the bypass.
  assert.deepEqual(
    [serverRuns[1].started.approvalPolicy, serverRuns[1].started.sandbox],
    ['never', 'danger-full-access'],
  )
  assert.deepEqual(tuiRuns[1].session, {
    launchId: 'launch-1',
    sessionId: A,
    revision: 1,
    empty: true,
    available: true,
  })
  assert.equal(alive(serverRuns[0].pid), false, 'the server went with the window')
  if (!WINDOWS)
    assert.equal(existsSync(dirname(socket)), false, 'the socket folder went with the session')
})

it("says why Codex's server could not start, in the window, and leaves nothing behind", async (t) => {
  const s = await supervised(t, [], { CF_TEST_CODEX_BACKEND: 'fail' })
  assert.deepEqual(await s.exit(), { code: 1, signal: null })
  assert.equal(
    s.stderr(),
    'ConsensFlow could not open Codex: Codex server could not start: not logged in\n\n',
  )
  const runs = await s.runs()
  assert.deepEqual(
    runs.map((entry) => entry.run),
    ['server'],
    'no TUI was opened',
  )
  if (!WINDOWS) assert.equal(existsSync(socketFolder(runs[0])), false)
  // A Codex gone from where the launch found it fails the same way, in its
  // own words: Rust's, deliberately, where Node said `spawn <path> ENOENT`.
  // Its folder is still there, so Windows too says the file is missing (os
  // error 2), not its path (3).
  const missing = await supervised(t, [], {}, (root) => join(root, 'codex'))
  assert.deepEqual(await missing.exit(), { code: 1, signal: null })
  const gone = RegExp.escape(join(missing.root, 'codex'))
  assert.match(
    missing.stderr(),
    new RegExp(
      `^ConsensFlow could not open Codex: Codex server could not start: ${gone}: .*\\(os error 2\\)\\n$`,
    ),
  )
  if (!WINDOWS) assert.deepEqual(readdirSync(join(missing.root, 'tmp')), [])
})

for (const signal of ['SIGTERM', 'SIGINT']) {
  it(`ends both Codex processes when its window is closed (${signal})`, {
    skip: WINDOWS && 'no SIGTERM on Windows',
  }, async (t) => {
    const s = await supervised(t, [], { CF_TEST_CODEX_TUI: 'wait' })
    const seen = await s.until((runs) => runs.some((entry) => entry.session !== undefined))
    const server = seen.find((entry) => entry.run === 'server')
    const tui = seen.find((entry) => entry.run === 'tui')
    s.child.kill(signal)
    assert.deepEqual(await s.exit(), { code: 0, signal: null }, s.stderr())
    assert.deepEqual([alive(server.pid), alive(tui.pid)], [false, false])
    assert.equal(existsSync(socketFolder(server)), false)
  })
}

/** The board's API, with a question it is asked and never answers: the polls it saw. */
async function silentBoard(t) {
  const polls = []
  const server = createServer(async (request, response) => {
    for await (const _ of request);
    if (request.method === 'POST' && request.url === '/api/questions') {
      response.writeHead(201, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ message: { id: 61 } }))
    } else if (request.method === 'GET' && request.url.startsWith('/api/questions/61')) {
      polls.push(request.url)
    } else {
      response.writeHead(404).end()
    }
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => {
    server.closeAllConnections()
    server.close()
  })
  return { polls, url: `http://127.0.0.1:${server.address().port}` }
}

it("ends with its TUI even while a question of Codex's is still waiting at the board", async (t) => {
  // The board's door waits nearly an hour for the answer to a question; the
  // window, whose TUI went, must not wait with it.
  const board = await silentBoard(t)
  const s = await supervised(t, [], {
    CF_TEST_CODEX_QUESTION: '1',
    CF_TEST_CODEX_TUI: 'wait',
    CONSENSFLOW_URL: board.url,
  })
  const seen = await s.until((runs) => runs.some((entry) => entry.session !== undefined))
  for (let i = 0; i < 400 && board.polls.length === 0; i++)
    await new Promise((resolve) => setTimeout(resolve, 25))
  assert.ok(board.polls.length > 0, `the question is held at the board: ${s.stderr()}`)
  process.kill(seen.find((entry) => entry.run === 'tui').pid)
  let giveUp
  const gone = new Promise((resolve) => {
    giveUp = setTimeout(resolve, 15_000, 'it is still there')
  })
  const outcome = await Promise.race([s.exit(), gone])
  clearTimeout(giveUp)
  assert.notEqual(outcome, 'it is still there', 'the window did not end with its TUI')
  const server = seen.find((entry) => entry.run === 'server')
  assert.equal(alive(server.pid), false)
  if (!WINDOWS) assert.equal(existsSync(socketFolder(server)), false)
})

it("ends the TUI when Codex's server dies under it, so the window does not hang", async (t) => {
  const s = await supervised(t, [], { CF_TEST_CODEX_BACKEND: 'die', CF_TEST_CODEX_TUI: 'wait' })
  const { code } = await s.exit()
  // The TUI's code is the window's: 0 when a signal ended it. Windows has no
  // signal, and a TUI ended there (taskkill /F) has the code that gave it.
  if (!WINDOWS) assert.equal(code, 0, s.stderr())
  const runs = await s.runs()
  assert.equal(alive(runs.find((entry) => entry.run === 'tui').pid), false)
  if (!WINDOWS)
    assert.equal(existsSync(socketFolder(runs.find((entry) => entry.run === 'server'))), false)
})
