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
 * Then a project is opened, and a meeting with its chief is started and stopped from the
 * button on the chief's card, whose head stays inside the card at every width. The app runs
 * with a home of its own and no folder of the user's on its PATH: no harness of the user's
 * is found, so the chief never starts on the user's login, and its card, which has no window
 * then, still has its head.
 *
 * The update check is made as the page makes it at the app's start, and answers up to date or an
 * update, with nothing in the app's error log and no warning of the app's own on the desktop: on
 * Windows the app reads the release feed itself, and for want of a TLS provider the build of its
 * client panicked at every start (T-1247). The feed is GitHub's, so this needs the machine online.
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
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
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
// The user's own folders, which hold their harnesses and their logins, kept off the app's PATH.
const profile = join(root, 'profile')
mkdirSync(join(profile, 'AppData', 'Roaming'), { recursive: true })
const own = [process.env.USERPROFILE, process.env.APPDATA, process.env.LOCALAPPDATA]
  .filter(Boolean)
  .map((folder) => folder.toLowerCase())
const pathKey = Object.keys(process.env).find((key) => key.toUpperCase() === 'PATH') ?? 'Path'
const path = (process.env[pathKey] ?? '')
  .split(';')
  .filter((folder) => !own.some((mine) => folder.toLowerCase().startsWith(mine)))
  .join(';')
const app = spawn(exe, [], {
  env: {
    ...process.env,
    [pathKey]: path,
    // The daemon looks for a harness under HOME before USERPROFILE, which the web view
    // keeps; npm's shims are under APPDATA.
    HOME: profile,
    APPDATA: join(profile, 'AppData', 'Roaming'),
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
/** What the app has written in its log (its errors and its panics), whole. */
const appLog = () => {
  try {
    return readFileSync(join(root, 'home', 'app', 'app.log'), 'utf8')
  } catch {
    return ''
  }
}
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

  // The update check: the command the page runs at the start (updates.js), asked again while one is
  // going. The app has no archive to install here and reads the feed itself (update_page.rs).
  const answered = await until(
    'the update check answered',
    async () => {
      const status = await main.evaluate(() => window.__TAURI__.core.invoke('update_check'))
      return status?.phase === 'checking' ? null : status
    },
    60_000,
  ).catch(() => null)
  console.log(
    `the update check: ${answered?.phase ?? 'no answer'}, last checked ${answered?.lastChecked ?? 'never'}, ` +
      `available ${answered?.available?.version ?? 'none'}, error ${answered?.error ?? 'none'}`,
  )
  check(
    (answered?.phase === 'idle' || answered?.phase === 'available') && !answered?.error,
    'the update check answers up to date or an update',
  )
  const logged = appLog()
  console.log(`the app's log after the update check: ${logged.length} bytes`)
  check(!/panicked/.test(logged), "the app's error log holds no panic after the update check")
  // "ConsensFlow kept running" is the app's warning that it stopped an error of its own
  // (panics.rs): a dialog of the system's class, put up a moment after the error. Nothing
  // this smoke does warrants one.
  await sleep(2000)
  check(
    !windowClasses(app.pid).includes('#32770'),
    'no dialog of the app tells the person it kept running after the update check',
  )

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

  // A meeting with the chief, from the button on its card, as a human starts and stops one.
  const directory = join(root, 'meeting')
  mkdirSync(directory)
  const daemon = (operation, body) =>
    main.evaluate(
      ([operation, body]) => window.__TAURI__.core.invoke('daemon_request', { operation, body }),
      [operation, body],
    )
  const opened = await daemon('project.open', { directory, agent: 'terpsichore', staff: [] })
  check(opened?.ok === true, `a project opens (${opened?.error ?? 'ok'})`)
  const project = opened?.project?.id
  await main.locator('.project-select', { hasText: 'meeting' }).click()
  const card = main.locator('.terminal-card[data-role="chief"]')
  const meeting = async () => (await daemon('board.get', { project })).board?.project?.meeting ?? null
  // Clicked once it is there, and asked again only while no meeting has started.
  const started = await until('the chief card offers Start meeting', async () => {
    const on = await meeting()
    if (on !== null) return on
    await card.getByRole('button', { name: 'Start meeting' }).click({ timeout: 1000 })
    return meeting()
  }).catch(() => null)
  check(typeof started === 'string', "Start meeting on the chief's card starts a meeting")
  const stop = card.getByRole('button', { name: 'Stop meeting (0)' })
  check(
    await stop.waitFor({ timeout: 10_000 }).then(
      () => true,
      () => false,
    ),
    'in a meeting the button reads Stop meeting (0)',
  )
  // The head (the name, History, Switch chief, the meeting's button, what the chief is
  // doing) inside the card, wrapped where it is narrow.
  for (const width of [700, 880, 1440]) {
    await main.setViewportSize({ width, height: 900 })
    const outside = await until(
      `the chief's head at ${width}px`,
      async () => {
        const parts = await card.evaluate((card) => {
          const edge = card.getBoundingClientRect()
          return [...card.querySelectorAll('.terminal-head > *, .terminal-head button')]
            .filter((part) => {
              const box = part.getBoundingClientRect()
              return box.left < edge.left || box.right > edge.right
            })
            .map((part) => part.textContent)
        })
        return parts.length === 0 ? [] : null
      },
      5000,
    ).catch(() => null)
    check(outside !== null, `the chief's head is inside its card at ${width}px`)
  }
  await stop.click().catch(() => {})
  const stopped = await until('the meeting stopped', async () => (await meeting()) === null).catch(
    () => false,
  )
  check(stopped === true, 'Stop meeting stops it')
  check(
    await card
      .getByRole('button', { name: 'Start meeting' })
      .waitFor({ timeout: 10_000 })
      .then(
        () => true,
        () => false,
      ),
    'the button reads Start meeting again',
  )

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
  check(
    /the page's process (ended|stopped answering) \(kind \d\): loading it again/.test(appLog()),
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
