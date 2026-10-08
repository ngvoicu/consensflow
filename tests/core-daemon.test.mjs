import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { assertStarted, linesOf, START_WORDS } from './choice.mjs'
import { daemonCommand, fakeNodeExecutable } from './helpers.mjs'

const BUNDLE_BIN = fileURLToPath(new URL('../bin', import.meta.url))
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/**
 * The cases that go through a process, run against the native daemon
 * (`npm run test:daemons`). What was a test of Node's own modules (the pass
 * loop, the API's doors, a Node preload) is held where the daemon is: the stop
 * and pass tests of `crates/cf-daemon/src/{stop,pass,errors}`, and the recorded
 * traces (`core-daemon-001`) it plays. What its start line says is `rust 3.0.0`.
 */
const STARTS = START_WORDS.native

/** The daemon as a child, started as `cf ui` is. */
function startDaemon(env) {
  const started = daemonCommand()
  return spawn(started.command, started.args, { env, stdio: ['pipe', 'pipe', 'pipe'] })
}

/** The environment of a daemon on `home`: this machine's, but for what the test's home gives. */
const homeEnv = (home) => ({
  ...process.env,
  HOME: home,
  CONSENSFLOW_HOME: home,
  CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
})

/**
 * A daemon already running on `home`, which holds its ledger: the other
 * ConsensFlow a second start is refused for. Resolves once it has said it is
 * ready; `stop` ends it by its input and waits for it to exit.
 */
async function runningDaemon(home) {
  const child = startDaemon(homeEnv(home))
  const exited = new Promise((resolve) => child.once('exit', resolve))
  let errors = ''
  child.stderr.on('data', (chunk) => {
    errors += chunk
  })
  const ready = new Promise((resolve) => child.stdout.once('data', () => resolve(true)))
  assert.ok(await Promise.race([ready, exited.then(() => false)]), `it did not start: ${errors}`)
  return {
    async stop() {
      child.stdin.end()
      const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
      await exited
      clearTimeout(stuck)
    },
  }
}

/** How long `work` took, failing past `limit` ms rather than waiting on it for good. */
async function timed(work, limit) {
  const started = Date.now()
  let timer
  const late = new Promise((resolve) => {
    timer = setTimeout(resolve, limit, 'late')
  })
  try {
    assert.notEqual(await Promise.race([work(), late]), 'late', `still waiting after ${limit} ms`)
    return Date.now() - started
  } finally {
    clearTimeout(timer)
  }
}

/** The daemon writes down what is worth knowing afterwards: its start, its stop and why. */
describe('the daemon and its log', () => {
  for (const [how, end, reason] of [
    ['its input ending', (child) => child.stdin.end(), 'stdin ended'],
    ['SIGTERM', (child) => child.kill('SIGTERM'), 'SIGTERM'],
  ]) {
    it(`starts its log with its pid and ends it with why it stopped: ${how}`, {
      skip:
        reason === 'SIGTERM' &&
        process.platform === 'win32' &&
        'Node on Windows never delivers SIGTERM',
    }, async () => {
      const home = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-'))
      try {
        const child = startDaemon(homeEnv(home))
        let errors = ''
        child.stderr.on('data', (chunk) => {
          errors += chunk
        })
        await new Promise((resolve) => child.stdout.once('data', resolve))
        end(child)
        const code = await new Promise((resolve) => child.once('exit', resolve))
        assert.equal(code, 0, errors)
        const log = await readFile(path.join(home, 'daemon.log'), 'utf8')
        // The runtime the start line names is the daemon's own: the native one's.
        assert.match(
          log,
          new RegExp(
            `^\\S+ info start pid ${child.pid} ${STARTS}\\S+ home \\S+\\n\\S+ info stop: ${reason}; rss \\d+ MB\\n\\S+ info exit 0\\n$`,
          ),
        )
      } finally {
        await rm(home, { recursive: true, force: true })
      }
    })
  }

  it('touches no window of the running daemon when a second start is refused its ledger', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-'))
    const launch = path.join(home, 'integrations', 'claude', '0b9f2c1e-5d4a-4c3b-9a8f-7e6d5c4b3a21')
    // A daemon is running on the home, and a window of its has its files.
    const running = await runningDaemon(home)
    try {
      await mkdir(launch, { recursive: true })
      await writeFile(path.join(launch, 'settings.json'), '{}\n')
      const child = startDaemon(homeEnv(home))
      let errors = ''
      child.stderr.on('data', (chunk) => {
        errors += chunk
      })
      await new Promise((resolve) => child.once('exit', resolve))
      const log = await readFile(path.join(home, 'daemon.log'), 'utf8').catch(() => '')
      assert.match(
        `${errors}${log}`,
        /another ConsensFlow has .*consensflow\.db open/,
        'the second start is refused',
      )
      assert.equal(
        await readFile(path.join(launch, 'settings.json'), 'utf8'),
        '{}\n',
        "the running daemon's window keeps its files",
      )
    } finally {
      await running.stop()
      await rm(home, { recursive: true, force: true })
    }
  })

  it('ends `cf ui` with 1 when a second start is refused its ledger, and its log says so', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-'))
    const running = await runningDaemon(home)
    try {
      const child = startDaemon(homeEnv(home))
      let errors = ''
      child.stderr.on('data', (chunk) => {
        errors += chunk
      })
      const code = await new Promise((resolve) => child.once('exit', resolve))
      assert.equal(code, 1, errors)
      assert.match(errors, /^cf: another ConsensFlow has .*consensflow\.db open\n$/)
      // What the process wrote, the log being the running daemon's too: the start
      // line, and right after it the exit logger's, which writes the code the
      // process ends with.
      const wrote = linesOf(await readFile(path.join(home, 'daemon.log'), 'utf8'), child.pid)
      assert.equal(wrote.length, 2, wrote.join('\n'))
      assert.match(
        wrote[0],
        new RegExp(`^\\S+ info start pid ${child.pid} ${STARTS}\\S+ home \\S+$`),
      )
      assert.match(wrote[1], /^\S+ info exit 1$/)
    } finally {
      await running.stop()
      await rm(home, { recursive: true, force: true })
    }
  })

  it('starts though its agents file cannot be used, and says why in its log', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-'))
    const file = path.join(home, 'agents.json')
    // A hand edit's trailing comma: the roster refuses the file and saves nothing over it.
    const broken = '{"schemaVersion": 1, "agents": [{"id": "mine", "kind": "codex"},]}\n'
    try {
      await writeFile(file, broken)
      const child = startDaemon(homeEnv(home))
      let errors = ''
      child.stderr.on('data', (chunk) => {
        errors += chunk
      })
      const exited = new Promise((resolve) => child.once('exit', resolve))
      const started = await Promise.race([
        new Promise((resolve) => child.stdout.once('data', () => resolve(true))),
        exited.then(() => false),
      ])
      assert.ok(started, `it did not start: ${errors}`)
      child.stdin.end()
      assert.equal(await exited, 0, errors)
      // The roster's words are under the line.
      assert.match(
        await readFile(path.join(home, 'daemon.log'), 'utf8'),
        /\n\S+ error the agents file could not be used\n {4}Your agents file .* is not valid JSON/,
      )
      assert.equal(await readFile(file, 'utf8'), broken, 'the file is left as the human wrote it')
    } finally {
      await rm(home, { recursive: true, force: true })
    }
  })
})

/**
 * The daemon as the app runs it, on a home of its own: its handle line, then
 * the bridge's JSON lines both ways. The test is the app's side of the
 * bridge: it asks what the page asks, and as the pane host it opens every
 * window it is asked to, though no window's program ever runs.
 */
async function daemonOverItsBridge(t, { agents = [] } = {}) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-bridge-'))
  let child = null
  let exited = null
  // The daemon stops before its home goes: it writes there until it exits.
  t.after(async () => {
    if (child !== null && child.exitCode === null && child.signalCode === null) {
      child.stdin.end()
      const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
      await exited
      clearTimeout(stuck)
    }
    await rm(root, { recursive: true, force: true })
  })
  const home = path.join(root, 'consensflow')
  const bin = path.join(root, 'bin')
  const workspace = path.join(root, 'workspace')
  for (const folder of [home, bin, workspace]) await mkdir(folder, { recursive: true })
  // A Claude to be found on PATH, and only it: no real harness is ever in reach.
  fakeNodeExecutable(path.join(bin, 'claude'), '// Never run: the test opens no window.\n')
  await writeFile(path.join(home, 'agents.json'), JSON.stringify({ schemaVersion: 1, agents }))
  const env = {
    ...Object.fromEntries(
      Object.entries(process.env).filter(
        ([name]) => !name.startsWith('CONSENSFLOW_') && name.toUpperCase() !== 'PATH',
      ),
    ),
    HOME: path.join(root, 'home'),
    // The home Node reads on Windows.
    USERPROFILE: path.join(root, 'home'),
    CONSENSFLOW_HOME: home,
    CLAUDE_CONFIG_DIR: path.join(root, 'home', '.claude'),
    CODEX_HOME: path.join(root, 'home', '.codex'),
    XDG_CONFIG_HOME: path.join(root, 'home', '.config'),
    PATH: bin,
  }
  child = startDaemon(env)
  exited = new Promise((resolve) => child.once('exit', resolve))
  let errors = ''
  child.stderr.on('data', (chunk) => {
    errors += chunk
  })

  const frames = []
  const answers = new Map()
  let handle = null
  let carry = ''
  const send = (frame) => child.stdin.write(`${JSON.stringify({ v: 1, ...frame })}\n`)
  /** The pane host's answers: a window opens at once, and goes when asked. */
  const host = ({ op, body }) =>
    op === 'pane.open'
      ? { ok: true, id: body.id, generation: body.generation }
      : op === 'pane.kill'
        ? { ok: true }
        : { ok: false, error: `${op} is not part of this test` }
  child.stdout.on('data', (chunk) => {
    carry += chunk
    for (let end = carry.indexOf('\n'); end !== -1; end = carry.indexOf('\n')) {
      const frame = JSON.parse(carry.slice(0, end))
      carry = carry.slice(end + 1)
      if (handle === null) {
        handle = frame
        continue
      }
      frames.push(frame)
      if (frame.kind === 'res') answers.get(frame.id)?.(frame.body)
      if (frame.kind === 'req') send({ id: frame.id, kind: 'res', op: frame.op, body: host(frame) })
    }
  })
  const until = async (found, what) => {
    for (let waited = 0; waited < 10_000; waited += 20) {
      const value = await found()
      if (value) return value
      await sleep(20)
    }
    assert.fail(`the daemon never ${what}: ${errors}`)
  }
  let asked = 0
  /** A request as the page makes it, answered over the bridge. */
  const request = async (op, body = {}) => {
    const id = `r-test-${++asked}`
    let answer
    answers.set(id, (value) => {
      answer = value
    })
    send({ id, kind: 'req', op, body })
    return until(() => answer, `answered ${op}`)
  }
  await until(() => handle, 'said it was ready')
  // The daemon that answers is the native one: its log's start line says so.
  assertStarted(await readFile(path.join(home, 'daemon.log'), 'utf8'), child.pid)
  return {
    home,
    workspace,
    env,
    child,
    handle,
    frames,
    request,
    until,
    exited,
    errors: () => errors,
  }
}

/** A saved agent of the human's own, on Claude Code. */
const MYBUILDER = {
  id: 'mybuilder',
  name: 'Mybuilder',
  kind: 'claude-code',
  model: 'fake',
  workTier: 'standard',
}

describe('the daemon over its bridge', () => {
  it("opens a window with the agents' API, its project and participant, and the bundled cf first on PATH, and names it no Node", {}, async (t) => {
    const d = await daemonOverItsBridge(t, { agents: [MYBUILDER] })
    const opened = await d.request('project.open', {
      directory: d.workspace,
      agent: 'mybuilder',
      staff: [{ agent: 'mybuilder', roles: ['worker', 'reviewer'] }],
    })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const open = await d.until(
      () => d.frames.find((frame) => frame.kind === 'req' && frame.op === 'pane.open'),
      'opened the chief',
    )
    assert.equal(open.body.id, `p${opened.project.id}-chief`)
    const { env } = open.body
    assert.deepEqual(
      [
        env.CONSENSFLOW_URL,
        env.CONSENSFLOW_PROJECT,
        env.CONSENSFLOW_PARTICIPANT,
        env.CONSENSFLOW_NODE,
        env.PATH,
      ],
      [
        d.handle.url.replace(/\/$/, ''),
        String(opened.project.id),
        'chief',
        // The app bundles no Node, and the daemon names none to a window.
        undefined,
        `${BUNDLE_BIN}${path.delimiter}${d.env.PATH}`,
      ],
    )
    assert.match(env.CONSENSFLOW_TOKEN, /^\S+$/)
    // The chief's role text: its staff with their roles and tiers, and the cf of this window.
    const { argv } = open.body
    const role = await readFile(argv[argv.indexOf('--append-system-prompt-file') + 1], 'utf8')
    assert.match(role, /^\| mybuilder \| worker, reviewer \| Standard work \|$/m)
    // On Windows, cf.exe, its path in forward slashes: Git Bash drops backslashes.
    const cf =
      process.platform === 'win32'
        ? `${BUNDLE_BIN.replaceAll('\\', '/')}/cf.exe`
        : path.join(BUNDLE_BIN, 'cf')
    assert.ok(role.includes(`Here \`cf\` is ${cf}.`), role.slice(-600))
  })

  it("tells the page when the board changes, and when a change to the agents moves a member's tier", {}, async (t) => {
    const d = await daemonOverItsBridge(t, { agents: [MYBUILDER] })
    const told = (reason) =>
      d.frames.filter(
        (frame) =>
          frame.kind === 'evt' && frame.op === 'state.changed' && frame.body.reason === reason,
      )
    await d.request('project.open', {
      directory: d.workspace,
      agent: 'mybuilder',
      staff: [{ agent: 'mybuilder', roles: ['worker'] }],
    })
    await d.until(() => told('core').length > 0, 'told the page of the new project')
    // The agents screens, as the app opens them with its token.
    const agents = (method, where, body) =>
      fetch(new URL(where, d.handle.url), {
        method,
        headers: { authorization: `Bearer ${d.handle.token}`, 'content-type': 'application/json' },
        body: JSON.stringify(body),
      })
    // A new agent moves nobody's tier; a member's agent given another does.
    const added = await agents('POST', 'api/agents', {
      name: 'myhelper',
      harness: 'claude',
      model: 'fake',
    })
    assert.equal(added.status, 201)
    const edited = await agents('PATCH', 'api/agents/mybuilder', { workTier: 'complex' })
    assert.equal(edited.status, 200)
    // A round trip on the bridge: every event sent before its answer has been read.
    assert.deepEqual(await d.request('ping'), { ok: true })
    assert.equal(told('roster').length, 1)
  })

  it("stops itself when the app's end of the bridge breaks", async (t) => {
    const d = await daemonOverItsBridge(t)
    d.child.stdout.destroy()
    d.child.stdin.write(
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'ping', body: {} })}\n`,
    )
    await timed(() => d.exited, 10_000)
    assert.equal(await d.exited, 0, d.errors())
    const log = await readFile(path.join(d.home, 'daemon.log'), 'utf8')
    // The bridge's own words for what broke are under the line.
    assert.match(log, /\n\S+ error the bridge failed\n {4}bridge I\/O error: /)
    assert.match(log, /\n\S+ info stop: the bridge failed; rss \d+ MB\n\S+ info exit 0\n$/)
  })
})
