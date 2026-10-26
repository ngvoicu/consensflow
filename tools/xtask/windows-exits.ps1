# With what exit code does a program end that is ended from outside? The Windows runner's tests
# read a program's exit code to tell one that ended by itself (0) from one that something
# ended (T-545). This starts three programs (`ping -n 30`, at most 30 seconds each, and each
# ended by this script before that), ends one with `taskkill /F`, one with `taskkill /T /F`
# and one with `Stop-Process -Force`, and says each one's exit code.
#
#   cargo xtask windows --host zeewin -- powershell -NoProfile -ExecutionPolicy Bypass `
#     -File tools\xtask\windows-exits.ps1
$ErrorActionPreference = 'Continue'
$ping = (Get-Command ping.exe).Source
$ways = [ordered]@{
  'taskkill /F'        = { param($id) taskkill /PID $id /F 2>&1 | Out-Null }
  'taskkill /T /F'     = { param($id) taskkill /PID $id /T /F 2>&1 | Out-Null }
  'Stop-Process -Force' = { param($id) Stop-Process -Id $id -Force }
}
$started = @()
try {
  foreach ($way in $ways.Keys) {
    $info = [Diagnostics.ProcessStartInfo]::new($ping, '-n 30 127.0.0.78')
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $process = [Diagnostics.Process]::Start($info)
    $started += $process
    Start-Sleep -Milliseconds 500
    & $ways[$way] $process.Id
    $null = $process.WaitForExit(10000)
    '== {0}: exit code {1} (0x{2:x8})' -f $way, $process.ExitCode, [BitConverter]::ToUInt32([BitConverter]::GetBytes([int]$process.ExitCode), 0)
  }
} finally {
  foreach ($process in $started) { try { if (-not $process.HasExited) { $process.Kill() } } catch {} }
}
