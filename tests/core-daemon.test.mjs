import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { Credentials, startApi } from '../src/core/api.js'
import { passLoop } from '../src/core/daemon.js'
import { openLedger } from '../src/ledger/index.js'
import { daemonCommand, fakeNodeExecutable } from './helpers.mjs'

const DAEMON = fileURLToPath(new URL('./integration/core-daemon.mjs', import.meta.url))
const BUNDLE_BIN = fileURLToPath(new URL('../bin', import.meta.url))
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/**
 * The same cases run against both daemons (`npm run test:daemons`): Node's, and
 * the native one `CONSENSFLOW_TEST_DAEMON` names. What only Node's own modules
 * can show (a pass loop, an API, a Node preload) is skipped for the native one
 * with its reason.
 */
const NATIVE = daemonCommand([DAEMON]).native
const ONLY_NODE_CAN = NATIVE && "a test of Node's own modules, which the native daemon has none of"

/** The daemon as a child, the one under test; Node flags only go to Node's. */
function startDaemon(env, nodeFlags = []) {
  const started = daemonCommand([...nodeFlags, DAEMON])
  return spawn(started.command, started.args, {
    env: { ...env, ...started.env },
    stdio: ['pipe', 'pipe', 'pipe'],
  })
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

/** The app ends the daemon 2 s after asking it to stop, so a stop takes no more than about 1.5 s. */
describe("the daemon's stop", () => {
  it('waits only a moment for a pass held up by a slow window', {
    skip: ONLY_NODE_CAN,
  }, async () => {
    const loop = passLoop(() => new Promise(() => {}))
    loop.kick()
    await sleep(20)
    assert.ok((await timed(() => loop.stop(), 3_000)) < 1_500)
  })

  it('answers a door still waiting for an answer at once, so the API closes', {
    skip: ONLY_NODE_CAN,
  }, async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-stop-'))
    const ledger = openLedger(path.join(dir, 'consensflow.db'))
    const credentials = new Credentials()
    const api = await startApi({ ledger, credentials })
    try {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code' },
      })
      ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const token = credentials.issue({ participant: zeus, project })
      const asked = ledger.ask(project.id, { from: 'zeus', to: 'chief', body: 'Which?' })
      const polling = fetch(`${api.url}/api/questions/${asked.id}?wait=25000`, {
        headers: { authorization: `Bearer ${token}` },
      })
      await sleep(100)
      assert.ok((await timed(() => api.close(), 5_000)) < 1_000)
      const answered = await polling
      assert.deepEqual([answered.status, (await answered.json()).answer], [200, null])
    } finally {
      ledger.close()
      await rm(dir, { recursive: true, force: true })
    }
  })
})

/** A log that keeps its lines, as `daemonLog` writes them, without the time and the stack. */
function lineLog() {
  const lines = []
  return {
    lines,
    info: (message) => lines.push(`info ${message}`),
    warn: (message) => lines.push(`warn ${message}`),
    error: (message, error) => lines.push(`error ${message}: ${error.message}`),
  }
}

describe("the daemon's passes", () => {
  it('runs one pass at a time: kicks during a pass run one more after it', {
    skip: ONLY_NODE_CAN,
  }, async (t) => {
    // Only the kicks run passes here: the once-a-second timer stands still.
    t.mock.timers.enable({ apis: ['setInterval'] })
    let passes = 0
    let running = 0
    let most = 0
    let release
    const loop = passLoop(async () => {
      passes += 1
      running += 1
      most = Math.max(most, running)
      if (passes === 1) await new Promise((resolve) => (release = resolve))
      running -= 1
    })
    loop.kick()
    await sleep(20)
    loop.kick()
    loop.kick()
    await sleep(20)
    assert.equal(passes, 1, 'the kicks wait for the pass in progress')
    release()
    await sleep(20)
    await loop.stop()
    assert.deepEqual([passes, most], [2, 1])
  })

  it('writes down a pass longer than five seconds, and every ten minutes that it is alive and how its passes went', {
    skip: ONLY_NODE_CAN,
  }, async (t) => {
    t.mock.timers.enable({ apis: ['setInterval', 'Date'] })
    const log = lineLog()
    const durations = [5_000, 6_000, 0]
    const loop = passLoop(async () => {
      t.mock.timers.setTime(Date.now() + (durations.shift() ?? 0))
    }, log)
    for (let i = 0; i < 3; i++) {
      loop.kick()
      await sleep(10)
    }
    assert.deepEqual(log.lines, ['warn a pass took 6000 ms'])
    // Ten minutes: the once-a-second timer started a pass, and the others it
    // fired while that one ran ask for one more after it.
    t.mock.timers.tick(10 * 60_000)
    assert.match(log.lines[1], /^info alive: 3 passes, slowest 6000 ms, rss \d+ MB, heap \d+ MB$/)
    // Each line counts the passes since the one before: those two.
    await sleep(20)
    t.mock.timers.tick(10 * 60_000)
    assert.match(log.lines[2], /^info alive: 2 passes, slowest 0 ms, rss \d+ MB, heap \d+ MB$/)
    await loop.stop()
    assert.equal(log.lines.length, 3)
  })
})

/** The daemon writes down what is worth knowing afterwards: its start, its stop and why, a pass that failed. */
describe('the daemon and its log', () => {
  it('writes a failed pass down and goes on with the next', { skip: ONLY_NODE_CAN }, async () => {
    const log = lineLog()
    let passes = 0
    const loop = passLoop(async () => {
      passes += 1
      if (passes === 1) throw new Error('boom')
    }, log)
    loop.kick()
    await sleep(30)
    loop.kick()
    await sleep(30)
    await loop.stop()
    assert.equal(passes, 2)
    assert.deepEqual(log.lines, ['error a pass failed: boom'])
  })

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
        const child = startDaemon({
          ...process.env,
          HOME: home,
          CONSENSFLOW_HOME: home,
          CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
        })
        let errors = ''
        child.stderr.on('data', (chunk) => {
          errors += chunk
        })
        await new Promise((resolve) => child.stdout.once('data', resolve))
        end(child)
        const code = await new Promise((resolve) => child.once('exit', resolve))
        assert.equal(code, 0, errors)
        const log = await readFile(path.join(home, 'daemon.log'), 'utf8')
        // The runtime the start line names is the daemon's own: Node's version, or the native one's.
        assert.match(
          log,
          new RegExp(
            `^\\S+ info start pid ${child.pid} (?:node v|rust )\\S+ home \\S+\\n\\S+ info stop: ${reason}; rss \\d+ MB\\n\\S+ info exit 0\\n$`,
          ),
        )
      } finally {
        await rm(home, { recursive: true, force: true })
      }
    })
  }

  it('touches no window of the running daemon when a second start is refused its ledger', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-'))
    const running = openLedger(path.join(home, 'consensflow.db'))
    const launch = path.join(home, 'integrations', 'claude', '0b9f2c1e-5d4a-4c3b-9a8f-7e6d5c4b3a21')
    try {
      await mkdir(launch, { recursive: true })
      await writeFile(path.join(launch, 'settings.json'), '{}\n')
      const child = startDaemon({
        ...process.env,
        HOME: home,
        CONSENSFLOW_HOME: home,
        CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
      })
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
      running.close()
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
      const child = startDaemon({
        ...process.env,
        HOME: home,
        CONSENSFLOW_HOME: home,
        CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
      })
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
      // Node's log has the error's stack under the line, which begins `Error: `;
      // the native daemon's has the roster's words alone.
      assert.match(
        await readFile(path.join(home, 'daemon.log'), 'utf8'),
        /\n\S+ error the agents file could not be used\n {4}(?:Error: )?Your agents file .* is not valid JSON/,
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
 * window it is asked to, though no window's program ever runs. `preload` is
 * a module the daemon's Node loads first.
 */
async function daemonOverItsBridge(t, { agents = [], preload = null } = {}) {
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
  child = startDaemon(env, preload === null ? [] : ['--import', pathToFileURL(preload).href])
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
  it("opens a window with the agents' API, its project and participant, its runtime, and the bundled cf first on PATH", {}, async (t) => {
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
        process.execPath,
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
    // Node's log has the error's stack under the line, which begins `Error: `;
    // the native daemon's has the bridge's own words for what broke.
    assert.match(log, /\n\S+ error the bridge failed\n {4}(?:Error: |bridge I\/O error: )/)
    assert.match(log, /\n\S+ info stop: the bridge failed; rss \d+ MB\n\S+ info exit 0\n$/)
  })

  it('writes down an error nobody caught, thrown or rejected, and goes on', {
    skip:
      (process.platform === 'win32' && 'the test throws from a SIGUSR2 handler') || ONLY_NODE_CAN,
  }, async (t) => {
    const preload = path.join(
      await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-fault-')),
      'fault.mjs',
    )
    t.after(() => rm(path.dirname(preload), { recursive: true, force: true }))
    // The first signal throws where nothing catches it; the next rejects where nothing handles it.
    await writeFile(
      preload,
      `let faults = 0
process.on('SIGUSR2', () => {
  faults += 1
  if (faults === 1) throw new Error('thrown where nothing catches it')
  Promise.reject(new Error('rejected where nothing handles it'))
})
`,
    )
    const d = await daemonOverItsBridge(t, { preload })
    const log = () => readFile(path.join(d.home, 'daemon.log'), 'utf8')
    d.child.kill('SIGUSR2')
    await d.until(
      async () => (await log()).includes('uncaught exception'),
      'wrote the exception down',
    )
    d.child.kill('SIGUSR2')
    await d.until(
      async () => (await log()).includes('unhandled rejection'),
      'wrote the rejection down',
    )
    assert.deepEqual(await d.request('ping'), { ok: true }, 'it goes on')
    assert.match(
      await log(),
      /\n\S+ error uncaught exception\n {4}Error: thrown where nothing catches it\n(.|\n)*\n\S+ error unhandled rejection\n {4}Error: rejected where nothing handles it\n/,
    )
    const events = (await readFile(path.join(d.home, 'events.jsonl'), 'utf8'))
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line))
      .filter((event) => event.kind === 'daemon.error')
    assert.deepEqual(
      events.map(({ project, reason }) => ({ project, reason })),
      [
        { project: null, reason: 'uncaught exception: thrown where nothing catches it' },
        { project: null, reason: 'unhandled rejection: rejected where nothing handles it' },
      ],
    )
  })
})
