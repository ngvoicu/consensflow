import assert from 'node:assert/strict'
import { execFile, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { promisify } from 'node:util'
import { currentSession, send } from '../src/channels/claude-peer.js'

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
  const response = await send(f.target, f.record.answer)
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
  assert.match(message.uuid, /^[a-f0-9-]{36}$/)
  assert.equal(message.msg_id, message.uuid)
  assert.equal(message.msgV, 1)
  assert.equal(message.priority, 'next')
  assert.equal(message.message.content, f.record.answer)
  assert.doesNotMatch(JSON.stringify(response), /111111111111/)
})

test('Claude native registry ignores version metadata', async (t) => {
  const f = await fixture(t)
  await writeFile(f.registry, JSON.stringify({ ...f.native, version: 'arbitrary-version' }), {
    mode: 0o600,
  })
  assert.equal((await send(f.target, f.record.answer)).ok, true)
  assert.equal(f.calls.length, 1)
})

for (const [label, fields] of [
  ['stale PID incarnation', { procStart: 'Mon Jan 1 00:00:00 2001' }],
  ['session mismatch', { sessionId: 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee' }],
])
  test(`Claude native peer refuses ${label} before sending`, async (t) => {
    const f = await fixture(t)
    await writeFile(f.registry, JSON.stringify({ ...f.native, ...fields }))
    const response = await send(f.target, f.record.answer)
    assert.equal(response.admitted, false)
    assert.equal(response.bytesWritten, 0)
    assert.equal(f.calls.length, 0)
  })

test('Claude native peer rejects an ambiguous live session and a symlinked key', async (t) => {
  const f = await fixture(t)
  await writeFile(join(f.root, 'sessions', '99999.json'), JSON.stringify(f.native))
  assert.equal((await send(f.target, f.record.answer)).admitted, false)
  await rm(join(f.root, 'sessions', '99999.json'))
  const original = await readFile(f.key)
  const elsewhere = join(f.root, 'key')
  await writeFile(elsewhere, original, { mode: 0o600 })
  await rm(f.key)
  await symlink(elsewhere, f.key)
  assert.equal((await send(f.target, f.record.answer)).admitted, false)
  assert.equal(f.calls.length, 0)
})

test('Claude native peer never retries a possibly written request', async (t) => {
  const f = await fixture(t)
  f.target.bridge.request = async (...args) => {
    f.calls.push(args)
    throw Error('deadline')
  }
  const response = await send(f.target, f.record.answer)
  assert.equal(response.admitted, null)
  assert.equal(response.error, 'uncertain')
  assert.equal(f.calls.length, 1)
})

test('Claude peer enforces the complete serialized frame byte limit before sending', async (t) => {
  const f = await fixture(t)
  await send(f.target, '')
  const overhead = Buffer.byteLength(f.calls[0].body.body)
  const text = 'x'.repeat(64 * 1024 - overhead)
  assert.equal((await send(f.target, text)).ok, true)
  assert.equal(Buffer.byteLength(f.calls[1].body.body), 64 * 1024)
  const refused = await send(f.target, `${text}x`)
  assert.equal(refused.admitted, false)
  assert.equal(refused.bytesWritten, 0)
  assert.equal(f.calls.length, 2)
})

test('Claude continuation needs explicit native linkage and descendant ownership even while the old process lives', async (t) => {
  const f = await fixture(t)
  const next = 'e6acbeb1-181f-4fdc-a963-6ff6d6d792ee'
  // A separate process group beneath the registered interactive root, as native continuation uses.
  const launcher = spawn(
    process.execPath,
    [
      '-e',
      `const {spawn}=require('node:child_process');const c=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'}); console.log(c.pid); process.on('SIGTERM',()=>{c.kill();process.exit()}); setInterval(()=>{},1000)`,
    ],
    { stdio: ['ignore', 'pipe', 'ignore'], detached: true },
  )
  t.after(() => launcher.kill())
  const nextPid = await new Promise((resolve) =>
    launcher.stdout.once('data', (chunk) => resolve(Number(String(chunk).trim()))),
  )
  t.after(() => {
    try {
      process.kill(nextPid)
    } catch {}
  })
  const register = async (pid, id, kind) => {
    const procStart = (
      await run('/bin/ps', ['-p', String(pid), '-o', 'lstart='], {
        env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' },
      })
    ).stdout.trim()
    const socket = `/tmp/cc-socks/${pid}.sock`
    const row = { ...f.native, pid, sessionId: id, kind, procStart, messagingSocketPath: socket }
    await writeFile(join(f.root, 'sessions', `${pid}.json`), JSON.stringify(row), { mode: 0o600 })
    await writeFile(
      join(f.root, 'sessions', `${pid}.${createHash('sha256').update(socket).digest('hex')}.key`),
      JSON.stringify({ peerToken: '1'.repeat(32), procStart }),
      { mode: 0o600 },
    )
  }
  await rm(f.registry)
  await register(launcher.pid, session, 'interactive')
  await register(nextPid, next, 'bg')
  const bridge = {
    request: async () => ({
      panes: [{ id: 'p-1', generation: 1, alive: true, processGroupId: launcher.pid }],
    }),
  }
  await mkdir(join(f.root, 'projects', 'test'), { recursive: true })
  const transcript = join(f.root, 'projects', 'test', `${session}.jsonl`)
  await writeFile(transcript, `${JSON.stringify({ type: 'system', sessionId: session })}\n`)
  assert.equal(
    await currentSession(f.target.launch, { id: 'p-1', generation: 1 }, bridge),
    session,
    'background children alone do not change the lead',
  )
  await writeFile(
    join(f.root, 'projects', 'test', `${next}.jsonl`),
    `${JSON.stringify({ type: 'system', sessionId: next })}\n`,
  )
  await writeFile(
    transcript,
    `${JSON.stringify({
      type: 'continued-in',
      sessionId: session,
      continuedInSessionId: next,
      timestamp: '2026-09-12T13:57:18.549Z',
    })}\n`,
  )
  assert.equal(await currentSession(f.target.launch, { id: 'p-1', generation: 1 }, bridge), next)
  // An unrelated process with the same asserted successor ID must fail closed.
  await rm(join(f.root, 'sessions', `${nextPid}.json`))
  await register(f.native.pid, next, 'bg')
  assert.equal(await currentSession(f.target.launch, { id: 'p-1', generation: 1 }, bridge), null)
})
