/**
 * Live, on Windows: Devin takes the app's pane for a terminal it knows. The
 * pane host names its terminal to every program (`TERM_PROGRAM=ConsensFlow`);
 * before it did, Devin took the pane for Windows' console host and opened
 * with "Windows Console Host (conhost) has limited support. Switch to
 * Windows Terminal or Git Bash for the best experience" (2026-10-05). A
 * window opened as the app opens one must not say it; one opened with the
 * name taken away must, or this test could not see it. Nothing is sent, no
 * model is asked.
 *
 *   npm run windows -- --host <ssh host> --build -- npm run live:terminal
 *
 * The exit code is 1 when the app's window warns, or the one with no
 * terminal named does not.
 */
import { lastLines, openWindow, READY_MS, startLiveApp } from './live-window.mjs'

const WARNING = 'Windows Console Host (conhost) has limited support'
const WINDOWS = [
  { name: 'as the app opens it', dropEnv: [], warns: false },
  { name: 'no terminal named', dropEnv: ['TERM_PROGRAM', 'TERM_PROGRAM_VERSION'], warns: true },
]

const app = await startLiveApp()
const results = []
try {
  for (const [index, { name, dropEnv, warns }] of WINDOWS.entries()) {
    const window = await openWindow(app, 'devin', {
      folder: 'terminal',
      id: `terminal-${index}`,
      dropEnv,
    })
    try {
      const drawn = await window.still(READY_MS)
      const screen = window.screen()
      const warned = screen.includes(WARNING)
      results.push({ name, ok: drawn && warned === warns, drawn, warned, last: lastLines(screen) })
    } finally {
      await window.kill()
    }
  }
} finally {
  await app.close()
}
for (const { name, ok, drawn, warned, last } of results) {
  const said = !drawn ? 'did not draw' : warned ? 'warns of conhost' : 'no warning'
  process.stdout.write(`${ok ? 'ok  ' : 'FAIL'} ${name.padEnd(22)} ${said}\n    ${last}\n`)
}
process.exitCode = results.length === WINDOWS.length && results.every((result) => result.ok) ? 0 : 1
