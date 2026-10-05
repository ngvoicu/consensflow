/**
 * What a client sent on one connection, as the server's socket received it,
 * cut into the requests and the bodies they carried. A body is read here, and
 * not from the request the handler is given, because a handler that refuses
 * before it reads one leaves its chunks to Node, which drops what has not
 * come yet: how much would depend on how the client's packets fell.
 */

const HEAD_END = Buffer.from('\r\n\r\n')
const LINE_END = Buffer.from('\r\n')

export class Wire {
  #buffer = Buffer.alloc(0)
  #current = null
  /** One entry per request seen: its body so far, how long the head said it is, whether it is whole. */
  bodies = []

  push(chunk) {
    this.#buffer = Buffer.concat([this.#buffer, chunk])
    while (this.#step());
  }

  /** The request in progress when the connection ended is left as far as it came. */
  end() {
    if (this.#current !== null) this.#finish(false)
  }

  /** One step of the parse: whether there may be another. */
  #step() {
    if (this.#current === null) return this.#head()
    return this.#current.chunked ? this.#chunk() : this.#plain()
  }

  #head() {
    const at = this.#buffer.indexOf(HEAD_END)
    if (at < 0) return false
    const head = this.#buffer.subarray(0, at).toString('latin1')
    this.#buffer = this.#buffer.subarray(at + HEAD_END.length)
    const declared = /^content-length:\s*(\d+)/im.exec(head)?.[1]
    this.#current = {
      declared: declared === undefined ? null : Number(declared),
      chunked: /^transfer-encoding:.*\bchunked\b/im.test(head),
      parts: [],
      length: 0,
    }
    if (!this.#current.chunked && !this.#current.declared) this.#finish(true)
    return true
  }

  #plain() {
    const current = this.#current
    const take = this.#buffer.subarray(0, current.declared - current.length)
    current.parts.push(take)
    current.length += take.length
    this.#buffer = this.#buffer.subarray(take.length)
    if (current.length < current.declared) return false
    this.#finish(true)
    return true
  }

  #chunk() {
    const line = this.#buffer.indexOf(LINE_END)
    if (line < 0) return false
    const size = Number.parseInt(this.#buffer.subarray(0, line).toString('latin1'), 16)
    if (size === 0) {
      // The last chunk, its trailers (none are sent) and the blank line.
      const end = this.#buffer.indexOf(HEAD_END, line)
      if (end < 0) return false
      this.#buffer = this.#buffer.subarray(end + HEAD_END.length)
      this.#finish(true)
      return true
    }
    const stop = line + LINE_END.length + size + LINE_END.length
    if (this.#buffer.length < stop) return false
    this.#current.parts.push(this.#buffer.subarray(line + LINE_END.length, stop - LINE_END.length))
    this.#current.length += size
    this.#buffer = this.#buffer.subarray(stop)
    return true
  }

  #finish(whole) {
    const { declared, parts, length } = this.#current
    this.bodies.push({
      chunks: parts,
      length,
      declared: whole ? length : (declared ?? length),
      whole,
    })
    this.#current = null
  }
}

/** The wire of each connection the server accepted. */
const wires = new WeakMap()

/** The server's connections are listened to, as they open. */
export function listen(server) {
  server.on('connection', (socket) => {
    const wire = new Wire()
    wires.set(socket, wire)
    socket.on('data', (chunk) => wire.push(chunk))
    socket.once('close', () => wire.end())
  })
}

/** The wire of the connection a request came on, and which request of it that is (from 0). */
export function wireOf(request) {
  const wire = wires.get(request.socket)
  if (wire === undefined) return undefined
  const seen = (seenOn.get(wire) ?? 0) + 1
  seenOn.set(wire, seen)
  return { wire, index: seen - 1 }
}

const seenOn = new WeakMap()
