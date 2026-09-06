/**
 * The Node half of the JSON-lines bridge (Phase 1, IMPL-PANE-06).
 *
 * One JSON object per line, `{v:1, id, kind:'req'|'res'|'evt', op, body}`.
 * Ids are namespaced by whoever originates the frame: this side mints `n-<n>`
 * (the Rust side mints `r-`). The two streams are always passed in — this
 * module never touches `process.stdin`/`process.stdout`/`process.env`, so
 * tests can pair two bridges over in-process pipes and the app can hand it
 * the real stdio.
 *
 * Rules, each with a test in `tests/bridge.test.mjs`:
 * - `request` resolves with the response body; past `deadlineMs` it resolves
 *   `{ok:false, error:'deadline'}` instead and a late answer is dropped. The
 *   deadline defaults to 30 s, so a request whose answer was discarded still
 *   settles and the bridge keeps working.
 * - A handler's throw (sync or async) answers `{ok:false, error:<message>}`;
 *   an op with no handler answers `{ok:false, error:'unknown-op'}`.
 * - Frames over `maxFrameBytes` are refused with `{ok:false,
 *   error:'too-large'}` and never written — outgoing ones resolve the caller
 *   that way (events return false), incoming requests are answered that way,
 *   and an oversized handler response is replaced by that bounded answer so
 *   the peer always hears back. When even the replacement cannot fit, the
 *   transport fails explicitly instead of hanging the peer.
 * - A terminal transport failure (input EOF excluded: that is the normal
 *   shutdown) rejects every outstanding request, marks the bridge closed,
 *   ends the output so the peer observes EOF, and is reported through
 *   `onError` and `onFatal`. Output errors fail the transport even with the
 *   input still open; response serialization and write errors are caught — a
 *   per-request serialization failure is answered, never dropped.
 * - A malformed incoming line — bad JSON, wrong shape, missing op or body
 *   — is reported through `onError` and skipped. The oversized path runs the
 *   same complete validator before routing: only a well-formed request is
 *   answered and only a well-formed response settles.
 * - EOF on the input rejects every outstanding request with `error:'eof'`
 *   and marks the bridge closed; a request made after that rejects the same.
 * - Dispatch never blocks on a handler: a handler is invoked and its promise
 *   observed, never awaited, so a handler awaiting a reverse request does not
 *   stop other frames from being dispatched.
 * - Nothing but frames is ever written to the output stream.
 */

const PROTOCOL_VERSION = 1
const DEFAULT_MAX_FRAME_BYTES = 1024 * 1024
const DEFAULT_DEADLINE_MS = 30000
const KINDS = new Set(['req', 'res', 'evt'])

function eofError() {
  const error = new Error('eof')
  error.error = 'eof'
  return error
}

function asErrorBody(cause) {
  return { ok: false, error: cause instanceof Error ? cause.message : String(cause) }
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function validFrame(value) {
  if (!isRecord(value)) return false
  if (value.v !== PROTOCOL_VERSION) return false
  if (typeof value.id !== 'string' || value.id.length === 0) return false
  if (!KINDS.has(value.kind)) return false
  if (typeof value.op !== 'string') return false
  // An own body property: a frame without one is malformed, even when the
  // kind would let the body be null — null is still a property.
  if (!Object.hasOwn(value, 'body')) return false
  return true
}

export class Bridge {
  constructor({
    input,
    output,
    maxFrameBytes = DEFAULT_MAX_FRAME_BYTES,
    defaultDeadlineMs = DEFAULT_DEADLINE_MS,
    idPrefix = 'n-',
    peerIdPrefix = idPrefix === 'n-' ? 'r-' : 'n-',
    onError = null,
    onFatal = null,
  } = {}) {
    if (input === null || input === undefined) throw new Error('Bridge needs an input stream')
    if (output === null || output === undefined) throw new Error('Bridge needs an output stream')
    this.input = input
    this.output = output
    this.maxFrameBytes = maxFrameBytes
    this.defaultDeadlineMs = defaultDeadlineMs
    this.idPrefix = idPrefix
    this.peerIdPrefix = peerIdPrefix
    this._bufs = []
    this._bufferedBytes = 0
    this._discarding = false
    this._counter = 0
    this._pending = new Map()
    this._handlers = new Map()
    this._eventHandlers = new Map()
    this._onError = null
    this._onFatal = null
    this._closed = false
    this._terminalError = null
    if (onError !== null && onError !== undefined) this.onError(onError)
    if (onFatal !== null && onFatal !== undefined) this.onFatal(onFatal)
    this._onData = (chunk) => this._receive(chunk)
    this._onEnd = () => this._eof()
    this._onInputError = (cause) => this._fail(cause)
    this._onOutputError = (cause) => this._fail(cause)
    input.on('data', this._onData)
    input.on('end', this._onEnd)
    input.on('close', this._onEnd)
    input.on('error', this._onInputError)
    output.on('error', this._onOutputError)
  }

  get closed() {
    return this._closed
  }

  /** Report malformed input and stream errors here instead of throwing. */
  onError(handler) {
    this._onError = handler
    return this
  }

  /** Called once when the transport fails fatally (never for input EOF). */
  onFatal(handler) {
    this._onFatal = handler
    return this
  }

  /** Handle incoming requests for `op`. The return becomes the response body. */
  on(op, handler) {
    this._handlers.set(op, handler)
    return () => {
      if (this._handlers.get(op) === handler) this._handlers.delete(op)
    }
  }

  /** Handle incoming events for `op`. Events are never answered. */
  onEvent(op, handler) {
    const handlers = this._eventHandlers.get(op) ?? []
    handlers.push(handler)
    this._eventHandlers.set(op, handlers)
    return () => {
      const current = this._eventHandlers.get(op) ?? []
      const at = current.indexOf(handler)
      if (at !== -1) current.splice(at, 1)
    }
  }

  /**
   * Ask the other side for `op`. Resolves with the response body, with
   * `{ok:false, error:'deadline'}` past `deadlineMs` (default 30 s when the
   * caller passes none, so a request whose answer was discarded still
   * settles), with `{ok:false, error:'too-large'}` when the frame would not
   * fit `maxFrameBytes`, and rejects with `error:'eof'` when the input ends
   * first.
   */
  request(op, body, { deadlineMs } = {}) {
    if (this._closed) return Promise.reject(this._terminalError ?? eofError())
    if (body === undefined) body = null
    const id = this._nextId()
    const line = JSON.stringify({ v: PROTOCOL_VERSION, id, kind: 'req', op, body })
    if (Buffer.byteLength(line, 'utf8') > this.maxFrameBytes) {
      return Promise.resolve({ ok: false, error: 'too-large' })
    }
    const effectiveDeadlineMs = deadlineMs ?? this.defaultDeadlineMs
    return new Promise((resolve, reject) => {
      const entry = { op, resolve, reject, timer: null }
      if (effectiveDeadlineMs !== null && effectiveDeadlineMs !== undefined) {
        entry.timer = setTimeout(() => {
          if (this._pending.delete(id)) resolve({ ok: false, error: 'deadline' })
        }, effectiveDeadlineMs)
        if (typeof entry.timer.unref === 'function') entry.timer.unref()
      }
      this._pending.set(id, entry)
      this._write(line)
    })
  }

  /**
   * Send an event the other side never answers. True when written, false
   * when the frame would not fit `maxFrameBytes` or the write itself failed
   * (and so was never written).
   */
  event(op, body) {
    if (this._closed) return false
    if (body === undefined) body = null
    const id = this._nextId()
    const line = JSON.stringify({ v: PROTOCOL_VERSION, id, kind: 'evt', op, body })
    if (Buffer.byteLength(line, 'utf8') > this.maxFrameBytes) return false
    return this._writeChecked(line)
  }

  /** Reject every outstanding request and stop dispatching. Idempotent. */
  close() {
    this.input.removeListener('data', this._onData)
    this.input.removeListener('end', this._onEnd)
    this.input.removeListener('close', this._onEnd)
    this.input.removeListener('error', this._onInputError)
    this.output.removeListener('error', this._onOutputError)
    this._eof()
  }

  _nextId() {
    this._counter += 1
    return `${this.idPrefix}${this._counter}`
  }

  _receive(chunk) {
    if (this._closed) return
    const buf = typeof chunk === 'string' ? Buffer.from(chunk, 'utf8') : chunk
    if (this._discarding) {
      this._discardThroughNewline(buf)
      return
    }
    this._bufs.push(buf)
    this._pump()
  }

  /**
   * Bytes are accumulated until the newline and decoded then, so a multibyte
   * character split across reads never becomes replacement characters. The
   * retained tail never exceeds maxFrameBytes: an unterminated line past the
   * budget switches to discarding (reported once) until its newline arrives,
   * and the transport stays up for whatever follows.
   */
  _pump() {
    let tail = Buffer.concat(this._bufs)
    this._bufs = []
    for (;;) {
      const newline = tail.indexOf(0x0a)
      if (newline === -1) break
      let line = tail.subarray(0, newline)
      tail = tail.subarray(newline + 1)
      if (line.length > 0 && line[line.length - 1] === 0x0d) {
        line = line.subarray(0, line.length - 1)
      }
      this._dispatchBytes(line)
      if (this._closed) return
    }
    if (tail.length > this.maxFrameBytes) {
      this._discarding = true
      this._report(new Error('bridge frame over maxFrameBytes'))
      this._bufferedBytes = 0
      return
    }
    if (tail.length > 0) this._bufs = [tail]
    this._bufferedBytes = tail.length
  }

  _discardThroughNewline(buf) {
    const newline = buf.indexOf(0x0a)
    if (newline === -1) return
    this._discarding = false
    const rest = buf.subarray(newline + 1)
    if (rest.length > 0) {
      this._bufs.push(rest)
      this._pump()
    }
  }

  _dispatchBytes(line) {
    if (line.length === 0) return
    // A complete line is decoded once, whole, so multibyte characters split
    // across reads arrive intact. Oversized complete lines are still decoded
    // for routing — the peer's request needs its too-large answer — but the
    // accumulator above never retains more than maxFrameBytes of unterminated
    // bytes.
    const raw = line.toString('utf8')
    if (line.length > this.maxFrameBytes) {
      this._refuseOversized(raw)
      return
    }
    this._dispatch(raw)
  }

  _dispatch(raw) {
    if (raw.length === 0) return
    if (Buffer.byteLength(raw, 'utf8') > this.maxFrameBytes) {
      this._refuseOversized(raw)
      return
    }
    let frame
    try {
      frame = JSON.parse(raw)
    } catch (cause) {
      this._report(cause)
      return
    }
    if (!validFrame(frame)) {
      this._report(new Error(`malformed bridge frame: ${raw.slice(0, 80)}`))
      return
    }
    const expectedPrefix = frame.kind === 'res' ? this.idPrefix : this.peerIdPrefix
    if (!frame.id.startsWith(expectedPrefix)) {
      this._report(new Error(`bridge frame id ${frame.id} has wrong namespace for ${frame.kind}`))
      return
    }
    if (frame.kind === 'res') {
      this._settle(frame.id, frame.op, frame.body)
      return
    }
    if (frame.kind === 'evt') {
      this._emit(frame.op, frame.body)
      return
    }
    this._serve(frame)
  }

  /** The complete shape plus the right namespace for the frame kind. */
  _validIncoming(value) {
    if (!validFrame(value)) return false
    const expectedPrefix = value.kind === 'res' ? this.idPrefix : this.peerIdPrefix
    return value.id.startsWith(expectedPrefix)
  }

  /**
   * An incoming line that does not fit is refused without trusting it. The
   * complete validator runs here too: only a well-formed request is answered
   * and only a well-formed response settles — a matching id alone (wrong
   * version, missing op or body) never routes.
   */
  _refuseOversized(raw) {
    let parsed = null
    try {
      parsed = JSON.parse(raw)
    } catch {
      parsed = null
    }
    if (parsed !== null && this._validIncoming(parsed)) {
      if (parsed.kind === 'req') {
        this._respond(parsed.id, parsed.op, { ok: false, error: 'too-large' })
        return
      }
      if (parsed.kind === 'res') {
        this._settle(parsed.id, parsed.op, { ok: false, error: 'too-large' })
        return
      }
    }
    this._report(new Error('bridge frame over maxFrameBytes'))
  }

  _serve(frame) {
    const handler = this._handlers.get(frame.op)
    if (handler === undefined) {
      this._respond(frame.id, frame.op, { ok: false, error: 'unknown-op' })
      return
    }
    // Never awaited: while this handler is awaiting a reverse request, other
    // incoming frames are still dispatched.
    let result
    try {
      result = handler(frame.body)
    } catch (cause) {
      this._respond(frame.id, frame.op, asErrorBody(cause))
      return
    }
    Promise.resolve(result).then(
      (body) => this._respond(frame.id, frame.op, body),
      (cause) => this._respond(frame.id, frame.op, asErrorBody(cause)),
    )
  }

  _emit(op, body) {
    // A snapshot: a handler unsubscribing itself mid-event must not make the
    // next subscriber skip this event.
    for (const handler of [...(this._eventHandlers.get(op) ?? [])]) {
      try {
        const result = handler(body)
        if (result !== null && typeof result === 'object' && typeof result.then === 'function') {
          result.then(undefined, (cause) => this._report(cause))
        }
      } catch (cause) {
        this._report(cause)
      }
    }
  }

  _settle(id, op, body) {
    const entry = this._pending.get(id)
    if (entry === undefined) return
    if (entry.op !== op) {
      this._report(
        new Error(`bridge response ${id} op ${op} does not match pending op ${entry.op}`),
      )
      return
    }
    this._pending.delete(id)
    if (entry.timer !== null) clearTimeout(entry.timer)
    entry.resolve(body)
  }

  _respond(id, op, body) {
    if (this._closed) return
    if (body === undefined) body = null
    // Serialize the actual frame once and validate the encoded form retains
    // the body: a key-sensitive or stateful toJSON can pass a separate probe
    // yet drop the key here, and functions, symbols and throwing values never
    // survive at all. Anything lost becomes the bounded not-serializable.
    let line = null
    try {
      const encoded = JSON.stringify({ v: PROTOCOL_VERSION, id, kind: 'res', op, body })
      if (encoded !== undefined && Object.hasOwn(JSON.parse(encoded), 'body')) line = encoded
    } catch {
      line = null
    }
    if (line === null) {
      line = JSON.stringify({
        v: PROTOCOL_VERSION,
        id,
        kind: 'res',
        op,
        body: { ok: false, error: 'not-serializable' },
      })
    }
    if (Buffer.byteLength(line, 'utf8') > this.maxFrameBytes) {
      const fallback = JSON.stringify({
        v: PROTOCOL_VERSION,
        id,
        kind: 'res',
        op,
        body: { ok: false, error: 'too-large' },
      })
      if (Buffer.byteLength(fallback, 'utf8') > this.maxFrameBytes) {
        this._fail(new Error(`bridge response to ${id} cannot fit maxFrameBytes`))
        return
      }
      this._writeChecked(fallback)
      return
    }
    this._writeChecked(line)
  }

  _write(line) {
    this._writeChecked(line)
  }

  _writeChecked(line) {
    try {
      this.output.write(`${line}\n`)
      return true
    } catch (cause) {
      this._fail(cause)
      return false
    }
  }

  _report(cause) {
    if (this._onError === null || this._onError === undefined) return
    try {
      this._onError(cause)
    } catch (ignored) {
      void ignored
    }
  }

  _eof() {
    if (this._closed) return
    // EOF is the normal shutdown, not a failure: reject what is outstanding
    // without reporting.
    this._closed = true
    this._terminalError = eofError()
    this._rejectPending(this._terminalError)
  }

  /** One place where the transport dies: reject everything, shut it down. */
  _fail(cause) {
    if (this._closed) return
    const error = cause instanceof Error ? cause : new Error(String(cause))
    this._closed = true
    this._terminalError = error
    this._rejectPending(error)
    this._report(error)
    // Real shutdown, not just a local flag: ending the output delivers EOF
    // to the peer, so it stops using a bridge that will never answer again.
    try {
      if (typeof this.output.end === 'function') this.output.end()
    } catch (ignored) {
      void ignored
    }
    if (this._onFatal !== null && this._onFatal !== undefined) {
      try {
        this._onFatal(error)
      } catch (ignored) {
        void ignored
      }
    }
  }

  _rejectPending(error) {
    for (const [id, entry] of this._pending) {
      this._pending.delete(id)
      if (entry.timer !== null) clearTimeout(entry.timer)
      entry.reject(error)
    }
  }
}
