import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertStarted, errorsWithCause, KINDS, linesOf, NATIVE_CF } from '../choice.mjs'
import { daemonCommand } from '../helpers.mjs'

const WINDOWS = process.platform === 'win32'
const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const BRIDGE =
  process.env.CONSENSFLOW_TEST_BRIDGE ??
  join(
    REPO,
    'app',
    'src-tauri',
    'target',
    'release',
    WINDOWS ? 'consensflow-bridge.exe' : 'consensflow-bridge',
  )
// A terminal asks where the cursor is; ConPTY asks before the child may print
// at all. The page's xterm answers, and with no page this harness does.
const CURSOR_QUERY = Buffer.from('\u001b[6n')
const CURSOR_REPLY = [...Buffer.from('\u001b[1;1R')]
const FAKE = join(dirname(fileURLToPath(import.meta.url)), 'fake-agent.mjs')
/** The `cf` first on every window's PATH: the native one, beside cf.mjs. */
const CF = NATIVE_CF

function parser(onLine) {
  let carry = Buffer.alloc(0)
  return {
    push(chunk) {
      carry = Buffer.concat([carry, Buffer.from(chunk)])
      for (;;) {
        const end = carry.indexOf(0x0a)
        if (end === -1) return
        const line = carry.subarray(0, end).toString('utf8').replace(/\r$/, '')
        carry = carry.subarray(end + 1)
        if (line.length > 0) onLine(line)
      }
    },
  }
}

function firstLine(readable, child) {
  return new Promise((resolve, reject) => {
    let carry = Buffer.alloc(0)
    const onData = (chunk) => {
      carry = Buffer.concat([carry, Buffer.from(chunk)])
      const end = carry.indexOf(0x0a)
      if (end === -1) return
      readable.removeListener('data', onData)
      // Hold what follows until the router is attached: a flowing stream with
      // no listener drops it, and the daemon's first frames are its restart
      // resume. The app's bridge keeps its reader, so it never loses them.
      readable.pause()
      resolve({
        line: carry.subarray(0, end).toString('utf8').replace(/\r$/, ''),
        rest: carry.subarray(end + 1),
      })
    }
    const onExit = (code, signal) => {
      readable.removeListener('data', onData)
      reject(new Error(`process exited before its handle: ${code ?? signal}`))
    }
    readable.on('data', onData)
    child.once('exit', onExit)
  })
}

function exited(child, timeoutMs = 5_000) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode })
  }
  return new Promise((resolveExit, reject) => {
    const timer = setTimeout(() => reject(new Error('process did not exit')), timeoutMs)
    child.once('exit', (code, signal) => {
      clearTimeout(timer)
      resolveExit({ code, signal })
    })
  })
}

function safeEnvironment(root, fakeBin, harnessFile) {
  const home = join(root, 'home')
  return {
    HOME: home,
    CONSENSFLOW_HOME: join(root, 'consensflow'),
    CONSENSFLOW_BIN_DIR: join(root, 'consensflow', 'bin'),
    CLAUDE_CONFIG_DIR: join(home, '.claude'),
    CODEX_HOME: join(home, '.codex'),
    XDG_CONFIG_HOME: join(home, '.config'),
    PATH: WINDOWS
      ? [fakeBin, join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')].join(delimiter)
      : `${fakeBin}:/usr/local/bin:/usr/bin:/bin`,
    // What Windows itself needs to start a process, and the home Node reads there.
    ...(WINDOWS
      ? {
          SystemRoot: process.env.SystemRoot,
          ComSpec: process.env.ComSpec,
          PATHEXT: process.env.PATHEXT,
          TEMP: tmpdir(),
          TMP: tmpdir(),
          USERPROFILE: home,
        }
      : {}),
    // The Node the stand-in harness runs on: the tests' own, not a product's.
    CF_TEST_NODE: process.execPath,
    CF_TEST_HARNESS: harnessFile,
    CF_TEST_WORKER_ANSWER: 'worker completed from a real PTY child',
    TERM: 'xterm-256color',
  }
}

function writeFakeInstall(root, sandbox, harnessFile) {
  const fakeBin = join(root, 'fake-bin')
  const projects = join(sandbox.CLAUDE_CONFIG_DIR, 'projects', 'integration')
  mkdirSync(fakeBin, { recursive: true })
  mkdirSync(projects, { recursive: true })
  // On Windows the shape of an npm shim, naming node and the script outright,
  // which is how a window opens on it.
  if (WINDOWS) {
    writeFileSync(
      join(fakeBin, 'claude.cmd'),
      `@echo off\r\n"${process.execPath}" "${harnessFile}" %*\r\n`,
    )
  } else {
    writeFileSync(
      join(fakeBin, 'claude'),
      '#!/bin/sh\nexec "$CF_TEST_NODE" "$CF_TEST_HARNESS" "$@"\n',
      { mode: 0o755 },
    )
  }
  return fakeBin
}

function writeRoster(env) {
  mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })
  writeFileSync(
    join(env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        agents: [
          { id: 'chief', kind: 'claude-code', model: 'fake-chief' },
          { id: 'worker', kind: 'claude-code', model: 'fake' },
        ],
      },
      null,
      2,
    )}\n`,
  )
}

function waitFor(predicate, timeoutMs = 10_000, intervalMs = 25) {
  const started = Date.now()
  return new Promise((resolveWait, reject) => {
    const tick = async () => {
      try {
        if (await predicate()) return resolveWait()
      } catch (cause) {
        return reject(cause)
      }
      if (Date.now() - started >= timeoutMs)
        return reject(new Error('timed out waiting for integration state'))
      setTimeout(tick, intervalMs)
    }
    tick()
  })
}

/**
 * A real standalone daemon and the real Rust headless bridge, connected by
 * their production JSON-lines pipes. The helper only observes and routes the
 * bytes; pane.open, PTYs, input arbitration and cleanup stay native.
 *
 * The daemon is the one `CONSENSFLOW_TEST_DAEMON` names (`daemonCommand`), or the
 * one `select` names (`node`, `native`, or a command as a JSON array, which is
 * the native one's) for a start that chooses in its own words whatever the
 * environment says, as a restart on the other daemon does. What starts is held
 * to what was asked for by the start line in its log (`assertStarted`), and
 * `daemon` on what this returns says which it was.
 */
export async function startIntegration({
  fakeEnv = {},
  bridgeEnv = {},
  existingRoot = null,
  select = undefined,
} = {}) {
  assert.equal(
    existsSync(BRIDGE),
    true,
    `missing built bridge: ${BRIDGE}; build it with npm run build:bridge`,
  )
  assert.equal(existsSync(CF), true, `missing built cf: ${CF}; build it with npm run build:cf`)
  const root = existingRoot ?? mkdtempSync(join(tmpdir(), 'consensflow-integration-'))
  const workspace = join(root, 'workspace')
  mkdirSync(workspace, { recursive: true })
  const initial = safeEnvironment(root, join(root, 'fake-bin'), FAKE)
  const fakeBin = writeFakeInstall(root, initial, fakeEnv.CF_TEST_HARNESS ?? FAKE)
  // A null override removes the sandbox default: a run on the real harnesses
  // must leave CLAUDE_CONFIG_DIR unset, so Claude keeps its own home config.
  const env = Object.fromEntries(
    Object.entries({ ...safeEnvironment(root, fakeBin, FAKE), ...fakeEnv }).filter(
      ([, value]) => value !== null && value !== undefined,
    ),
  )
  // A restart over the same home keeps its roster, as the app's does: the
  // agents a test wrote are still the ones its chief and staff run on.
  if (existingRoot === null) writeRoster(env)

  // The daemon under test, chosen on purpose (see above), in the home it runs
  // on: the product's `cf` verbs choose by the file in the home, so the home
  // has the file for Node's daemon and none for the native one's, and a
  // restart of it on the other daemon takes the file away or makes it.
  const asked = daemonCommand({
    ...(select === undefined
      ? {}
      : { named: select, leg: KINDS.includes(select) ? select : 'native' }),
    home: env.CONSENSFLOW_HOME,
  })
  const node = spawn(asked.command, asked.args, {
    cwd: REPO,
    env: { ...env, ...asked.env },
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const nodeErrors = []
  node.stderr.on('data', (chunk) => nodeErrors.push(String(chunk)))
  const nodeStderrEnded = new Promise((resolve) => node.stderr.once('close', resolve))
  const handleLine = await firstLine(node.stdout, node).catch(async (cause) => {
    // A daemon that refuses its home says why on its standard error, which may
    // still be on its way when the process is seen to exit, and Node's says it
    // in its log: it ends with 0, its refusal an unhandled rejection it logged.
    await Promise.race([nodeStderrEnded, new Promise((resolve) => setTimeout(resolve, 1000))])
    let logged = []
    try {
      const log = readFileSync(join(env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8')
      logged = errorsWithCause(linesOf(log, node.pid))
    } catch {}
    cause.message += [nodeErrors.join('').trim(), ...logged]
      .filter(Boolean)
      .map((said) => `: ${said}`)
      .join('')
    throw cause
  })
  // Both daemons write their start line before the handle line. One that is not
  // the one asked for is ended, with the home it was given if it was this call's.
  let handle
  let said
  try {
    said = assertStarted(
      asked,
      readFileSync(join(env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8'),
      node.pid,
      env.CONSENSFLOW_HOME,
    )
    handle = JSON.parse(handleLine.line)
    assert.match(handle.url, /^http:\/\/127\.0\.0\.1:\d+\/$/)
  } catch (cause) {
    node.kill('SIGKILL')
    if (existingRoot === null) rmSync(root, { recursive: true, force: true })
    throw cause
  }

  const nodeFrames = []
  const rustFrames = []
  const openFrames = []
  // What every pane printed and how it ended, for a failure to explain itself.
  const outputs = new Map()
  const exits = []
  const nodePending = new Map()
  const rustPending = new Map()
  const rust = spawn(BRIDGE, [], {
    cwd: REPO,
    env: { ...env, ...bridgeEnv },
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const rustErrors = []
  rust.stderr.on('data', (chunk) => rustErrors.push(String(chunk)))
  const rustHandleLine = await firstLine(rust.stdout, rust)
  const rustHandle = JSON.parse(rustHandleLine.line)
  assert.equal(rustHandle.kind, 'consensflow-bridge')

  // The page request is the same direction as the desktop app's
  // Rust-originated request: inject an `r-` request into Node's stdin and
  // intercept only its response on Node's stdout. Rust receives all other
  // Node-originated requests, while this test-owned response is not an
  // unsolicited frame for the Rust bridge.
  const nodeToRust = parser((line) => {
    const frame = JSON.parse(line)
    nodeFrames.push(frame)
    if (frame.kind === 'req' && frame.op === 'pane.open') openFrames.push(frame.body)
    if (frame.kind === 'res' && nodePending.has(frame.id)) {
      nodePending.get(frame.id).resolve(frame.body)
      return
    }
    if (!rust.stdin.destroyed) rust.stdin.write(`${line}\n`)
  })
  const rustToNode = parser((line) => {
    const frame = JSON.parse(line)
    rustFrames.push(frame)
    if (frame.op === 'pane.output' && Array.isArray(frame.body?.bytes)) {
      const key = frame.body.id
      const bytes = Buffer.from(frame.body.bytes)
      outputs.set(key, [...(outputs.get(key) ?? []), bytes])
      if (bytes.includes(CURSOR_QUERY)) {
        const { id, generation } = frame.body
        request('pane.reply', { id, generation, bytes: CURSOR_REPLY }).catch(() => {})
      }
    }
    if (frame.op === 'pane.exit') exits.push(frame.body)
    if (frame.kind === 'res' && rustPending.has(frame.id)) {
      rustPending.get(frame.id).resolve(frame.body)
    }
    if (!node.stdin.destroyed) node.stdin.write(`${line}\n`)
  })

  rust.stdout.on('data', (chunk) => {
    rustToNode.push(chunk)
  })
  rust.stdout.on('error', () => {})
  rust.once('close', () => {
    if (!node.stdin.destroyed) node.stdin.end()
  })
  node.stdout.on('data', (chunk) => {
    nodeToRust.push(chunk)
  })
  node.stdout.on('error', () => {})
  // Both processes emit a handle for their parent. The native headless
  // bridge does not consume the daemon's handle; after both observations only
  // the buffered post-handshake frames are wired in either direction.
  if (handleLine.rest.length > 0) {
    nodeToRust.push(handleLine.rest)
  }
  if (rustHandleLine.rest.length > 0) {
    rustToNode.push(rustHandleLine.rest)
  }
  node.stdout.resume()
  rust.stdout.resume()
  node.stdin.on('error', () => {})
  rust.stdin.on('error', () => {})

  let requestNumber = 0
  const request = (op, body) => {
    const id = `n-test-${++requestNumber}`
    const line = `${JSON.stringify({ v: 1, id, kind: 'req', op, body })}\n`
    return new Promise((resolveRequest, reject) => {
      const timer = setTimeout(() => {
        rustPending.delete(id)
        reject(
          new Error(
            `timed out waiting for ${op}; node=${nodeFrames.length} rust=${rustFrames.length}; ` +
              `node stderr=${nodeErrors.join('').trim()} rust stderr=${rustErrors.join('').trim()}`,
          ),
        )
      }, 10_000)
      rustPending.set(id, {
        resolve(value) {
          clearTimeout(timer)
          rustPending.delete(id)
          resolveRequest(value)
        },
      })
      rust.stdin.write(line)
    })
  }

  let pageRequestNumber = 0
  const requestNode = (op, body) => {
    const id = `r-test-${++pageRequestNumber}`
    const line = `${JSON.stringify({ v: 1, id, kind: 'req', op, body })}\n`
    return new Promise((resolveRequest, reject) => {
      const timer = setTimeout(() => {
        nodePending.delete(id)
        reject(new Error(`timed out waiting for Node ${op}`))
      }, 10_000)
      nodePending.set(id, {
        resolve(value) {
          clearTimeout(timer)
          nodePending.delete(id)
          resolveRequest(value)
        },
      })
      node.stdin.write(line)
    })
  }

  const transcript = (sessionId) => {
    const file = join(env.CLAUDE_CONFIG_DIR, 'projects', 'integration', `${sessionId}.jsonl`)
    try {
      return readFileSync(file, 'utf8')
    } catch {
      return ''
    }
  }

  const processes = () => {
    const file = join(root, 'processes.jsonl')
    try {
      return readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((line) => {
          const [pid, sessionId] = line.split('\t')
          return { pid: Number(pid), sessionId }
        })
    } catch {
      return []
    }
  }

  const pidAlive = (pid) => {
    try {
      process.kill(pid, 0)
      return true
    } catch {
      return false
    }
  }

  let closed = false
  return {
    root,
    env,
    workspace,
    handle,
    /** Which daemon this is, as its own start line says: `kind` (`node` or `native`), `runtime` (`node v26.8.1`, `rust 3.0.0-alpha.79`) and `line`. */
    daemon: { kind: said.kind, runtime: said.runtime, line: said.line },
    nodeFrames,
    rustFrames,
    openFrames,
    /** A window's pane.open frame, once it is sent: an operation answers before its window opens. */
    async openFrame(id, timeoutMs = 10_000) {
      await waitFor(() => openFrames.some((frame) => frame.id === id), timeoutMs)
      return openFrames.find((frame) => frame.id === id)
    },
    exits,
    /**
     * Everything a pane printed so far, control sequences stripped, newest
     * last. Windows' console host draws a run of blanks as a cursor move and
     * starts a line by jumping to it: those stay the blanks and the line
     * break they stand for, so text reads the same on every platform.
     */
    output(paneId) {
      const esc = String.fromCharCode(27)
      const bell = String.fromCharCode(7)
      return Buffer.concat(outputs.get(paneId) ?? [])
        .toString('utf8')
        .replace(new RegExp(`${esc}\\[(\\d*)C`, 'g'), (_, count) =>
          ' '.repeat(Math.min(Number(count || 1), 512)),
        )
        .replace(new RegExp(`${esc}\\[[0-9;]*[Hf]`, 'g'), '\n')
        .replace(new RegExp(`${esc}\\[[0-9;?]*[ -/]*[@-~]`, 'g'), '')
        .replace(new RegExp(`${esc}\\][^${bell}]*${bell}`, 'g'), '')
        .replaceAll('\r', '')
    },
    request,
    requestNode,
    requestRust: request,
    /**
     * The human types into the chief's terminal, the one way work reaches the
     * chief: a bracketed paste of `text`, then Enter, once the chief is idle.
     */
    async tell(project, text, { idleMs = 60_000 } = {}) {
      const chief = async () =>
        (await requestNode('board.get', { project })).board.lanes.find(
          (lane) => lane.participant.handle === 'chief',
        )
      await waitFor(async () => (await chief())?.activity?.state === 'idle', idleMs)
      const { pane } = await chief()
      const typed = await request('pane.input', {
        id: pane.id,
        generation: pane.generation,
        bytes: [...Buffer.from(`\u001b[200~${text}\u001b[201~\r`)],
      })
      assert.equal(typed.ok, true, JSON.stringify(typed))
    },
    killRust(signal = 'SIGKILL') {
      return rust.kill(signal)
    },
    /** The app's own quit order: the daemon dies first, then the pane host. */
    killDaemon(signal = 'SIGKILL') {
      return node.kill(signal)
    },
    rustExited() {
      return rust.exitCode !== null || rust.signalCode !== null
    },
    daemonExited() {
      return node.exitCode !== null || node.signalCode !== null
    },
    rustPid() {
      return rust.pid
    },
    daemonPid() {
      return node.pid
    },
    signalRust(signal) {
      return process.kill(rust.pid, signal)
    },
    // A timeout names what both processes said on stderr: an automatic resume
    // that failed, for one, logs there and nowhere else.
    waitFor: (predicate, timeoutMs, intervalMs) =>
      waitFor(predicate, timeoutMs, intervalMs).catch((cause) => {
        cause.message += `; node stderr=${nodeErrors.join('').trim()} rust stderr=${rustErrors.join('').trim()}`
        throw cause
      }),
    transcript,
    processes,
    pidAlive,
    async close({ preserveRoot = false } = {}) {
      if (closed) {
        if (!preserveRoot) rmSync(root, { recursive: true, force: true })
        return
      }
      closed = true
      node.stdin.end()
      rust.stdin.end()
      await Promise.all([exited(node), exited(rust)])
      const pidsFile = join(root, 'pids.jsonl')
      if (existsSync(pidsFile)) {
        // The bridge kills each pane's process as it shuts down, and a killed
        // process leaves the process table a moment later (once it is reaped):
        // one still there after a few seconds was left behind.
        for (const line of readFileSync(pidsFile, 'utf8').split('\n').filter(Boolean)) {
          const pid = Number(line)
          await waitFor(() => !pidAlive(pid), 3000).catch(() => {
            throw new Error(`fake harness process remains after bridge shutdown: ${pid}`)
          })
        }
      }
      if (!preserveRoot) rmSync(root, { recursive: true, force: true })
    },
  }
}
