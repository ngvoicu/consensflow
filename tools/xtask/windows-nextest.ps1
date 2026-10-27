# Runs a plant's nextest by hand on a Windows machine, as the plants' runner starts it (config file, store
# folder, profile), and says what nextest said and what it left, so that a plant whose "report could not be
# read" can be told from a nextest that did not run. Run it from the copy's root:
#
#   cargo xtask windows --machine zeewin2 -- powershell -NoProfile -File tools\xtask\windows-nextest.ps1 [-Fetch]
#
# `-Fetch` runs `cargo fetch --locked` first, which is what a copy that never fetched every crate needs
# for nextest's `cargo metadata --offline` (T-627).
param([string]$Test = 'a_window_whose_program_exits_by_itself', [string]$Package = 'cf-panes', [switch]$Fetch)
$ErrorActionPreference = 'Continue'
if ($Fetch) {
  $watch = [Diagnostics.Stopwatch]::StartNew()
  cargo fetch --locked 2>&1 | ForEach-Object { "$_" }
  Write-Output "== cargo fetch --locked ended with $LASTEXITCODE in $([int]$watch.Elapsed.TotalSeconds) s"
}
$dir = Join-Path $PWD 'app\src-tauri\target\plants-runs\by-hand'
Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dir | Out-Null
$store = $dir.Replace('\', '/')
$config = "[store]`ndir = `"$store`"`n`n[profile.plants]`nretries = 0`nfail-fast = false`n`n[profile.plants.junit]`npath = `"junit.xml`"`nreport-skipped = `"all`"`n"
[IO.File]::WriteAllText((Join-Path $dir 'nextest.toml'), $config)
Write-Output "== config`n$config"
Write-Output "== cargo-nextest: $((Get-Command cargo-nextest -ErrorAction SilentlyContinue).Source)"
cargo-nextest nextest run --profile plants --config-file (Join-Path $dir 'nextest.toml') --offline -p $Package --lib $Test 2>&1 | ForEach-Object { "$_" }
Write-Output "== nextest ended with $LASTEXITCODE"
Write-Output '== what it left'
Get-ChildItem -LiteralPath $dir -Recurse | ForEach-Object { $_.FullName }
