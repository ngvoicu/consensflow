
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
$dir = Join-Path $env:USERPROFILE 'consensflow-build'
$tarball = Join-Path $env:USERPROFILE 'consensflow-wrun-0000abcd-12345678/tree.tgz'
$runFolder = Split-Path $tarball
$running = Join-Path $dir '.windows-run.pid'
$owner = Join-Path $dir '.windows-run.id'
$held = $false
$lock = $null
$job = $null
$completed = $null
$recordable = $false
function Free-Machine {
  if ($script:held) {
    ri -Lit $owner -Force -ea 0
    if ((gc $running -ea 0) -eq $PID) { ri -Lit $running -Force -ea 0 }
    $script:held = $false
  }
  if ($script:lock) { $script:lock.Dispose(); $script:lock = $null }
}
function Exit-Run([int]$code) { $script:completed = $code; exit $code }
try {
$env:USERPROFILE,$dir,$runFolder|%{if((gi -Lit $_ -Force -EA 0).Attributes -band 1024){"$_ is a link: nothing was made or unpacked";Exit-Run 1}}
$recordable = $true
ni -Type Directory -Force $dir | Out-Null
try { $lock = [IO.File]::Open((Join-Path $dir '.windows-run.lock'), 'OpenOrCreate', 'ReadWrite', 'None') } catch [IO.IOException] {} catch { $why = $_.Exception.Message }
$holder = gc $running -ea 0
$other = if ($holder) { try { gps -Id $holder -ea Stop | ? { ($_.Name -in @('powershell', 'pwsh')) -and $_.StartTime -le (gi $running).LastWriteTime } } catch {} }
if (-not $lock -or $other) { echo "A run is still going on this machine$(if ($other) { " (process $($other.Id))" })$(if ($why) { " (the lock could not be opened: $why)" }): one at a time."; Exit-Run 75 }
Set-Content -Path $running -Value $PID
Set-Content -Path $owner -Value @('0000abcd-12345678', $PID)
$held = $true
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path -LiteralPath $tar -PathType Leaf)) { echo "Windows' own tar is not there ($tar), so the tree cannot be unpacked."; Exit-Run 1 }
& $tar -xmzf $tarball -C $dir
if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-sweep.ps1'))))
if ($failed.Count) { Exit-Run 1 }
echo "== tree $(Get-Content -LiteralPath (Join-Path $dir '.windows-tree') -TotalCount 1)"
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-run.ps1'))))
} finally {
  if ($null -ne $job) { [CfJob]::Release($job) }
  Free-Machine
  if ($null -ne $script:completed) {
    if ($recordable) { [IO.File]::WriteAllText((Join-Path $runFolder 'completed.txt'), 'finished 0000abcd-12345678') }
    echo "`n== exit $script:completed"
  }
}
