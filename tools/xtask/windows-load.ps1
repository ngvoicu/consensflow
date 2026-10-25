# Runs xtask's tests on a machine that is busy, as CI's Windows runner is while the whole
# workspace's tests go on in parallel, so that a case that fails only when the machine is slow
# fails here. It is for the Windows runner's own tests (`windows::script`), which start
# PowerShell, ping and cmd by the dozen:
#
#   cargo xtask windows --host zeewin -- powershell -NoProfile -ExecutionPolicy Bypass `
#     -File tools\xtask\windows-load.ps1 -Runs 20 -Filter windows::script
#
# The load is a fixed number of processes, every one of them ended by its own clock: `-Burners`
# PowerShells that loop for `-MaxMinutes` and no longer (the processor, nothing more), and
# `-Churners` PowerShells that start a cmd and wait for it, one after the other, for as long
# (process ids given out and taken back, one process at a time each). So at most
# `-Burners + -Churners` processes are the load's, whatever happens to this script: if it is
# killed they end by themselves, and a run of `cargo xtask windows` has them in the run's job
# as well, which the system ends with the run. Then `cargo test` of the filter, `-Runs` times,
# one after the other; each run's own summary line is said, a failed run's failures are said
# whole, and the script ends 1 if any run failed or the time ran out. `-TestArgs` are the test
# program's own words (`--nocapture`, `--test-threads=1`).
param(
  [int]$Runs = 20,
  [int]$Burners = 4,
  [int]$Churners = 2,
  [int]$MaxMinutes = 240,
  [string]$Package = 'xtask',
  [string]$Filter = 'windows::script',
  [string]$TestArgs = ''
)
$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
if ($Burners -gt 12 -or $Churners -gt 12) { Write-Output "== at most 12 of each, so that the load stays a load"; exit 2 }
$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;" + $env:Path
$powershell = (Get-Command powershell.exe).Source
$ends = (Get-Date).AddMinutes($MaxMinutes)
$burn = '$w = [Diagnostics.Stopwatch]::StartNew(); while ($w.Elapsed.TotalMinutes -lt ' + $MaxMinutes + ') { }'
$churn = '$w = [Diagnostics.Stopwatch]::StartNew(); while ($w.Elapsed.TotalMinutes -lt ' + $MaxMinutes + ') { cmd /c exit 0 }'
$started = [Collections.Generic.List[Diagnostics.Process]]::new()
$failed = 0
$ran = 0
try {
  for ($i = 0; $i -lt $Burners; $i++) { $started.Add((Start-Process -FilePath $powershell -ArgumentList @('-NoProfile', '-NonInteractive', '-Command', $burn) -WindowStyle Hidden -PassThru)) }
  for ($i = 0; $i -lt $Churners; $i++) { $started.Add((Start-Process -FilePath $powershell -ArgumentList @('-NoProfile', '-NonInteractive', '-Command', $churn) -WindowStyle Hidden -PassThru)) }
  Write-Output "== load: $Burners burners, $Churners churners, for $MaxMinutes minutes at the most; $Runs runs of cargo test -p $Package $Filter"
  for ($run = 1; $run -le $Runs; $run++) {
    if ((Get-Date) -gt $ends) { Write-Output "== the $MaxMinutes minutes ran out before run $run"; $failed++; break }
    $log = Join-Path $env:TEMP "windows-load-$PID-$run.log"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    cmd /c "cargo test --offline --locked -p $Package --lib $Filter -- $TestArgs > `"$log`" 2>&1"
    $code = $LASTEXITCODE
    $ran++
    $result = (Select-String -LiteralPath $log -Pattern '^test result: ' | Select-Object -Last 1).Line
    Write-Output "== run ${run}: exit $code in $([int]$watch.Elapsed.TotalSeconds) s: $result"
    # What the cases say of themselves with `--nocapture` (their lines begin with `round `).
    Select-String -LiteralPath $log -Pattern '^round ' | ForEach-Object { Write-Output "   $($_.Line)" }
    if ($code -ne 0) {
      $failed++
      $text = Get-Content -LiteralPath $log -Raw
      $from = $text.IndexOf('failures:')
      Write-Output $(if ($from -ge 0) { $text.Substring($from) } else { $text })
    }
    Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue
  }
} finally {
  foreach ($process in $started) { try { $process.Kill() } catch {} }
}
Write-Output "== $failed of $ran runs failed"
if ($failed) { exit 1 }
