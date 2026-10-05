import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { send } from '../src/channels/pi.js'
import { play } from './goldens/launch/runner.mjs'

/** Plays `steps` against a stand-in adapter whose window does what `window` says. */
function played(steps, window) {
  const scenario = { name: 'stand-in', harness: 'stand-in', env: { HOME: '$ROOT/home' }, steps }
  return play(scenario, { 'stand-in': ({ env }) => window(env) })
}

/** A timer of the scenario's clock as a promise. */
const after = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

describe("the launch recorder's clock", () => {
  it('runs a timer’s continuations before the next timer due with it, as Node does', async () => {
    const { records } = await played([{ observe: true }, { advance: 10 }], () => ({
      observe: () =>
        new Promise((resolve) => {
          after(10).then(() => {
            clearTimeout(second)
            resolve('first')
          })
          const second = setTimeout(() => resolve('second'), 10)
        }),
    }))
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ timer: 10 }, { timer: 10 }] }])
    assert.deepEqual(records[1].settled, [{ answer: 'first', op: 0 }])
  })

  it('lets a timer do file work once the one due with it is cleared', async () => {
    const { records } = await played([{ observe: true }, { advance: 10 }], () => ({
      observe: () =>
        new Promise((resolve) => {
          let second
          setTimeout(async () => {
            clearTimeout(second)
            await fs.readFile(fileURLToPath(import.meta.url))
            resolve('first')
          }, 10)
          second = setTimeout(() => resolve('second'), 10)
        }),
    }))
    assert.deepEqual(records[1].settled, [{ answer: 'first', op: 0 }])
  })

  it('refuses file work landing while a timer due with the one that began it waits', async () => {
    await assert.rejects(
      played([{ observe: true }, { advance: 10 }], () => ({
        observe: () =>
          new Promise((resolve) => {
            let second
            setTimeout(async () => {
              await fs.readFile(fileURLToPath(import.meta.url))
              clearTimeout(second)
              resolve('first')
            }, 10)
            second = setTimeout(() => resolve('second'), 10)
          }),
      })),
      /file request or turn of the loop landed between two timers due together: Node fires the second before it lands/,
    )
  })

  it('refuses a turn of the loop landing while a timer due with the one that began it waits', async () => {
    await assert.rejects(
      played([{ observe: true }, { advance: 10 }], () => ({
        observe: () =>
          new Promise((resolve) => {
            let second
            setTimeout(() => {
              setImmediate(() => {
                clearTimeout(second)
                resolve('first')
              })
            }, 10)
            second = setTimeout(() => resolve('second'), 10)
          }),
      })),
      /turn of the loop landed between two timers due together/,
    )
  })

  it('runs what a timer queues with nextTick before its promises, as Node does', async () => {
    const { records } = await played([{ observe: true }, { advance: 10 }], () => ({
      observe: () =>
        new Promise((resolve) => {
          const order = []
          setTimeout(() => {
            Promise.resolve().then(() => {
              order.push('promise')
              resolve(order)
            })
            process.nextTick(() => order.push('tick'))
          }, 10)
        }),
    }))
    assert.deepEqual(records[1].settled, [{ answer: ['tick', 'promise'], op: 0 }])
  })

  it('refuses timers of different lengths due together', async () => {
    let looks = 0
    await assert.rejects(
      played([{ observe: true }, { advance: 10 }, { observe: true }, { advance: 10 }], () => ({
        observe: () => {
          looks += 1
          return after(looks === 1 ? 20 : 10)
        },
      })),
      /timers of different lengths due together \(20 ms, 10 ms\): Node orders them by its timer lists/,
    )
  })

  it('takes a timer of no length as one of a millisecond, as Node does', async () => {
    const { records } = await played([{ observe: true }, { advance: 1 }], () => ({
      observe: () => after(0).then(() => Date.now()),
    }))
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ timer: 1 }] }])
    assert.deepEqual(records[1].settled, [
      { answer: Date.parse('2026-09-19T12:00:00.001Z'), op: 0 },
    ])
  })

  it('holds a timer armed in a timer’s callback as the work’s own', async () => {
    const { records } = await played([{ observe: true }, { advance: 10 }, { advance: 10 }], () => ({
      observe: () => new Promise((resolve) => setTimeout(() => setTimeout(resolve, 10), 10)),
    }))
    assert.deepEqual(records[1].pending, [{ op: 0, waits: [{ timer: 10 }] }])
    assert.deepEqual(records[2].settled, [{ answer: { undefined: true }, op: 0 }])
  })
})

describe('the launch recorder holds still', () => {
  it('only once the file work beside a held request is done', async () => {
    const steps = [
      { observe: true, answers: { 'held.op': [{ held: true }] } },
      { release: 'held.op', answer: { ok: true } },
    ]
    const { records } = await played(steps, (env) => ({
      observe: (target) => {
        const files = (async () => {
          await fs.mkdir(env.HOME, { recursive: true })
          for (let round = 0; round < 500; round += 1) {
            await fs.writeFile(path.join(env.HOME, 'round'), `${round}`)
          }
          await fs.writeFile(path.join(env.HOME, 'done'), 'done')
        })()
        return Promise.all([target.host.request('held.op', {}), files]).then(() => 'both')
      },
    }))
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ request: 'held.op' }] }])
    assert.ok(
      records[0].tree.some((entry) => entry.path === '$ROOT/home/done' && entry.text === 'done'),
      JSON.stringify(records[0].tree),
    )
    assert.deepEqual(records[1].settled, [{ answer: 'both', op: 0 }])
  })
})

describe('the launch recorder writes', () => {
  it('a string holding half a surrogate pair as its code units, apart from its escape and from an object', async () => {
    const answers = ['a\ud800', 'a\\ud800', { $utf16: [0x61, 0xd800] }]
    const steps = answers.map(() => ({ observe: true }))
    const { records } = await played(steps, () => ({ observe: async () => answers.shift() }))
    assert.deepEqual(
      records.map((record) => record.settled[0].answer),
      [{ $utf16: [0x61, 0xd800] }, 'a\\ud800', { $$utf16: [0x61, 0xd800] }],
    )
  })

  it('a backslash in a POSIX name as the name’s own', {
    skip: process.platform === 'win32',
  }, async () => {
    const { records } = await played([{ observe: true }], (env) => ({
      observe: async () => {
        await fs.mkdir(env.HOME, { recursive: true })
        await fs.writeFile(path.join(env.HOME, 'a\\b'), 'x')
        await fs.symlink(path.join(env.HOME, 'a\\b'), path.join(env.HOME, 'link'))
      },
    }))
    const entries = Object.fromEntries(records[0].tree.map(({ path, ...entry }) => [path, entry]))
    assert.equal(entries['$ROOT/home/a\\b']?.text, 'x')
    assert.equal(entries['$ROOT/home/link']?.link, '$ROOT/home/a\\b')
    assert.equal(entries['$ROOT/home/a/b'], undefined)
  })

  it('the root as the system names it, so that a folder’s real name is under it', async () => {
    const { records } = await played([{ observe: true }], (env) => ({
      observe: async () => {
        const root = path.dirname(env.HOME)
        return (await fs.realpath(root)) === root
      },
    }))
    assert.deepEqual(records[0].settled, [{ answer: true, op: 0 }])
  })

  it('the root as a file URL, as JSON and as a URL’s query hold it, and an OpenCode bundle’s hash', async () => {
    const { records } = await played([{ observe: true }], (env) => ({
      observe: async () => {
        const root = path.dirname(env.HOME)
        const hash = 'ab12'.repeat(16)
        return [
          `${pathToFileURL(root).href}/extensions/opencode/${hash}/tui.json`,
          `?directory=${encodeURIComponent(root)}%2Fwork`,
          `?directory=${encodeURIComponent(`${root}'s`).replaceAll("'", '%27')}`,
          `?directory=${new URLSearchParams({ d: `${root} x` }).toString().slice(2)}`,
          JSON.stringify({ at: root }),
          `${root}${path.sep}extensions${path.sep}pi${path.sep}${hash}`,
        ]
      },
    }))
    const [url, component, quoted, form, json, pi] = records[0].settled[0].answer
    assert.equal(url, 'file://$ROOT/extensions/opencode/$HASH/tui.json')
    assert.equal(component, '?directory=$ROOT%2Fwork')
    assert.equal(quoted, '?directory=$ROOT%27s')
    assert.equal(form, '?directory=$ROOT+x')
    assert.equal(json, '{"at":"$ROOT"}')
    assert.match(
      pi,
      /^\$ROOT\/extensions\/pi\/[0-9a-f]{64}$/,
      'Pi’s bundle is named by a hash that stays',
    )
  })
})

describe("the launch recorder's scene", () => {
  it('makes a stand-in a window starts as the script a node shim names, one of a window’s own as a plain one', async () => {
    const steps = [
      { executable: 'opencode', node: true },
      { executable: 'plain' },
      { observe: true },
    ]
    const { records } = await played(steps, (env) => ({
      observe: async () => {
        const bin = path.join(path.dirname(env.HOME), 'bin')
        const first = await fs.readFile(path.join(bin, 'opencode'), 'utf8').catch(() => '')
        return { names: (await fs.readdir(bin)).sort(), shebang: first.split('\n')[0] }
      },
    }))
    const { names, shebang } = records[0].settled[0].answer
    if (process.platform === 'win32') {
      assert.deepEqual(names, ['opencode.cmd', 'opencode.mjs', 'plain.cmd'])
    } else {
      assert.deepEqual(names, ['opencode', 'plain'])
      assert.equal(shebang, `#!${process.execPath}`)
    }
  })
})

describe("the launch recorder's peer on loopback", () => {
  const ask = (url, init) =>
    fetch(url, init)
      .then(async (response) => ({ status: response.status, body: await response.text() }))
      .catch((cause) => ({ name: cause.name, message: cause.message }))

  it('answers a route as the step scripts it and writes each request down as it was written', async () => {
    const steps = [
      { observe: true, served: { 'POST /deliver': [{ status: 200, body: '{"ok":true}' }] } },
    ]
    const { records } = await played(steps, () => ({
      observe: () =>
        ask('http://127.0.0.1:41000/deliver?directory=a+b', {
          method: 'POST',
          headers: { authorization: 'Bearer t' },
          body: '{"text":"x"}',
        }),
    }))
    assert.deepEqual(records[0].settled, [{ answer: { status: 200, body: '{"ok":true}' }, op: 0 }])
    assert.deepEqual(records[0].fetches, [
      {
        route: 'POST /deliver',
        url: 'http://127.0.0.1:41000/deliver?directory=a+b',
        headers: [['authorization', 'Bearer t']],
        body: '{"text":"x"}',
      },
    ])
  })

  it('holds a head and then a body until steps release them, each a wait of the work', async () => {
    const steps = [
      { observe: true, served: { 'GET /session': [{ held: true }] } },
      { release: 'GET /session', answer: { status: 200, body: { held: true } } },
      { releaseBody: 'GET /session', body: 'whole' },
    ]
    const { records } = await played(steps, () => ({
      observe: () => ask('http://127.0.0.1:41000/session'),
    }))
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ fetch: 'GET /session' }] }])
    assert.deepEqual(records[1].pending, [{ op: 0, waits: [{ fetch: 'GET /session' }] }])
    assert.deepEqual(records[2].settled, [{ answer: { status: 200, body: 'whole' }, op: 0 }])
  })

  it('fails as undici does: no head, a body cut, a timeout before the head and in the body', async () => {
    const steps = [
      {
        observe: true,
        served: {
          'GET /none': [{ noHead: true }],
          'GET /cut': [{ status: 200, body: { cut: true } }],
          'GET /slow': [{ held: true }],
          'GET /slow-body': [{ status: 200, body: { held: true } }],
        },
      },
      { advance: 100 },
    ]
    const { records } = await played(steps, () => ({
      observe: () =>
        Promise.all([
          ask('http://127.0.0.1:41000/none'),
          ask('http://127.0.0.1:41000/cut'),
          ask('http://127.0.0.1:41000/slow', { signal: AbortSignal.timeout(100) }),
          ask('http://127.0.0.1:41000/slow-body', { signal: AbortSignal.timeout(100) }),
        ]),
    }))
    const timeout = { name: 'TimeoutError', message: 'The operation was aborted due to timeout' }
    assert.deepEqual(records[1].settled, [
      {
        answer: [
          { name: 'TypeError', message: 'fetch failed' },
          { name: 'TypeError', message: 'terminated' },
          timeout,
          timeout,
        ],
        op: 0,
      },
    ])
  })

  it('clears a timeout once no request or body waits under it, as Rust drops its timer', async () => {
    const steps = [
      { observe: true, served: { 'GET /health': [{ status: 200, body: 'up' }] } },
      { advance: 100 },
    ]
    const { records } = await played(steps, () => ({
      observe: async () => {
        const lifetime = new AbortController()
        setTimeout(() => lifetime.abort(new Error('over')), 5000)
        const attempt = AbortSignal.any([lifetime.signal, AbortSignal.timeout(500)])
        const answer = await ask('http://127.0.0.1:41000/health', { signal: attempt })
        await after(100)
        return { ...answer, aborted: attempt.aborted }
      },
    }))
    // The attempt's 500 ms went with its request: only the lifetime and the sleep wait.
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ timer: 5000 }, { timer: 100 }] }])
    assert.deepEqual(records[1].settled, [
      { answer: { status: 200, body: 'up', aborted: false }, op: 0 },
    ])
  })

  it('gives a reply of no content no body', async () => {
    const steps = [{ observe: true, served: { 'POST /prompt': [{ status: 204 }] } }]
    const { records } = await played(steps, () => ({
      observe: () =>
        fetch('http://127.0.0.1:41000/prompt', { method: 'POST' }).then((reply) => reply.body),
    }))
    assert.deepEqual(records[0].settled, [{ answer: { undefined: true }, op: 0 }])
  })

  it('answers a body as JSON with the paths in it under the root made whole, or as a text repeated', async () => {
    const steps = [
      {
        observe: true,
        served: {
          'GET /session': [
            { status: 200, body: { json: { id: 'ses_a', directory: '$ROOT/work', n: [1] } } },
          ],
          'GET /big': [{ status: 200, body: { repeat: 'xy', times: 3 } }],
        },
      },
    ]
    const { records } = await played(steps, (env) => ({
      observe: async () => {
        const session = await (await fetch('http://127.0.0.1:41000/session')).json()
        const big = await (await fetch('http://127.0.0.1:41000/big')).text()
        return {
          whole: session.directory === path.join(path.dirname(env.HOME), 'work'),
          session,
          big,
        }
      },
    }))
    const { whole, session, big } = records[0].settled[0].answer
    assert.equal(whole, true)
    assert.deepEqual([session.id, session.n], ['ses_a', [1]])
    assert.equal(big, 'xyxyxy')
  })

  it('clears a timeout once a request that fails has nothing left waiting under it', async () => {
    const steps = [{ observe: true }, { advance: 100 }]
    const { records } = await played(steps, () => ({
      observe: async () => {
        const answer = await fetch('http://127.0.0.1:41000/nobody', {
          signal: AbortSignal.timeout(500),
        }).catch((cause) => cause.message)
        await after(100)
        return answer
      },
    }))
    // The request's 500 ms went with it: only the sleep waits.
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ timer: 100 }] }])
    assert.deepEqual(records[1].settled, [{ answer: 'fetch failed', op: 0 }])
  })

  it('hands out free ports on loopback from 41000 up, as Rust does', async () => {
    const { records } = await played([{ observe: true }], () => ({
      observe: async () => {
        const { createServer } = await import('node:net')
        const ports = []
        for (const _ of [1, 2]) {
          const server = createServer()
          await new Promise((listening) => server.listen(0, '127.0.0.1', listening))
          ports.push(server.address().port)
          await new Promise((closed) => server.close(closed))
        }
        return ports
      },
    }))
    assert.deepEqual(records[0].settled, [{ answer: [41000, 41001], op: 0 }])
  })
})

describe("the launch recorder's children", () => {
  const WINDOWS = process.platform === 'win32'
  /** A stand-in's program and arguments as `runnable` gives them: on Windows its shim's node and script. */
  const program = (name, ...args) =>
    WINDOWS ? [process.execPath, [`C:\\bin\\${name}.mjs`, ...args]] : [`/bin/${name}`, args]
  /** Starts `name` as the stand-in's window would, and what it said until it closed. */
  const run = async (name, args, stdio, act) => {
    const { spawn } = await import('node:child_process')
    const [file, given] = program(name, ...args)
    const child = spawn(file, given, { stdio })
    const events = []
    for (const event of ['spawn', 'error', 'exit', 'close']) {
      child.on(event, (...values) => events.push([event, ...values.map((v) => v?.message ?? v)]))
    }
    const closed = new Promise((done) => child.on('close', done))
    await act(child)
    await closed
    return { events, exitCode: child.exitCode, signalCode: child.signalCode }
  }

  it('speaks a line at a time, and keeps what it was started as and what it was sent', async () => {
    const steps = [
      { observe: true, children: { codex: [{ lines: ['{"id":1}', '{"id":2}'], ends: 'asked' }] } },
    ]
    const { records } = await played(steps, () => ({
      observe: () =>
        run('codex', ['app-server'], ['pipe', 'pipe', 'ignore'], async (child) => {
          child.stdin.write('{"method":"initialize"}\n')
          const lines = []
          await new Promise((two) => {
            child.stdout.on('data', (chunk) => {
              lines.push(...String(chunk).split('\n').filter(Boolean))
              if (lines.length === 2) two()
            })
          })
          child.kill('SIGTERM')
          child.lines = lines
        }),
    }))
    assert.deepEqual(records[0].spawned, ['codex app-server'])
    assert.deepEqual(records[0].written, ['{"method":"initialize"}'])
    const { events, signalCode } = records[0].settled[0].answer
    assert.deepEqual(events, [['spawn'], ['exit', null, 'SIGTERM'], ['close', null, 'SIGTERM']])
    assert.equal(signalCode, 'SIGTERM')
  })

  it('ends by itself, and is not asked to end after it has', async () => {
    const steps = [{ observe: true, children: { opencode: [{ ends: 'itself' }] } }]
    const { records } = await played(steps, () => ({
      observe: () =>
        run('opencode', ['serve'], ['ignore', 'ignore', 'pipe'], async (child) => {
          await new Promise((exited) => child.on('exit', exited))
          child.refused = child.kill('SIGTERM')
        }),
    }))
    assert.deepEqual(records[0].settled[0].answer.events, [
      ['spawn'],
      ['exit', 0, null],
      ['close', 0, null],
    ])
  })

  it('goes on asked when only forcing ends it, and ends when forced', {
    skip: process.platform === 'win32' && 'an end on Windows is always forced',
  }, async () => {
    const steps = [{ observe: true, children: { opencode: [{ ends: 'forced' }] } }]
    const { records } = await played(steps, () => ({
      observe: () =>
        run('opencode', ['serve'], ['ignore', 'ignore', 'pipe'], async (child) => {
          child.kill('SIGTERM')
          await Promise.resolve()
          child.kill('SIGKILL')
        }),
    }))
    assert.deepEqual(records[0].settled[0].answer.events, [
      ['spawn'],
      ['exit', null, 'SIGKILL'],
      ['close', null, 'SIGKILL'],
    ])
  })

  it('says error and close and no exit for a program that is not there, as Node does', async () => {
    const { records } = await played([{ observe: true }], () => ({
      observe: () => run('missing', [], ['ignore', 'ignore', 'pipe'], async () => {}),
    }))
    const { events, exitCode } = records[0].settled[0].answer
    assert.equal(events[0][0], 'error')
    assert.match(events[0][1], /^spawn .* ENOENT$/)
    assert.deepEqual(events.slice(1), [['close', -2, null]])
    assert.equal(exitCode, -2)
    assert.equal(records[0].spawned, undefined, 'one that is not there is not started')
  })

  it('closes a child that a timer’s callback forces to end', {
    skip: WINDOWS && 'an end on Windows is always forced',
  }, async () => {
    const steps = [
      { observe: true, children: { opencode: [{ ends: 'forced' }] } },
      { advance: 2000 },
    ]
    const { records } = await played(steps, () => ({
      observe: () =>
        run('opencode', ['serve'], ['ignore', 'ignore', 'pipe'], async (child) => {
          child.stderr.on('data', () => {})
          child.kill('SIGTERM')
          setTimeout(() => child.kill('SIGKILL'), 2000)
        }),
    }))
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ timer: 2000 }] }])
    assert.deepEqual(records[1].settled[0].answer.events, [
      ['spawn'],
      ['exit', null, 'SIGKILL'],
      ['close', null, 'SIGKILL'],
    ])
  })

  it('answers what a prepare asks of the peer and of the programs it starts', async () => {
    const steps = [
      {
        prepare: { launchId: 'launch-1' },
        served: { 'GET /up': [{ status: 200, body: 'ok' }] },
        children: { opencode: [{ ends: 'itself' }] },
      },
    ]
    const { records } = await played(steps, () => ({
      prepare: async () => {
        const text = await (await fetch('http://127.0.0.1:41000/up')).text()
        const { spawn } = await import('node:child_process')
        const [file, args] = program('opencode', 'serve')
        spawn(file, args, { stdio: ['ignore', 'ignore', 'pipe'] })
        return { argv: [text], env: {}, dropEnv: [], launch: {} }
      },
    }))
    assert.deepEqual(records[0].spawned, ['opencode serve'])
    assert.deepEqual(
      records[0].fetches.map((fetch) => fetch.route),
      ['GET /up'],
    )
    assert.deepEqual(records[0].settled[0].answer.argv, ['ok'])
  })

  it('ends a scripted child that taskkill is asked to end, and nothing of the machine', async () => {
    const steps = [{ observe: true, children: { opencode: [{ ends: 'never' }] } }]
    const { records } = await played(steps, () => ({
      observe: () =>
        run('opencode', ['serve'], ['ignore', 'ignore', 'pipe'], async (child) => {
          const { spawnSync } = await import('node:child_process')
          spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'])
        }),
    }))
    assert.deepEqual(records[0].settled[0].answer.events.slice(-2), [
      ['exit', 1, null],
      ['close', 1, null],
    ])
  })
})

describe("Pi's wait for its acknowledgement", () => {
  it('ends at its deadline on a clock that moves only with timers', async () => {
    // The last millisecond is slept out: read through, it never ended here.
    const steps = [{ deliver: 'nobody admits this' }, { advance: 1101 }]
    const { records } = await played(steps, (env) => ({
      deliver: (target) =>
        send(
          {
            ...target,
            session: 'session-1',
            claim: async () => ({ ok: true }),
            launch: {
              channel: {
                kind: 'pi-extension',
                inbox: path.join(env.HOME, 'inbox'),
                ack: path.join(env.HOME, 'ack'),
                ackTimeoutMs: 100,
                launchId: 'launch-1',
              },
            },
          },
          target.text,
        ),
    }))
    assert.deepEqual(records[0].pending, [{ op: 0, waits: [{ timer: 10 }] }])
    assert.deepEqual(records[1].settled, [
      {
        answer: { ok: false, admitted: null, error: 'uncertain', cause: 'admission-unknown' },
        op: 0,
      },
    ])
  })
})
