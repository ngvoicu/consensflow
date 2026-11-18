// A same-named file in TEMP belongs to somebody else, even when a previous
// process happened to have this PID.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { PowerShellStartLock, pwshEnvironment } from './support/powershell-start-lock.mjs'

const windows = process.platform === 'win32'
const shell = windows ? 'powershell.exe' : 'pwsh'
const available = spawnSync(windows ? 'where' : 'which', [shell], { stdio: 'ignore' }).status === 0
const actual = fileURLToPath(new URL('../tools/xtask/windows-load.ps1', import.meta.url))
const quote = (value) => value.replaceAll("'", "''")

test('the Windows load tool leaves a same-named temp file untouched', {
  skip: !available,
}, async () => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-windows-load-'))
  const script = path.join(work, 'case.ps1')
  const target = process.env.CF_TEST_LOAD_SCRIPT || actual
  writeFileSync(
    script,
    `
$ErrorActionPreference = 'Stop'
$decoy = Join-Path $env:TEMP "windows-load-$PID-1.log"
[IO.File]::WriteAllText($decoy, 'keep')
function Get-Command { param($Name) [pscustomobject]@{ Source = 'unused' } }
function cmd {
  param($flag, $command)
  if ($env:CF_TEST_FORCE_FAIL) {
    Write-Output ('x' * 1100000)
    Write-Output 'round one'
    Write-Output 'test result: FAILED'
    $global:LASTEXITCODE = 1
    return
  }
  if ($command -match '>\\s*"([^"]+)"') { [IO.File]::WriteAllText($Matches[1], 'overwritten') }
  else { Write-Output 'test result: ok. 1 passed; 0 failed' }
  $global:LASTEXITCODE = 0
}
. '${quote(target)}' -Runs 1 -Burners 0 -Churners 0 -MaxMinutes 1
if (-not (Test-Path -LiteralPath $decoy)) { throw 'the unrelated file was removed' }
if ([IO.File]::ReadAllText($decoy) -ne 'keep') { throw 'the unrelated file was overwritten' }
`,
  )
  try {
    const run = (failure) =>
      spawnSync(shell, ['-NoProfile', '-NonInteractive', '-File', script], {
        encoding: 'utf8',
        maxBuffer: 2 << 20,
        env: pwshEnvironment({
          ...process.env,
          TEMP: work,
          TMP: work,
          XDG_CACHE_HOME: path.join(work, 'cache'),
          CF_TEST_FORCE_FAIL: failure ? '1' : '',
        }),
      })
    const attempt = async (failure) =>
      windows
        ? run(failure)
        : await new PowerShellStartLock().start(() => ({
            ready: Promise.resolve(),
            value: run(failure),
          }))
    const ran = await attempt(false)
    assert.equal(ran.status, 0, `${ran.stdout}\n${ran.stderr}`)
    const failed = await attempt(true)
    assert.match(failed.stdout, /== 1 of 1 runs failed/)
    assert.match(failed.stdout, /test result: FAILED/)
    assert.match(failed.stdout, /round one/)
    assert.match(failed.stdout, /diagnostic truncated after 1048576 characters/)
    assert.ok(failed.stdout.length < 1_100_000, 'the retained diagnostic is bounded')
    assert.deepEqual(
      readdirSync(work).filter((name) => name.startsWith('cf-windows-load-')),
      [],
      'each owned log was removed when its handle closed',
    )
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})

test('replacing the log directory leaves the replacement untouched', {
  skip: !available,
}, async () => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-windows-load-swap-'))
  const temp = path.join(work, 'temp')
  const moved = path.join(work, 'moved')
  const outside = path.join(work, 'outside')
  const script = path.join(work, 'swap.ps1')
  mkdirSync(temp)
  mkdirSync(outside)
  writeFileSync(
    script,
    `
$ErrorActionPreference = 'Stop'
function Get-Command { param($Name) [pscustomobject]@{ Source = 'unused' } }
function cmd {
  param($flag, $command)
  [IO.Directory]::Move($env:TEMP, $env:CF_TEST_MOVED)
  if ($env:OS -eq 'Windows_NT') {
    & "$env:SystemRoot\\System32\\cmd.exe" /c mklink /J "$env:TEMP" "$env:CF_TEST_OUTSIDE" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'could not create test junction' }
  } else {
    & /bin/ln -s "$env:CF_TEST_OUTSIDE" "$env:TEMP"
    if ($LASTEXITCODE -ne 0) { throw 'could not create test link' }
  }
  $leaf = if ($name) { [IO.Path]::GetFileName($name) } else { 'not-ours.log' }
  $global:replacement = Join-Path $env:CF_TEST_OUTSIDE $leaf
  [IO.File]::WriteAllText($global:replacement, 'keep')
  Write-Output 'test result: ok. 1 passed; 0 failed'
  $global:LASTEXITCODE = 0
}
. '${quote(actual)}' -Runs 1 -Burners 0 -Churners 0 -MaxMinutes 1
if (-not (Test-Path -LiteralPath $global:replacement)) { throw 'the replacement file was removed' }
if ([IO.File]::ReadAllText($global:replacement) -ne 'keep') { throw 'the replacement file was changed' }
if ((Get-ChildItem -LiteralPath $env:CF_TEST_MOVED).Count -ne 0) { throw 'the owned log was not removed' }
`,
  )
  try {
    const run = () =>
      spawnSync(shell, ['-NoProfile', '-NonInteractive', '-File', script], {
        encoding: 'utf8',
        env: pwshEnvironment({
          ...process.env,
          TEMP: temp,
          TMP: temp,
          XDG_CACHE_HOME: path.join(work, 'cache'),
          CF_TEST_MOVED: moved,
          CF_TEST_OUTSIDE: outside,
        }),
      })
    const ran = windows
      ? run()
      : await new PowerShellStartLock().start(() => ({ ready: Promise.resolve(), value: run() }))
    assert.equal(ran.status, 0, `${ran.stdout}\n${ran.stderr}`)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})
