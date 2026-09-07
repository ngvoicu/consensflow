import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { describe, it } from 'node:test'
import { envelope, pointer } from '../hosts/lib/deliveries.js'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { Bridge } from '../src/bridge.js'
import { DEFAULT_DEADLINE_MS } from '../src/channels/opencode.js'
import { deliver, enabledChannels, launchConfiguration } from '../src/channels.js'

function bridgePair() {
  const nodeToRust = new PassThrough()
  const rustToNode = new PassThrough()
  const node = new Bridge({
    input: rustToNode,
    output: nodeToRust,
    defaultDeadlineMs: 1000,
    idPrefix: 'n-',
    peerIdPrefix: 'r-',
  })
  const rust = new Bridge({
    input: nodeToRust,
    output: rustToNode,
    defaultDeadlineMs: 1000,
    idPrefix: 'r-',
    peerIdPrefix: 'n-',
  })
  return { node, rust }
}

function record(overrides = {}) {
  return {
    id: 'd-33',
    answerId: 'answer-7',
    conversation: 'worker-one',
    agent: 'zeus',
    answer: 'The answer is complete.',
    channel: 'pty-inline',
    ...overrides,
  }
}

function ptyTarget(bridge, enabledChannels = ['pty-inline']) {
  return {
    enabledChannels,
    bridge,
    pane: 'lead-pane',
    generation: 4,
    epoch: 19,
  }
}

function listen(server) {
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve(server.address().port))
  })
}

function closeServer(server) {
  return new Promise((resolve, reject) => {
    server.close((cause) => (cause ? reject(cause) : resolve()))
  })
}

function startPiStandIn(inbox, onRecord) {
  let active = true
  let running = false
  const timer = setInterval(async () => {
    if (!active || running) return
    running = true
    try {
      const files = (await readdir(inbox)).filter((name) => name.endsWith('.json'))
      if (files.length === 0) return
      await onRecord(JSON.parse(await readFile(join(inbox, files[0]), 'utf8')))
    } finally {
      running = false
    }
  }, 5)
  timer.unref()
  return {
    close() {
      active = false
      clearInterval(timer)
    },
  }
}

function realPi() {
  const handlers = new Map()
  const sent = []
  const context = { isIdle: () => true }
  return {
    context,
    handlers,
    sent,
    on(event, handler) {
      handlers.set(event, handler)
    },
    sendUserMessage(content) {
      sent.push(content)
      queueMicrotask(() =>
        handlers.get('message_start')?.(
          { message: { role: 'user', content: [{ type: 'text', text: content }] } },
          context,
        ),
      )
    },
  }
}

describe('delivery channels', () => {
  it('pty-inline writes the complete envelope with the observed input epoch', async () => {
    const { node, rust } = bridgePair()
    let request
    rust.on('pane.write_paste', (body) => {
      request = body
      return { ok: true }
    })

    const delivery = record()
    const answer = await deliver('pty-inline', ptyTarget(node), delivery)

    assert.deepEqual(answer, { ok: true })
    assert.deepEqual(request, {
      id: 'lead-pane',
      generation: 4,
      epoch: 19,
      body: envelope(delivery),
    })
    node.close()
    rust.close()
  })

  it('keeps PTY channels available without an optional harness channel list', async () => {
    const { node, rust } = bridgePair()
    rust.on('pane.write_paste', () => ({ ok: true }))
    const target = ptyTarget(node)
    delete target.enabledChannels

    assert.deepEqual(await deliver('pty-inline', target, record()), { ok: true })
    node.close()
    rust.close()
  })

  it('cf-read writes its pointer with the observed input epoch', async () => {
    const { node, rust } = bridgePair()
    let request
    rust.on('pane.write_paste', (body) => {
      request = body
      return { ok: true }
    })

    const delivery = record({ channel: 'cf-read' })
    const answer = await deliver('cf-read', ptyTarget(node, ['cf-read']), delivery)

    assert.deepEqual(answer, { ok: true })
    assert.equal(request.epoch, 19)
    assert.equal(request.body, pointer(delivery))
    node.close()
    rust.close()
  })

  it('returns Rust Stale and Draft answers without translating them', async () => {
    for (const rustAnswer of [
      { ok: false, error: 'stale pane generation or input epoch' },
      { ok: false, error: 'human draft is latched' },
    ]) {
      const { node, rust } = bridgePair()
      rust.on('pane.write_paste', () => rustAnswer)
      assert.deepEqual(await deliver('pty-inline', ptyTarget(node), record()), rustAnswer)
      node.close()
      rust.close()
    }
  })

  it('maps a PTY transport failure to uncertain', async () => {
    const input = new PassThrough()
    const output = new PassThrough()
    const node = new Bridge({ input, output, defaultDeadlineMs: 1000 })
    input.end()
    await new Promise((resolve) => input.once('end', resolve))

    assert.deepEqual(await deliver('pty-inline', ptyTarget(node), record()), {
      ok: false,
      error: 'uncertain',
      cause: 'eof',
    })
    node.close()
  })

  it('maps a bridge deadline to uncertain because admission is unknown', async () => {
    const input = new PassThrough()
    const output = new PassThrough()
    const node = new Bridge({ input, output, defaultDeadlineMs: 1000 })

    assert.deepEqual(
      await deliver('pty-inline', { ...ptyTarget(node), deadlineMs: 20 }, record()),
      { ok: false, error: 'uncertain', cause: 'deadline' },
    )
    node.close()
  })

  it('POSTs an authenticated OpenCode prompt and reports a 204 admission', async () => {
    let request
    const server = createServer(async (incoming, response) => {
      let body = ''
      for await (const chunk of incoming) body += chunk
      request = { headers: incoming.headers, method: incoming.method, url: incoming.url, body }
      response.writeHead(204)
      response.end()
    })
    const port = await listen(server)
    const delivery = record({ channel: 'opencode-server' })

    try {
      const answer = await deliver(
        'opencode-server',
        {
          enabledChannels: ['opencode-server'],
          session: 'ses_probe',
          launch: {
            endpoint: `http://127.0.0.1:${port}`,
            auth: { username: 'probe', password: 'secret' },
          },
        },
        delivery,
      )

      assert.deepEqual(answer, { ok: true, admitted: true, status: 204 })
      assert.equal(request.method, 'POST')
      assert.equal(request.url, '/session/ses_probe/prompt_async')
      assert.equal(
        request.headers.authorization,
        `Basic ${Buffer.from('probe:secret').toString('base64')}`,
      )
      assert.deepEqual(JSON.parse(request.body), {
        parts: [{ type: 'text', text: envelope(delivery) }],
      })
    } finally {
      await closeServer(server)
    }
  })

  it('POSTs the cf-read pointer to OpenCode instead of the whole envelope', async () => {
    let requestBody
    const server = createServer(async (incoming, response) => {
      let body = ''
      for await (const chunk of incoming) body += chunk
      requestBody = JSON.parse(body)
      response.writeHead(204)
      response.end()
    })
    const port = await listen(server)
    const delivery = record({ channel: 'cf-read' })

    try {
      assert.deepEqual(
        await deliver(
          'opencode-server',
          {
            enabledChannels: ['opencode-server'],
            session: 'ses_probe',
            launch: { endpoint: `http://127.0.0.1:${port}` },
          },
          delivery,
        ),
        { ok: true, admitted: true, status: 204 },
      )
      assert.deepEqual(requestBody, { parts: [{ type: 'text', text: pointer(delivery) }] })
    } finally {
      await closeServer(server)
    }
  })

  it('reports an OpenCode HTTP refusal as not admitted', async () => {
    const server = createServer((_incoming, response) => {
      response.writeHead(401)
      response.end('unauthorized')
    })
    const port = await listen(server)
    try {
      assert.deepEqual(
        await deliver(
          'opencode-server',
          {
            enabledChannels: ['opencode-server'],
            session: 'ses_probe',
            launch: { endpoint: `http://127.0.0.1:${port}` },
          },
          record({ channel: 'opencode-server' }),
        ),
        { ok: false, admitted: false, status: 401, error: 'unauthorized' },
      )
    } finally {
      await closeServer(server)
    }
  })

  it('reports an admitted OpenCode response that explicitly declines the message', async () => {
    const server = createServer((_incoming, response) => {
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ admitted: false, reason: 'session-busy' }))
    })
    const port = await listen(server)
    try {
      assert.deepEqual(
        await deliver(
          'opencode-server',
          {
            enabledChannels: ['opencode-server'],
            session: 'ses_probe',
            launch: { endpoint: `http://127.0.0.1:${port}` },
          },
          record({ channel: 'opencode-server' }),
        ),
        { ok: true, admitted: false, status: 200 },
      )
    } finally {
      await closeServer(server)
    }
  })

  it('maps an OpenCode deadline abort to uncertain', async () => {
    const server = createServer((_incoming, response) => {
      const timer = setTimeout(() => response.destroy(), 100)
      response.on('close', () => clearTimeout(timer))
    })
    const port = await listen(server)
    const started = Date.now()
    try {
      assert.deepEqual(
        await deliver(
          'opencode-server',
          {
            enabledChannels: ['opencode-server'],
            deadlineMs: 20,
            session: 'ses_probe',
            launch: { endpoint: `http://127.0.0.1:${port}` },
          },
          record({ channel: 'opencode-server' }),
        ),
        { ok: false, error: 'uncertain', cause: 'deadline' },
      )
      assert.ok(Date.now() - started < 80, 'the request must stop at the caller deadline')
    } finally {
      server.closeAllConnections?.()
      await closeServer(server)
    }
  })

  it('uses the OpenCode module default deadline when the target has none', async () => {
    const server = createServer((_incoming, response) => {
      const timer = setTimeout(() => response.destroy(), DEFAULT_DEADLINE_MS + 500)
      response.on('close', () => clearTimeout(timer))
    })
    const port = await listen(server)
    const started = Date.now()
    try {
      assert.deepEqual(
        await deliver(
          'opencode-server',
          {
            enabledChannels: ['opencode-server'],
            session: 'ses_probe',
            launch: { endpoint: `http://127.0.0.1:${port}` },
          },
          record({ channel: 'opencode-server' }),
        ),
        { ok: false, error: 'uncertain', cause: 'deadline' },
      )
      assert.ok(
        Date.now() - started < DEFAULT_DEADLINE_MS + 350,
        'the request must stop at the module default deadline',
      )
    } finally {
      server.closeAllConnections?.()
      await closeServer(server)
    }
  })

  it('writes a Pi inbox record and reports the extension ack', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-'))
    const inbox = join(root, 'inbox')
    const ack = join(root, 'ack')
    await mkdir(inbox)
    await mkdir(ack)
    const delivery = record({ channel: 'pi-extension' })
    let received
    const extension = startPiStandIn(inbox, async (record) => {
      received = record
      await writeFile(
        join(ack, `${received.id}.json`),
        `${JSON.stringify({ id: received.id, admitted: true, mode: 'tui' })}\n`,
      )
    })

    try {
      const answer = await deliver(
        'pi-extension',
        {
          enabledChannels: ['pi-extension'],
          launch: {
            extensionPath: '/repo/hosts/pi-extension/consensflow-delivery.mjs',
            inbox,
            ack,
            ackTimeoutMs: 1000,
          },
        },
        delivery,
      )

      assert.deepEqual(answer, {
        ok: true,
        admitted: true,
        ack: { id: 'd-33', admitted: true, mode: 'tui' },
      })
      assert.deepEqual(received, { ...delivery, text: envelope(delivery) })
      assert.deepEqual(JSON.parse(await readFile(join(ack, 'd-33.json'), 'utf8')), {
        id: 'd-33',
        admitted: true,
        mode: 'tui',
      })
    } finally {
      extension.close()
      await rm(root, { recursive: true, force: true })
    }
  })

  it('writes the cf-read pointer through the real Pi extension', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-pi-pointer-'))
    const inbox = join(root, 'inbox')
    const ack = join(root, 'ack')
    await mkdir(inbox)
    await mkdir(ack)
    const delivery = record({ channel: 'cf-read' })
    const pi = realPi()
    createDeliveryExtension(pi, {
      inbox,
      ack,
      quarantine: join(root, 'quarantine'),
      ackTimeoutMs: 1000,
    })
    await pi.handlers.get('session_start')({}, pi.context)

    try {
      assert.deepEqual(
        await deliver(
          'pi-extension',
          {
            enabledChannels: ['pi-extension'],
            launch: { inbox, ack, ackTimeoutMs: 1000 },
          },
          delivery,
        ),
        {
          ok: true,
          admitted: true,
          ack: { id: 'd-33', admitted: true, mode: 'tui' },
        },
      )
      assert.deepEqual(pi.sent, [pointer(delivery)])
    } finally {
      await pi.handlers.get('session_shutdown')()
      await rm(root, { recursive: true, force: true })
    }
  })

  it('requires the Pi channel ack timeout in the launch configuration', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-pi-timeout-'))
    try {
      await assert.rejects(
        () =>
          deliver(
            'pi-extension',
            {
              enabledChannels: ['pi-extension'],
              launch: { inbox: join(root, 'inbox'), ack: join(root, 'ack') },
            },
            record({ channel: 'pi-extension' }),
          ),
        /channel ack timeout/,
      )
    } finally {
      await rm(root, { recursive: true, force: true })
    }
  })

  it('maps a missing Pi ack to uncertain', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-no-ack-'))
    try {
      assert.deepEqual(
        await deliver(
          'pi-extension',
          {
            enabledChannels: ['pi-extension'],
            launch: { inbox: join(root, 'inbox'), ack: join(root, 'ack'), ackTimeoutMs: 20 },
          },
          record({ channel: 'pi-extension' }),
        ),
        { ok: false, error: 'uncertain', cause: 'ack-timeout' },
      )
    } finally {
      await rm(root, { recursive: true, force: true })
    }
  })

  it('reports a Pi ack that explicitly declines the message', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-pi-declined-'))
    const inbox = join(root, 'inbox')
    const ack = join(root, 'ack')
    await mkdir(inbox)
    await mkdir(ack)
    const extension = startPiStandIn(inbox, async (received) => {
      await writeFile(
        join(ack, `${received.id}.json`),
        `${JSON.stringify({ id: received.id, admitted: false, reason: 'not-observed' })}\n`,
      )
    })
    try {
      assert.deepEqual(
        await deliver(
          'pi-extension',
          {
            enabledChannels: ['pi-extension'],
            launch: { inbox, ack, ackTimeoutMs: 1000 },
          },
          record({ channel: 'pi-extension' }),
        ),
        {
          ok: true,
          admitted: false,
          ack: { id: 'd-33', admitted: false, reason: 'not-observed' },
        },
      )
    } finally {
      extension.close()
      await rm(root, { recursive: true, force: true })
    }
  })

  it('refuses a Pi delivery without the prepared envelope', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-no-envelope-'))
    try {
      assert.deepEqual(
        await deliver(
          'pi-extension',
          {
            enabledChannels: ['pi-extension'],
            launch: { inbox: join(root, 'inbox'), ack: join(root, 'ack'), ackTimeoutMs: 20 },
          },
          record({ channel: 'pi-extension', answer: undefined }),
        ),
        { ok: false, error: 'missing-envelope' },
      )
    } finally {
      await rm(root, { recursive: true, force: true })
    }
  })

  it('exposes only the probe-enabled optional channels', () => {
    assert.deepEqual(enabledChannels('codex'), ['pty-inline', 'cf-read'])
    assert.deepEqual(enabledChannels('opencode'), ['pty-inline', 'cf-read', 'opencode-server'])
    assert.deepEqual(enabledChannels('pi'), ['pty-inline', 'cf-read', 'pi-extension'])
  })

  it('documents missing-envelope as a channel error code', async () => {
    const source = await readFile(join(process.cwd(), 'src/channels.js'), 'utf8')
    assert.match(source, /missing-envelope/)
  })

  it('launch configuration feeds the OpenCode adapter without fixture-only fields', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-launch-opencode-'))
    const configuration = await launchConfiguration('opencode', {
      launchId: 'launch-opencode-1',
      workspace: root,
    })
    const server = createServer((incoming, response) => {
      assert.equal(
        incoming.headers.authorization,
        `Basic ${Buffer.from(`opencode:${configuration.channel.password}`).toString('base64')}`,
      )
      response.writeHead(204)
      response.end()
    })
    try {
      const endpoint = new URL(configuration.channel.endpoint)
      await new Promise((resolve, reject) => {
        server.once('error', reject)
        server.listen(Number(endpoint.port), '127.0.0.1', resolve)
      })
      assert.deepEqual(
        await deliver(
          'opencode-server',
          {
            enabledChannels: enabledChannels('opencode'),
            session: 'ses_from_launch',
            launch: configuration,
          },
          record({ channel: 'opencode-server' }),
        ),
        { ok: true, admitted: true, status: 204 },
      )
      assert.deepEqual(configuration.args.slice(0, 1), ['--port'])
      assert.deepEqual(configuration.args.slice(2), ['--hostname', '127.0.0.1'])
      assert.equal(configuration.env.OPENCODE_SERVER_PASSWORD, configuration.channel.password)
    } finally {
      await closeServer(server)
      await rm(root, { recursive: true, force: true })
    }
  })

  it('launch configuration feeds the Pi adapter without fixture-only fields', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-launch-pi-'))
    const configuration = await launchConfiguration('pi', {
      launchId: 'launch-pi-1',
      workspace: root,
    })
    await mkdir(configuration.channel.inbox, { recursive: true })
    const extension = startPiStandIn(configuration.channel.inbox, async (received) => {
      await writeFile(
        join(configuration.channel.ack, `${received.id}.json`),
        `${JSON.stringify({ id: received.id, admitted: true })}\n`,
      )
    })
    try {
      assert.deepEqual(
        await deliver(
          'pi-extension',
          {
            enabledChannels: enabledChannels('pi'),
            launch: configuration,
          },
          record({ channel: 'pi-extension' }),
        ),
        {
          ok: true,
          admitted: true,
          ack: { id: 'd-33', admitted: true },
        },
      )
      assert.equal(configuration.args[0], '--extension')
      assert.equal(
        configuration.args[1],
        join(process.cwd(), 'hosts/pi-extension/consensflow-delivery.mjs'),
      )
      assert.equal(configuration.env.CF_DELIVERY_INBOX, configuration.channel.inbox)
      assert.equal(configuration.env.CF_DELIVERY_ACK, configuration.channel.ack)
      assert.equal(
        configuration.env.CF_DELIVERY_ACK_TIMEOUT_MS,
        String(configuration.channel.ackTimeoutMs),
      )
      assert.equal(
        configuration.env.CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS,
        String(configuration.channel.extensionAckTimeoutMs),
      )
      assert.ok(configuration.channel.extensionAckTimeoutMs < configuration.channel.ackTimeoutMs)
      assert.equal(configuration.env.CF_DELIVERY_QUARANTINE, configuration.channel.quarantine)
    } finally {
      extension.close()
      await rm(root, { recursive: true, force: true })
    }
  })

  it('does not choose a channel absent from the lead harness configuration', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-channels-disabled-'))
    try {
      const answer = await deliver(
        'pi-extension',
        {
          enabledChannels: ['pty-inline'],
          launch: { inbox: join(root, 'inbox'), ack: join(root, 'ack') },
        },
        record({ channel: 'pi-extension' }),
      )
      assert.deepEqual(answer, { ok: false, error: 'channel-disabled' })
      assert.deepEqual(await deliver('not-a-channel', {}, record()), {
        ok: false,
        error: 'unknown-channel',
      })
    } finally {
      await rm(root, { recursive: true, force: true })
    }
  })
})
