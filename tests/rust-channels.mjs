import { execFileSync, spawn } from 'node:child_process'
import { chmodSync, copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * The channels of `crates/cf-harness` in Rust, as the suites that hold them
 * call them: each function here runs one channel through a test binary, built
 * once, on the machine's own clock, randomness, processes and loopback.
 */

/** Why the cases against Rust do not run, or false: a machine without cargo cannot build the test binaries. */
export const cargoMissing = hasCargo() ? false : 'cargo is not installed'

function hasCargo() {
  try {
    execFileSync('cargo', ['--version'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

/** The folder this process keeps its copies of the test binaries in, made when the first is. */
let copies = null
/** The copies this process has made, by the binary's name. */
const mine = new Map()

/**
 * A copy of `executable` that this process alone runs. Every suite that runs a
 * channel builds the binary it needs, and the suites run at the same time: a
 * `cargo build` of a binary that is built puts it in place again, and for the
 * moment between taking the old one away and linking the new one the path it
 * reports names nothing. A copy of the binary is not touched by any build.
 */
function privateCopy(name, executable) {
  if (!mine.has(name)) {
    if (copies === null) {
      copies = mkdtempSync(join(tmpdir(), 'cf-rust-channels-'))
      process.once('exit', () => {
        try {
          rmSync(copies, { recursive: true, force: true })
        } catch {}
      })
    }
    const copy = join(copies, basename(executable))
    for (let attempt = 1; ; attempt += 1) {
      try {
        copyFileSync(executable, copy)
        chmodSync(copy, 0o755)
        break
      } catch (cause) {
        // The binary is back within the moment the build that took it away is over.
        if (cause.code !== 'ENOENT' || attempt === 100) throw cause
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50)
      }
    }
    mine.set(name, copy)
  }
  return mine.get(name)
}

/** The test binary `name`, built, and a copy of it to run (see `privateCopy`). */
function build(name) {
  return privateCopy(name, cargoBuild(name))
}

/** The path cargo reports for the test binary `name`, once it has built it. */
function cargoBuild(name) {
  const built = execFileSync(
    'cargo',
    [
      'build',
      '-p',
      'cf-harness',
      '--features',
      'test-support',
      '--bin',
      name,
      '--message-format=json',
    ],
    {
      cwd: fileURLToPath(new URL('..', import.meta.url)),
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  )
  return built
    .split('\n')
    .filter((line) => line.startsWith('{'))
    .map((line) => JSON.parse(line))
    .find(
      (message) =>
        message.reason === 'compiler-artifact' &&
        message.target.name === name &&
        message.executable,
    ).executable
}

/**
 * The pane host a target names, as a channel asks it (`claim`,
 * `src/channels/pty.js`): the target's own claim, or the bridge to the host.
 */
function paneHost(target) {
  return (op, body) =>
    typeof target.claim === 'function'
      ? target.claim(body)
      : target.bridge.request(op, body, { deadlineMs: target.deadlineMs })
}

/**
 * A question put to a test binary (`crates/cf-harness/src/bin/common/mod.rs`
 * says how): what it answered, or the failure it says JavaScript would have
 * thrown. What it asks of the pane host on the way is answered by `host` as it
 * asks, a host that throws as one that never answered.
 */
function ask(executable, question, host) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, [], { stdio: ['pipe', 'pipe', 'inherit'] })
    let heard = ''
    let last
    const hear = (line) => {
      const message = JSON.parse(line)
      if (message.ask === undefined) {
        last = message
        return
      }
      const reply = (answer) => child.stdin.write(`${JSON.stringify(answer)}\n`)
      host(message.ask.op, message.ask.body).then(
        (answer) => reply({ answer }),
        (cause) => reply({ throws: cause.message, error: cause.error }),
      )
    }
    child.on('error', reject)
    child.stdin.on('error', reject)
    child.stdout.setEncoding('utf8')
    child.stdout.on('data', (chunk) => {
      heard += chunk
      try {
        for (let end = heard.indexOf('\n'); end !== -1; end = heard.indexOf('\n')) {
          const line = heard.slice(0, end)
          heard = heard.slice(end + 1)
          hear(line)
        }
      } catch (cause) {
        reject(cause)
      }
    })
    child.on('close', (code) => {
      if (last === undefined) {
        reject(new Error(`${executable} ended with ${code} and said ${JSON.stringify(heard)}`))
      } else if (last.threw !== undefined) {
        reject(new Error(last.threw))
      } else {
        resolve(last.answered)
      }
    })
    child.stdin.write(`${JSON.stringify(question)}\n`)
  })
}

/**
 * Pi's channel, through `pi-send`. The process cannot call back for a claim,
 * so what the claim answers (or the failure it throws) is asked for first.
 */
export function rustPi() {
  const executable = build('pi-send')
  return {
    send: async (target, text) => {
      const { launchId, inbox, ack, ackTimeoutMs } = target.launch.channel
      const claim = await paneHost(target)('pane.claim', {
        pane: target.pane,
        generation: target.generation,
      }).catch((cause) => ({ throws: cause.message, error: cause.error }))
      const asked = {
        channel: { launchId, inbox, ack, ackTimeoutMs },
        session: target.session,
        pane: target.pane,
        generation: target.generation,
        claim,
        text,
      }
      const answered = await new Promise((resolve, reject) => {
        const child = spawn(executable, [], { stdio: ['pipe', 'pipe', 'inherit'] })
        let output = ''
        child.stdout.setEncoding('utf8')
        child.stdout.on('data', (chunk) => {
          output += chunk
        })
        child.on('error', reject)
        child.on('close', (code) => {
          try {
            resolve(JSON.parse(output))
          } catch {
            reject(new Error(`pi-send ended with ${code} and said ${JSON.stringify(output)}`))
          }
        })
        child.stdin.end(JSON.stringify(asked))
      })
      if (answered.threw !== undefined) throw new Error(answered.threw)
      return answered
    },
  }
}

/** Codex's channel, through `codex-send`: a send, and the broker's word on the window. */
export function rustCodex() {
  const executable = build('codex-send')
  return {
    send: (target, text) => {
      const { launchId, sessionBridge } = target.launch
      return ask(
        executable,
        {
          op: 'send',
          channel: { launchId, sessionBridge },
          session: target.session,
          pane: target.pane,
          generation: target.generation,
          text,
        },
        paneHost(target),
      )
    },
    sessionState: async ({ launchId, sessionBridge }) =>
      (await ask(executable, { op: 'shown', channel: { launchId, sessionBridge } })) ?? undefined,
  }
}

/**
 * OpenCode's channel, through `opencode-channel`. A `timeoutMs` is carried
 * (a window's are 15 s to create a session and 60 s to seed one); no caller
 * can cancel a seed, so a `signal` is not.
 */
export function rustOpenCode() {
  const executable = build('opencode-channel')
  return {
    send: (target, text) => {
      const { launchId, sessionBridge } = target.launch.channel
      return ask(
        executable,
        {
          op: 'send',
          channel: { launchId, sessionBridge },
          session: target.session,
          pane: target.pane,
          generation: target.generation,
          text,
        },
        paneHost(target),
      )
    },
    createSession: ({ executable: opencode, cwd, env, configuration, timeoutMs }) =>
      ask(executable, {
        op: 'create',
        executable: opencode,
        directory: cwd,
        env,
        configuration,
        timeoutMs,
      }),
    seedSession: async ({ channel, sessionId, cwd, text, model, variant, resume, timeoutMs }) => {
      await ask(executable, {
        op: 'seed',
        channel,
        session: sessionId,
        directory: cwd,
        text,
        model,
        variant,
        resume,
        timeoutMs,
      })
    },
  }
}
