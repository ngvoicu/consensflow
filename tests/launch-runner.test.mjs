import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
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
