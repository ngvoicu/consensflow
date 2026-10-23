
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
function Exit-Run([int]$code) { Write-Output "`n== exit $code"; exit $code }
$dir = Join-Path $env:USERPROFILE 'build two'
$tarball = Join-Path $env:USERPROFILE 'build two.0000abcd-12345678.tgz'
$running = Join-Path $dir '.windows-run.pid'
$owner = Join-Path $dir '.windows-run.id'
$held = $false
$lock = $null
$job = $null
try {
New-Item -ItemType Directory -Force $dir | Out-Null
try { $lock = [IO.File]::Open((Join-Path $dir '.windows-run.lock'), 'OpenOrCreate', 'ReadWrite', 'None') } catch [IO.IOException] {} catch { $why = $_.Exception.Message }
$holder = Get-Content $running -ErrorAction SilentlyContinue
$other = if ($holder) { try { Get-Process -Id $holder -ErrorAction Stop | Where-Object { ($_.Name -in @('powershell', 'pwsh')) -and $_.StartTime -le (Get-Item $running).LastWriteTime } } catch {} }
if (-not $lock -or $other) { Write-Output "A run is still going on this machine$(if ($other) { " (process $($other.Id))" })$(if ($why) { " (the lock could not be opened: $why)" }): one at a time."; Exit-Run 75 }
Set-Content -Path $running -Value $PID
Set-Content -Path $owner -Value @('0000abcd-12345678', $PID)
$held = $true
tar.exe -xmzf $tarball -C $dir
if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-sweep.ps1'))))
if ($failed.Count) { Exit-Run 1 }
Write-Output "== tree $(Get-Content -LiteralPath (Join-Path $dir '.windows-tree') -TotalCount 1)"
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-run.ps1'))))
} finally {
  if ($null -ne $job) { [CfJob]::Release($job) }
  Remove-Item -LiteralPath $tarball -Force -ErrorAction SilentlyContinue
  if ($held) {
    Remove-Item -LiteralPath $owner -Force -ErrorAction SilentlyContinue
    if ((Get-Content $running -ErrorAction SilentlyContinue) -eq $PID) { Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue }
  }
  if ($lock) { $lock.Dispose() }
}
