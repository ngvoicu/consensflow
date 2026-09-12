import { execFile } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { constants } from 'node:fs'
import { open, readdir } from 'node:fs/promises'
import { isAbsolute, join } from 'node:path'
import { promisify } from 'node:util'
import { answers } from '../../hosts/lib/completion.js'

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

async function discover(config, session, deadline, allowBackground = false) {
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
  if (row.kind === 'bg' && !allowBackground) {
    const error = Error('native Claude background inbox requires verified continuation ownership')
    error.code = 'background-peer'
    throw error
  }
  if (
    row.pid !== Number(file.slice(0, -5)) ||
    !Number.isSafeInteger(row.pid) ||
    row.pid <= 0 ||
    (row.kind !== 'interactive' && !(allowBackground && row.kind === 'bg')) ||
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
  return { pid: row.pid, socket: row.messagingSocketPath, peerToken: key.peerToken, kind: row.kind }
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
  if (matches.length !== 1) return null
  let session = matches[0]
  const visited = new Set()
  for (let hop = 0; hop < 16; hop++) {
    if (visited.has(session)) return null
    visited.add(session)
    const completion = await answers('claude-code', session, {
      CLAUDE_CONFIG_DIR: config.configDir,
    })
    if (!completion.continuedInSessionId) {
      if (completion.unknown && visited.size > 1) return null
      return session
    }
    const successor = completion.continuedInSessionId
    try {
      const peer = await discover(config, successor, Date.now() + 3000, true)
      if (!(await belongsToPane(peer.pid, live.processGroupId))) return null
    } catch {
      return null
    }
    session = successor
  }
  return null
}

async function belongsToPane(pid, group) {
  const seen = new Set()
  for (let depth = 0; pid > 1 && depth < 64 && !seen.has(pid); depth++) {
    seen.add(pid)
    const values = (
      await run('/bin/ps', ['-p', String(pid), '-o', 'ppid=,pgid='], { timeout: 1000 })
    ).stdout
      .trim()
      .split(/\s+/)
      .map(Number)
    if (values.length !== 2 || values.some((n) => !Number.isSafeInteger(n) || n <= 0)) return false
    if (values[1] === group) return true
    pid = values[0]
  }
  return false
}

async function sendText(target, text) {
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
  const deadline = Date.now() + Math.min(...budgets)
  if (deadline <= Date.now()) return refused('native Claude peer delivery expired')
  let peer
  let allowDescendant = false
  try {
    peer = await discover(launch, target.session, deadline)
  } catch (error) {
    if (error.code !== 'background-peer')
      return { ...refused(error.message), error: error.code ?? 'peer-refused' }
    try {
      // Only an explicit continuation from this pane's interactive root may use
      // a background descendant. Other background agents remain ineligible.
      if ((await currentSession(launch, pane, target.bridge)) !== target.session) throw error
      peer = await discover(launch, target.session, deadline, true)
      allowDescendant = peer.kind === 'bg'
    } catch {
      return { ...refused(error.message), error: error.code ?? 'peer-refused' }
    }
  }
  const completion = await answers('claude-code', target.session, {
    CLAUDE_CONFIG_DIR: launch.configDir,
  })
  if (completion.continuedInSessionId || completion.reason?.includes('native continuation'))
    return {
      ...refused('native Claude conversation continued before submission'),
      error: 'native-session-changed',
    }
  const uuid = randomUUID()
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
        ...(allowDescendant ? { allowDescendant: true } : {}),
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
