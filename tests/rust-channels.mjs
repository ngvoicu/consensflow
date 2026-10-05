import { execFileSync, spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

/**
 * The channels of `crates/cf-harness` in Rust, as the suites that hold
 * JavaScript's channels call them: each function here has the signature of
 * the JavaScript one and runs the Rust one through a test binary, built once,
 * on the machine's own clock, randomness, processes and loopback. A suite
 * runs its cases with each in turn.
 */

/** Why the cases against Rust do not run, or false: a machine without cargo holds them against JavaScript alone. */
export const cargoMissing = hasCargo() ? false : 'cargo is not installed'

function hasCargo() {
  try {
    execFileSync('cargo', ['--version'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

/** The test binary `name`, built and found by the path cargo reports. */
function build(name) {
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
