import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { workspaceKey } from '../../hosts/lib/state.js'

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const CF = join(REPO, 'bin', 'cf.mjs')
const BRIDGE = join(REPO, 'app', 'src-tauri', 'target', 'debug', 'consensflow-bridge')
const FAKE = join(dirname(fileURLToPath(import.meta.url)), 'fake-claude.mjs')

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
    CONSENSFLOW_BIN_DIR: join(root, 'user-bin'),
    CLAUDE_CONFIG_DIR: join(home, '.claude'),
    CODEX_HOME: join(home, '.codex'),
    XDG_CONFIG_HOME: join(home, '.config'),
    PATH: `${fakeBin}:/usr/local/bin:/usr/bin:/bin`,
    CONSENSFLOW_NODE: process.execPath,
    CF_TEST_HARNESS: harnessFile,
    CF_TEST_WORKER_ANSWER: 'worker completed from a real PTY child',
    TERM: 'xterm-256color',
  }
}

function writeFakeInstall(root, env) {
  const fakeBin = join(root, 'fake-bin')
  const projects = join(env.CLAUDE_CONFIG_DIR, 'projects', 'integration')
  mkdirSync(fakeBin, { recursive: true })
  mkdirSync(projects, { recursive: true })
  writeFileSync(
    join(fakeBin, 'claude'),
    '#!/bin/sh\nexec "$CONSENSFLOW_NODE" "$CF_TEST_HARNESS" "$@"\n',
    { mode: 0o755 },
  )
  return fakeBin
}

function writeRoster(env) {
  mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })
  writeFileSync(
    join(env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify({ schemaVersion: 1, agents: [{ id: 'worker', kind: 'claude-code', model: 'fake' }] }, null, 2)}\n`,
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
 * A real standalone editor and the real Rust headless bridge, connected by
 * their production JSON-lines pipes. The helper only observes and routes the
 * bytes; pane.open, PTYs, input arbitration and cleanup stay native.
 */
export async function startIntegration({ fakeEnv = {}, existingRoot = null } = {}) {
  assert.equal(existsSync(BRIDGE), true, `missing built bridge: ${BRIDGE}`)
  const root = existingRoot ?? mkdtempSync(join(tmpdir(), 'consensflow-integration-'))
  const workspace = join(root, 'workspace')
  mkdirSync(workspace, { recursive: true })
  const initial = safeEnvironment(root, join(root, 'fake-bin'), FAKE)
  const fakeBin = writeFakeInstall(root, initial)
  const env = { ...safeEnvironment(root, fakeBin, FAKE), ...fakeEnv }
  writeRoster(env)

  const ui = spawn(process.execPath, [CF, 'ui', '--json', '--no-open'], {
    cwd: REPO,
    env,
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const uiErrors = []
  ui.stderr.on('data', (chunk) => uiErrors.push(String(chunk)))
  const handleLine = await firstLine(ui.stdout, ui)
  const handle = JSON.parse(handleLine.line)
  assert.match(handle.url, /^http:\/\/127\.0\.0\.1:\d+\/$/)

  const nodeFrames = []
  const rustFrames = []
  const openFrames = []
  const nodePending = new Map()
  const rustPending = new Map()
  const rust = spawn(BRIDGE, [], { cwd: REPO, env, stdio: ['pipe', 'pipe', 'pipe'] })
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
    if (frame.kind === 'res' && rustPending.has(frame.id)) {
      rustPending.get(frame.id).resolve(frame.body)
    }
    if (!ui.stdin.destroyed) ui.stdin.write(`${line}\n`)
  })

  rust.stdout.on('data', (chunk) => {
    rustToNode.push(chunk)
  })
  rust.stdout.on('error', () => {})
  rust.once('close', () => {
    if (!ui.stdin.destroyed) ui.stdin.end()
  })
  ui.stdout.on('data', (chunk) => {
    nodeToRust.push(chunk)
  })
  ui.stdout.on('error', () => {})
  // Both processes emit a handle for their parent. The native headless
  // bridge does not consume the editor handle; after both observations only
  // the buffered post-handshake frames are wired in either direction.
  if (handleLine.rest.length > 0) {
    nodeToRust.push(handleLine.rest)
  }
  if (rustHandleLine.rest.length > 0) {
    rustToNode.push(rustHandleLine.rest)
  }
  ui.stdin.on('error', () => {})
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
              `ui stderr=${uiErrors.join('').trim()} rust stderr=${rustErrors.join('').trim()}`,
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
      ui.stdin.write(line)
    })
  }

  const http = async (pathname, { method = 'GET', body, token = handle.token } = {}) => {
    const response = await fetch(`${handle.url}${pathname.replace(/^\//, '')}`, {
      method,
      headers: {
        authorization: `Bearer ${token}`,
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    const text = await response.text()
    return { status: response.status, body: text.length === 0 ? null : JSON.parse(text) }
  }

  const runCli = (args, extraEnv = {}, timeoutMs = 10_000) =>
    new Promise((resolveRun, rejectRun) => {
      const child = spawn(process.execPath, [CF, ...args], {
        cwd: workspace,
        env: { ...env, ...extraEnv },
        stdio: ['pipe', 'pipe', 'pipe'],
      })
      let stdout = ''
      let stderr = ''
      let settled = false
      const timer = setTimeout(() => {
        if (settled) return
        child.kill('SIGKILL')
        rejectRun(new Error(`CLI timed out: cf ${args.join(' ')}`))
      }, timeoutMs)
      child.stdout.on('data', (chunk) => {
        stdout += String(chunk)
      })
      child.stderr.on('data', (chunk) => {
        stderr += String(chunk)
      })
      child.on('error', (cause) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        rejectRun(cause)
      })
      child.on('close', (code, signal) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        resolveRun({ code, signal, stdout, stderr })
      })
    })

  const threads = (dir = workspace) => {
    const file = join(env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(dir), 'threads.json')
    try {
      return JSON.parse(readFileSync(file, 'utf8'))
    } catch {
      return {}
    }
  }

  const deliveries = (dir = workspace) => {
    const file = join(env.CONSENSFLOW_HOME, 'workspaces', workspaceKey(dir), 'deliveries.json')
    try {
      return Object.values(JSON.parse(readFileSync(file, 'utf8')))
    } catch {
      return []
    }
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
    nodeFrames,
    rustFrames,
    openFrames,
    request,
    requestNode,
    requestRust: request,
    killRust(signal = 'SIGKILL') {
      return rust.kill(signal)
    },
    rustExited() {
      return rust.exitCode !== null || rust.signalCode !== null
    },
    uiExited() {
      return ui.exitCode !== null || ui.signalCode !== null
    },
    rustPid() {
      return rust.pid
    },
    signalRust(signal) {
      return process.kill(rust.pid, signal)
    },
    http,
    async openTab(options = {}) {
      const opened = await http('/api/tabs', {
        method: 'POST',
        body: { dir: workspace, harness: 'claude-code', ...options },
      })
      assert.equal(opened.status, 201, JSON.stringify(opened.body))
      return opened.body
    },
    runCli,
    waitFor,
    threads,
    deliveries,
    transcript,
    processes,
    pidAlive,
    async close({ preserveRoot = false } = {}) {
      if (closed) {
        if (!preserveRoot) rmSync(root, { recursive: true, force: true })
        return
      }
      closed = true
      ui.stdin.end()
      rust.stdin.end()
      await Promise.all([exited(ui), exited(rust)])
      const pidsFile = join(root, 'pids.jsonl')
      if (existsSync(pidsFile)) {
        for (const line of readFileSync(pidsFile, 'utf8').split('\n').filter(Boolean)) {
          const pid = Number(line)
          try {
            process.kill(pid, 0)
          } catch {
            continue
          }
          throw new Error(`fake harness process remains after bridge shutdown: ${pid}`)
        }
      }
      if (!preserveRoot) rmSync(root, { recursive: true, force: true })
    },
  }
}
