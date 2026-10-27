# What a run's PowerShell does after its exit line when its command leaves programs running (T-701 item 4,
# from T-602: a run printed `== exit 101`, its Mac side ended at 03:04:01, and the next run at 03:04:32 was
# refused: "A run is still going on this machine (process 54196)"). It says its own run's process, leaves a
# program running the way a window closed early does, and ends with 101, as that case did:
#
#   cargo xtask windows --machine zeewin2 -- powershell -NoProfile -File tools\xtask\windows-linger.ps1 -How cmd-start
#
# -How is how the program is left: cmd-start (`start /b` from cmd, which inherits the run's output and every
# handle it can), start-process (Start-Process, shell-started and inheriting none), inherit (a process started
# with the run's standard handles inherited), nested-job (what T-602's wrapper did: this PowerShell makes a job of
# its own, inside the run's, which ends what is in it when this PowerShell ends and is never released, with a timer
# thread that ends the job at the limit, and a program is left in it), or none (leaves nothing).
# tools/xtask/windows-linger.sh reads what the machine looks like afterwards.
param([string]$How = 'cmd-start', [int]$Seconds = 150)
$owner = @(Get-Content (Join-Path $PWD '.windows-run.id'))
Write-Output "== linger: the run's PowerShell is process $($owner[1]); it leaves $How for $Seconds s, at $(Get-Date -Format 'HH:mm:ss.fff')"
if ($How -eq 'nested-job') {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LingerJob {
  [StructLayout(LayoutKind.Sequential)] struct Basic { public long UserTime, JobUserTime; public uint Flags; public UIntPtr MinWs, MaxWs; public uint Active; public UIntPtr Affinity; public uint Priority, Scheduling; }
  [StructLayout(LayoutKind.Sequential)] struct Io { public ulong A, B, C, D, E, F; }
  [StructLayout(LayoutKind.Sequential)] struct Extended { public Basic Basic; public Io Io; public UIntPtr ProcessMem, JobMem, PeakProcess, PeakJob; }
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int kind, ref Extended info, int size);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] static extern bool TerminateJobObject(IntPtr job, uint code);
  static System.Threading.Timer timer;
  public static IntPtr Enter(int seconds) {
    var job = CreateJobObject(IntPtr.Zero, null);
    var info = new Extended();
    info.Basic.Flags = 0x2000;
    if (job == IntPtr.Zero || !SetInformationJobObject(job, 9, ref info, Marshal.SizeOf(typeof(Extended))) || !AssignProcessToJobObject(job, GetCurrentProcess())) {
      throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    }
    timer = new System.Threading.Timer(delegate(object state) { TerminateJobObject(job, 124); }, null, seconds * 1000, System.Threading.Timeout.Infinite);
    return job;
  }
}
'@
  $nested = [LingerJob]::Enter(1560)
  Write-Output '== linger: nested job entered (26-minute limit, ended with this PowerShell)'
}
switch ($How) {
  'cmd-start' { cmd /c "start /b ping -n $Seconds 127.0.0.1 >nul" }
  'start-process' { Start-Process -FilePath ping -ArgumentList '-n', $Seconds, '127.0.0.1' -WindowStyle Hidden }
  'inherit' {
    $psi = New-Object Diagnostics.ProcessStartInfo 'ping.exe', "-n $Seconds 127.0.0.1"
    $psi.UseShellExecute = $false
    $null = [Diagnostics.Process]::Start($psi)
  }
  'nested-job' { cmd /c "start /b ping -n $Seconds 127.0.0.1 >nul" }
  'none' { }
}
exit 101
