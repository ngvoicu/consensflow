
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
Set-Location (Join-Path $dir '.')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off␍
" + 'npm run eval -- --scenario round-trip --chief devin' + "␍
")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
