
$job = $null
try {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CfJob {
  [StructLayout(LayoutKind.Sequential)] struct Basic { public long UserTime, JobUserTime; public uint Flags; public UIntPtr MinWs, MaxWs; public uint Active; public UIntPtr Affinity; public uint Priority, Scheduling; }
  [StructLayout(LayoutKind.Sequential)] struct Io { public ulong A, B, C, D, E, F; }
  [StructLayout(LayoutKind.Sequential)] struct Extended { public Basic Basic; public Io Io; public UIntPtr ProcessMem, JobMem, PeakProcess, PeakJob; }
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int kind, ref Extended info, int size);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  static bool Limit(IntPtr job, uint flags) { var e = new Extended(); e.Basic.Flags = flags; return SetInformationJobObject(job, 9, ref e, Marshal.SizeOf(typeof(Extended))); }
  public static string Why = "";
  public static IntPtr Enter() {
    IntPtr job = CreateJobObject(IntPtr.Zero, null);
    if (job == IntPtr.Zero) { Why = "CreateJobObject failed: " + Marshal.GetLastWin32Error(); return IntPtr.Zero; }
    if (!Limit(job, 0x2000)) { Why = "SetInformationJobObject failed: " + Marshal.GetLastWin32Error(); return IntPtr.Zero; }
    if (!AssignProcessToJobObject(job, GetCurrentProcess())) { Why = "AssignProcessToJobObject failed: " + Marshal.GetLastWin32Error(); return IntPtr.Zero; }
    return job;
  }
  public static void Release(IntPtr job) { Limit(job, 0); }
}
'@
  $made = [CfJob]::Enter()
  if ($made -eq [IntPtr]::Zero) { throw [CfJob]::Why }
  $job = $made
} catch { Write-Output "== no job: $($_.Exception.Message), so a stop will not end at once what this run starts" }
if ($null -ne $job) {
  try { Add-Content -LiteralPath $owner -Value 'job' } catch { Write-Output "== no job record: $($_.Exception.Message), so a stop will walk this run's tree" }
}
$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;" + $env:Path
$codex = (Get-Item "$env:USERPROFILE\.codex\packages\standalone\current" -ErrorAction SilentlyContinue).Target
if ($codex) { $env:Path = "$(@($codex)[0])\bin;" + $env:Path }
Set-Location $dir
$ErrorActionPreference = 'Continue'
$env:CONSENSFLOW_TEST_MACHINE = 'zeewin'
Write-Output "== test machine: $env:CONSENSFLOW_TEST_MACHINE"
$env:CARGO_BUILD_JOBS = '4'
$env:RUST_TEST_THREADS = '4'
Write-Output "== limits: CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 (what this machine takes at once)"
$steps = @()
# packages: begin
$packagesJs = @'
const fs = require('fs')
const root = process.argv[2]
const read = (file) => {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8')).packages || {}
  } catch (error) {
    if (error.code === 'ENOENT') return null
    throw error
  }
}
const another = (list, mine) => {
  if (!Array.isArray(list) || list.length === 0) return false
  if (list.includes('!' + mine)) return true
  const allowed = list.filter((word) => !word.startsWith('!'))
  return allowed.length > 0 && !allowed.includes(mine)
}
const list = (words) => words.slice(0, 3).join(', ') + (words.length > 3 ? ' and ' + (words.length - 3) + ' more' : '')
const name = (key) => key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length)
try {
  const shipped = read(root + '/package-lock.json')
  if (shipped === null) process.exit(0)
  const installed = read(root + '/node_modules/.package-lock.json')
  if (installed === null) {
    console.log('node_modules holds no .package-lock.json: no npm ci has been run there')
    process.exit(1)
  }
  const missing = []
  const changed = []
  const extra = []
  for (const [key, entry] of Object.entries(shipped)) {
    if (!key.startsWith('node_modules/')) continue
    const have = installed[key]
    if (!have) {
      const left = entry.optional === true || another(entry.os, process.platform) || another(entry.cpu, process.arch)
      if (!left) missing.push(name(key) + ' ' + entry.version)
    } else if (have.version !== entry.version || (have.integrity && entry.integrity && have.integrity !== entry.integrity)) {
      changed.push(name(key) + ' ' + have.version + ' where the lockfile says ' + entry.version)
    }
  }
  for (const key of Object.keys(installed)) {
    if (key.startsWith('node_modules/') && !(key in shipped)) extra.push(name(key))
  }
  const said = []
  if (missing.length) said.push('missing: ' + list(missing))
  if (changed.length) said.push('changed: ' + list(changed))
  if (extra.length) said.push('extra: ' + list(extra))
  if (said.length) {
    console.log(said.join('; '))
    process.exit(1)
  }
} catch (error) {
  console.log('could not be compared: ' + error.message)
  process.exit(1)
}
'@
foreach ($package in @(@('.', 'node_modules', 'npm ci --no-audit --no-fund'), @('app', 'app/node_modules', 'npm ci --prefix app --no-audit --no-fund'))) {
  $differs = (@($packagesJs | node - $package[0] 2>&1 | ForEach-Object { "$_" }) -join ' ').Trim()
  if ($LASTEXITCODE -ne 0) {
    if (-not $differs) { $differs = 'node could not compare them' }
    Write-Output "== packages: $($package[1]) is not what the lockfile says ($differs), so its npm ci runs first"
    $steps += $package[2]
  }
}
# packages: end
foreach ($step in $steps) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
}
foreach ($step in @('cargo fetch --locked --offline >nul 2>&1 || cargo fetch --locked', 'cargo xtask conpty --into app/src-tauri/target/debug', 'cargo xtask conpty --into app/src-tauri/target/release')) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) {
    Write-Output "== the run needs what '$step' makes there, and it could not be made (status $LASTEXITCODE): the command is not run on what happens to be on the machine"
    Exit-Run $LASTEXITCODE
  }
}
Set-Location (Join-Path $dir '.')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off␍
" + 'node -e "console.log(\"it''s 100%%\")"' + "␍
")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
