
$ProgressPreference = 'SilentlyContinue'
$dir = Join-Path $env:USERPROFILE 'consensflow-build'
$owner = Join-Path $dir '.windows-run.id'
$of = @(Get-Content -LiteralPath $owner -ErrorAction SilentlyContinue)
if ($of.Count -eq 0 -or $of[0] -ne '0000abcd-12345678') { Write-Output 'No matching run record; it may still be going.'; exit 1 }
[uint32]$number = 0; [int64]$created = 0
if ($of.Count -lt 5 -or $of[2] -ne 'job' -or $of[4] -ne 'consensflow-wrun-0000abcd-12345678/tree.tgz' -or -not [uint32]::TryParse($of[1], [ref]$number) -or -not [int64]::TryParse($of[3], [ref]$created)) {
  Write-Output 'The run has no complete process identity and job record; it may still be going.'; exit 1
}
$run = try { Get-Process -Id $number -ErrorAction Stop } catch { $null }
if ($null -eq $run) { Write-Output 'The run could not be identified; it may still be going.'; exit 1 }
try { $actual = $run.StartTime.ToFileTimeUtc() } catch { Write-Output 'The process start time could not be read; the run may still be going.'; exit 1 }
if ($actual -ne $created) { Write-Output 'The recorded process is gone; the current PID belongs to another process.'; exit 0 }
try {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CfStop {
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr p, out long created, out long exited, out long kernel, out long user);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr p, uint code);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr p, uint ms);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr p);
  public static string End(uint pid, long expected) {
    IntPtr p = OpenProcess(0x101001, false, pid);
    if (p == IntPtr.Zero) return "failed";
    try {
      long created, exited, kernel, user;
      if (!GetProcessTimes(p, out created, out exited, out kernel, out user)) return "failed";
      if (created != expected) return "gone";
      if (!TerminateProcess(p, 1) && WaitForSingleObject(p, 0) != 0) return "failed";
      return WaitForSingleObject(p, 5000) == 0 ? "ended" : "failed";
    } finally { CloseHandle(p); }
  }
}
'@
  $result = [CfStop]::End($number, $created)
  if ($result -notin @('ended', 'gone')) { throw 'the verified process could not be ended' }
} catch { Write-Output "The run could not be ended safely: $($_.Exception.Message)"; exit 1 }
exit 0
