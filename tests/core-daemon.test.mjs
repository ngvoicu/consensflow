import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { Credentials, startApi } from '../src/core/api.js'
import { passLoop } from '../src/core/daemon.js'
import { openLedger } from '../src/ledger/index.js'

const EDITOR = fileURLToPath(new URL('./integration/core-editor.mjs', import.meta.url))
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

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
  it('waits only a moment for a pass held up by a slow window', async () => {
    const loop = passLoop(() => new Promise(() => {}))
    loop.kick()
    await sleep(20)
    assert.ok((await timed(() => loop.stop(), 3_000)) < 1_500)
  })

  it('answers a door still waiting for an answer at once, so the API closes', async () => {
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

/** The daemon writes down what is worth knowing afterwards: its start, its stop and why, a pass that failed. */
describe('the daemon and its log', () => {
  it('writes a failed pass down and goes on with the next', async () => {
    const lines = []
    const log = {
      info: (message) => lines.push(`info ${message}`),
      warn: (message) => lines.push(`warn ${message}`),
      error: (message, error) => lines.push(`error ${message}: ${error.message}`),
    }
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
    assert.deepEqual(lines, ['error a pass failed: boom'])
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
        const child = spawn(process.execPath, [EDITOR], {
          env: {
            ...process.env,
            HOME: home,
            CONSENSFLOW_HOME: home,
            CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
          },
          stdio: ['pipe', 'pipe', 'pipe'],
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
        assert.match(
          log,
          new RegExp(
            `^\\S+ info start pid ${child.pid} node v\\S+ home \\S+\\n\\S+ info stop: ${reason}; rss \\d+ MB\\n\\S+ info exit 0\\n$`,
          ),
        )
      } finally {
        await rm(home, { recursive: true, force: true })
      }
    })
  }

  it('starts though its agents file cannot be used, and says why in its log', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-daemon-'))
    const file = path.join(home, 'agents.json')
    // A hand edit's trailing comma: the roster refuses the file and saves nothing over it.
    const broken = '{"schemaVersion": 1, "agents": [{"id": "mine", "kind": "codex"},]}\n'
    try {
      await writeFile(file, broken)
      const child = spawn(process.execPath, [EDITOR], {
        env: {
          ...process.env,
          HOME: home,
          CONSENSFLOW_HOME: home,
          CLAUDE_CONFIG_DIR: path.join(home, '.claude'),
        },
        stdio: ['pipe', 'pipe', 'pipe'],
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
      assert.match(
        await readFile(path.join(home, 'daemon.log'), 'utf8'),
        /\n\S+ error the agents file could not be used\n {4}Error: Your agents file .* is not valid JSON/,
      )
      assert.equal(await readFile(file, 'utf8'), broken, 'the file is left as the human wrote it')
    } finally {
      await rm(home, { recursive: true, force: true })
    }
  })
})
