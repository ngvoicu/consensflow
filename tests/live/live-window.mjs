/**
 * A real harness's window in the app's own pane host, opened as the app opens
 * one, for the live tests (`npm run live:paste`, `npm run live:interrupt`):
 * in a folder of its own under the user's home, on its cheap eval model, with
 * its screen and its own record to read.
 */
import { randomUUID } from 'node:crypto'
import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { findSession } from '../../evals/bare.mjs'
import { HARNESSES, liveEnvironment, realOnPath } from '../../evals/plan.mjs'
import { answers } from '../../hosts/lib/completion.js'
import { interactiveStart } from '../../hosts/lib/windows.js'
import { recordState, windowText } from '../../src/adapters/shared.js'
import { prepareClaudeSettings } from '../../src/claude-install.js'
import { consoleText } from '../../src/console-text.js'
import { onWindows, paneArgv } from '../../src/harnesses.js'
import { startIntegration } from '../integration/harness.mjs'
import { trustForClaude } from './trust-claude.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const DAEMON = join(HERE, 'core-live-daemon.mjs')
const H = process.env.HOME ?? homedir()
export const ENV = liveEnvironment({ home: H })
/** Where a harness looks for its records: the environment without the sandbox's removals. */
const RECORD_ENV = Object.fromEntries(Object.entries(ENV).filter(([, value]) => value !== null))

/** How long a window may take to draw its prompt, and a sent message its answer. */
export const READY_MS = 120_000
export const ANSWER_MS = 180_000
/** How long a window holds still before it counts as drawn, or done with its turn. */
const STILL_MS = 3_000

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
export const lastLines = (text) =>
  text
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(-8)
    .join(' ⏎ ')

/** The two harnesses the app pastes into; Codex, Pi and OpenCode take messages through their own queues. */
export function pastedHarnesses(names) {
  for (const name of names) {
    if (!['claude', 'devin'].includes(name)) {
      throw new Error(
        `the app pastes into claude and devin only; ${name} takes its messages through its own queue`,
      )
    }
  }
  return names
}

/** The app's daemon and pane host, as the live tests drive them. */
export const startLiveApp = () => startIntegration({ daemon: DAEMON, fakeEnv: ENV })

/** A live test's own folder, `~/.consensflow-candidate/live/<folder>`. */
export function liveFolder(folder) {
  const workspace = join(H, '.consensflow-candidate', 'live', folder)
  mkdirSync(workspace, { recursive: true })
  return workspace
}

/** The environment an adapter prepares a window with here: the folder is its ConsensFlow home. */
export const windowEnv = (workspace) => ({
  ...RECORD_ENV,
  CONSENSFLOW_HOME: join(workspace, '.consensflow'),
})

/**
 * What a scripted Claude window needs besides its command line: no MCP
 * servers, connectors or browser, and the settings file the app writes for
 * each launch, which skips the full-permission warning (here, its home is
 * the folder's own).
 */
async function claudeExtras(workspace) {
  return [
    ...(await prepareClaudeSettings(windowEnv(workspace), 'live', { boardQuestions: false })),
    '--strict-mcp-config',
    '--no-chrome',
  ]
}

/**
 * Opens `name`'s window (claude or devin) in its own folder under
 * `~/.consensflow-candidate/live/<folder>`, as pane `id`.
 */
export async function openWindow(app, name, { folder, id }) {
  const workspace = liveFolder(folder)
  const { kind, model } = HARNESSES[name]
  // Claude and Pi open on an id they are given; the others name their own.
  const session = kind === 'claude-code' || kind === 'pi' ? randomUUID() : null
  const start = interactiveStart({ kind, model }, session, null)
  const executable = realOnPath(start.command, ENV.PATH)
  const trust = name === 'claude' ? await trustForClaude(app, workspace, executable) : null
  const pane = { id, generation: 1 }
  let native = session
  const openedAt = Date.now()
  const opened = await app.request('pane.open', {
    ...pane,
    cwd: workspace,
    argv: paneArgv(
      [executable, ...(name === 'claude' ? await claudeExtras(workspace) : []), ...start.args],
      ENV,
    ),
    env: start.env,
    dropEnv: start.dropEnv,
    size: { rows: 40, cols: 120 },
  })
  if (opened?.ok !== true) throw new Error(`${name} did not open: ${JSON.stringify(opened)}`)
  const screen = () => app.output(pane.id)
  const closed = () => app.exits.some((exit) => exit.id === pane.id)
  return {
    name,
    pane,
    workspace,
    trust,
    screen,
    closed,
    /** The message as the app gives it to this window: Devin on Windows gets its marks in ASCII. */
    given: (body) =>
      name === 'devin' && onWindows(ENV) ? consoleText(windowText(body)) : windowText(body),
    /** What the harness's own record holds so far. */
    async recorded() {
      native ??= findSession(kind, {
        workspace,
        since: openedAt,
        home: H,
        env: RECORD_ENV,
      })
      if (native === null) return []
      const read = await answers(kind, native, RECORD_ENV).catch(() => null)
      if (read === null || read.unknown) return []
      return recordState(read).items
    },
    /** Until the window has printed and then held still, or `ms` passed; whether it did. */
    async still(ms) {
      const end = Date.now() + ms
      let seen = -1
      let since = Date.now()
      while (Date.now() < end && !closed()) {
        const length = screen().length
        if (length !== seen) [seen, since] = [length, Date.now()]
        else if (length > 0 && Date.now() - since >= STILL_MS) return true
        await sleep(200)
      }
      return false
    },
    kill: () => app.request('pane.kill', pane).catch(() => {}),
  }
}

/**
 * Sends `body` as the daemon does (`pane.write_paste`: a bracketed paste,
 * then Enter once the window has drawn it), and waits for `answer` on the
 * screen or in the record: Windows' console host may draw "5555" in two
 * strokes, which the screen's text then splits. Nothing presses Enter a
 * second time.
 */
export async function send(app, window, body, answer) {
  const from = window.screen().length
  const pasted = Date.now()
  const written = await app.request('pane.write_paste', { ...window.pane, body })
  let shown = false
  while (!shown && Date.now() - pasted < ANSWER_MS && !window.closed()) {
    await sleep(500)
    shown =
      window.screen().slice(from).includes(answer) ||
      (await window.recorded()).some(
        (item) => item.role === 'assistant' && item.text.includes(answer),
      )
  }
  return { written, shown, seconds: ((Date.now() - pasted) / 1000).toFixed(1) }
}
