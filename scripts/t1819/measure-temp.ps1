$ErrorActionPreference = 'Stop'
$parent = Join-Path (Get-Location).Path '.runs\T-1822'
[IO.Directory]::CreateDirectory($parent) | Out-Null
$root = Join-Path $parent ('measure-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
Write-Output "MEASUREMENT-ROOT $root"
$env:CF_PWSH_START_LOCK = Join-Path $root 'powershell-start.lock'
Write-Output "MEASUREMENT-LOCK $env:CF_PWSH_START_LOCK"
$results = @()
$crates = @('cf-base', 'cf-process', 'cf-portable', 'cf-launcher', 'cf-harness', 'cf-engine', 'cf', 'cf-daemon')
foreach ($crate in $crates) {
    $temp = Join-Path $root $crate
    [IO.Directory]::CreateDirectory($temp) | Out-Null
    $env:TEMP = $temp
    $env:TMP = $temp
    Write-Output "MEASUREMENT-START $crate"
    & cargo test -j 4 -p $crate --no-fail-fast -- --test-threads=1
    $status = $LASTEXITCODE
    Start-Sleep -Seconds 5
    $folders = @(Get-ChildItem -LiteralPath $temp -Force -Directory)
    $files = @(Get-ChildItem -LiteralPath $temp -Force -File)
    Write-Output "MEASUREMENT $crate status=$status folders=$($folders.Count) files=$($files.Count)"
    foreach ($folder in $folders) { Write-Output "LEFTOVER $($folder.FullName)" }
    foreach ($file in $files) { Write-Output "LEFTOVER-FILE $($file.FullName)" }
    $results += [pscustomobject]@{ crate = $crate; status = $status; folders = $folders.Count; files = $files.Count }
}
$results | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'receipt.json') -Encoding UTF8
Write-Output "MEASUREMENT-RECEIPT $(Join-Path $root 'receipt.json')"
if (@($results | Where-Object { $_.status -ne 0 -or $_.folders -ne 0 }).Count) { exit 1 }
exit 0
