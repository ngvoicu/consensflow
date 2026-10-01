import { paneArgv } from '../harnesses.js'

/**
 * The Rust pane host, as the new core sees it: open and end windows, hear
 * when one exits, and pass an adapter's harness-specific request (snapshot,
 * paste, peer send, epoch claims) straight through. The bridge protocol is the
 * app's (`app/src-tauri/src/commands.rs`); this class adds no rules of its own,
 * only the Windows shim a window's program may be (`paneArgv`).
 */
export class PaneHost {
  #bridge

  constructor(bridge) {
    this.#bridge = bridge
  }

  /** Opens a window; resolves `{ok, id, generation}` or `{ok: false, error}`. */
  async open(body) {
    return this.#bridge.request(
      'pane.open',
      { ...body, argv: paneArgv(body.argv) },
      { deadlineMs: 60_000 },
    )
  }

  kill(pane) {
    return this.#bridge.request('pane.kill', { id: pane.id, generation: pane.generation })
  }

  onExit(listener) {
    this.#bridge.onEvent('pane.exit', (body) => listener(body))
  }

  request(op, body, options) {
    return this.#bridge.request(op, body, options)
  }
}
