/**
 * Live, on Windows: the app's terminal as Devin sees it. The pane host names
 * its terminal to every program (`TERM_PROGRAM=ConsensFlow`); before it did,
 * Devin took the pane for Windows' console host and opened with "Windows
 * Console Host (conhost) has limited support. Switch to Windows Terminal or
 * Git Bash for the best experience" (2026-10-05). A window opened as the app
 * opens one must not say it; one opened with the name taken away must, or
 * this test could not see it. And the console host under each window is
 * Microsoft's OpenConsole.exe the app ships beside its pane host
 * (`cargo xtask conpty`), not the system's conhost. Nothing is sent, no
 * model is asked.
 *
 *   npm run windows -- --host <ssh host> --build -- npm run live:terminal
 *
 * The exit code is 1 when the app's window warns, or the one with no
 * terminal named does not.
 */
import { execFileSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { lastLines, openWindow, READY_MS, startLiveApp } from './live-window.mjs'

const WARNING = 'Windows Console Host (conhost) has limited support'
/** Where the pane host runs from, the console host it loads beside it. */
const PANE_HOST = resolve(
  process.env.CONSENSFLOW_TEST_BRIDGE ??
    resolve(
      import.meta.dirname,
      '..',
      '..',
      'app',
      'src-tauri',
      'target',
      'release',
      'consensflow-bridge.exe',
    ),
)

/** The OpenConsole.exe processes running now, by where each runs from. */
function consoleHosts() {
  const listed = execFileSync(
    'powershell',
    [
      '-NoProfile',
      '-Command',
      'Get-CimInstance Win32_Process -Filter "Name=\'OpenConsole.exe\'" | ForEach-Object { $_.ExecutablePath }',
    ],
    { encoding: 'utf8' },
  )
  return listed.split(/\r?\n/).filter((line) => line.trim() !== '')
}
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
      // Its console host, while it is open: the one beside the pane host.
      const ours = consoleHosts().some(
        (host) => resolve(dirname(host)).toLowerCase() === dirname(PANE_HOST).toLowerCase(),
      )
      results.push({
        name,
        ok: drawn && warned === warns && ours,
        drawn,
        warned,
        ours,
        last: lastLines(screen),
      })
    } finally {
      await window.kill()
    }
  }
} finally {
  await app.close()
}
for (const { name, ok, drawn, warned, ours, last } of results) {
  const said = !drawn ? 'did not draw' : warned ? 'warns of conhost' : 'no warning'
  const host = ours
    ? 'the shipped OpenConsole'
    : "not the shipped OpenConsole (Windows' own conhost?)"
  process.stdout.write(`${ok ? 'ok  ' : 'FAIL'} ${name.padEnd(22)} ${said}; ${host}\n    ${last}\n`)
}
process.exitCode = results.length === WINDOWS.length && results.every((result) => result.ok) ? 0 : 1
