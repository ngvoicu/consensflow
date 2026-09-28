import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { passLoop } from '../src/core/daemon.js'

const EDITOR = fileURLToPath(new URL('./integration/core-editor.mjs', import.meta.url))
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

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
})
