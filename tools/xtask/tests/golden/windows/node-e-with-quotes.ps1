
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
function Exit-Run([int]$code) { Write-Output "`n== exit $code"; exit $code }
$dir = Join-Path $env:USERPROFILE 'consensflow-build'
$tarball = Join-Path $env:USERPROFILE 'consensflow-build.0000abcd-12345678.tgz'
$running = Join-Path $dir '.windows-run.pid'
$owner = Join-Path $dir '.windows-run.id'
$held = $false
$lock = $null
try {
New-Item -ItemType Directory -Force $dir | Out-Null
try { $lock = [IO.File]::Open((Join-Path $dir '.windows-run.lock'), 'OpenOrCreate', 'ReadWrite', 'None') } catch [IO.IOException] {}
$holder = if (Test-Path $running) { Get-Content $running }
$other = if ($holder) { Get-Process -Id $holder -ErrorAction SilentlyContinue }
if (-not $lock -or $other) { Write-Output "A run is still going on this machine$(if ($other) { " (process $($other.Id))" }): one at a time."; Exit-Run 75 }
Set-Content -Path $running -Value $PID
Set-Content -Path $owner -Value '0000abcd-12345678'
$held = $true
tar.exe -xmzf $tarball -C $dir
if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-sweep.ps1'))))
if ($failed.Count) { Exit-Run 1 }
Write-Output "== tree $(Get-Content -LiteralPath (Join-Path $dir '.windows-tree') -TotalCount 1)"
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
" + 'node -e "console.log(\"it''s 100%%\")"' + "␍
")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
} finally {
  Remove-Item -LiteralPath $tarball -Force -ErrorAction SilentlyContinue
  if ($held) { Remove-Item -LiteralPath $running, $owner -Force -ErrorAction SilentlyContinue }
  if ($lock) { $lock.Dispose() }
}
