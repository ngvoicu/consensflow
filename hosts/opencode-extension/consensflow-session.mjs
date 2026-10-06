import { createServer } from 'node:http'
import {
  acknowledge,
  answerFromWindow,
  askTheBoard,
  boardClient,
  refusalReason,
} from '../lib/question-door.js'

export const id = 'consensflow-session'
const SESSION = /^ses_[A-Za-z0-9]+$/
const refused = (error) => ({ ok: false, admitted: false, bytesWritten: 0, error })

/**
 * Whether OpenCode took a reply to its question tool: only then is the board's
 * answer received. By its SDK's contract a request that failed does not throw:
 * an HTTP error (a native 404 for a question that is gone), or a request that
 * got no response, resolves as a result with its `error` (`throwOnError` is
 * off), so a reply that resolved may be one that did not take. Throwing is
 * asked for, for the SDKs that honour it, and the result is read all the same:
 * it took when it holds no error and its response was a success, or it holds
 * the `true` the question's reply answers with (an SDK that answers with the
 * data alone says `true`, and nothing at all for an error).
 */
async function replied(client, input) {
  try {
    const result = await client.question.reply(input, { throwOnError: true })
    if (result === true) return true
    return result != null && !result.error && (result.response?.ok === true || result.data === true)
  } catch {
    return false
  }
}

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
  // The question tool's door in a member's window: the questions go to the
  // board as this window's participant, the board's answer comes back through
  // the API as the question's reply, and a reply given in the window first
  // goes to the board instead, so the question is answered once either way.
  // The chief's question stays in its window, where the human answers it.
  const board = process.env.CONSENSFLOW_PARTICIPANT === 'chief' ? null : boardClient()
  const held = new Map()
  const relay = async ({ id, sessionID, questions }) => {
    if (board === null || sessionID !== currentSession() || held.has(id)) return
    const control = new AbortController()
    held.set(id, control)
    try {
      const asked = await askTheBoard(
        board,
        questions.map((q) => ({
          question: q.question,
          header: q.header,
          options: q.options.map((o) => ({ label: o.label, description: o.description })),
          multiple: q.multiple === true,
        })),
        { signal: control.signal },
      )
      if (control.window !== undefined) {
        // An answer the board gave in the same moment was claimed and is not handed over.
        if (asked.answer !== null) await acknowledge(board, asked.answer, false)
        await answerFromWindow(board, asked.id, control.window)
      } else if (asked.answer !== null) {
        // Handed to the tool, then said so: the other way round, an answer would be lost.
        const handed = await replied(api.client, { requestID: id, answers: asked.answer.choices })
        await acknowledge(board, asked.answer, handed)
      }
    } catch (cause) {
      // Refused: the model hears why, as the answer. Not reached: the window's
      // own dialog stays.
      if (cause?.refused) {
        const reason = refusalReason(cause)
        await api.client.question
          .reply({ requestID: id, answers: questions.map(() => [reason]) })
          .catch(() => {})
      }
    } finally {
      held.delete(id)
    }
  }
  const settle = (requestID, answers) => {
    const control = held.get(requestID)
    if (control === undefined) return
    if (answers !== undefined) control.window = answers
    control.abort()
  }
  // OpenCode 1.18.31's TUI bus carries these under their plain names, with
  // the v2 shape its types describe under `question.v2.*`; both are heard.
  for (const suffix of ['', 'v2.']) {
    api.event?.on(`question.${suffix}asked`, (event) => void relay(event.properties))
    api.event?.on(`question.${suffix}replied`, (event) =>
      settle(event.properties.requestID, event.properties.answers),
    )
    api.event?.on(`question.${suffix}rejected`, (event) => settle(event.properties.requestID))
  }

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
    if (request.method === 'GET' && request.url === '/session') {
      // What OpenCode says the shown conversation is doing: idle, busy, or
      // waiting to retry a refused request. A spent quota shows only here:
      // OpenCode writes nothing to its store while it waits for the reset.
      // No status is idle, as for delivery: it has not worked since the window opened.
      const sessionId = currentSession()
      const status =
        sessionId === null ? null : (api.state?.session?.status(sessionId) ?? { type: 'idle' })
      return reply(200, { launchId, sessionId, status })
    }
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
  api.lifecycle.onDispose(async () => {
    await new Promise((resolve) => {
      server.close(resolve)
      server.closeAllConnections()
    })
  })
}

export default { id, tui }
