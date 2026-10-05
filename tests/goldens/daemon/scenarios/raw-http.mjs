import { request } from 'node:http'

/**
 * One request to a server of the daemon, its method, target, headers and body
 * as given: `http.request` keeps a target as written, which `fetch` does not
 * (it resolves dot segments and turns a backslash into a slash). What comes
 * back is the status, the type, the text and the JSON of the answer (null when
 * the answer has no body).
 */
export function send(server, { method = 'GET', target, headers = {}, body }) {
  const { hostname, port } = new URL(server.url)
  // A body is always declared by its length: `http.request` frames none for a DELETE, and the server then reads it as a request.
  const framed =
    body === undefined || 'content-length' in headers
      ? headers
      : { ...headers, 'content-length': Buffer.byteLength(body) }
  return new Promise((resolve, reject) => {
    const asked = request({ hostname, port, method, path: target, headers: framed }, (reply) => {
      const chunks = []
      reply.on('data', (chunk) => chunks.push(chunk))
      reply.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8')
        let json = null
        try {
          json = text === '' ? null : JSON.parse(text)
        } catch {}
        resolve({ status: reply.statusCode, type: reply.headers['content-type'], text, json })
      })
    })
    asked.on('error', reject)
    asked.end(body)
  })
}

export const bearer = (token) => ({ authorization: `Bearer ${token}` })

/** The status and the error code an answer carried: what a refusal is. */
export const code = (answer) => [answer.status, answer.json?.error ?? null]
