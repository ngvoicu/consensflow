import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { assertStarted } from './choice.mjs'
import { daemonCommand, tempEnv } from './helpers.mjs'

/**
 * The bridge as the app meets it: the daemon's end of it is held by
 * `crates/cf-bridge/src/local/tests/`, and this is the process, `cf ui`, with
 * the handle line the app reads before it speaks.
 */
describe('cf ui --json --no-open speaks the bridge after its handle line', () => {
  it('keeps the handle line first, then answers a ping frame, then exits on EOF', async () => {
    const t = tempEnv()
    // `cf ui` as the app runs it: the native `cf` (CONSENSFLOW_TEST_DAEMON may
    // name another build of it), and the daemon it starts is the native one: the
    // start line in its log says so.
    const started = daemonCommand()
    const child = spawn(started.command, started.args, {
      env: t.env,
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    try {
      let buffer = ''
      child.stdout.on('data', (chunk) => {
        buffer += chunk
      })
      const nextLine = (timeoutMs = 10_000) =>
        new Promise((resolve, reject) => {
          const started = Date.now()
          const tick = () => {
            const end = buffer.indexOf('\n')
            if (end !== -1) {
              const line = buffer.slice(0, end)
              buffer = buffer.slice(end + 1)
              return resolve(line)
            }
            if (Date.now() - started > timeoutMs) return reject(new Error('no line arrived'))
            setTimeout(tick, 5)
          }
          tick()
        })

      const handleLine = await nextLine()
      const handle = JSON.parse(handleLine)
      assertStarted(readFileSync(join(t.env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8'), child.pid)
      assert.ok(handle.url.length > 0)
      assert.match(handle.url, /^http:\/\/127\.0\.0\.1:\d+\/$/)
      assert.equal(typeof handle.token, 'string')

      child.stdin.write(
        `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'ping', body: {} })}\n`,
      )
      const resLine = await nextLine()
      assert.deepEqual(JSON.parse(resLine), {
        v: 1,
        id: 'r-1',
        kind: 'res',
        op: 'ping',
        body: { ok: true },
      })

      child.stdin.end()
      const code = await new Promise((resolve, reject) => {
        child.once('exit', resolve)
        setTimeout(() => reject(new Error('the daemon kept serving')), 10_000)
      })
      assert.equal(code, 0)
    } finally {
      child.kill()
      t.cleanup()
    }
  })
})
