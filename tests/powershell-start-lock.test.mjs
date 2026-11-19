import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { existsSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { PassThrough } from 'node:stream'
import test, { after } from 'node:test'
import { fileURLToPath } from 'node:url'
import { PowerShellStartLock, watchReady } from './support/powershell-start-lock.mjs'

const own = mkdtempSync(path.join(os.tmpdir(), 'cf-ps-lock-test-'))
const inherited = process.env.CF_PWSH_START_LOCK
process.env.CF_PWSH_START_LOCK = path.join(own, 'lock')
writeFileSync(process.env.CF_PWSH_START_LOCK, '')
after(() => {
  if (inherited === undefined) delete process.env.CF_PWSH_START_LOCK
  else process.env.CF_PWSH_START_LOCK = inherited
  rmSync(own, { recursive: true, force: true })
})

test('a split startup marker is removed without losing surrounding output', async () => {
  const stream = new PassThrough()
  let sent = ''
  const ready = watchReady(stream, '__CF_READY__', (text) => {
    sent += text
  })
  stream.write('before __CF_')
  stream.write('READY__after')
  stream.end()
  await ready
  assert.equal(sent, 'before after')
})

test('output stays intact when a process ends before its marker', async () => {
  const stream = new PassThrough()
  let sent = ''
  const ready = watchReady(stream, '__CF_READY__', (text) => {
    sent += text
  })
  const text = Buffer.from('întrebare', 'utf8')
  stream.write(text.subarray(0, 1))
  stream.end(text.subarray(1))
  await ready
  assert.equal(sent, 'întrebare')
})

test('a second stand-in starts only after the first has finished starting', async () => {
  const lock = new PowerShellStartLock()
  const children = []
  const events = []
  const launch = (name) => () => {
    events.push(`${name} started`)
    const child = spawn(
      process.execPath,
      ['-e', "setTimeout(() => console.log('ready'), 200); setTimeout(() => {}, 400)"],
      { stdio: ['ignore', 'pipe', 'ignore'] },
    )
    children.push(child)
    const ready = once(child.stdout, 'data').then(() => {
      events.push(`${name} ready`)
    })
    return { ready, value: child }
  }
  try {
    const first = lock.start(launch('first'))
    for (let i = 0; i < 300 && events.length === 0; i++) {
      await new Promise((resolve) => setTimeout(resolve, 10))
    }
    assert.deepEqual(events, ['first started'])
    const second = lock.start(launch('second'))
    await new Promise(setImmediate)
    assert.deepEqual(events, ['first started'])
    await Promise.all([first, second])
    assert.deepEqual(events, ['first started', 'first ready', 'second started', 'second ready'])
  } finally {
    await Promise.all(
      children.map(async (child) => {
        if (child.exitCode === null && child.signalCode === null) {
          await once(child, 'close')
        }
      }),
    )
  }
})

test('a failed start releases the next turn', async () => {
  const lock = new PowerShellStartLock()
  const first = lock.start(() => ({
    ready: Promise.reject(new Error('start failed')),
    value: null,
  }))
  const second = lock.start(() => ({ ready: Promise.resolve(), value: 'next' }))
  await assert.rejects(first, /start failed/)
  assert.equal(await second, 'next')
})

test('a started process may keep running while the next starts', async () => {
  const lock = new PowerShellStartLock()
  let finish
  const stillRunning = new Promise((resolve) => {
    finish = resolve
  })
  const first = lock.start(() => ({ ready: Promise.resolve(), value: stillRunning }))
  const second = lock.start(() => ({ ready: Promise.resolve(), value: 'next' }))
  assert.equal(await second, 'next')
  finish('finished')
  assert.equal(await first, 'finished')
})

async function twoWorkers(work, env) {
  const helper = fileURLToPath(new URL('./support/powershell-start-standin.mjs', import.meta.url))
  const children = ['a', 'b'].map((name) =>
    spawn(process.execPath, [helper, work, name], {
      stdio: 'ignore',
      env,
    }),
  )
  const exits = children.map((child) => once(child, 'close'))
  try {
    for (
      let i = 0;
      i < 300 && !['a', 'b'].every((name) => existsSync(path.join(work, name)));
      i++
    ) {
      await new Promise((resolve) => setTimeout(resolve, 10))
    }
    assert.ok(
      ['a', 'b'].every((name) => existsSync(path.join(work, name))),
      'both workers ready',
    )
    writeFileSync(path.join(work, 'go'), '')
    assert.deepEqual(
      (await Promise.all(exits)).map(([code]) => code),
      [0, 0],
    )
    const events = readFileSync(path.join(work, 'events'), 'utf8').trim().split('\n')
    assert.equal(events.length, 4)
    assert.match(events[0], /^start [ab]$/)
    assert.equal(events[1], events[0].replace('start ', 'ready '))
    assert.equal(events[3], events[2].replace('start ', 'ready '))
  } finally {
    await Promise.all(exits)
  }
}

test('different test-file processes bound their starts in the same run', async () => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-ps-lock-'))
  const lock = path.join(work, 'lock')
  writeFileSync(lock, '')
  try {
    await twoWorkers(work, { ...process.env, CF_PWSH_START_LOCK: lock })
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})

test('without a supplied lock, two processes share a persistent temp lock', async () => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-ps-lock-fallback-'))
  const lock = path.join(work, 'consensflow-pwsh-start-v1.lock')
  writeFileSync(lock, 'keep')
  try {
    await twoWorkers(work, {
      ...process.env,
      CF_PWSH_START_LOCK: '',
      TMPDIR: work,
      TMP: work,
      TEMP: work,
    })
    assert.equal(readFileSync(lock, 'utf8'), 'keep', 'the lock was not rewritten or removed')
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})

test('a link at the lock path is refused', { skip: process.platform === 'win32' }, async () => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-ps-lock-link-'))
  const target = path.join(work, 'another-file')
  const link = path.join(work, 'lock')
  writeFileSync(target, 'keep')
  symlinkSync(target, link)
  const probe = fileURLToPath(new URL('./support/powershell-lock-probe.mjs', import.meta.url))
  try {
    const child = spawn(
      process.execPath,
      [probe, path.join(work, 'ready'), path.join(work, 'entered')],
      {
        stdio: 'ignore',
        env: { ...process.env, CF_PWSH_START_LOCK: link },
      },
    )
    const [code] = await once(child, 'close')
    assert.notEqual(code, 0, 'a lock path leading to another file must be refused')
    assert.equal(readFileSync(target, 'utf8'), 'keep')
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})
