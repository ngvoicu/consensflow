import assert from 'node:assert/strict'
import { execFile, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { promisify } from 'node:util'
import { deliver, send, submissionId } from '../src/channels/claude-peer.js'

const session = '17499106-8778-48e1-a306-87bd186c9f7e'
const run = promisify(execFile)

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'cf-peer-adapter-'))
  const child = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'ignore' })
  t.after(async () => {
    child.kill()
    await rm(root, { recursive: true, force: true })
  })
  const procStart = (
    await run('/bin/ps', ['-p', String(child.pid), '-o', 'lstart='], {
      env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' },
    })
  ).stdout.trim()
  const directory = join(root, 'sessions')
  await mkdir(directory)
  const socket = `/tmp/cc-socks/${child.pid}.sock`
  const registry = join(directory, `${child.pid}.json`)
  const key = join(
    directory,
    `${child.pid}.${createHash('sha256').update(socket).digest('hex')}.key`,
  )
  const native = {
    pid: child.pid,
    sessionId: session,
    procStart,
    messagingSocketPath: socket,
    kind: 'interactive',
    entrypoint: 'cli',
    version: '2.1.265',
    peerProtocol: 1,
  }
  await writeFile(registry, JSON.stringify(native), { mode: 0o600 })
  await writeFile(key, JSON.stringify({ peerToken: '1'.repeat(32), procStart }), { mode: 0o600 })
  const calls = []
  const target = {
    session,
    pane: 'p-1',
    generation: 1,
    epoch: 17,
    launch: { kind: 'claude-peer', configDir: root, preservesDraft: 1 },
    bridge: {
      request: async (op, body) => {
        calls.push({ op, body })
        return { ok: true }
      },
    },
  }
  const record = {
    id: 'd-19',
    answerId: 'a-1',
    conversation: 'worker-one',
    agent: 'zeus',
    answer: 'The whole result.',
    channel: 'pty-inline',
    target: { session },
    expiresAt: Date.now() + 3000,
  }
  return { root, registry, key, native, target, record, calls }
}

test('Claude native peer sends a complete result without claiming native acceptance from a write', async (t) => {
  const f = await fixture(t)
  const response = await deliver('claude-peer', f.target, f.record)
  assert.equal(response.ok, true)
  assert.equal(response.admitted, undefined)
  assert.equal(f.calls.length, 1)
  assert.equal(f.calls[0].op, 'pane.send_peer')
  const { body } = f.calls[0]
  assert.equal(body.epoch, 17)
  assert.equal(body.peerPid, f.native.pid)
  const [auth, message] = body.body.trimEnd().split('\n').map(JSON.parse)
  assert.equal(auth.type, 'auth')
  assert.equal(message.session_id, session)
  assert.equal(message.uuid, submissionId(f.target, f.record))
  assert.equal(message.msg_id, message.uuid)
  assert.equal(message.msgV, 1)
  assert.equal(message.priority, 'next')
  assert.match(message.message.content, /The whole result\.\n\[end of delivery d-19\]/)
  assert.doesNotMatch(JSON.stringify(response), /111111111111/)
})

test('large result pointers and ordinary follow-ups use the same native ingress', async (t) => {
  const f = await fixture(t)
  await deliver('claude-peer', f.target, { ...f.record, channel: 'cf-read' })
  await send(f.target, 'Continue the same task.')
  const messages = f.calls.map((c) => JSON.parse(c.body.body.trimEnd().split('\n')[1]))
  assert.match(messages[0].message.content, /cf read d-19/)
  assert.equal(messages[1].message.content, 'Continue the same task.')
  assert.notEqual(messages[0].uuid, messages[1].uuid)
})

test('Claude native registry ignores version metadata', async (t) => {
  const f = await fixture(t)
  await writeFile(f.registry, JSON.stringify({ ...f.native, version: 'arbitrary-version' }), {
    mode: 0o600,
  })
  assert.equal((await deliver('claude-peer', f.target, f.record)).ok, true)
  assert.equal(f.calls.length, 1)
})

for (const [label, fields] of [
  ['stale PID incarnation', { procStart: 'Mon Jan 1 00:00:00 2001' }],
  ['session mismatch', { sessionId: 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee' }],
])
  test(`Claude native peer refuses ${label} before sending`, async (t) => {
    const f = await fixture(t)
    await writeFile(f.registry, JSON.stringify({ ...f.native, ...fields }))
    const response = await deliver('claude-peer', f.target, f.record)
    assert.equal(response.admitted, false)
    assert.equal(response.bytesWritten, 0)
    assert.equal(f.calls.length, 0)
  })

test('Claude native peer rejects an ambiguous live session and a symlinked key', async (t) => {
  const f = await fixture(t)
  await writeFile(join(f.root, 'sessions', '99999.json'), JSON.stringify(f.native))
  assert.equal((await deliver('claude-peer', f.target, f.record)).admitted, false)
  await rm(join(f.root, 'sessions', '99999.json'))
  const original = await readFile(f.key)
  const elsewhere = join(f.root, 'key')
  await writeFile(elsewhere, original, { mode: 0o600 })
  await rm(f.key)
  await symlink(elsewhere, f.key)
  assert.equal((await deliver('claude-peer', f.target, f.record)).admitted, false)
  assert.equal(f.calls.length, 0)
})

test('Claude native peer never retries a possibly written request', async (t) => {
  const f = await fixture(t)
  f.target.bridge.request = async (...args) => {
    f.calls.push(args)
    throw Error('deadline')
  }
  const response = await deliver('claude-peer', f.target, f.record)
  assert.equal(response.admitted, null)
  assert.equal(response.error, 'uncertain')
  assert.equal(f.calls.length, 1)
})

test('an expired Claude peer delivery sends zero bytes', async (t) => {
  const f = await fixture(t)
  const result = await deliver('claude-peer', f.target, { ...f.record, expiresAt: Date.now() - 1 })
  assert.equal(result.admitted, false)
  assert.equal(result.bytesWritten, 0)
  assert.equal(f.calls.length, 0)
})

test('Claude peer enforces the complete serialized frame byte limit before sending', async (t) => {
  const f = await fixture(t)
  await send(f.target, '')
  const overhead = Buffer.byteLength(f.calls[0].body.body)
  const text = 'x'.repeat(64 * 1024 - overhead)
  assert.equal((await send(f.target, text)).ok, true)
  assert.equal(Buffer.byteLength(f.calls[1].body.body), 64 * 1024)
  const refused = await send(f.target, text + 'x')
  assert.equal(refused.admitted, false)
  assert.equal(refused.bytesWritten, 0)
  assert.equal(f.calls.length, 2)
})
