
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
# What the command prints is UTF-8, and goes back as it came.
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
function Exit-Run([int]$code) { Write-Output "== exit $code"; exit $code }
$dir = Join-Path $env:USERPROFILE 'consensflow-build'
New-Item -ItemType Directory -Force $dir | Out-Null
$running = Join-Path $dir '.windows-run.pid'
if (Test-Path $running) {
  $other = Get-Process -Id (Get-Content $running) -ErrorAction SilentlyContinue
  if ($other) { Write-Output "A run is still going on this machine (process $($other.Id)): one at a time."; Exit-Run 75 }
}
Set-Content -Path $running -Value $PID
try {
$synced = Join-Path $dir '.synced-files'
$before = if (Test-Path $synced) { Get-Content $synced } else { @() }
tar.exe -xzf (Join-Path $env:USERPROFILE 'consensflow-build.tgz') -C $dir
if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
$now = [Collections.Generic.HashSet[string]]::new([string[]](Get-Content $synced))
foreach ($file in $before) {
  if (-not $now.Contains($file)) { Remove-Item -LiteralPath (Join-Path $dir $file) -Force -ErrorAction SilentlyContinue }
}
$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;" + $env:Path
# Codex's own PATH entry is a junction an SSH session may not cross ("an
# untrusted mount point"): its release folder, named outright.
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
} finally {
  Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue
}
