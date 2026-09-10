import { execFile } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { constants } from 'node:fs'
import { open, readdir } from 'node:fs/promises'
import { isAbsolute, join } from 'node:path'
import { promisify } from 'node:util'
import { envelope, pointer } from '../../hosts/lib/deliveries.js'

const run = promisify(execFile)
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const normalizeStart = (text) => String(text).trim().replace(/\s+/g, ' ')
const refused = (cause) => ({
  ok: false,
  admitted: false,
  bytesWritten: 0,
  error: 'peer-refused',
  cause,
})

/** A durable delivery always has the same native UUID, including after restart. */
export function submissionId(target, record) {
  const bytes = createHash('sha256')
    .update(`${target.session}\0${record.id}\0${record.answerId}`)
    .digest()
    .subarray(0, 16)
  bytes[6] = (bytes[6] & 15) | 0x50
  bytes[8] = (bytes[8] & 63) | 0x80
  const hex = bytes.toString('hex')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

async function readNativeJson(path, { key = false } = {}) {
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    const stat = await file.stat()
    if (
      !stat.isFile() ||
      stat.uid !== process.getuid?.() ||
      stat.size > (key ? 4096 : 32768) ||
      (stat.mode & (key ? 0o077 : 0o022)) !== 0
    ) {
      throw Error('native peer metadata has unsafe ownership, permissions or size')
    }
    return JSON.parse(await file.readFile('utf8'))
  } finally {
    await file.close()
  }
}

async function discover(config, session, deadline) {
  const directory = join(config.configDir, 'sessions')
  const matches = []
  for (const file of await readdir(directory)) {
    if (!/^\d+\.json$/.test(file)) continue
    let row
    try {
      row = await readNativeJson(join(directory, file))
    } catch {
      continue
    }
    if (row.sessionId === session) matches.push({ file, row })
  }
  if (matches.length !== 1) {
    const error = Error(
      matches.length
        ? 'native Claude session has ambiguous inboxes'
        : 'native Claude inbox is not registered',
    )
    error.code = 'native-session-unavailable'
    throw error
  }
  const { file, row } = matches[0]
  if (
    row.pid !== Number(file.slice(0, -5)) ||
    !Number.isSafeInteger(row.pid) ||
    row.pid <= 0 ||
    row.kind !== 'interactive' ||
    row.entrypoint !== 'cli' ||
    typeof row.procStart !== 'string' ||
    typeof row.messagingSocketPath !== 'string' ||
    !isAbsolute(row.messagingSocketPath)
  ) {
    throw Error('native Claude inbox has unsupported or inconsistent identity')
  }
  if (deadline <= Date.now()) throw Error('native Claude peer deadline expired')
  const { stdout } = await run('/bin/ps', ['-p', String(row.pid), '-o', 'lstart='], {
    timeout: Math.max(1, deadline - Date.now()),
    maxBuffer: 4096,
    // Native Claude records this identity in UTC with the C locale.
    env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' },
  })
  if (normalizeStart(stdout) !== normalizeStart(row.procStart))
    throw Error('native Claude inbox belongs to a stale process')
  const digest = createHash('sha256').update(row.messagingSocketPath).digest('hex')
  const key = await readNativeJson(join(directory, `${row.pid}.${digest}.key`), { key: true })
  if (
    !/^[0-9a-f]{32}$/.test(key.peerToken) ||
    normalizeStart(key.procStart) !== normalizeStart(row.procStart)
  ) {
    throw Error('native Claude peer credential belongs to a different process')
  }
  // /clear or resume may replace the session while credentials are read.
  const current = await readNativeJson(join(directory, file))
  if (
    current.sessionId !== session ||
    current.procStart !== row.procStart ||
    current.messagingSocketPath !== row.messagingSocketPath
  ) {
    throw Error('native Claude conversation changed before submission')
  }
  return { pid: row.pid, socket: row.messagingSocketPath, peerToken: key.peerToken }
}

/** Resolve only the native process owned by this pane, never by cwd or recency. */
export async function currentSession(config, pane, bridge) {
  const listed = await bridge.request('pane.list', {})
  const live = listed?.panes?.find(
    (p) => p.id === pane.id && p.generation === pane.generation && p.alive === true,
  )
  if (!Number.isSafeInteger(live?.processGroupId) || live.processGroupId <= 0) return null
  const matches = []
  for (const file of await readdir(join(config.configDir, 'sessions'))) {
    if (!/^\d+\.json$/.test(file)) continue
    try {
      const row = await readNativeJson(join(config.configDir, 'sessions', file))
      if (!UUID.test(row.sessionId)) continue
      const group = Number(
        (
          await run('/bin/ps', ['-p', String(row.pid), '-o', 'pgid='], { timeout: 1000 })
        ).stdout.trim(),
      )
      if (group !== live.processGroupId) continue
      const peer = await discover(config, row.sessionId, Date.now() + 3000)
      if (peer.pid === row.pid) matches.push(row.sessionId)
    } catch {
      /* A disappearing or invalid process is not binding evidence. */
    }
  }
  return matches.length === 1 ? matches[0] : null
}

async function sendText(target, text, record) {
  const pane =
    typeof target?.pane === 'object'
      ? target.pane
      : { id: target?.pane, generation: target?.generation }
  const launch = target?.launch?.channel ?? target?.launch
  if (
    !UUID.test(target?.session) ||
    typeof pane?.id !== 'string' ||
    !Number.isSafeInteger(pane.generation) ||
    pane.generation < 1 ||
    !Number.isSafeInteger(target.epoch) ||
    target.epoch < 0 ||
    launch?.kind !== 'claude-peer' ||
    typeof launch.configDir !== 'string' ||
    !isAbsolute(launch.configDir) ||
    typeof target.bridge?.request !== 'function' ||
    typeof text !== 'string'
  ) {
    throw Error('claude-peer requires a native session, pane generation, epoch, launch and bridge')
  }
  const budgets = [3000, target.deadlineMs, launch.ackTimeoutMs].filter((n) => n !== undefined)
  if (budgets.some((n) => !Number.isSafeInteger(n) || n < 0))
    throw Error('claude-peer requires a non-negative deadline')
  const deadline = Math.min(Date.now() + Math.min(...budgets), record?.expiresAt ?? Infinity)
  if (deadline <= Date.now()) return refused('native Claude peer delivery expired')
  let peer
  try {
    peer = await discover(launch, target.session, deadline)
  } catch (error) {
    return { ...refused(error.message), error: error.code ?? 'peer-refused' }
  }
  const uuid = record?.nativeSubmissionId ?? (record ? submissionId(target, record) : randomUUID())
  const body =
    JSON.stringify({ type: 'auth', token: peer.peerToken }) +
    '\n' +
    JSON.stringify({
      type: 'user',
      msgV: 1,
      msg_id: uuid,
      uuid,
      session_id: target.session,
      from: 'consensflow',
      priority: 'next',
      message: { role: 'user', content: text },
    }) +
    '\n'
  // Native 2.1.263/265 frames allow 1 MiB. Keep our transport at the
  // app's stricter 64 KiB input bound; large answers use a read pointer.
  if (Buffer.byteLength(body) > 64 * 1024)
    return refused('native Claude peer message exceeds the frame limit')
  const timeoutMs = deadline - Date.now()
  if (timeoutMs <= 0) return refused('native Claude peer delivery expired')
  try {
    // Rust checks the CONNECTED peer PID, UID and pane process group, then
    // claims this input epoch before the first byte. No terminal input occurs.
    const result = await target.bridge.request(
      'pane.send_peer',
      {
        id: pane.id,
        generation: pane.generation,
        epoch: target.epoch,
        socket: peer.socket,
        peerPid: peer.pid,
        body,
        timeoutMs,
      },
      { deadlineMs: timeoutMs + 100 },
    )
    // A write only starts receipt tracking. The watcher accepts the result
    // from the complete envelope in the native lead history, never from flush.
    return result
  } catch {
    // Once handed to the bridge, even a timeout may follow a complete write.
    return {
      ok: false,
      admitted: null,
      error: 'uncertain',
      cause: 'native Claude peer transport outcome is unknown',
    }
  }
}

export async function send(target, text) {
  return await sendText(target, text)
}

export async function deliver(channel, target, record) {
  if (channel !== 'claude-peer') throw Error(`unsupported Claude channel: ${channel}`)
  return await sendText(
    target,
    record.channel === 'cf-read' ? pointer(record) : envelope(record),
    record,
  )
}
