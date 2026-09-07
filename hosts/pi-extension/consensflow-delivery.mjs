import { watch } from 'node:fs'
import { access, mkdir, readdir, readFile, rename, unlink, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { envelope, pointer } from '../../hosts/lib/deliveries.js'

const DELIVERY_ID = /^d-\d+$/
const DEFAULT_ACK_TIMEOUT_MS = 30_000

function messageText(message) {
  if (typeof message?.content === 'string') return message.content
  if (!Array.isArray(message?.content)) return null
  return message.content
    .filter((part) => part?.type === 'text')
    .map((part) => part.text)
    .join('')
}

function envelopeText(record) {
  if (typeof record?.text !== 'string' || typeof record?.answer !== 'string') return null
  try {
    if (record.text === pointer(record)) return record.text
  } catch {}
  try {
    if (record.text === envelope(record)) return record.text
  } catch {}
  return null
}

function validDeliveryId(id) {
  return typeof id === 'string' && DELIVERY_ID.test(id)
}

/**
 * Install the delivery extension against Pi's ExtensionAPI.
 *
 * The fake in tests follows the installed Pi 0.85.1 types: ExtensionAPI.on
 * registers `session_start`, `agent_settled`, `message_start`, and
 * `session_shutdown` handlers (dist/core/extensions/types.d.ts:909, 926,
 * 931, 916), while `sendUserMessage` is a void action (lines 976-983).
 * `message_start` is the admission boundary: its MessageStartEvent carries
 * the user AgentMessage (lines 591-595), after Pi has put it into the agent
 * run, and it does not wait for the turn's promise.
 */
export function createDeliveryExtension(
  pi,
  { inbox, ack, quarantine, ackTimeoutMs = DEFAULT_ACK_TIMEOUT_MS, logger = console } = {},
) {
  let context
  let watcher
  let running = false
  let queued = false
  const pending = new Map()
  const timeout =
    Number.isFinite(ackTimeoutMs) && ackTimeoutMs >= 0 ? ackTimeoutMs : DEFAULT_ACK_TIMEOUT_MS

  const logError = (...args) => logger?.error?.(...args)

  const quarantinePath = async (file) => {
    await mkdir(quarantine, { recursive: true })
    for (let suffix = 0; ; suffix += 1) {
      const name = suffix === 0 ? file : `${file}.${suffix}`
      const destination = join(quarantine, name)
      try {
        await access(destination)
      } catch (cause) {
        if (cause?.code === 'ENOENT') return destination
        throw cause
      }
    }
  }

  const acknowledge = async (id, admitted, reason) => {
    const entry = pending.get(id)
    if (entry === undefined || entry.done) return
    entry.done = true
    clearTimeout(entry.timer)
    const response = { id, admitted }
    if (admitted) response.mode = 'tui'
    else response.reason = reason
    const ackPath = join(ack, `${id}.json`)
    const temporary = `${ackPath}.tmp`
    try {
      await mkdir(ack, { recursive: true })
      await writeFile(temporary, `${JSON.stringify(response)}\n`, 'utf8')
      await rename(temporary, ackPath)
      await unlink(entry.path)
      pending.delete(id)
    } catch (cause) {
      logError(`could not acknowledge delivery ${id}: ${cause?.message ?? String(cause)}`)
    }
  }

  const observeMessage = (event, ctx) => {
    context = ctx ?? context
    if (event?.message?.role !== 'user') return
    const text = messageText(event.message)
    if (text === null) return
    for (const [id, entry] of pending) {
      if (entry.text === text) void acknowledge(id, true)
    }
  }

  const consume = async () => {
    if (running) {
      queued = true
      return
    }
    if (context?.isIdle() !== true || typeof inbox !== 'string' || typeof ack !== 'string') return
    running = true
    try {
      const files = (await readdir(inbox)).filter((name) => name.endsWith('.json')).sort()
      for (const file of files) {
        if (context.isIdle() !== true) break
        const path = join(inbox, file)
        let record
        try {
          record = JSON.parse(await readFile(path, 'utf8'))
        } catch (cause) {
          logError(`could not read delivery ${file}: ${cause?.message ?? String(cause)}`)
          continue
        }
        const id = record?.id
        if (!validDeliveryId(id)) {
          if (typeof quarantine === 'string') {
            try {
              await rename(path, await quarantinePath(file))
              logError(`invalid delivery id quarantined from ${file}`)
            } catch (cause) {
              logError(
                `could not quarantine invalid delivery ${file}: ${cause?.message ?? String(cause)}`,
              )
            }
          } else {
            logError(`invalid delivery id in ${file}: quarantine is not configured`)
          }
          continue
        }
        const text = envelopeText(record)
        if (text === null) {
          logError(`delivery ${id} is missing envelope or pointer`)
          continue
        }
        if (pending.has(id)) continue
        const entry = { path, text, timer: null, done: false }
        pending.set(id, entry)
        entry.timer = setTimeout(
          () => void acknowledge(id, false, 'user-message-not-observed'),
          timeout,
        )
        try {
          // Pi's ExtensionAPI deliberately returns void here. Admission is
          // determined only by the message_start event above.
          pi.sendUserMessage(text)
        } catch (cause) {
          void acknowledge(id, false, 'send-user-message-threw')
          logError(`could not submit delivery ${id}: ${cause?.message ?? String(cause)}`)
          break
        }
        break
      }
    } finally {
      running = false
      if (queued) {
        queued = false
        void consume()
      }
    }
  }

  pi.on('session_start', async (_event, ctx) => {
    context = ctx
    if (typeof inbox !== 'string' || typeof ack !== 'string') return
    await mkdir(inbox, { recursive: true })
    watcher = watch(inbox, { persistent: false }, () => void consume())
    await consume()
  })
  pi.on('message_start', observeMessage)
  pi.on('agent_settled', async (_event, ctx) => {
    context = ctx ?? context
    await consume()
  })
  pi.on('session_shutdown', () => {
    watcher?.close()
    watcher = undefined
    for (const entry of pending.values()) clearTimeout(entry.timer)
    pending.clear()
    context = undefined
  })

  return { consume }
}

// This is the only environment-reading entry point. The launch producer
// supplies these names in the enabled Pi process environment.
export default function consensflowDelivery(pi) {
  return createDeliveryExtension(pi, {
    inbox: process.env.CF_DELIVERY_INBOX,
    ack: process.env.CF_DELIVERY_ACK,
    quarantine: process.env.CF_DELIVERY_QUARANTINE,
    ackTimeoutMs: Number(process.env.CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS),
  })
}
