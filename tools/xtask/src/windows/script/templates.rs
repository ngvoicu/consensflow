//! The PowerShell, and the C# and JavaScript inside it, that [`super::remote_script`] and the other
//! builders fill: text with a `«name»` mark where each part goes.

/// The bootstrap, with a mark where each part of it goes. It begins and ends with a line
/// feed, as the JavaScript's template did. `Exit-Run` is the one way out: the line that
/// says how the run went, then the exit (the `finally` that takes the run's record away
/// writes nothing, so the line is the last).
///
/// - The command's output goes to the stream as the command writes it, not line by line
///   through PowerShell, so it may end in the middle of a line (`type` of a file that has
///   no final line end does): the exit line begins with a line feed, so that it is always
///   a line of its own and never glued to the command's last words.
/// - What the command prints is UTF-8, and goes back as it came.
/// - The lock is opened before anything else is done in the folder, and a run that is
///   refused closes it and leaves the folder as it found it. Whatever the open says that
///   is not "it is open" (`IOException`) is said as the reason, and the run is refused all
///   the same, with its exit line: a run never ends without one for the lock.
/// - The process in [`super::RUNNING`] is looked at after it, as a script of the form before
///   [`super::LOCK`] holds only that: it counts only if the process is a PowerShell that was
///   started before the file was written, so that a process id a later program has taken
///   is no run, and the lock decides. The file is read as nothing if it is gone or is
///   being written.
/// - The job the run script puts the run in ([`RUN_TEMPLATE`]) is released here, in the
///   `finally`, which every way the run ends by itself goes through (`Exit-Run`'s `exit`
///   too) and the end of a run by a stop does not: so a program a command leaves running
///   stays running when the run ends, and is ended with the run when the run is killed.
pub(super) const REMOTE: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
$dir = Join-Path $env:USERPROFILE '«dir»'
$tarball = Join-Path $env:USERPROFILE '«tarball»'
$running = Join-Path $dir '«running»'
$owner = Join-Path $dir '«owner»'
$held = $false
$lock = $null
$job = $null
function Free-Machine {
  if ($script:held) {
    Remove-Item -LiteralPath $owner -Force -ErrorAction SilentlyContinue
    if ((Get-Content $running -ErrorAction SilentlyContinue) -eq $PID) { Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue }
    $script:held = $false
  }
  if ($script:lock) { $script:lock.Dispose(); $script:lock = $null }
}
function Exit-Run([int]$code) { Free-Machine; Write-Output "`n«exit»$code"; exit $code }
try {
$env:USERPROFILE,$dir|%{if((gi -Lit $_ -Force -EA 0).Attributes -band 1024){"$_ is a link: nothing was made or unpacked";Exit-Run 1}}
New-Item -ItemType Directory -Force $dir | Out-Null
try { $lock = [IO.File]::Open((Join-Path $dir '«lock»'), 'OpenOrCreate', 'ReadWrite', 'None') } catch [IO.IOException] {} catch { $why = $_.Exception.Message }
$holder = Get-Content $running -ErrorAction SilentlyContinue
$other = if ($holder) { try { Get-Process -Id $holder -ErrorAction Stop | Where-Object { ($_.Name -in @('powershell', 'pwsh')) -and $_.StartTime -le (Get-Item $running).LastWriteTime } } catch {} }
if (-not $lock -or $other) { Write-Output "A run is still going on this machine$(if ($other) { " (process $($other.Id))" })$(if ($why) { " (the lock could not be opened: $why)" }): one at a time."; Exit-Run 75 }
Set-Content -Path $running -Value $PID
Set-Content -Path $owner -Value @('«id»', $PID)
$held = $true
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path -LiteralPath $tar -PathType Leaf)) { Write-Output "Windows' own tar is not there ($tar), so the tree cannot be unpacked."; Exit-Run 1 }
& $tar -xmzf $tarball -C $dir
if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '«sweep»'))))
if ($failed.Count) { Exit-Run 1 }
Write-Output "«tree_mark»$(Get-Content -LiteralPath (Join-Path $dir '«tree_file»') -TotalCount 1)"
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '«run»'))))
} finally {
  if ($null -ne $job) { [CfJob]::Release($job) }
  Free-Machine
  Remove-Item -LiteralPath $tarball -Force -ErrorAction SilentlyContinue
}
"##;

/// The run script, with a mark where each part of it goes. It begins and ends with a line
/// feed.
///
/// - It puts the run in a job (`CfJob`, [`JOB_SOURCE`]) before anything it asks for is
///   started: the job ends every process in it when the last handle to it closes, and
///   this process holds the only one, so that a run that is killed (a stop's `taskkill`
///   ends its process) has every program it started ended with it by the system, and not
///   as `taskkill /T` does, a tree it has read and then walks while the run goes on
///   starting programs, which leaves what is started during the walk (measured on zeewin,
///   2026-10-23: a stop that landed while a command was starting programs left 21 to 50 of
///   them running every time, and a single `ping -n 60` was left 7 times in 142 stops, for
///   its whole 60 seconds; with the job, 0 in 142). A run that cannot be put in a job is said (`== no job`) and goes on as it
///   did before: its stop is the one that walks the tree.
/// - Once it is in the job it says so in the owner file ([`super::OWNER`], a third line,
///   [`super::IN_JOB`]), which is how a stop knows that ending the run's process ends all it
///   started and walks no tree ([`super::stop_script`]). A run that cannot say it is ended with its
///   tree, as one that is not in the job.
/// - Codex's own PATH entry is a junction an SSH session may not cross ("an untrusted
///   mount point"): its release folder is named outright.
/// - After the build's steps, if there were any, the steps of [`super::PREPARE_STEPS`] make sure of
///   what the tests of the copy run on (the crates, the console host), and a run that cannot
///   have them ends with that step's status and says what it needed, before its command, which
///   is never run on what happens to be there.
pub(super) const RUN_TEMPLATE: &str = r##"
$job = $null
try {
  Add-Type -TypeDefinition @'
«job»
'@
  $made = [CfJob]::Enter()
  if ($made -eq [IntPtr]::Zero) { throw [CfJob]::Why }
  $job = $made
} catch { Write-Output "== no job: $($_.Exception.Message), so a stop will not end at once what this run starts" }
if ($null -ne $job) {
  try { Add-Content -LiteralPath $owner -Value '«in_job»' } catch { Write-Output "== no job record: $($_.Exception.Message), so a stop will walk this run's tree" }
}
$env:Path = "«node»;$env:USERPROFILE\.cargo\bin;" + $env:Path
$codex = (Get-Item "$env:USERPROFILE\.codex\packages\standalone\current" -ErrorAction SilentlyContinue).Target
if ($codex) { $env:Path = "$(@($codex)[0])\bin;" + $env:Path }
Set-Location $dir
$ErrorActionPreference = 'Continue'
$env:«test_machine» = «machine»
Write-Output "== test machine: $env:«test_machine»"
«limits»$steps = @(«steps»)
«packages»foreach ($step in $steps) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
}
foreach ($step in @(«prepare»)) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) {
    Write-Output "== the run needs what '$step' makes there, and it could not be made (status $LASTEXITCODE): the command is not run on what happens to be on the machine"
    Exit-Run $LASTEXITCODE
  }
}
«given»Set-Location (Join-Path $dir '«inside»')
$run = Join-Path $dir '«command_file»'
[IO.File]::WriteAllText($run, "@echo off«crlf»" + '«command»' + "«crlf»")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
"##;

/// What the packages of a run without `--build` are held to ([`super::packages_check`]): the program,
/// in Node (the lockfiles are JSON, and Windows PowerShell 5.1 reads little of it), that the check
/// pipes to `node - <folder>`. It prints one line and ends with 1 where the folder's
/// `node_modules` are not what its `package-lock.json` says, and ends with 0, saying nothing,
/// where they are or the folder has no lockfile.
///
/// - npm's own record of what it installed (`node_modules/.package-lock.json`, with no entry for
///   the root) is compared with the lockfile the run shipped: a package missing, at another
///   version or integrity, or left that the lockfile has dropped.
/// - What the lockfile lists and `npm ci` leaves out is not missing: a package marked optional,
///   and one for another system or processor than the one Node runs on (`esbuild`'s binaries for
///   every system are listed, and Windows installs its own). Were it, every run would be refused.
/// - A lockfile it cannot read is a difference too, said.
pub(super) const PACKAGES_JS: &str = r#"const fs = require('fs')
const root = process.argv[2]
const read = (file) => {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8')).packages || {}
  } catch (error) {
    if (error.code === 'ENOENT') return null
    throw error
  }
}
const another = (list, mine) => {
  if (!Array.isArray(list) || list.length === 0) return false
  if (list.includes('!' + mine)) return true
  const allowed = list.filter((word) => !word.startsWith('!'))
  return allowed.length > 0 && !allowed.includes(mine)
}
const list = (words) => words.slice(0, 3).join(', ') + (words.length > 3 ? ' and ' + (words.length - 3) + ' more' : '')
const name = (key) => key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length)
try {
  const shipped = read(root + '/package-lock.json')
  if (shipped === null) process.exit(0)
  const installed = read(root + '/node_modules/.package-lock.json')
  if (installed === null) {
    console.log('node_modules holds no .package-lock.json: no npm ci has been run there')
    process.exit(1)
  }
  const missing = []
  const changed = []
  const extra = []
  for (const [key, entry] of Object.entries(shipped)) {
    if (!key.startsWith('node_modules/')) continue
    const have = installed[key]
    if (!have) {
      const left = entry.optional === true || another(entry.os, process.platform) || another(entry.cpu, process.arch)
      if (!left) missing.push(name(key) + ' ' + entry.version)
    } else if (have.version !== entry.version || (have.integrity && entry.integrity && have.integrity !== entry.integrity)) {
      changed.push(name(key) + ' ' + have.version + ' where the lockfile says ' + entry.version)
    }
  }
  for (const key of Object.keys(installed)) {
    if (key.startsWith('node_modules/') && !(key in shipped)) extra.push(name(key))
  }
  const said = []
  if (missing.length) said.push('missing: ' + list(missing))
  if (changed.length) said.push('changed: ' + list(changed))
  if (extra.length) said.push('extra: ' + list(extra))
  if (said.length) {
    console.log(said.join('; '))
    process.exit(1)
  }
} catch (error) {
  console.log('could not be compared: ' + error.message)
  process.exit(1)
}
"#;

pub(super) const PACKAGES_TEMPLATE: &str = r##"# packages: begin
$packagesJs = @'
«js»'@
foreach ($package in @(@('.', 'node_modules', '«root»'), @('app', 'app/node_modules', '«app»'))) {
  $differs = (@($packagesJs | node - $package[0] 2>&1 | ForEach-Object { "$_" }) -join ' ').Trim()
  if ($LASTEXITCODE -ne 0) {
    if (-not $differs) { $differs = 'node could not compare them' }
    Write-Output "== packages: $($package[1]) is not what the lockfile says ($differs), so its npm ci runs first"
    $steps += $package[2]
  }
}
# packages: end
"##;

/// The C# that `Add-Type` makes of the job in the run script: a job that ends every
/// process in it when its last handle closes (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
/// 0x2000), the calling process put in it (so are the programs it starts from then on,
/// and theirs), and `Release`, which takes the end-on-close away again at a normal end.
/// The structures are the system's (`JOBOBJECT_EXTENDED_LIMIT_INFORMATION`, 144 bytes
/// where a pointer is 8) and `9` is `JobObjectExtendedLimitInformation`. `Why` says what
/// failed, with the system's error number, where `Enter` answers zero.
pub(super) const JOB_SOURCE: &str = r#"using System;
using System.Runtime.InteropServices;
public static class CfJob {
  [StructLayout(LayoutKind.Sequential)] struct Basic { public long UserTime, JobUserTime; public uint Flags; public UIntPtr MinWs, MaxWs; public uint Active; public UIntPtr Affinity; public uint Priority, Scheduling; }
  [StructLayout(LayoutKind.Sequential)] struct Io { public ulong A, B, C, D, E, F; }
  [StructLayout(LayoutKind.Sequential)] struct Extended { public Basic Basic; public Io Io; public UIntPtr ProcessMem, JobMem, PeakProcess, PeakJob; }
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int kind, ref Extended info, int size);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  static bool Limit(IntPtr job, uint flags) { var e = new Extended(); e.Basic.Flags = flags; return SetInformationJobObject(job, 9, ref e, Marshal.SizeOf(typeof(Extended))); }
  public static string Why = "";
  public static IntPtr Enter() {
    IntPtr job = CreateJobObject(IntPtr.Zero, null);
    if (job == IntPtr.Zero) { Why = "CreateJobObject failed: " + Marshal.GetLastWin32Error(); return IntPtr.Zero; }
    if (!Limit(job, 0x2000)) { Why = "SetInformationJobObject failed: " + Marshal.GetLastWin32Error(); return IntPtr.Zero; }
    if (!AssignProcessToJobObject(job, GetCurrentProcess())) { Why = "AssignProcessToJobObject failed: " + Marshal.GetLastWin32Error(); return IntPtr.Zero; }
    return job;
  }
  public static void Release(IntPtr job) { Limit(job, 0); }
}"#;

/// The sweep, with a mark where each part of it goes. `Clear-Stale` answers how many
/// things it left in the folder it was given, and says nothing else, so what it
/// removed and could not are kept in lists and said once it is done.
pub(super) const SWEEP_TEMPLATE: &str = r##"
$failed = [Collections.Generic.List[string]]::new()
foreach ($folder in @($env:USERPROFILE, $dir)) {
  if ((Get-Item -LiteralPath $folder -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
    Write-Output "$folder is a link, so the copy is not swept and nothing is removed from it: what a link leads to is none of the copy's. Make the profile and the copy real folders."
    $failed.Add("$folder is a link")
    return
  }
}
$now = [Collections.Generic.HashSet[string]]::new([string[]](Get-Content -Encoding UTF8 (Join-Path $dir '«synced»')), [StringComparer]::OrdinalIgnoreCase)
$kept = @(«kept»)
$removed = [Collections.Generic.List[string]]::new()
function Clear-Stale([IO.DirectoryInfo]$folder) {
  $left = 0
  try { $items = $folder.GetFileSystemInfos() } catch { $failed.Add("$($folder.FullName): $($_.Exception.Message)"); return 1 }
  foreach ($item in $items) {
    $name = $item.FullName.Substring($base).Replace('\', '/')
    $deep = $item -is [IO.DirectoryInfo] -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
    if (($kept | Where-Object { $name -like $_ }) -or ($item -is [IO.FileInfo] -and $now.Contains($name)) -or ($deep -and (Clear-Stale $item))) {
      $left++
      continue
    }
    try {
      $item.Attributes = $item.Attributes -band -bnot [IO.FileAttributes]::ReadOnly
      $item.Delete()
      $removed.Add($name)
    } catch {
      $failed.Add("${name}: $($_.Exception.Message)")
      $left++
    }
  }
  $left
}
$cutoff = (Get-Date).ToUniversalTime().AddDays(-«days»)
$unpruned = [Collections.Generic.List[string]]::new()
function Clear-Old([IO.DirectoryInfo]$folder) {
  $left = 0
  try { $items = $folder.GetFileSystemInfos() } catch { $unpruned.Add("$($folder.FullName): $($_.Exception.Message)"); return 1 }
  foreach ($item in $items) {
    $name = $item.FullName.Substring($base).Replace('\', '/')
    $written = $item.LastWriteTimeUtc
    $deep = $item -is [IO.DirectoryInfo] -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
    $inside = if ($deep) { Clear-Old $item } else { 0 }
    if ($inside -or $written -ge $cutoff) {
      $left++
      continue
    }
    try {
      $item.Attributes = $item.Attributes -band -bnot [IO.FileAttributes]::ReadOnly
      $item.Delete()
      $removed.Add($name)
    } catch {
      $unpruned.Add("${name}: $($_.Exception.Message)")
      $left++
    }
  }
  $left
}
$root = Get-Item -LiteralPath $dir
$base = $root.FullName.TrimEnd('\').Length + 1
$null = Clear-Stale $root
$reports = Join-Path $dir 'evals/reports'
if (Test-Path -LiteralPath $reports -PathType Container) {
  $folder = Get-Item -LiteralPath $reports
  if (-not ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $null = Clear-Old $folder }
}
$cutoff = (Get-Date).ToUniversalTime().AddDays(-«runs_days»)
$runs = Join-Path $dir '.runs'
if (Test-Path -LiteralPath $runs -PathType Container) {
  $folder = Get-Item -Force -LiteralPath $runs
  if (-not ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $null = Clear-Old $folder }
}
foreach ($name in $removed) { Write-Output "== removed $name" }
if ($failed.Count) { Write-Output "What left the tree could not be removed from the copy: $($failed -join '; ')" }
if ($unpruned.Count) { Write-Output "What the reports and the logs of the tasks held past their age could not be removed: $($unpruned -join '; ')" }
$sha = [Security.Cryptography.SHA256]::Create()
$utf8 = [Text.UTF8Encoding]::new($false)
foreach ($name in @(Get-Content -Encoding UTF8 (Join-Path $dir '«synced»'))) {
  $bytes = $utf8.GetBytes($name)
  $null = $sha.TransformBlock([BitConverter]::GetBytes([uint64]$bytes.Length), 0, 8, $null, 0)
  $null = $sha.TransformBlock($bytes, 0, $bytes.Length, $null, 0)
  $path = Join-Path $dir $name
  $kind = 'missing'
  try { $kind = if ([IO.File]::GetAttributes($path) -band 1040) { 'other' } else { 'file' } } catch {}
  $bytes = $utf8.GetBytes($kind)
  $null = $sha.TransformBlock($bytes, 0, $bytes.Length, $null, 0)
  if ($kind -eq 'file') { $bytes = [IO.File]::ReadAllBytes($path); $null = $sha.TransformBlock($bytes, 0, $bytes.Length, $null, 0) }
}
$null = $sha.TransformFinalBlock([byte[]]::new(0), 0, 0)
$sent = (Get-Content -LiteralPath (Join-Path $dir '«tree_file»') -TotalCount 1).Split(' ')[1]
$unpacked = ($sha.Hash[0..7] | ForEach-Object { $_.ToString('x2') }) -join ''
if ($unpacked -ne $sent) { Write-Output "The files unpacked are not the tree that was sent: they hash to $unpacked, and its identity says $sent."; $failed.Add('the files unpacked are not the tree that was sent') }
function Save-Tree {
  $ErrorActionPreference = 'Continue'
  $as = @('-c', 'user.name=windows-run', '-c', 'user.email=windows-run@localhost', '-c', 'commit.gpgsign=false')
  Push-Location -LiteralPath $dir
  try {
    foreach ($words in @(@('init', '-q'), @('config', 'core.autocrlf', 'false'), @('read-tree', '--empty'), @('--literal-pathspecs', 'add', '-f', '--pathspec-from-file=«synced»'), @('commit', '-q', '--allow-empty', '--no-verify', '-m', 'what-went-over'))) {
      $said = & git @as @words 2>&1
      if ($LASTEXITCODE -ne 0) { $failed.Add("git $($words -join ' '): $($said -join ' ')"); return }
      if ($words[0] -eq 'init') { [IO.File]::WriteAllText((Join-Path $dir '.git\info\exclude'), (@(«excluded») -join "`n") + "`n") }
    }
  } finally { Pop-Location }
}
if (-not $failed.Count) {
  if (Get-Command git -ErrorAction SilentlyContinue) { Save-Tree } else { $failed.Add('git is not on the PATH, so the copy cannot be made a checkout of what went over') }
  if ($failed.Count) { Write-Output "The copy could not be made a checkout of what went over: $($failed -join '; ')" }
}
"##;

/// The script that lists the reports, with a mark where the copy's folder goes.
pub(super) const REPORTS: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$reports = Join-Path (Join-Path $env:USERPROFILE '«dir»') 'evals\reports'
if (Test-Path -LiteralPath $reports -PathType Container) {
  $folder = Get-Item -LiteralPath $reports
  if (-not ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    $base = $folder.FullName.TrimEnd('\').Length + 1
    foreach ($file in Get-ChildItem -LiteralPath $reports -Recurse -File -Force) {
      $name = $file.FullName.Substring($base).Replace('\', '/')
      if ($name -notlike 'windows/*') { "$name`t$($file.Length)" }
    }
  }
}
"##;

/// The script that ends a run, with a mark where each part of it goes. It begins
/// as the run's does: a PowerShell that has no progress to show says it had some
/// (`Preparing modules for first use`) as XML on the error stream, which the
/// JavaScript's did, and the person who stopped the run read.
pub(super) const STOP: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$dir = Join-Path $env:USERPROFILE '«dir»'
$running = Join-Path $dir '«running»'
$owner = Join-Path $dir '«owner»'
$of = @(Get-Content $owner -ErrorAction SilentlyContinue)
if ($of[0] -eq '«id»') {
  $run = try { Get-Process -Id $of[1] -ErrorAction Stop | Where-Object { ($_.Name -in @('powershell', 'pwsh')) -and $_.StartTime -le (Get-Item -LiteralPath $owner).LastWriteTime } } catch {}
  $ends = @('/PID', $of[1], '/F'); if ($of[2] -ne '«in_job»') { $ends += '/T' }
  if ($run) { taskkill @ends 2>&1 | Out-Null }
  Remove-Item -LiteralPath $owner -Force -ErrorAction SilentlyContinue; if ((Get-Content $running -ErrorAction SilentlyContinue) -eq $of[1]) { Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue }
}
Remove-Item -LiteralPath (Join-Path $env:USERPROFILE '«tarball»') -Force -ErrorAction SilentlyContinue
exit 0
"##;
