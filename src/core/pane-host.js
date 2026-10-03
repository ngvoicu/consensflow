import { paneArgv } from '../harnesses.js'

/**
 * The Rust pane host, as the daemon sees it: open and end windows, hear
 * when one exits, and pass an adapter's harness-specific request (snapshot,
 * paste, claim) straight through. The bridge protocol is the
 * app's (`app/src-tauri/src/pane_handlers.rs`); this class adds no rules of its own,
 * only the Windows shim a window's program may be (`paneArgv`).
 */
export class PaneHost {
  #bridge

  constructor(bridge) {
    this.#bridge = bridge
  }

  /**
   * Opens a window; resolves `{ok, id, generation, pid}` (`pid`, the window's
   * process, when the host knows it) or `{ok: false, error}`.
   */
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
