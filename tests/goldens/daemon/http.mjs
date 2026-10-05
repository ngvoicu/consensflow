/**
 * `node:http` as the API's module imports it (`hooks.mjs` puts this in its
 * place for that module alone): a server whose request listener is run in
 * the frame of the exchange it answers.
 */
import * as real from 'node:http'
import { watch } from './exchange.mjs'
import * as session from './session.mjs'
import { listen } from './wire.mjs'

export * from 'node:http'
export default real

const watched = (listener) => (request, reply) => {
  const exchange = watch(request, reply)
  return exchange === null
    ? listener(request, reply)
    : session.frames.run(exchange, () => listener(request, reply))
}

/** `http.createServer`, the listener wrapped and the connections listened to; the server is the real one. */
export function createServer(...args) {
  const at = typeof args[0] === 'function' ? 0 : 1
  if (typeof args[at] === 'function') args[at] = watched(args[at])
  const server = real.createServer(...args)
  listen(server)
  return server
}
