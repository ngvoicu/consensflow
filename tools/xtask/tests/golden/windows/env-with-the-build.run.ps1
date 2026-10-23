
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
$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;" + $env:Path
$codex = (Get-Item "$env:USERPROFILE\.codex\packages\standalone\current" -ErrorAction SilentlyContinue).Target
if ($codex) { $env:Path = "$(@($codex)[0])\bin;" + $env:Path }
Set-Location $dir
$ErrorActionPreference = 'Continue'
foreach ($step in @('npm ci --no-audit --no-fund', 'npm ci --prefix app --no-audit --no-fund', 'npm --prefix app run bundle:ui', 'cargo xtask stage', 'npm run build:bridge', 'cargo xtask conpty --into app/src-tauri/target/release')) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
}
$env:CONSENSFLOW_TEST_DAEMON = 'native'
Write-Output "== env CONSENSFLOW_TEST_DAEMON=$env:CONSENSFLOW_TEST_DAEMON"
$env:CONSENSFLOW_NOTE = 'it''s 100% $x `y` "q" & |'
Write-Output "== env CONSENSFLOW_NOTE=$env:CONSENSFLOW_NOTE"
Set-Location (Join-Path $dir '.')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off␍
" + 'cargo xtask eval --scenario round-trip --chief devin' + "␍
")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
