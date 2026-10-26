#!/usr/bin/env node
/**
 * The Windows UI smoke (windows-smoke.mjs) on the desktop of the user this runs as, from
 * a session that has none. Over SSH, as `cargo xtask windows` runs a command, a program
 * has no desktop and the app's window is never shown; so the smoke is run by a scheduled
 * task of the same user, logged on interactively, what it printed is printed here, and
 * this ends as it ended. The user has to be logged in on the machine's screen, which is
 * said when the task never starts. From a desktop it runs the same way.
 *
 *   node app/scripts/windows-smoke-desktop.mjs [--build | --build-app] [<ConsensFlow.exe>]
 *
 * `--build` builds the app from this checkout first (`npm --prefix app run build --
 * --no-bundle`), and the exe that leaves is the one run when none is named. `--build-app`
 * builds the app's crate alone, its staged parts (the page's bundle, `cf`) as the last
 * build left them: Tauri's step before the build (`cargo xtask stage`) is left out, which
 * under the plant runner would link xtask again while it runs, and Windows refuses to
 * replace a program that is running. The smoke's last step crashes the page's renderer,
 * which the app has to load again (app/src-tauri/src/page_crash.rs, T-454); the plant that
 * takes the reload away runs this with `--build-app` (tests/plants/release/packaged.toml,
 * "windows smoke"), after a build:
 *
 *   cargo xtask windows --build -- node app/scripts/windows-smoke-desktop.mjs --build
 *   cargo xtask windows -- cargo xtask plants release "windows smoke, on the app:"
 */
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..')
/** How long the smoke is given on the desktop, and how long its task is given to start. */
const LIMIT_MS = 15 * 60_000
const START_MS = 60_000

const args = process.argv.slice(2)
const builds = args.filter((arg) => arg === '--build' || arg === '--build-app')
const named = args.filter((arg) => !builds.includes(arg))
if (
  process.platform !== 'win32' ||
  builds.length > 1 ||
  named.length > 1 ||
  named.some((arg) => arg.startsWith('-'))
) {
  console.error(
    'usage (Windows only): node app/scripts/windows-smoke-desktop.mjs [--build | --build-app] [<ConsensFlow.exe>]',
  )
  process.exit(2)
}
const exe = resolve(named[0] ?? join(ROOT, 'app', 'src-tauri', 'target', 'release', 'ConsensFlow.exe'))

const folder = mkdtempSync(join(tmpdir(), 'cf-windows-smoke-desktop-'))
if (builds.length > 0) {
  const words = ['--prefix', 'app', 'run', 'build', '--', '--no-bundle']
  if (builds[0] === '--build-app') {
    // Merged into Tauri's config as a JSON merge patch: a null takes the step away.
    const config = join(folder, 'app-alone.json')
    writeFileSync(config, JSON.stringify({ build: { beforeBuildCommand: null } }))
    words.push('--config', config)
  }
  console.log(`== build: npm ${words.join(' ')}`)
  rmSync(exe, { force: true })
  const built = spawnSync('npm', words, { cwd: ROOT, stdio: 'inherit', shell: true })
  if (built.status !== 0) {
    console.error(`== the build failed (${built.status ?? built.signal})`)
    rmSync(folder, { recursive: true, force: true })
    process.exit(1)
  }
}

const log = join(folder, 'smoke.log')
const task = `cf-windows-smoke-${process.pid}`
const smoke = join(ROOT, 'app', 'scripts', 'windows-smoke.mjs')
// The task's command, and what it is told, go to PowerShell in variables: no quoting of
// paths inside a script.
const ran = spawnSync(
  'powershell',
  [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    `
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$command = 'cd /d "' + $env:CF_ROOT + '" && node "' + $env:CF_SMOKE + '" "' + $env:CF_EXE + '" > "' + $env:CF_LOG + '" 2>&1'
$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument "/c $command"
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
Register-ScheduledTask -TaskName $env:CF_TASK -Action $action -Principal $principal -Force | Out-Null
try {
  Start-ScheduledTask -TaskName $env:CF_TASK
  $started = (Get-Date).AddMilliseconds([int]$env:CF_START_MS)
  $deadline = (Get-Date).AddMilliseconds([int]$env:CF_LIMIT_MS)
  while ((Get-ScheduledTaskInfo -TaskName $env:CF_TASK).LastRunTime -lt (Get-Date).AddDays(-1)) {
    if ((Get-Date) -gt $started) {
      Write-Output "== the task never started: is $env:USERNAME logged in on this machine's screen?"
      exit 3
    }
    Start-Sleep -Milliseconds 500
  }
  while ((Get-ScheduledTask -TaskName $env:CF_TASK).State -eq 'Running') {
    if ((Get-Date) -gt $deadline) {
      Stop-ScheduledTask -TaskName $env:CF_TASK
      Write-Output "== the smoke ran past $([int]$env:CF_LIMIT_MS / 60000) minutes and was stopped"
      exit 4
    }
    Start-Sleep -Seconds 2
  }
  exit (Get-ScheduledTaskInfo -TaskName $env:CF_TASK).LastTaskResult
} finally {
  Unregister-ScheduledTask -TaskName $env:CF_TASK -Confirm:$false
}
`,
  ],
  {
    cwd: ROOT,
    encoding: 'utf8',
    env: {
      ...process.env,
      CF_ROOT: ROOT,
      CF_SMOKE: smoke,
      CF_EXE: exe,
      CF_LOG: log,
      CF_TASK: task,
      CF_START_MS: String(START_MS),
      CF_LIMIT_MS: String(LIMIT_MS),
    },
  },
)
process.stdout.write(ran.stdout ?? '')
process.stderr.write(ran.stderr ?? '')
console.log(`== the smoke, on ${process.env.USERNAME}'s desktop: ${exe}`)
try {
  process.stdout.write(readFileSync(log, 'utf8'))
} catch {
  console.log('(it printed nothing)')
}
const status = ran.status ?? 1
console.log(`== the smoke's task ended with ${status}`)
rmSync(folder, { recursive: true, force: true })
// A scheduled task that ran says its program's status; any other is no pass.
process.exit(status === 0 ? 0 : 1)
