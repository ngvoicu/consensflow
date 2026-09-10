import assert from 'node:assert/strict'
import { chmod, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import * as nativeOpenCode from '../src/channels/opencode.js'
import { launchConfiguration } from '../src/channels.js'

const { createSession } = nativeOpenCode

async function seedServer(t, mode = 'ok') {
  const calls = []
  let healthy = false
  const server = createServer((request, response) => {
    const chunks = []
    request.on('data', (chunk) => chunks.push(chunk))
    request.on('end', () => {
      const url = new URL(request.url, 'http://127.0.0.1')
      calls.push({
        method: request.method,
        path: url.pathname,
        directory: url.searchParams.get('directory'),
        authorization: request.headers.authorization,
        body: Buffer.concat(chunks).toString(),
      })
      if (
        request.headers.authorization !==
          `Basic ${Buffer.from('opencode:test-secret').toString('base64')}` ||
        mode === 'unauthorized'
      ) {
        response.writeHead(401).end()
      } else if (url.pathname === '/global/health') {
        response.writeHead(healthy ? 200 : 503).end('{}')
      } else if (mode === 'disconnect') {
        request.socket.destroy()
      } else if (mode === 'hang') {
        // Accept the bytes but never answer: the caller must not retry.
      } else {
        response.writeHead(204).end()
      }
    })
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(async () => {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
  })
  return {
    calls,
    ready: () => {
      healthy = true
    },
    channel: {
      kind: 'opencode-server',
      endpoint: `http://127.0.0.1:${server.address().port}`,
      password: 'test-secret',
    },
  }
}

describe('OpenCode worker seed uses the native API after the TUI starts', () => {
  it('waits for readiness, then sends the exact task and roster model once', async (t) => {
    const server = await seedServer(t)
    const task = 'Only the actual task.\nKeep all of it.'
    const ready = setTimeout(server.ready, 60)
    t.after(() => clearTimeout(ready))
    await nativeOpenCode.seedSession({
      channel: server.channel,
      sessionId: 'ses_native123',
      cwd: os.tmpdir(),
      text: task,
      model: 'opencode/model/variant',
      timeoutMs: 2000,
    })
    const posts = server.calls.filter((call) => call.method === 'POST')
    assert.equal(posts.length, 1)
    assert.equal(posts[0].path, '/session/ses_native123/prompt_async')
    assert.equal(posts[0].directory, await realpath(os.tmpdir()))
    assert.deepEqual(JSON.parse(posts[0].body), {
      parts: [{ type: 'text', text: task }],
      model: { providerID: 'opencode', modelID: 'model/variant' },
    })
  })

  it('does not override the existing native model on a resumed conversation', async (t) => {
    const server = await seedServer(t)
    server.ready()
    await nativeOpenCode.seedSession({
      channel: server.channel,
      sessionId: 'ses_resume123',
      cwd: os.tmpdir(),
      text: 'follow-up',
      timeoutMs: 2000,
    })
    assert.deepEqual(JSON.parse(server.calls.find((call) => call.method === 'POST').body), {
      parts: [{ type: 'text', text: 'follow-up' }],
    })
  })

  for (const mode of ['disconnect', 'hang']) {
    it(`reports uncertain admission and never retries after ${mode}`, async (t) => {
      const server = await seedServer(t, mode)
      server.ready()
      await assert.rejects(
        nativeOpenCode.seedSession({
          channel: server.channel,
          sessionId: 'ses_once123',
          cwd: os.tmpdir(),
          text: 'send once',
          timeoutMs: 200,
        }),
        /uncertain.*not retried/,
      )
      assert.equal(server.calls.filter((call) => call.method === 'POST').length, 1)
    })
  }

  it('tolerates slow native startup past the old 15s budget', { timeout: 90000 }, async (t) => {
    const server = await seedServer(t)
    const task = 'Slow starter task.\nKeep all of it.'
    const ready = setTimeout(server.ready, 15500)
    t.after(() => clearTimeout(ready))
    await nativeOpenCode.seedSession({
      channel: server.channel,
      sessionId: 'ses_slow123',
      cwd: os.tmpdir(),
      text: task,
      model: 'opencode/model/variant',
    })
    const posts = server.calls.filter((call) => call.method === 'POST')
    assert.equal(posts.length, 1)
    assert.equal(posts[0].path, '/session/ses_slow123/prompt_async')
    assert.equal(posts[0].directory, await realpath(os.tmpdir()))
    assert.deepEqual(JSON.parse(posts[0].body), {
      parts: [{ type: 'text', text: task }],
      model: { providerID: 'opencode', modelID: 'model/variant' },
    })
  })

  it('cancels readiness when the TUI exits and sends no task', async (t) => {
    const server = await seedServer(t)
    const cancel = new AbortController()
    const timer = setTimeout(() => cancel.abort(), 40)
    t.after(() => clearTimeout(timer))
    await assert.rejects(
      nativeOpenCode.seedSession({
        channel: server.channel,
        sessionId: 'ses_closed123',
        cwd: os.tmpdir(),
        text: 'must not send',
        signal: cancel.signal,
        timeoutMs: 2000,
      }),
    )
    assert.equal(server.calls.filter((call) => call.method === 'POST').length, 0)
  })

  it('keeps a cancelled in-flight POST uncertain without retrying it', async (t) => {
    const server = await seedServer(t, 'hang')
    server.ready()
    const cancel = new AbortController()
    const timer = setInterval(() => {
      if (server.calls.some((call) => call.method === 'POST')) cancel.abort()
    }, 5)
    t.after(() => clearInterval(timer))
    await assert.rejects(
      nativeOpenCode.seedSession({
        channel: server.channel,
        sessionId: 'ses_cancel123',
        cwd: os.tmpdir(),
        text: 'send once',
        signal: cancel.signal,
        timeoutMs: 2000,
      }),
      /uncertain.*not retried/,
    )
    assert.equal(server.calls.filter((call) => call.method === 'POST').length, 1)
  })

  it('refuses failed authentication before sending task bytes', async (t) => {
    const server = await seedServer(t, 'unauthorized')
    server.ready()
    await assert.rejects(
      nativeOpenCode.seedSession({
        channel: server.channel,
        sessionId: 'ses_auth123',
        cwd: os.tmpdir(),
        text: 'must not send',
        timeoutMs: 2000,
      }),
      /unauthorized/,
    )
    assert.equal(server.calls.filter((call) => call.method === 'POST').length, 0)
  })
})

const FIXTURE = `#!/usr/bin/env node
import http from 'node:http'
import fs from 'node:fs'
const args = process.argv.slice(2)
if (args[0] !== 'serve') process.exit(2)
const port = Number(args[args.indexOf('--port') + 1])
const state = process.env.CF_FIXTURE_STATE ?? ''
const mode = process.env.CF_FIXTURE_MODE ?? 'ok'
if (mode === 'early-exit') process.exit(1)
if (mode === 'ignore-sigterm') process.on('SIGTERM', () => {})
const pass = process.env.OPENCODE_SERVER_PASSWORD ?? ''
const want = 'Basic ' + Buffer.from('opencode:' + pass).toString('base64')
try {
  fs.writeFileSync(
    state + '.startup',
    JSON.stringify({
      pid: process.pid,
      argv: args,
      cwd: process.cwd(),
      username: process.env.OPENCODE_SERVER_USERNAME,
      hasPassword: pass.length > 0,
      ping: process.env.CF_FIXTURE_PING,
    }),
  )
} catch {}
let creates = 0
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1')
  const pathname = url.pathname
  const dirParam = url.searchParams.get('directory')
  const here = dirParam || process.cwd()
  if (req.method === 'GET' && pathname === '/global/health') {
    if (mode === 'hang-health') return
    if (req.headers.authorization !== want) {
      res.writeHead(401)
      res.end('unauthorized')
      return
    }
    res.writeHead(200, { 'content-type': 'application/json' })
    res.end('{"healthy":true}')
    return
  }
  if (req.method === 'POST' && pathname === '/session') {
    if (mode === 'hang-session') return
    if (req.headers.authorization !== want) {
      res.writeHead(401)
      res.end('unauthorized')
      return
    }
    let body = ''
    req.on('data', (c) => {
      body += c
    })
    req.on('end', () => {
      creates += 1
      try {
        fs.writeFileSync(state + '.creates', String(creates))
        fs.writeFileSync(state + '.body', body)
        fs.writeFileSync(state + '.query', dirParam ?? '')
      } catch {}
      if (mode === 'bad-json') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end('not json{')
        return
      }
      if (mode === 'oversized') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ id: 'ses_' + 'x'.repeat(2 * 1024 * 1024), directory: here }))
        return
      }
      if (mode === 'invalid-id') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ id: 'bad', directory: here }))
        return
      }
      if (mode === 'wrong-dir') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ id: 'ses_abc123', directory: '/elsewhere' }))
        return
      }
      const id = 'ses_' + process.pid.toString(36) + creates.toString(36) + Date.now().toString(36)
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({ id, directory: here }))
    })
    return
  }
  res.writeHead(404)
  res.end('nope')
})
server.listen(port, '127.0.0.1', () => {})
`

async function setup(mode = 'ok', extraEnv = {}) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-opencode-create-'))
  const rawWorkspace = await mkdtemp(path.join(os.tmpdir(), 'cf-opencode-ws-'))
  const workspace = await realpath(rawWorkspace)
  const exe = path.join(dir, 'fake-opencode')
  await writeFile(exe, FIXTURE, 'utf8')
  await chmod(exe, 0o755)
  const state = path.join(dir, 'state')
  const configuration = await launchConfiguration('opencode', {
    launchId: `t-${Date.now().toString(36)}`,
    workspace,
  })
  const env = {
    ...process.env,
    ...extraEnv,
    CF_FIXTURE_STATE: state,
    CF_FIXTURE_MODE: mode,
    CF_FIXTURE_PING: 'pong',
  }
  return { dir, workspace, exe, state, configuration, env }
}

async function teardown(setupResult) {
  await rm(setupResult.dir, { recursive: true, force: true })
  await rm(setupResult.workspace, { recursive: true, force: true })
}

async function childGone(pid, ms = 3000) {
  const until = Date.now() + ms
  while (Date.now() < until) {
    try {
      process.kill(pid, 0)
    } catch (cause) {
      if (cause?.code === 'ESRCH') return true
    }
    await new Promise((r) => setTimeout(r, 50))
  }
  return false
}

async function startupOf(state, ms = 5000) {
  const until = Date.now() + ms
  for (;;) {
    try {
      return JSON.parse(await readFile(state + '.startup', 'utf8'))
    } catch (cause) {
      if (Date.now() >= until || cause?.code !== 'ENOENT') throw cause
      await new Promise((r) => setTimeout(r, 50))
    }
  }
}

describe('opencode createSession', () => {
  it('creates exactly one empty session and frees the port', async () => {
    const s = await setup()
    try {
      const id = await createSession({
        executable: s.exe,
        cwd: s.workspace,
        env: s.env,
        configuration: s.configuration,
      })
      assert.match(id, /^ses_[A-Za-z0-9]+$/)
      assert.equal(await readFile(s.state + '.creates', 'utf8'), '1')
      assert.equal(await readFile(s.state + '.body', 'utf8'), '{}')
      assert.equal(await readFile(s.state + '.query', 'utf8'), s.workspace)
      const started = await startupOf(s.state)
      assert.deepEqual(started.argv, ['serve', ...s.configuration.args])
      assert.equal(started.cwd, s.workspace)
      assert.equal(started.username, 'opencode')
      assert.equal(started.hasPassword, true)
      assert.equal(started.ping, 'pong')
      assert.ok(await childGone(started.pid), 'temporary server is reaped before return')
    } finally {
      await teardown(s)
    }
  })

  it('mints distinct ids on two calls', async () => {
    const a = await setup()
    const b = await setup()
    try {
      const first = await createSession({
        executable: a.exe,
        cwd: a.workspace,
        env: a.env,
        configuration: a.configuration,
      })
      const second = await createSession({
        executable: b.exe,
        cwd: b.workspace,
        env: b.env,
        configuration: b.configuration,
      })
      assert.notEqual(first, second)
    } finally {
      await teardown(a)
      await teardown(b)
    }
  })

  it('refuses a missing executable without a child', async () => {
    const s = await setup()
    try {
      await assert.rejects(
        createSession({
          executable: path.join(s.dir, 'does-not-exist'),
          cwd: s.workspace,
          env: s.env,
          configuration: s.configuration,
          timeoutMs: 3000,
        }),
      )
    } finally {
      await teardown(s)
    }
  })

  it('surfaces early exit and unauthorized without leaking secrets', async () => {
    const dead = await setup('early-exit')
    try {
      await assert.rejects(
        createSession({
          executable: dead.exe,
          cwd: dead.workspace,
          env: dead.env,
          configuration: dead.configuration,
          timeoutMs: 3000,
        }),
      )
    } finally {
      await teardown(dead)
    }
    const s = await setup()
    try {
      const bad = {
        ...s.configuration,
        channel: { ...s.configuration.channel, password: 'wrong' },
      }
      const error = await createSession({
        executable: s.exe,
        cwd: s.workspace,
        env: s.env,
        configuration: bad,
        timeoutMs: 4000,
      }).then(
        () => null,
        (cause) => cause,
      )
      assert.ok(error instanceof Error)
      assert.doesNotMatch(String(error?.message), /wrong|hunter2|SECRET/i)
      assert.doesNotMatch(String(error?.message), new RegExp(s.configuration.channel.password))
    } finally {
      await teardown(s)
    }
  })

  it('rejects malformed, invalid-id, wrong-dir and oversized responses', async () => {
    for (const mode of ['bad-json', 'invalid-id', 'wrong-dir', 'oversized']) {
      const s = await setup(mode)
      try {
        const error = await createSession({
          executable: s.exe,
          cwd: s.workspace,
          env: s.env,
          configuration: s.configuration,
          timeoutMs: 5000,
        }).then(
          () => null,
          (cause) => cause,
        )
        assert.ok(error instanceof Error, mode)
        let started = null
        try {
          started = await startupOf(s.state)
        } catch {}
        if (started) assert.ok(await childGone(started.pid), `${mode}: child reaped`)
      } finally {
        await teardown(s)
      }
    }
  })

  it('times out on hanging endpoints and reaps an ignored SIGTERM', async () => {
    const hanging = await setup('hang-health')
    try {
      await assert.rejects(
        createSession({
          executable: hanging.exe,
          cwd: hanging.workspace,
          env: hanging.env,
          configuration: hanging.configuration,
          timeoutMs: 5000,
        }),
      )
      const started = await startupOf(hanging.state)
      assert.ok(await childGone(started.pid), 'hanging child reaped')
    } finally {
      await teardown(hanging)
    }
    const stubborn = await setup('ignore-sigterm')
    try {
      const id = await createSession({
        executable: stubborn.exe,
        cwd: stubborn.workspace,
        env: stubborn.env,
        configuration: stubborn.configuration,
        timeoutMs: 8000,
      })
      assert.match(id, /^ses_[A-Za-z0-9]+$/)
      const started = await startupOf(stubborn.state)
      assert.ok(await childGone(started.pid), 'SIGKILL fallback reaps the child')
    } finally {
      await teardown(stubborn)
    }
  })
})
