/**
 * The failures the admin's goldens script are the failures Node gives: what
 * `execFile` rejects with (the `killed` flag, the message, both streams) and
 * what `fetch` says of a connection that failed, a redirect it refuses, a body
 * cut off, a request out of time and an answer with no body. The goldens
 * (tests/goldens/admin) script these, never the machine's own, and the Rust
 * feed says the same words (crates/cf-harness/src/admin/feed.rs).
 */
import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { createServer } from 'node:http'
import test from 'node:test'
import { promisify } from 'node:util'

const run = promisify(execFile)

/** What `execFile` rejects with when `script` is run by this very Node. */
async function rejection(script, options = {}) {
  try {
    await run(process.execPath, ['-e', script], options)
  } catch (error) {
    return error
  }
  throw new Error('it did not reject')
}

test('execFile rejects with the message, both streams and whether it was ended for its time', async () => {
  const exit = await rejection(
    "process.stdout.write('out'); process.stderr.write('err'); process.exit(3)",
  )
  assert.equal(exit.killed, false)
  assert.equal(exit.code, 3)
  assert.equal(exit.stdout, 'out')
  assert.equal(exit.stderr, 'err')
  assert.match(exit.message, /^Command failed: .*\nerr$/s)

  const slow = await rejection("process.stdout.write('hi'); setTimeout(() => {}, 5000)", {
    timeout: 300,
  })
  assert.equal(slow.killed, true)
  assert.equal(slow.stdout, 'hi')
  assert.equal(slow.stderr, '')

  // Too much on a stream is no time out: `killed` is not set, and the stream is named.
  const loud = await rejection("process.stdout.write('x'.repeat(100))", { maxBuffer: 10 })
  assert.ok(!loud.killed)
  assert.equal(loud.message, 'stdout maxBuffer length exceeded')
  assert.equal(loud.stdout, 'xxxxxxxxxx')
  const noisy = await rejection("process.stderr.write('y'.repeat(100))", { maxBuffer: 10 })
  assert.equal(noisy.message, 'stderr maxBuffer length exceeded')
  assert.equal(noisy.stderr, 'yyyyyyyyyy')

  // A program that is not there never started: no streams, nothing killed.
  const missing = await run('/nonexistent/consensflow-cli', ['--version']).catch((error) => error)
  assert.ok(!missing.killed)
  assert.equal(missing.message, 'spawn /nonexistent/consensflow-cli ENOENT')
  assert.equal(missing.stdout, '')
  assert.equal(missing.stderr, '')
})

/** A server that answers `/<kind>` as the kind says, and the address of it. */
async function feed() {
  const server = createServer((request, response) => {
    if (request.url === '/redirect') {
      response.writeHead(302, { location: '/ok' })
      response.end()
    } else if (request.url === '/cut') {
      response.writeHead(200, { 'content-length': '100' })
      response.write('{"version":')
      setTimeout(() => response.socket.destroy(), 20)
    } else if (request.url === '/stall') {
      // never answers
    } else if (request.url === '/stall-body') {
      response.writeHead(200)
      response.write('{"vers')
    } else if (request.url === '/empty') {
      response.writeHead(204)
      response.end()
    } else if (request.url.startsWith('/status/')) {
      const [, , status, located] = request.url.split('/')
      response.writeHead(Number(status), located === 'with' ? { location: '/ok' } : {})
      response.end('{"version":"1.2.3"}')
    } else {
      response.writeHead(request.url === '/500' ? 500 : 200)
      response.end('{"version":"1.2.3"}')
    }
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  return { server, base: `http://127.0.0.1:${port}`, port }
}

/** What fetching `url` the way the admin does comes to: the text read, or the error. */
async function fetched(url, signal) {
  try {
    const response = await fetch(url, { signal, redirect: 'error' })
    if (response.body === null) return { status: response.status, body: null }
    let text = ''
    for await (const bytes of response.body) text += Buffer.from(bytes).toString('utf8')
    return { status: response.status, ok: response.ok, text }
  } catch (error) {
    return { error: { name: error.name, message: error.message } }
  }
}

test('fetch says what the goldens script of a connection that fails, a redirect, a cut and a timeout', async () => {
  const { server, base, port } = await feed()
  try {
    assert.deepEqual(await fetched(`${base}/ok`), {
      status: 200,
      ok: true,
      text: '{"version":"1.2.3"}',
    })
    assert.deepEqual(await fetched(`${base}/500`), {
      status: 500,
      ok: false,
      text: '{"version":"1.2.3"}',
    })
    assert.deepEqual(await fetched(`${base}/redirect`), {
      error: { name: 'TypeError', message: 'fetch failed' },
    })
    assert.deepEqual(await fetched(`${base}/cut`), {
      error: { name: 'TypeError', message: 'terminated' },
    })
    for (const stall of ['stall', 'stall-body']) {
      assert.deepEqual(await fetched(`${base}/${stall}`, AbortSignal.timeout(100)), {
        error: { name: 'TimeoutError', message: 'The operation was aborted due to timeout' },
      })
    }
    // An answer of 204 has no body to read: iterating it threw V8's TypeError.
    assert.deepEqual(await fetched(`${base}/empty`), { status: 204, body: null })
    // A redirect is refused by its status alone, with a Location or without;
    // every other status is an answer (the feed's REDIRECTS).
    for (const status of [300, 301, 302, 303, 304, 305, 307, 308]) {
      for (const located of ['with', 'without']) {
        const answer = await fetched(`${base}/status/${status}/${located}`)
        if ([301, 302, 303, 307, 308].includes(status)) {
          assert.deepEqual(
            answer,
            { error: { name: 'TypeError', message: 'fetch failed' } },
            `${status} ${located} a Location`,
          )
        } else {
          assert.equal(answer.status, status, `${status} ${located} a Location`)
          assert.equal(answer.error, undefined, `${status} ${located} a Location`)
        }
      }
    }
  } finally {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
  }
  assert.deepEqual(await fetched(`http://127.0.0.1:${port}/ok`), {
    error: { name: 'TypeError', message: 'fetch failed' },
  })
})
