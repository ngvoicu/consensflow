import { randomUUID } from 'node:crypto'

/** Claude's question about an unknown folder, and the answer its ❯ marks. */
const ASKS = /trust this folder/i
const CHOSEN = /❯\s*(No, exit|Yes, I trust this folder)/g
const YES = 'Yes, I trust this folder'
const DOWN = [27, 91, 66]
const ENTER = [13]
/** How long a window holds still before it counts as drawn. */
const STILL_MS = 3_000

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/**
 * Trusts one folder for Claude Code, once, the way a person would. Claude
 * asks whether to trust a folder it has not seen before an interactive
 * session starts, and nobody can answer that in an unattended window: this
 * opens Claude there in a pane of the app's own pane host (`app`, from
 * startIntegration), moves to "Yes, I trust this folder", confirms it with
 * Enter once the ❯ is on it, lets Claude write that down, and closes the
 * pane, on every platform the pane host runs on. Since Claude Code 2.1.287
 * the question opens on "No, exit", where an Enter alone would quit.
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
  const screen = () => app.output(pane.id)
  // What the window showed, so a run that stops here explains itself.
  const shown = () =>
    screen()
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean)
      .slice(-6)
      .join(' ⏎ ')
  /** Until the window has printed and then held still; whether it did before `end`. */
  const still = async (end) => {
    let printed = -1
    let since = Date.now()
    while (Date.now() < end) {
      const length = screen().length
      if (length !== printed) [printed, since] = [length, Date.now()]
      else if (length > 0 && Date.now() - since >= STILL_MS) return true
      await sleep(250)
    }
    return false
  }
  try {
    const end = Date.now() + timeoutMs
    if (!(await still(end))) return `no screen seen; its screen: ${shown()}`
    if (!ASKS.test(screen())) return 'already trusted'
    for (let presses = 0; [...screen().matchAll(CHOSEN)].at(-1)?.[1] !== YES; presses += 1) {
      if (presses === 3) return `could not choose "${YES}"; its screen: ${shown()}`
      await app.request('pane.input', { ...pane, bytes: DOWN })
      await sleep(500)
    }
    await app.request('pane.input', { ...pane, bytes: ENTER })
    // Claude writes the trust down as it moves on to its prompt.
    await still(end)
    return 'trusted'
  } finally {
    await app.request('pane.kill', pane).catch(() => {})
  }
}
