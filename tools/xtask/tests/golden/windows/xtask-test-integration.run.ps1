
$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;" + $env:Path
$codex = (Get-Item "$env:USERPROFILE\.codex\packages\standalone\current" -ErrorAction SilentlyContinue).Target
if ($codex) { $env:Path = "$(@($codex)[0])\bin;" + $env:Path }
Set-Location $dir
$ErrorActionPreference = 'Continue'
foreach ($step in @()) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
}
Set-Location (Join-Path $dir '.')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off␍
" + 'cargo xtask test integration' + "␍
")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
