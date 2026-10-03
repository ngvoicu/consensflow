import { randomUUID } from 'node:crypto'

/** Claude's question about an unknown folder, and the prompt it shows once past it. */
const ASKS = /trust (the files|this folder)/i
const READY = /bypass permissions|\? for shortcuts|shortcuts/i

/**
 * Trusts one folder for Claude Code, once, the way a person would. Claude
 * asks whether to trust a folder it has not seen before an interactive
 * session starts, and nobody can answer that in an unattended window: this
 * opens Claude there in a pane of the app's own pane host (`app`, from
 * startIntegration), says yes with Enter, waits for its prompt, and closes
 * the pane, on every platform the pane host runs on.
 */
export async function trustForClaude(app, folder, claude, { env = {}, timeoutMs = 60_000 } = {}) {
  const pane = { id: `trust-${randomUUID()}`, generation: 1 }
  const opened = await app.request('pane.open', {
    ...pane,
    cwd: folder,
    argv: [claude],
    env,
    size: { rows: 40, cols: 120 },
  })
  if (opened?.ok === false)
    throw new Error(`Claude did not open in ${folder}: ${JSON.stringify(opened)}`)
  let answered = false
  // What the pane printed before the answer is not read for its prompt again.
  let from = 0
  try {
    const end = Date.now() + timeoutMs
    while (Date.now() < end) {
      await new Promise((resolve) => setTimeout(resolve, 250))
      const screen = app.output(pane.id)
      if (!answered && ASKS.test(screen.slice(from))) {
        await app.request('pane.input', { ...pane, bytes: [13] })
        answered = true
        from = screen.length
      } else if (READY.test(screen.slice(from))) {
        return answered ? 'trusted' : 'already trusted'
      }
    }
    // What the window showed instead, so a run that stops here explains itself.
    const shown = app
      .output(pane.id)
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean)
      .slice(-6)
      .join(' ⏎ ')
    return `${answered ? 'trusted, its prompt not seen' : 'no prompt seen'}; its screen: ${shown}`
  } finally {
    await app.request('pane.kill', pane).catch(() => {})
  }
}
