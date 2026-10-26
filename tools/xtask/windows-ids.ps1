# How soon does this machine give a process id again? The Windows runner's stop used to end the
# programs of a run by the process ids a walk of its tree had read, and an id that has been
# given to another program since ends that one (T-545). This starts `-Starts` programs one
# after another (each `cmd /c exit 0`, waited for, so one process at a time and no more), and says
# how many were given an id that one of the `-Window` before them had had, and the longest
# and shortest run of starts between a program's id and its being given again.
#
#   cargo xtask windows --host zeewin -- powershell -NoProfile -ExecutionPolicy Bypass `
#     -File tools\xtask\windows-ids.ps1
param(
  [int]$Starts = 60,
  [int]$Window = 10,
  [int]$MaxSeconds = 120
)
$ErrorActionPreference = 'Continue'
if ($Starts -gt 200) { Write-Output "== at most 200 starts"; exit 2 }
$cmd = (Get-Command cmd.exe).Source
$ids = [Collections.Generic.List[int]]::new()
$gaps = [Collections.Generic.List[int]]::new()
$watch = [Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt $Starts -and $watch.Elapsed.TotalSeconds -lt $MaxSeconds; $i++) {
  $info = [Diagnostics.ProcessStartInfo]::new($cmd, '/c exit 0')
  $info.UseShellExecute = $false
  $info.CreateNoWindow = $true
  $process = [Diagnostics.Process]::Start($info)
  $id = $process.Id
  $null = $process.WaitForExit(10000)
  $back = $ids.LastIndexOf($id)
  if ($back -ge 0) { $gaps.Add($i - $back) }
  $ids.Add($id)
  $process.Dispose()
}
$near = @($gaps | Where-Object { $_ -le $Window })
Write-Output ("== {0} starts in {1:N1} s: {2} were given an id an earlier start had had, {3} of them within {4} starts; ids seen: {5} different" -f $ids.Count, $watch.Elapsed.TotalSeconds, $gaps.Count, $near.Count, $Window, ($ids | Sort-Object -Unique).Count)
if ($gaps.Count) { Write-Output ("== starts between a program's id and its being given again: min {0}, max {1}" -f ($gaps | Measure-Object -Minimum).Minimum, ($gaps | Measure-Object -Maximum).Maximum) }
Write-Output ("== first ids: {0}" -f (($ids | Select-Object -First 20) -join ' '))
