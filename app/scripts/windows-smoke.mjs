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
 * Last, the page's renderer is crashed, and the page must come back by itself.
 *
 * It runs beside an installed ConsensFlow: its own home, its own WebView2
 * data folder, and the page reached over WebView2's remote debugging port. It
 * starts only an app of the smoke identity (built-identity.mjs): one of the
 * release's keeps its runtime in the release's own folder of the user, whatever
 * the environment says, and on a machine someone uses that is their ConsensFlow's.
 * It needs a desktop session: over SSH, run it with windows-smoke-desktop.mjs, which
 * runs it from a scheduled task on the user's desktop.
 */
import { execFileSync, spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { chromium } from '@playwright/test'
import { refusalOf } from './built-identity.mjs'

const exe = process.argv[2]
if (process.platform !== 'win32' || !exe) {
  console.error('usage (Windows only): node app/scripts/windows-smoke.mjs <ConsensFlow.exe>')
  process.exit(2)
}
const refused = refusalOf(exe)
if (refused) {
  console.error(`windows smoke: ${refused}`)
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

/** The WebView2 renderer processes under the app of `pid`: its page's, and its frames'. */
function rendererPids(pid) {
  const script = `
$tree = @(${pid})
do {
  $before = $tree.Count
  $tree = @($tree + (Get-CimInstance Win32_Process | Where-Object { $tree -contains $_.ParentProcessId } | ForEach-Object { $_.ProcessId })) | Sort-Object -Unique
} while ($tree.Count -gt $before)
Get-CimInstance Win32_Process | Where-Object { $tree -contains $_.ProcessId -and $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -match '--type=renderer' } | ForEach-Object { $_.ProcessId }
`
  return execFileSync('powershell', ['-NoProfile', '-Command', script], { encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean)
    .map(Number)
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
    // What the app said meanwhile: a refusal shows in the status line for a few seconds.
    const said = new Set()
    const shown = await until(
      `${screen} in its dialog`,
      async () => {
        const status = await main.locator('#status').innerText({ timeout: 1000 }).catch(() => '')
        if (status.trim()) said.add(status.trim())
        const text = await dialog.frameLocator('iframe').locator('h1').innerText({ timeout: 1000 })
        return text.trim().startsWith(screen)
      },
      20_000,
    ).catch(() => false)
    check(shown, `${screen} opens in its dialog and shows its page`)
    if (!shown) console.log(`the app said meanwhile: ${JSON.stringify([...said])}`)
    await main.keyboard.press('Escape')
  }

  const classes = windowClasses(app.pid)
  console.log(`the app's visible windows: ${classes.join(', ') || 'none'}`)
  check(
    !classes.some((name) => /^(ConsoleWindowClass|CASCADIA_HOSTING_WINDOW_CLASS|PseudoConsoleWindow)$/.test(name)),
    'no window of the app is a console',
  )

  // A page whose process failed is loaded again (app/src-tauri/src/page_crash.rs): a page
  // that is gone acknowledges no window's output, and the pane host stops reading a window
  // that is owed 1 MiB of it. The page's renderers are ended from outside, as a crash ends
  // them (a crash asked of the page itself over the debugging port takes Playwright's own
  // session down with it), with Playwright away; the page that comes back is a new one (the
  // mark set on the old is gone) showing the board.
  await main.evaluate(() => {
    window.__beforeTheCrash = true
  })
  // Away while it happens: a page that goes and comes back under an attached session
  // throws inside Playwright, out of reach of any catch.
  await browser.close()
  browser = undefined
  const renderers = rendererPids(app.pid)
  console.log(`the app's WebView2 renderers: ${renderers.join(', ') || 'none'}`)
  for (const pid of renderers) {
    try {
      execFileSync('taskkill', ['/PID', String(pid), '/F'], { stdio: 'ignore' })
    } catch {}
  }
  await sleep(3000)
  // A web view whose page is gone may not answer the debugging port at all: that is the
  // page not loaded again too.
  const loadedAgain = await until(
    'the page loaded again after its process failed',
    async () => {
      browser ??= await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`, { timeout: 5000 })
      for (const page of pages().filter((page) => page.url().startsWith('http://tauri.localhost'))) {
        const fresh = await page.evaluate(
          () => window.__beforeTheCrash === undefined && document.querySelector('#settings-button') !== null,
        )
        if (fresh) return true
      }
      return null
    },
    30_000,
  ).catch(() => false)
  check(loadedAgain, 'a page whose process failed is loaded again and shows the board')
  const log = (() => {
    try {
      return readFileSync(join(root, 'home', 'app', 'app.log'), 'utf8')
    } catch {
      return ''
    }
  })()
  check(
    /the page's process (ended|stopped answering) \(kind \d\): loading it again/.test(log),
    "the app's log says it loaded the page again",
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
