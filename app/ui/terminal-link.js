import { paneKey } from './term.js'

const INPUT_CHUNK_BYTES = 32 * 1024

/**
 * The page's side of the terminals: bytes from the Rust pane host into each
 * pane's emulator (in order, acknowledged only after the emulator committed
 * them), keystrokes and emulator replies back (in chunks, each admitted with a
 * ticket and waited for, so a large paste keeps later typing behind it), and
 * the emulator's size into the pane. Pane ownership stays in Rust.
 */
export class TerminalLink {
  #invoke
  #registry
  #report
  #outputs = new Map()
  #inputs = new Map()
  #sequences = new Map()
  #sizes = new Map()
  #retired = new Set()

  constructor({ invoke, registry, report }) {
    this.#invoke = invoke
    this.#registry = registry
    this.#report = report
  }

  /** One message from `subscribe_output`: bytes for one pane, in order. */
  output(message, emulatorFor) {
    if (message === null || typeof message !== 'object') return
    const key = paneKey(message)
    if (this.#retired.has(key)) return
    const previous = this.#outputs.get(key) ?? Promise.resolve()
    const next = previous
      .then(async () => {
        // Retired while these bytes waited their turn: no emulator comes back for them.
        if (this.#retired.has(key)) return
        const emulator = emulatorFor(message)
        if (emulator === null) return
        await emulator.write(new Uint8Array(message.bytes ?? []))
        await this.#invoke('pane_ack', {
          id: message.id,
          generation: message.generation,
          seq: message.seq,
        })
      })
      .catch((cause) => this.#report(cause))
    this.#outputs.set(key, next)
  }

  /** What the human types into a pane. */
  input(pane, data) {
    return this.#stream('pane_input_enqueue', pane, data).catch((cause) => this.#report(cause))
  }

  /** What the emulator answers the program on its own (cursor reports and the like). */
  reply(pane, data) {
    return this.#stream('pane_reply_enqueue', pane, data)
  }

  /** The emulator's size, sent until the live pane accepts it. */
  resize(pane, cols, rows) {
    const key = paneKey(pane)
    if (this.#retired.has(key)) return
    const size = this.#sizes.get(key) ?? { running: false, applied: null }
    size.desired = `${cols}x${rows}`
    size.request = { id: pane.id, generation: pane.generation, cols, rows }
    this.#sizes.set(key, size)
    void this.#syncSize(key)
  }

  /** A pane that is gone: no more output, input or resizes for it. */
  retire(key) {
    this.#retired.add(key)
    this.#outputs.delete(key)
    this.#inputs.delete(key)
    this.#sequences.delete(key)
    this.#sizes.delete(key)
    this.#registry.retire(key)
  }

  /** Whether a pane is gone for good: it gets no emulator again. */
  retired(key) {
    return this.#retired.has(key)
  }

  async #syncSize(key) {
    const size = this.#sizes.get(key)
    if (size === undefined || size.running || size.applied === size.desired) return
    size.running = true
    const { desired, request } = size
    try {
      const result = await this.#invoke('pane_resize', request)
      if (result?.ok === true) size.applied = desired
    } catch (cause) {
      this.#report(cause)
    } finally {
      size.running = false
      if (this.#sizes.get(key) === size && size.desired !== desired) void this.#syncSize(key)
    }
  }

  async #stream(command, pane, data) {
    const key = paneKey(pane)
    const bytes = new TextEncoder().encode(data)
    const previous = this.#inputs.get(key)
    const submit = async () => {
      for (let offset = 0; offset < bytes.length; offset += INPUT_CHUNK_BYTES) {
        if (this.#retired.has(key)) return
        const chunk = bytes.subarray(offset, offset + INPUT_CHUNK_BYTES)
        if (!(await this.#chunk(command, pane, chunk))) return
      }
    }
    // A keystroke goes at once; a large paste streams and keeps later input behind it.
    if (!previous && bytes.length <= INPUT_CHUNK_BYTES) return submit()
    const stream = (previous ?? Promise.resolve()).then(submit)
    this.#inputs.set(key, stream)
    try {
      await stream
    } finally {
      if (this.#inputs.get(key) === stream) this.#inputs.delete(key)
    }
  }

  async #chunk(command, pane, bytes) {
    const key = paneKey(pane)
    const sequence = (this.#sequences.get(key) ?? 0) + 1
    this.#sequences.set(key, sequence)
    const admitted = await this.#invoke(command, {
      id: pane.id,
      generation: pane.generation,
      sequence,
      bytes: Array.from(bytes),
    })
    // A refused keystroke shows, instead of vanishing.
    if (admitted?.ok !== true || typeof admitted.ticket !== 'string') {
      this.#report(
        new Error(`typing refused: ${admitted?.error ?? 'no answer from the pane host'}`),
      )
      return false
    }
    const settled = await this.#invoke('pane_input_wait', { ticket: admitted.ticket })
    if (settled?.ok !== true) {
      this.#report(
        new Error(`typing was not taken: ${settled?.error ?? 'no answer from the pane host'}`),
      )
      return false
    }
    return true
  }
}
