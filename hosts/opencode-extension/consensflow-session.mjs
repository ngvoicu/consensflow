import { createServer } from 'node:http'
import { createReceiver } from '../lib/receiver.js'

export const id = 'consensflow-session'
const SESSION = /^ses_[A-Za-z0-9]+$/
const refused = (error) => ({ ok: false, admitted: false, bytesWritten: 0, error })

/** This API runs inside the TUI: server-side session lists cannot prove its selected route. */
export async function tui(api, options) {
  const { launchId, port, token } =
    options ?? JSON.parse(process.env.CF_OPENCODE_SESSION_BRIDGE ?? '{}')
  if (
    typeof launchId !== 'string' ||
    !/^[A-Za-z0-9._-]+$/.test(launchId) ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65535 ||
    typeof token !== 'string' ||
    token.length < 24
  )
    throw new Error('Invalid ConsensFlow session bridge configuration')
  const currentSession = () => {
    const route = api.route.current
    return route.name === 'session' && SESSION.test(route.params?.sessionID ?? '')
      ? route.params.sessionID
      : null
  }
  const receiverOptions =
    options?.receiver ??
    (process.env.CF_RESULT_RECEIVER ? { config: process.env.CF_RESULT_RECEIVER } : null)
  const nativeReady = () =>
    api.state.ready &&
    currentSession() &&
    (api.state.session.status(currentSession())?.type ?? 'idle') === 'idle' &&
    api.mode.current() === 'base'
  const receiver = receiverOptions
    ? createReceiver({
        ...receiverOptions,
        session: currentSession,
        ready: nativeReady,
        insert: async (claim) => {
          if (currentSession() !== claim.receiver.session || !nativeReady())
            return refused('native-session-changed')
          const result = await api.client.session.promptAsync(
            {
              sessionID: claim.receiver.session,
              parts: [{ type: 'text', text: claim.text }],
            },
            { signal: AbortSignal.timeout(5000) },
          )
          if (result.error || result.response?.status !== 204)
            throw new Error('native admission unknown')
          return { admitted: true }
        },
      })
    : null
  const server = createServer(async (request, response) => {
    const reply = (status, value) => {
      response.writeHead(status, {
        'content-type': 'application/json',
        'cache-control': 'no-store',
      })
      response.end(JSON.stringify(value))
    }
    if (request.headers.authorization !== `Bearer ${token}`) {
      request.resume()
      return reply(401, refused('unauthorized'))
    }
    if (request.method === 'GET' && request.url === '/session')
      return reply(200, { launchId, sessionId: currentSession() })
    if (request.method !== 'POST' || request.url !== '/deliver') {
      request.resume()
      return reply(404, refused('unknown-operation'))
    }
    let input
    try {
      let bytes = 0
      const chunks = []
      for await (const chunk of request) {
        bytes += chunk.length
        if (bytes > 128 * 1024) return reply(413, refused('request-too-large'))
        chunks.push(chunk)
      }
      input = JSON.parse(Buffer.concat(chunks).toString('utf8'))
    } catch {
      return reply(400, refused('invalid-request'))
    }
    if (
      input?.launchId !== launchId ||
      !SESSION.test(input?.sessionId ?? '') ||
      typeof input.text !== 'string' ||
      !input.text ||
      Buffer.byteLength(input.text) > 64 * 1024 ||
      !Number.isFinite(input.expiresAt)
    )
      return reply(400, refused('invalid-request'))
    if (input.expiresAt <= Date.now()) return reply(200, refused('expired'))
    // No await between reading the actual route and beginning native submission.
    if (currentSession() !== input.sessionId) return reply(200, refused('native-session-changed'))
    try {
      const result = await api.client.session.promptAsync(
        { sessionID: input.sessionId, parts: [{ type: 'text', text: input.text }] },
        { signal: AbortSignal.timeout(Math.min(3000, input.expiresAt - Date.now())) },
      )
      if (result.error || result.response?.status !== 204)
        throw new Error('native admission unknown')
      reply(200, { ok: true, admitted: true })
    } catch {
      // A started request can have reached the native server; never claim zero bytes.
      reply(200, { ok: false, admitted: null, error: 'uncertain' })
    }
  })
  server.requestTimeout = 3000
  server.headersTimeout = 3000
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(port, '127.0.0.1', resolve)
  })
  receiver?.start()
  api.lifecycle.onDispose(async () => {
    await receiver?.stop().catch(() => {})
    await new Promise((resolve) => {
      server.close(resolve)
      server.closeAllConnections()
    })
  })
  return { receiver }
}

export default { id, tui }
