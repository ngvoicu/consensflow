#!/usr/bin/env node
/**
 * The Windows UI smoke: the built app, on a real desktop, does what a human
 * does first. It opens Agents and then Harnesses from Settings, and each
 * dialog shows its screen, the daemon's page framed in it; and no window of
 * the app's own processes is a console. Both once failed on Windows while
 * every test passed: the agents screens came up white and hung, and the
 * daemon opened a console window of its own.
 *
 *   node app/scripts/windows-smoke.mjs <path to ConsensFlow.exe>
 *
 * It runs beside an installed ConsensFlow: its own home, its own WebView2
 * data folder, and the page reached over WebView2's remote debugging port.
 * It needs a desktop session (over SSH, start it from a scheduled task).
 */
import { execFileSync, spawn } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { chromium } from '@playwright/test'

const exe = process.argv[2]
if (process.platform !== 'win32' || !exe) {
  console.error('usage (Windows only): node app/scripts/windows-smoke.mjs <ConsensFlow.exe>')
  process.exit(2)
}
const PORT = 9333
const root = mkdtempSync(join(tmpdir(), 'cf-windows-smoke-'))
const app = spawn(exe, [], {
  env: {
    ...process.env,
    CONSENSFLOW_HOME: join(root, 'home'),
    WEBVIEW2_USER_DATA_FOLDER: join(root, 'webview'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  },
  stdio: 'ignore',
})

const failures = []
const check = (ok, what) => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${what}`)
  if (!ok) failures.push(what)
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
async function until(what, probe, ms = 30_000) {
  const deadline = Date.now() + ms
  while (Date.now() < deadline) {
    const value = await probe().catch(() => null)
    if (value) return value
    await sleep(250)
  }
  throw new Error(`${what} did not happen within ${ms} ms`)
}

/**
 * The class of every visible top-level window whose process is the app or one
 * it started. A console window of the app's own is `ConsoleWindowClass`; where
 * Windows Terminal hosts consoles, the terminal's window belongs to Terminal and
 * the app's tree holds only its `PseudoConsoleWindow`.
 */
function windowClasses(pid) {
  const script = `
Add-Type @'
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class W {
  delegate bool Each(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(Each e, IntPtr p);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static List<string> All() {
    var found = new List<string>();
    EnumWindows((h, p) => {
      if (!IsWindowVisible(h)) return true;
      uint pid; GetWindowThreadProcessId(h, out pid);
      var name = new StringBuilder(256); GetClassName(h, name, 256);
      found.Add(pid + " " + name);
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
'@
$tree = @(${pid})
do {
  $before = $tree.Count
  $tree = @($tree + (Get-CimInstance Win32_Process | Where-Object { $tree -contains $_.ParentProcessId } | ForEach-Object { $_.ProcessId })) | Sort-Object -Unique
} while ($tree.Count -gt $before)
[W]::All() | Where-Object { $tree -contains [int]($_.Split(' ')[0]) } | ForEach-Object { $_.Split(' ', 2)[1] }
`
  return execFileSync('powershell', ['-NoProfile', '-Command', script], { encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean)
}

let browser
try {
  await until('the debugging port', async () => (await fetch(`http://127.0.0.1:${PORT}/json/version`)).ok)
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`)
  const pages = () => browser.contexts().flatMap((context) => context.pages())
  const main = await until('the main page', async () =>
    pages().find((page) => page.url().startsWith('http://tauri.localhost')),
  )
  await main.locator('#settings-button').waitFor({ timeout: 30_000 })
  check(true, 'the main window shows the board')

  for (const screen of ['Agents', 'Harnesses']) {
    await main.locator('#settings-button').click()
    await main.locator('[data-agents-page]', { hasText: new RegExp(`^${screen}$`) }).click()
    const dialog = main.getByRole('dialog', { name: screen })
    const shown = await until(
      `${screen} in its dialog`,
      async () => {
        const text = await dialog.frameLocator('iframe').locator('h1').innerText({ timeout: 1000 })
        return text.trim().startsWith(screen)
      },
      20_000,
    ).catch(() => false)
    check(shown, `${screen} opens in its dialog and shows its page`)
    await main.keyboard.press('Escape')
  }

  const classes = windowClasses(app.pid)
  console.log(`the app's visible windows: ${classes.join(', ') || 'none'}`)
  check(
    !classes.some((name) => /^(ConsoleWindowClass|CASCADIA_HOSTING_WINDOW_CLASS|PseudoConsoleWindow)$/.test(name)),
    'no window of the app is a console',
  )
} catch (cause) {
  check(false, cause.message)
} finally {
  await browser?.close().catch(() => {})
  try {
    execFileSync('taskkill', ['/PID', String(app.pid), '/T', '/F'], { stdio: 'ignore' })
  } catch {}
  await sleep(1000)
  rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 })
}
console.log(failures.length === 0 ? 'windows smoke: pass' : `windows smoke: ${failures.length} failed`)
process.exit(failures.length === 0 ? 0 : 1)
