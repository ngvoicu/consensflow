# Runs cf-e2e's tests (-Test: one test binary, or all of the crate's; -Filter: the names
# they hold) on this machine beside a CPU load that is bounded three ways: -Burners busy
# processes (never more), each ending by itself after -Minutes, and all stopped when the run
# ends however it ends. For a flake of the Windows runs that "passed alone" (T-373: folder
# trust's late-keys case, once in a 31-minute run).
# From the Mac, through the runner:
#   windows-from-tree.sh <tree> powershell -NoProfile -ExecutionPolicy Bypass -File tools\windows-load.ps1 -Test regressions -Filter a_trust_question -Repeat 3
param(
  [string]$Test = '',
  [string]$Filter = '',
  [int]$Burners = 12,
  [int]$Minutes = 45,
  [int]$Repeat = 1,
  [int]$Copies = 1
)
$burn = '$end = (Get-Date).AddMinutes(' + $Minutes + '); $x = 1; while ((Get-Date) -lt $end) { $x = ($x * 7 + 3) % 1000003 }'
$busy = @()
if ($Burners -gt 0) {
  $busy = @(1..$Burners | ForEach-Object {
    Start-Process -FilePath powershell -ArgumentList '-NoProfile', '-Command', $burn -WindowStyle Hidden -PassThru
  })
}
$code = 0
try {
  if ($Copies -gt 1) {
    # -Copies: once through cargo (which builds the programs the rig uses), then that many
    # copies of the test binary at once, -Repeat rounds, each copy's output kept apart.
    cargo test --no-fail-fast -p cf-e2e --test $Test -- $Filter
    if ($LASTEXITCODE -ne 0) { $code = $LASTEXITCODE }
    $exe = (Get-ChildItem 'app\src-tauri\target\debug\deps' -Filter "$Test-*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
    $env:CF_E2E_BUILT = (Resolve-Path 'app\src-tauri\target\release').Path
    for ($run = 1; $run -le $Repeat; $run++) {
      Write-Output "== copies round $run of $Repeat, $Copies copies, $Burners burners for at most $Minutes minutes"
      $kept = Join-Path $env:TEMP "windows-load-$PID-$run"
      New-Item -ItemType Directory -Force $kept | Out-Null
      $running = @(1..$Copies | ForEach-Object {
        $words = @(); if ($Filter) { $words += $Filter }
        $copy = Start-Process -FilePath $exe -ArgumentList ($words + '--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $kept "$_.out") -RedirectStandardError (Join-Path $kept "$_.err")
        $null = $copy.Handle
        $copy
      })
      $running | Wait-Process
      $number = 0
      foreach ($copy in $running) {
        $number++
        $err = Get-Content (Join-Path $kept "$number.err") -ErrorAction SilentlyContinue
        Write-Output "copy ${number}: exit $($copy.ExitCode)"
        if ($copy.ExitCode -ne 0) {
          $code = 1
          Write-Output "--- copy $number failed"
          $err | Select-Object -Last 60
          Get-Content (Join-Path $kept "$number.out") -ErrorAction SilentlyContinue | Select-Object -Last 40
          $left = $err | Where-Object { $_ -match 'is kept in (.+)$' } | ForEach-Object { $Matches[1] }
          foreach ($file in $left) { Write-Output "--- $file"; Get-Content $file -ErrorAction SilentlyContinue }
        }
      }
      Remove-Item -Recurse -Force $kept -ErrorAction SilentlyContinue
    }
  }
  else {
    for ($run = 1; $run -le $Repeat; $run++) {
      Write-Output "== load run $run of $Repeat, $Burners burners for at most $Minutes minutes"
      $words = @('test', '--no-fail-fast', '-p', 'cf-e2e')
      if ($Test) { $words += @('--test', $Test) }
      if ($Filter) { $words += @('--', $Filter) }
      cargo @words
      if ($LASTEXITCODE -ne 0) { $code = $LASTEXITCODE }
    }
  }
}
finally {
  $busy | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
}
exit $code
