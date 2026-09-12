import assert from 'node:assert/strict'
import { closeSync, openSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { describe, it } from 'node:test'
import { Bridge } from '../src/bridge.js'
import { stdinIsPipe } from '../src/ui.js'
import { tempEnv } from './helpers.mjs'

/** Two bridges talking to each other over in-process pipes, like Node and Rust. */
function pair(options = {}) {
  const aToB = new PassThrough()
  const bToA = new PassThrough()
  const a = new Bridge({
    input: bToA,
    output: aToB,
    ...options,
    idPrefix: 'n-',
    peerIdPrefix: 'r-',
  })
  const b = new Bridge({
    input: aToB,
    output: bToA,
    ...options,
    idPrefix: 'r-',
    peerIdPrefix: 'n-',
  })
  return { a, b }
}

/** A bridge with nobody on the other end: requests hang until EOF or deadline. */
function lonely(options = {}) {
  const input = new PassThrough()
  const output = new PassThrough()
  const bridge = new Bridge({ input, output, ...options })
  return { input, output, bridge }
}

function waitFor(predicate, timeoutMs = 2000) {
  const started = Date.now()
  return new Promise((resolve, reject) => {
    const tick = () => {
      try {
        if (predicate()) return resolve()
      } catch (cause) {
        return reject(cause)
      }
      if (Date.now() - started > timeoutMs) return reject(new Error('timed out waiting'))
      setTimeout(tick, 5)
    }
    tick()
  })
}

describe('Bridge request/response over two streams', () => {
  it('resolves a request with what the peer handler returns', async () => {
    const { a, b } = pair()
    b.on('add', ({ x }) => ({ sum: x + 1 }))
    assert.deepEqual(await a.request('add', { x: 1 }), { sum: 2 })
    a.close()
    b.close()
  })

  it('turns a throwing handler into {ok:false, error}', async () => {
    const { a, b } = pair()
    b.on('boom', () => {
      throw new Error('nope')
    })
    assert.deepEqual(await a.request('boom', {}), { ok: false, error: 'nope' })
    a.close()
    b.close()
  })

  it('turns an async rejection into {ok:false, error}', async () => {
    const { a, b } = pair()
    b.on('later', async () => {
      throw new Error('async nope')
    })
    assert.deepEqual(await a.request('later', {}), { ok: false, error: 'async nope' })
    a.close()
    b.close()
  })

  it('stops calling a request handler after unsubscribe', async () => {
    const { a, b } = pair()
    let calls = 0
    const off = b.on('temp', () => {
      calls += 1
      return { ok: true }
    })
    assert.deepEqual(await a.request('temp', {}), { ok: true })
    off()
    assert.deepEqual(await a.request('temp', {}), { ok: false, error: 'unknown-op' })
    assert.equal(calls, 1)
    a.close()
    b.close()
  })

  it('answers an unknown op instead of hanging', async () => {
    const { a, b } = pair()
    assert.deepEqual(await a.request('missing', {}), { ok: false, error: 'unknown-op' })
    a.close()
    b.close()
  })

  it('namespaces originated ids n-<n>', async () => {
    const { input, output, bridge } = lonely()
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    const pending = bridge.request('echo', { a: 1 })
    await waitFor(() => captured.includes('\n'))
    const first = JSON.parse(captured.trim())
    assert.equal(first.id, 'n-1')
    assert.equal(first.kind, 'req')
    assert.equal(first.op, 'echo')
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'echo', body: { ok: true } })}\n`,
    )
    assert.deepEqual(await pending, { ok: true })

    const pending2 = bridge.request('echo', {})
    await waitFor(() => captured.split('\n').filter((l) => l.length > 0).length === 2)
    const second = JSON.parse(captured.split('\n').filter((l) => l.length > 0)[1])
    assert.equal(second.id, 'n-2')
    input.end()
    await assert.rejects(pending2, (cause) => cause?.error === 'eof')
    bridge.close()
  })

  it('does not settle a request from a wrong namespace or mismatched response op', async () => {
    const errors = []
    const { input, output, bridge } = lonely({ onError: (cause) => errors.push(String(cause)) })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    const pending = bridge.request('echo', {})
    await waitFor(() => captured.includes('\n'))
    input.write(
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'res', op: 'echo', body: { wrong: 'namespace' } })}\n`,
    )
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'wrong.echo', body: { wrong: 'op' } })}\n`,
    )
    await waitFor(() => errors.length === 2)
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'echo', body: { ok: true } })}\n`,
    )
    assert.deepEqual(await pending, { ok: true })
    assert.match(errors[0], /wrong namespace/)
    assert.match(errors[1], /wrong\.echo/)
    bridge.close()
  })

  it('resolves {ok:false, error:deadline} past the deadline', async () => {
    const { bridge } = lonely()
    const answer = await bridge.request('never', {}, { deadlineMs: 20 })
    assert.deepEqual(answer, { ok: false, error: 'deadline' })
    bridge.close()
  })

  it('ignores a response arriving after its deadline', async () => {
    const errors = []
    const { input, output, bridge } = lonely({ onError: (cause) => errors.push(String(cause)) })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    const answer = await bridge.request('never', {}, { deadlineMs: 20 })
    assert.deepEqual(answer, { ok: false, error: 'deadline' })
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'never', body: { late: true } })}\n`,
    )
    await new Promise((resolve) => setTimeout(resolve, 50))
    assert.equal(errors.length, 0)
    assert.equal(bridge.closed, false)
    const pending = bridge.request('echo', {})
    await waitFor(() => captured.split('\n').filter((l) => l.length > 0).length === 2)
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-2', kind: 'res', op: 'echo', body: { ok: true } })}\n`,
    )
    assert.deepEqual(await pending, { ok: true })
    bridge.close()
  })
})

describe('Bridge events', () => {
  it('delivers outgoing events to onEvent handlers', async () => {
    const { a, b } = pair()
    const seen = []
    b.onEvent('tick', (body) => {
      seen.push(body)
    })
    assert.equal(a.event('tick', { n: 1 }), true)
    await waitFor(() => seen.length === 1)
    assert.deepEqual(seen, [{ n: 1 }])
    a.close()
    b.close()
  })

  it('stops calling an event handler after unsubscribe', async () => {
    const { a, b } = pair()
    const seen = []
    const off = b.onEvent('tick', (body) => {
      seen.push(body)
    })
    a.event('tick', { n: 1 })
    await waitFor(() => seen.length === 1)
    off()
    a.event('tick', { n: 2 })
    await new Promise((resolve) => setTimeout(resolve, 50))
    assert.deepEqual(seen, [{ n: 1 }])
    a.close()
    b.close()
  })

  it('does not skip subscribers when a handler unsubscribes mid-event', async () => {
    const { a, b } = pair()
    const seenA = []
    const seenB = []
    let offA
    offA = b.onEvent('tick', (body) => {
      seenA.push(body)
      offA()
    })
    b.onEvent('tick', (body) => {
      seenB.push(body)
    })
    a.event('tick', { n: 1 })
    a.event('tick', { n: 2 })
    await waitFor(() => seenB.length === 2)
    assert.deepEqual(seenA, [{ n: 1 }])
    assert.deepEqual(seenB, [{ n: 1 }, { n: 2 }])
    a.close()
    b.close()
  })

  it('reports event handler failures through onError', async () => {
    const errors = []
    const { a, b } = pair({ onError: (cause) => errors.push(String(cause)) })
    b.onEvent('bad', () => {
      throw new Error('sync event boom')
    })
    b.onEvent('badder', async () => {
      throw new Error('async event boom')
    })
    a.event('bad', {})
    a.event('badder', {})
    await waitFor(() => errors.length === 2)
    assert.match(errors[0], /sync event boom/)
    assert.match(errors[1], /async event boom/)
    a.close()
    b.close()
  })

  it('refuses events after close', async () => {
    const { bridge } = lonely()
    bridge.close()
    assert.equal(bridge.event('tick', {}), false)
    assert.equal(bridge.closed, true)
  })

  it('returns false when the event write itself fails', async () => {
    const { EventEmitter } = await import('node:events')
    const input = new PassThrough()
    const output = new EventEmitter()
    output.write = () => {
      throw new Error('write failed')
    }
    const bridge = new Bridge({ input, output })
    assert.equal(bridge.event('tick', {}), false)
    assert.equal(bridge.closed, true)
    bridge.close()
  })
})

describe('Bridge frame limits', () => {
  it('refuses an outgoing frame over maxFrameBytes and never writes it', async () => {
    const { output, bridge } = lonely({ maxFrameBytes: 64 })
    let written = ''
    output.on('data', (chunk) => {
      written += chunk
    })
    const answer = await bridge.request('big', { text: 'x'.repeat(200) })
    assert.deepEqual(answer, { ok: false, error: 'too-large' })
    assert.equal(written, '')
    assert.equal(bridge.event('big', { text: 'y'.repeat(200) }), false)
    assert.equal(written, '')
    bridge.close()
  })

  it('answers an incoming oversized request with too-large', async () => {
    // The budget fits the small refusal but not the incoming frame.
    const { input, output, bridge } = lonely({ maxFrameBytes: 100 })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('big', () => ({ ok: true }))
    input.write(
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'big', body: { text: 'x'.repeat(200) } })}\n`,
    )
    await waitFor(() => captured.includes('\n'))
    const [line] = captured.split('\n').filter((l) => l.length > 0)
    assert.deepEqual(JSON.parse(line), {
      v: 1,
      id: 'r-1',
      kind: 'res',
      op: 'big',
      body: { ok: false, error: 'too-large' },
    })
    bridge.close()
  })

  it('refuses to answer an oversized request without a valid op', async () => {
    const errors = []
    const { input, output, bridge } = lonely({
      maxFrameBytes: 100,
      onError: (cause) => errors.push(String(cause)),
    })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('ping', () => ({ ok: true }))
    // No valid op to copy into a response: report, write nothing, stay open.
    input.write(
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', body: { text: 'x'.repeat(200) } })}\n`,
    )
    input.write(
      `${JSON.stringify({ v: 1, id: 'r-2', kind: 'req', op: 42, body: { text: 'y'.repeat(200) } })}\n`,
    )
    await waitFor(() => errors.length === 2)
    assert.match(errors[0], /maxFrameBytes/)
    assert.match(errors[1], /maxFrameBytes/)
    assert.equal(captured, '')
    assert.equal(bridge.closed, false)
    input.write(`${JSON.stringify({ v: 1, id: 'r-3', kind: 'req', op: 'ping', body: {} })}\n`)
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()).body, { ok: true })
    bridge.close()
  })

  it('answers not-serializable when the body would not survive serialization', async () => {
    const { a, b } = pair()
    b.on('fn', () => () => {})
    b.on('sym', () => Symbol('s'))
    b.on('ghost', () => ({ toJSON: () => undefined }))
    for (const op of ['fn', 'sym', 'ghost']) {
      assert.deepEqual(await a.request(op, {}), { ok: false, error: 'not-serializable' }, op)
    }
    a.close()
    b.close()
  })

  it('rejects a response without a body and keeps the request pending', async () => {
    const errors = []
    const { input, bridge } = lonely({ onError: (cause) => errors.push(String(cause)) })
    const pending = bridge.request('echo', {})
    input.write(`${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'echo' })}\n`)
    await waitFor(() => errors.length === 1)
    assert.match(errors[0], /malformed/)
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'echo', body: { ok: true } })}\n`,
    )
    assert.deepEqual(await pending, { ok: true })
    bridge.close()
  })

  it('rejects requests and events without a body', async () => {
    const errors = []
    const { input, output, bridge } = lonely({ onError: (cause) => errors.push(String(cause)) })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    let pinged = 0
    let noted = 0
    bridge.on('ping', () => {
      pinged += 1
      return { ok: true }
    })
    bridge.onEvent('note', () => {
      noted += 1
    })
    input.write(`${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'ping' })}\n`)
    input.write(`${JSON.stringify({ v: 1, id: 'r-2', kind: 'evt', op: 'note' })}\n`)
    await waitFor(() => errors.length === 2)
    assert.equal(pinged, 0)
    assert.equal(noted, 0)
    assert.equal(captured, '')
    bridge.close()
  })

  it('reports a malformed oversized request and answers what follows', async () => {
    const errors = []
    const { input, output, bridge } = lonely({
      maxFrameBytes: 100,
      onError: (cause) => errors.push(String(cause)),
    })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('ping', () => ({ ok: true }))
    input.write(
      `${JSON.stringify({ v: 2, id: 'r-1', kind: 'req', op: 'big', body: { text: 'x'.repeat(200) } })}\n`,
    )
    await waitFor(() => errors.length === 1)
    assert.match(errors[0], /maxFrameBytes/)
    input.write(`${JSON.stringify({ v: 1, id: 'r-2', kind: 'req', op: 'ping', body: {} })}\n`)
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()), {
      v: 1,
      id: 'r-2',
      kind: 'res',
      op: 'ping',
      body: { ok: true },
    })
    bridge.close()
  })

  it('never settles a pending request from a malformed oversized response', async () => {
    const errors = []
    const { input, bridge } = lonely({
      maxFrameBytes: 100,
      onError: (cause) => errors.push(String(cause)),
    })
    const pending = bridge.request('echo', {})
    let settled = false
    pending.then(
      () => {
        settled = true
      },
      () => {
        settled = true
      },
    )
    input.write(
      `${JSON.stringify({ v: 2, id: 'n-1', kind: 'res', op: 'echo', body: { text: 'x'.repeat(200) } })}\n`,
    )
    await waitFor(() => errors.length === 1)
    assert.match(errors[0], /maxFrameBytes/)
    assert.equal(settled, false)
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-1', kind: 'res', op: 'echo', body: { ok: true } })}\n`,
    )
    assert.deepEqual(await pending, { ok: true })
    bridge.close()
  })

  it('answers not-serializable when toJSON drops the body key', async () => {
    const { a, b } = pair()
    b.on('keyed', () => ({
      toJSON: (key) => (key === 'body' ? undefined : { ok: true }),
    }))
    assert.deepEqual(await a.request('keyed', {}), { ok: false, error: 'not-serializable' })
    a.close()
    b.close()
  })

  it('serializes a stateful body exactly once', async () => {
    const { a, b } = pair()
    b.on('counted', () => {
      let calls = 0
      return {
        toJSON: () => {
          calls += 1
          return { n: calls }
        },
      }
    })
    assert.deepEqual(await a.request('counted', {}), { n: 1 })
    a.close()
    b.close()
  })

  it('defaults maxFrameBytes to 1 MiB and the request deadline to 30 s', () => {
    const { bridge } = lonely()
    assert.equal(bridge.maxFrameBytes, 1024 * 1024)
    assert.equal(bridge.defaultDeadlineMs, 30000)
    bridge.close()
  })

  it('answers too-large when a handler response would not fit', async () => {
    const { input, output, bridge } = lonely({ maxFrameBytes: 100 })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('big', () => ({ text: 'x'.repeat(200) }))
    input.write(`${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'big', body: {} })}\n`)
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()), {
      v: 1,
      id: 'r-1',
      kind: 'res',
      op: 'big',
      body: { ok: false, error: 'too-large' },
    })
    bridge.close()
  })

  it('fails the transport when even the too-large response cannot fit', async () => {
    const errors = []
    const { input, output, bridge } = lonely({
      maxFrameBytes: 60,
      onError: (cause) => errors.push(String(cause)),
    })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    const pending = bridge.request('hangs', {})
    const settled = Promise.allSettled([pending])
    // The incoming request is oversized, and the bounded too-large refusal
    // does not fit the 60-byte budget either — so the peer hears nothing and
    // the transport fails explicitly instead of hanging.
    input.write(
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'big', body: { text: 'x'.repeat(200) } })}\n`,
    )
    const [result] = await settled
    assert.equal(result.status, 'rejected')
    assert.deepEqual(
      captured
        .trim()
        .split('\n')
        .filter((line) => line.length > 0)
        .map((line) => JSON.parse(line)),
      [{ v: 1, id: 'n-1', kind: 'req', op: 'hangs', body: {} }],
    )
    assert.equal(bridge.closed, true)
    await waitFor(() => errors.length >= 1)
    assert.match(errors[0], /maxFrameBytes/)
    bridge.close()
  })
})

describe('Bridge default request deadline', () => {
  it('settles an unanswered request on the default deadline and keeps working', async () => {
    const { input, output, bridge } = lonely({ maxFrameBytes: 100, defaultDeadlineMs: 50 })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    const pending = bridge.request('hangs', {})
    // An oversized, fragmented response: discarded through its newline, so
    // the request never settles from the wire and the default deadline does.
    const res = JSON.stringify({
      v: 1,
      id: 'n-1',
      kind: 'res',
      op: 'hangs',
      body: { text: 'x'.repeat(200) },
    })
    input.write(res.slice(0, 60))
    input.write(res.slice(60))
    assert.deepEqual(await pending, { ok: false, error: 'deadline' })
    input.write('\n')
    const next = bridge.request('ping', {})
    await waitFor(() => captured.split('\n').filter((l) => l.length > 0).length === 2)
    input.write(
      `${JSON.stringify({ v: 1, id: 'n-2', kind: 'res', op: 'ping', body: { ok: true } })}\n`,
    )
    assert.deepEqual(await next, { ok: true })
    bridge.close()
  })
})

describe('Bridge input robustness', () => {
  it('reports a malformed line through onError and keeps going', async () => {
    const errors = []
    const { a, b } = pair({ onError: (cause) => errors.push(String(cause)) })
    a.on('ping', () => ({ ok: true }))
    b.input.write('not json at all\n')
    assert.deepEqual(await b.request('ping', {}), { ok: true })
    await waitFor(() => errors.length >= 1)
    a.close()
    b.close()
  })

  it('keeps buffered bytes when junk and a frame arrive in one chunk', async () => {
    const errors = []
    const { input, output, bridge } = lonely({ onError: (cause) => errors.push(String(cause)) })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('ping', () => ({ ok: true }))
    input.write(
      `junk before the first newline\n${JSON.stringify({ v: 1, id: 'r-5', kind: 'req', op: 'ping', body: {} })}\n`,
    )
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()), {
      v: 1,
      id: 'r-5',
      kind: 'res',
      op: 'ping',
      body: { ok: true },
    })
    await waitFor(() => errors.length === 1)
    bridge.close()
  })

  it('keeps a frame split across two chunks', async () => {
    const { input, output, bridge } = lonely()
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('ping', () => ({ ok: true }))
    const frame = `${JSON.stringify({ v: 1, id: 'r-9', kind: 'req', op: 'ping', body: {} })}\n`
    input.write(frame.slice(0, 10))
    await new Promise((resolve) => setTimeout(resolve, 20))
    assert.equal(captured, '')
    input.write(frame.slice(10))
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()), {
      v: 1,
      id: 'r-9',
      kind: 'res',
      op: 'ping',
      body: { ok: true },
    })
    bridge.close()
  })

  it('rejects every outstanding request with eof and marks itself closed', async () => {
    const { input, bridge } = lonely()
    const first = bridge.request('hangs', {})
    const second = bridge.request('hangs-too', {}, { deadlineMs: 10_000 })
    const settled = Promise.allSettled([first, second])
    input.end()
    const [r1, r2] = await settled
    for (const result of [r1, r2]) {
      assert.equal(result.status, 'rejected')
      assert.equal(result.reason?.error, 'eof')
    }
    assert.equal(bridge.closed, true)
    await assert.rejects(bridge.request('late', {}), (cause) => cause?.error === 'eof')
    bridge.close()
  })

  it('writes nothing but frames to the output stream', async () => {
    const { a, b } = pair()
    const seen = []
    for (const stream of [a.output, b.output]) {
      stream.on('data', (chunk) => seen.push(chunk.toString()))
    }
    b.on('ping', () => ({ ok: true }))
    b.onEvent('note', () => {})
    await a.request('ping', {})
    a.event('note', { n: 2 })
    await new Promise((resolve) => setTimeout(resolve, 50))
    const text = seen.join('')
    assert.ok(text.length > 0)
    for (const line of text.split('\n').filter((l) => l.length > 0)) {
      const frame = JSON.parse(line)
      assert.equal(frame.v, 1)
      assert.ok(['req', 'res', 'evt'].includes(frame.kind))
      assert.equal(typeof frame.id, 'string')
      assert.equal(typeof frame.op, 'string')
      assert.ok('body' in frame)
    }
    a.close()
    b.close()
  })
})

describe('Bridge multibyte input', () => {
  it('decodes a multibyte character split at every byte boundary', async () => {
    const body = { text: 'héllo 🌊 世界' }
    const bytes = Buffer.from(
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'echo', body })}\n`,
      'utf8',
    )
    for (let at = 1; at < bytes.length - 1; at += 1) {
      const { input, output, bridge } = lonely()
      let captured = ''
      output.on('data', (chunk) => {
        captured += chunk
      })
      bridge.on('echo', (echoed) => echoed)
      input.write(bytes.subarray(0, at))
      input.write(bytes.subarray(at))
      await waitFor(() => captured.includes('\n'))
      assert.deepEqual(JSON.parse(captured.trim()).body, body)
      bridge.close()
    }
  })
})

describe('Bridge input size limit', () => {
  it('refuses an unterminated line over maxFrameBytes and recovers', async () => {
    const errors = []
    const { input, output, bridge } = lonely({
      maxFrameBytes: 100,
      onError: (cause) => errors.push(String(cause)),
    })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('ping', () => ({ ok: true }))
    input.write('x'.repeat(300))
    await waitFor(() => errors.length >= 1)
    assert.match(errors[0], /maxFrameBytes/)
    assert.equal(captured, '')
    assert.equal(bridge.closed, false)
    // The rest of the over-long line is discarded through its newline...
    input.write('y'.repeat(50))
    input.write('\n')
    // ...and the next frame is served whole.
    input.write(`${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'ping', body: {} })}\n`)
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()).body, { ok: true })
    bridge.close()
  })

  it('bounds fragmented overflow across chunks', async () => {
    const errors = []
    const { input, output, bridge } = lonely({
      maxFrameBytes: 100,
      onError: (cause) => errors.push(String(cause)),
    })
    let captured = ''
    output.on('data', (chunk) => {
      captured += chunk
    })
    bridge.on('ping', () => ({ ok: true }))
    for (let i = 0; i < 10; i += 1) input.write('x'.repeat(30))
    await waitFor(() => errors.length >= 1)
    assert.equal(bridge.closed, false)
    input.write('\n')
    input.write(`${JSON.stringify({ v: 1, id: 'r-2', kind: 'req', op: 'ping', body: {} })}\n`)
    await waitFor(() => captured.includes('\n'))
    assert.deepEqual(JSON.parse(captured.trim()).body, { ok: true })
    bridge.close()
  })
})

describe('Bridge transport failure', () => {
  it('rejects pending requests when the output errors with input still open', async () => {
    const errors = []
    const { input, output, bridge } = lonely({ onError: (cause) => errors.push(cause) })
    const pending = bridge.request('hangs', {})
    const settled = Promise.allSettled([pending])
    assert.equal(input.destroyed, false)
    output.destroy(new Error('pipe broke'))
    const [result] = await settled
    assert.equal(result.status, 'rejected')
    assert.match(String(result.reason), /pipe broke/)
    assert.equal(bridge.closed, true)
    assert.equal(input.destroyed, false)
    await waitFor(() => errors.length >= 1)
    await assert.rejects(bridge.request('late', {}), /pipe broke/)
    bridge.close()
  })

  it('rejects a request whose write throws synchronously', async () => {
    const { EventEmitter } = await import('node:events')
    const input = new PassThrough()
    const output = new EventEmitter()
    output.write = () => {
      throw new Error('write failed')
    }
    const bridge = new Bridge({ input, output })
    await assert.rejects(bridge.request('x', {}), /write failed/)
    assert.equal(bridge.closed, true)
    bridge.close()
  })

  it('answers not-serializable for an unserializable handler result', async () => {
    const { a, b } = pair()
    b.on('circular', () => {
      const body = { ok: true }
      body.self = body
      return body
    })
    assert.deepEqual(await a.request('circular', {}), { ok: false, error: 'not-serializable' })
    // A per-request failure, not a transport failure: both sides stay up.
    assert.equal(a.closed, false)
    assert.equal(b.closed, false)
    b.on('ping', () => ({ ok: true }))
    assert.deepEqual(await a.request('ping', {}), { ok: true })
    a.close()
    b.close()
  })

  it('a fatal failure ends the output so the peer observes EOF', async () => {
    const errors = []
    const fatal = []
    const { a, b } = pair({ maxFrameBytes: 60 })
    b.onError((cause) => errors.push(String(cause)))
    b.onFatal((cause) => fatal.push(String(cause)))
    b.on('hangs', () => new Promise(() => {}))
    const pending = a.request('hangs', {})
    const settled = Promise.allSettled([pending])
    let peerEof = false
    a.input.once('end', () => {
      peerEof = true
    })
    // An oversized request into b whose bounded refusal cannot fit either:
    // b fails fatally. Nothing here touches a's input — its EOF must arrive
    // as the consequence of b ending its output.
    a.output.write(
      `${JSON.stringify({ v: 1, id: 'n-99', kind: 'req', op: 'big', body: { text: 'x'.repeat(200) } })}\n`,
    )
    const [result] = await settled
    assert.equal(result.status, 'rejected')
    assert.equal(result.reason?.error, 'eof')
    assert.equal(peerEof, true)
    assert.equal(a.closed, true)
    assert.equal(b.closed, true)
    assert.match(errors[0], /cannot fit maxFrameBytes/)
    assert.match(fatal[0], /cannot fit maxFrameBytes/)
    a.close()
    b.close()
  })
})

describe('Bridge dispatch never blocks on a handler', () => {
  it('serves a nested reverse request while the outer handler is awaiting', async () => {
    const { a, b } = pair()
    a.on('inner', () => ({ v: 42 }))
    b.on('outer', async () => ({ got: await b.request('inner', {}) }))
    assert.deepEqual(await a.request('outer', {}), { got: { v: 42 } })
    a.close()
    b.close()
  })

  it('dispatches other frames while one handler is still pending', async () => {
    const { a, b } = pair()
    let release
    const gate = new Promise((resolve) => {
      release = resolve
    })
    b.on('slow', () => gate.then(() => ({ ok: true })))
    b.on('fast', () => ({ ok: 'fast' }))
    const slow = a.request('slow', {})
    assert.deepEqual(await a.request('fast', {}), { ok: 'fast' })
    release()
    assert.deepEqual(await slow, { ok: true })
    a.close()
    b.close()
  })
})

describe('bridge activation: pipe and --json gate', () => {
  it('treats a stream without an fd as a pipe unless it is a TTY', () => {
    assert.equal(stdinIsPipe(new PassThrough()), true)
    assert.equal(stdinIsPipe({ isTTY: true }), false)
    assert.equal(stdinIsPipe({ isTTY: false }), true)
  })

  it('classifies real descriptors: regular file, /dev/null and invalid fds are not pipes', () => {
    const t = tempEnv()
    try {
      const regular = join(t.root, 'stdin.txt')
      writeFileSync(regular, 'x\n')
      const fileFd = openSync(regular, 'r')
      const nullFd = openSync('/dev/null', 'r')
      try {
        assert.equal(stdinIsPipe({ fd: fileFd }), false)
        assert.equal(stdinIsPipe({ fd: nullFd }), false)
        assert.equal(stdinIsPipe({ fd: 987654 }), false)
      } finally {
        closeSync(fileFd)
        closeSync(nullFd)
      }
    } finally {
      t.cleanup()
    }
  })

  it('ignores frames on a regular-file stdin: handle line only, no bridge', async () => {
    const t = tempEnv()
    const { spawn } = await import('node:child_process')
    const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
    const inputPath = join(t.root, 'stdin.txt')
    writeFileSync(
      inputPath,
      `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'ping', body: {} })}\n`,
    )
    const inputFd = openSync(inputPath, 'r')
    const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
      env: t.env,
      stdio: [inputFd, 'pipe', 'pipe'],
    })
    closeSync(inputFd)
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
      assert.ok(handle.url.length > 0)
      // The ping frame sitting in the regular file must produce no response:
      // a non-pipe stdin never starts the bridge.
      await assert.rejects(nextLine(500), /no line arrived/)
    } finally {
      child.kill()
      t.cleanup()
    }
  })

  it('stays prose without --json: a ping frame gets no answer, EOF still exits', async () => {
    const t = tempEnv()
    const { spawn } = await import('node:child_process')
    const { join } = await import('node:path')
    const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
    const child = spawn(process.execPath, [cf, 'ui', '--no-open'], {
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

      // Prose mode prints two lines, not a handle: drain both first.
      const first = await nextLine()
      assert.match(first, /^roster editor: /)
      await nextLine()

      child.stdin.write(
        `${JSON.stringify({ v: 1, id: 'r-1', kind: 'req', op: 'ping', body: {} })}\n`,
      )
      await assert.rejects(nextLine(500), /no line arrived/)

      child.stdin.end()
      const code = await new Promise((resolve, reject) => {
        child.once('exit', resolve)
        setTimeout(() => reject(new Error('the editor kept serving')), 10_000)
      })
      assert.equal(code, 0)
    } finally {
      child.kill()
      t.cleanup()
    }
  })
})

describe('cf ui --json --no-open speaks the bridge after its handle line', () => {
  it('keeps the handle line first, then answers a ping frame, then exits on EOF', async () => {
    const t = tempEnv()
    const { spawn } = await import('node:child_process')
    const { join } = await import('node:path')
    const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
    const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
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
        setTimeout(() => reject(new Error('the editor kept serving')), 10_000)
      })
      assert.equal(code, 0)
    } finally {
      child.kill()
      t.cleanup()
    }
  })
})
