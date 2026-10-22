//! What `cargo xtask windows` has the Windows machine run, as text: nothing here
//! reaches a machine, so a test can read it. The one script is PowerShell, handed
//! to ssh encoded; the command to run goes from a file there, so that nothing on
//! its way (ssh, PowerShell) reads it again.
//!
//! The script says how the command ended, in the last line it writes: `== exit N`
//! ([`EXIT_MARK`], which [`exit_said`] reads). Every way out of it goes through
//! one function that writes that line and then exits, because what ssh answers
//! cannot be taken for the command's status: the module doc of `windows` has what
//! was measured of it.
//!
//! The script was written in JavaScript first, and `tests/golden/windows` holds
//! its text as recorded: from the JavaScript's own, until the last line was added,
//! and from this module's since (the test that records them again is
//! `record_the_goldens`). One thing in it is easy to take for a mistake: the batch
//! file the command goes to is written with a carriage return and a line feed that
//! are *in* the PowerShell string (`"@echo off<CR><LF>"`), not an escape
//! PowerShell reads, so the script carries those two bytes where the template says
//! `«crlf»`.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// What the line that ends the script's output begins with: the number is the
/// status the command ended with (`== exit 7`), a signed decimal as PowerShell
/// writes an `[int]`. It is a line of its own, as `== env NAME=VALUE` and
/// `== <step>` are.
pub const EXIT_MARK: &str = "== exit ";

/// The list of what went over, kept beside it there, so the next run knows what left the tree.
pub const SYNCED: &str = ".synced-files";

/// The running script's process id, there, while a run goes on: the lock as every version
/// of the script before [`LOCK`] read it, which this one still writes and reads (a script
/// of an older form refuses a run that is still going, and so does this one, whichever of
/// them holds it).
pub const RUNNING: &str = ".windows-run.pid";

/// The lock itself, there: the file a run opens for itself alone (`FileShare.None`) as the
/// first thing it does, and holds open until it ends. Opening it is one act of the system,
/// so of two runs that start within a millisecond one is refused; and a run that ended
/// any way (killed, the machine reset) has closed it, so a lock left by a dead run is no
/// lock and needs no looking at. It stays in the folder, empty, between runs.
pub const LOCK: &str = ".windows-run.lock";

/// The id of the run ([`run_id`]) that holds the lock, there, beside it: so that a run
/// that is ended there ends its own and no other.
pub const OWNER: &str = ".windows-run.id";

/// The tree's identity ([`crate::windows::tree::identity`]), kept beside the files in the
/// tarball: the script says it once the tarball is unpacked, as the line `== tree <identity>`
/// ([`TREE_MARK`]), and this program holds the machine to the tree it sent.
pub const TREE: &str = ".windows-tree";

/// What the line begins with that says which tree the machine unpacked.
pub const TREE_MARK: &str = "== tree ";

/// The batch file the command is written to, there.
const COMMAND_FILE: &str = ".windows-run.cmd";

/// The script that removes from the copy what is no file of the tree ([`sweep_script`]),
/// which goes in the tarball beside the list of what went over: the run's own script
/// reaches the machine on a command line, which has no room for it.
pub const SWEEP: &str = ".windows-sweep.ps1";

/// What of the copy there is no file of the tree and stays from one run to the next,
/// from its root, with `/` (a `*` stands for any characters, as PowerShell's `-like`
/// reads it; a folder is kept whole and not looked into). Every other file there that
/// the list of what went over ([`SYNCED`]) does not name is removed after the unpack,
/// wherever it came from: a run of another tree left 87 files there no sync listed,
/// which a test of the `cli` suite named (zeewin, 2026-10-16).
pub const KEPT: [&str; 16] = [
    // The run's own: the list of what went over, the tree's identity and the sweep (the
    // tarball holds them beside the files, and the list names the files only), the lock
    // taken before the unpack, the command's batch file.
    SYNCED,
    TREE,
    SWEEP,
    RUNNING,
    OWNER,
    LOCK,
    COMMAND_FILE,
    // The packages, which `npm ci` (the build's first two steps) takes away and installs
    // again from the tree's lockfiles.
    "node_modules",
    "app/node_modules",
    // The workspace's one build folder (`.cargo/config.toml`), which cargo builds again
    // where the tree changed, the files being unpacked with the time of the unpack: the
    // pane host and the console host the build puts there, and the tests' own folders.
    "app/src-tauri/target",
    // The console host's package, checked each time it is used (`cargo xtask conpty`).
    "app/.cache",
    // What `cargo xtask stage` stages for the bundle, written whole by each staging.
    "app/src-tauri/resources",
    // What the app's build script (tauri-build) writes each time it runs.
    "app/src-tauri/gen/schemas",
    // The native `cf` that `cargo xtask build-cf` puts there (through `stage`), and the
    // copies it sets aside, as Windows refuses to delete a `cf.exe` a hook still runs.
    "bin/cf.exe",
    "bin/cf.exe.old-*",
    // What a run there writes for the one who reads it afterwards: the evals' reports and
    // ledgers (a long run's are `evals/reports/long/...`). They are no file of the tree, so
    // the next run's sweep took a long run's from the copy (zeewin, 2026-10-20).
    "evals/reports",
];

/// What `--build` runs first: the packages, the page, a pane's cf.exe and the pane host.
pub const BUILD_STEPS: [&str; 6] = [
    "npm ci --no-audit --no-fund",
    "npm ci --prefix app --no-audit --no-fund",
    "npm --prefix app run bundle:ui",
    "cargo xtask stage",
    "npm run build:bridge",
    // The console host the app ships, beside the pane host, as the app has it.
    "cargo xtask conpty --into app/src-tauri/target/release",
];

/// What the machine's shell reads of a command line: cmd.exe's 8191 characters,
/// less what ssh puts before the script (`powershell -NoProfile -NonInteractive
/// -EncodedCommand `) and a margin. A script with the build's steps takes about
/// 6,600 of it, and each variable a run names about 300 more.
const ENCODED_LIMIT: usize = 8000;

/// Why what a run was asked to do cannot be sent: what is wrong with the words,
/// as the command says it.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("--env is NAME=VALUE, a variable's name and what to give it: {0}")]
    EnvForm(String),
    #[error("--env {0} is not printable ASCII: it could not reach the run as it is")]
    EnvNotAscii(String),
    #[error("--env {0} is given twice")]
    EnvTwice(String),
    #[error("the script for the machine is {length} characters encoded, and its shell reads {ENCODED_LIMIT} at most: name fewer variables with --env, or run a shorter command")]
    TooLong { length: usize },
}

/// A name for one run, which no other run on the machine has at the same time: this
/// process's id and a random part, each as eight hex digits, so that it is always the
/// same width (the script it goes into must fit the shell's line whatever it is).
pub fn run_id() -> String {
    let random = RandomState::new().build_hasher().finish();
    format!("{:08x}-{:08x}", std::process::id(), random & 0xffff_ffff)
}

/// The tarball a run sends, as its folder under the profile there names it: a name
/// of the run's own, never one another run's tarball has. The machine is one folder
/// and one lock, and runs from several trees queue for it: a name they shared would
/// let the run that sent last be the one every script there unpacks.
pub fn upload_name(dir: &str, id: &str) -> String {
    format!("{dir}.{id}.tgz")
}

/// The command as cmd.exe reads a line: an argument with a space or a quote in
/// it quoted, the way programs split their command line; each percent sign
/// doubled, which is how a batch file says one.
pub fn command_line<S: AsRef<str>>(words: &[S]) -> String {
    let words: Vec<String> = words.iter().map(|word| word_of(word.as_ref())).collect();
    words.join(" ").replace('%', "%%")
}

/// One word of a command line: as it is when it is made of the characters no
/// shell reads for anything, else in double quotes with each quote in it said again.
fn word_of(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '.' | '/' | ':' | '=' | '@' | '+' | ',' | '-')
        });
    if plain {
        word.to_owned()
    } else {
        format!("\"{}\"", word.replace('"', "\\\""))
    }
}

/// The variables a run names, `--env NAME=VALUE` each, as `(name, value)`: no
/// others go. A value is printable ASCII, since it is written into the script
/// between single quotes (where only a quote needs saying again) and read back
/// by programs that take it as it is; an empty one takes the variable away
/// there, as PowerShell does.
pub fn parse_env(entries: &[String]) -> Result<Vec<(String, String)>, Refusal> {
    let mut named: Vec<(String, String)> = Vec::new();
    for entry in entries {
        let (name, value) = entry
            .split_once('=')
            .filter(|(name, _)| is_name(name))
            .ok_or_else(|| Refusal::EnvForm(entry.clone()))?;
        if !value.chars().all(|c| (' '..='~').contains(&c)) {
            return Err(Refusal::EnvNotAscii(name.to_owned()));
        }
        if named.iter().any(|(given, _)| given == name) {
            return Err(Refusal::EnvTwice(name.to_owned()));
        }
        named.push((name.to_owned(), value.to_owned()));
    }
    Ok(named)
}

/// Whether `name` is a variable's name as PowerShell's `$env:NAME` reads one.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `text` as PowerShell takes it between single quotes.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The script as `powershell -EncodedCommand` takes it, or the refusal that says it will not fit.
pub fn encode_script(script: &str) -> Result<String, Refusal> {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = STANDARD.encode(bytes);
    if encoded.len() > ENCODED_LIMIT {
        return Err(Refusal::TooLong {
            length: encoded.len(),
        });
    }
    Ok(encoded)
}

/// The status a line says the command ended with, if it is the line the script
/// ends by (`== exit 7`, `== exit -1073741819`): the mark and a signed decimal
/// that an `i32` holds, and nothing else. Any other line (a command can write
/// anything, this one too) says nothing.
pub fn exit_said(line: &str) -> Option<i32> {
    let number = line.trim_end().strip_prefix(EXIT_MARK)?;
    let digits = number.strip_prefix('-').unwrap_or(number);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// What a run is, for the script: which run it is, where its copy is, where in it
/// the command runs, what runs first, the command, and the variables it is given.
#[derive(Debug, Clone, Copy)]
pub struct Script<'a> {
    /// The run's own name ([`run_id`]), which its tarball is named by ([`upload_name`]).
    pub id: &'a str,
    /// The folder of the copy, under the user's profile there.
    pub dir: &'a str,
    /// A folder of the copy the command runs in.
    pub inside: &'a str,
    /// What runs in the copy first, one line each (`BUILD_STEPS`, or none).
    pub steps: &'a [&'a str],
    /// The command, as `command_line` makes it.
    pub command: &'a str,
    /// The variables the command is given (`parse_env`), in the order named.
    pub env: &'a [(String, String)],
}

/// The script: the run's own tarball ([`upload_name`]) unpacked into `dir` under the
/// user's profile (what left the tree since the last run removed), the tree it held
/// said (`== tree <identity>`), then `steps` run there, then the variables `env` given
/// to the process and said in the output, then `command` run from `inside`, a folder of
/// the copy. The steps do not see the variables: they are the command's.
///
/// The run's tarball is removed whichever way the script ends, a refusal by the lock
/// too: the one thing it takes away is its own, and the lock only if it holds it (the
/// `finally`). A tarball a run left there when it never got so far (this program killed,
/// the connection lost, an upload cut short) is taken away by the sweep of the next run
/// that holds the lock, once an hour has passed since it was written to.
///
/// The unpack is `tar.exe -xmzf`: each file is given the time of the unpack (`-m`,
/// which GNU tar and bsdtar both take), not the one the tarball keeps of it, which
/// is the time it was changed on the machine that packed it. Cargo builds what is
/// newer than its last build, so a checkout switched to a branch whose file was
/// changed before that build, there, would leave the file as it was built: a test
/// that called a function the switch added did not compile against the old crate.
pub fn remote_script(script: &Script) -> String {
    let steps: Vec<String> = script
        .steps
        .iter()
        .map(|step| format!("'{step}'"))
        .collect();
    let given: String = script
        .env
        .iter()
        .map(|(name, value)| {
            format!(
                "$env:{name} = {}\nWrite-Output \"== env {name}=$env:{name}\"\n",
                quoted(value)
            )
        })
        .collect();
    fill(
        REMOTE,
        &[
            ("dir", script.dir),
            ("id", script.id),
            ("tarball", &upload_name(script.dir, script.id)),
            ("tree_mark", TREE_MARK),
            ("tree_file", TREE),
            ("running", RUNNING),
            ("owner", OWNER),
            ("lock", LOCK),
            ("sweep", SWEEP),
            ("command_file", COMMAND_FILE),
            ("steps", &steps.join(", ")),
            ("given", &given),
            ("inside", script.inside),
            ("command", &script.command.replace('\'', "''")),
            ("crlf", "\r\n"),
            ("exit", EXIT_MARK),
        ],
    )
}

/// The sweep ([`SWEEP`]), which the run's script reads and runs in its own scope once
/// the tarball is unpacked, with `$dir` the copy's folder: every file of the copy that
/// the list of what went over does not name is removed, and every folder that is then
/// empty (git sends none), but for what [`KEPT`] names. Each removal is said
/// (`== removed <name>`), and what could not be removed is said and left in `$failed`,
/// which ends the run.
///
/// - The list is read as UTF-8, as it was written, and names are compared in any case,
///   as Windows finds a file: a file renamed by its case alone is the tree's.
/// - A folder that is a link (a junction, a symbolic link) is not looked into, and only
///   the link is removed: what it points at is no file of the copy.
/// - Files and folders are removed by .NET's `Delete`, which asks nothing (PowerShell's
///   `Remove-Item` asks before it takes a folder, and a run cannot answer: it stopped a
///   run on zeewin, 2026-10-16), a file's read-only mark taken away first.
/// - A folder [`KEPT`] names is not walked: the build folder holds hundreds of thousands
///   of files.
/// - Last, the tarballs runs left beside the copy (`<folder>.<id>.tgz`, [`upload_name`],
///   and the `<folder>.tgz` the runs sent before each had its own) that were written to
///   more than an hour ago: a run removes its own, so these are of runs that never got
///   so far (this program killed, the connection lost, an upload cut short). It never
///   fails the run, and it runs with the lock held, which no run is waiting on.
pub fn sweep_script() -> String {
    let kept: Vec<String> = KEPT.iter().map(|name| quoted(name)).collect();
    fill(
        SWEEP_TEMPLATE,
        &[("synced", SYNCED), ("kept", &kept.join(", "))],
    )
}

/// What ends the run `id` that goes on there: its whole tree ended, its record taken
/// away, and its tarball. Sent when the run is stopped here, which Windows' SSH server
/// does not pass on. Only the run the lock's owner file ([`OWNER`]) names is ended: a run
/// that was stopped before its script had the lock (or that the lock refused) must not
/// end the one that holds it.
pub fn stop_script(dir: &str, id: &str) -> String {
    fill(
        STOP,
        &[
            ("dir", dir),
            ("id", id),
            ("tarball", &upload_name(dir, id)),
            ("running", RUNNING),
            ("owner", OWNER),
        ],
    )
}

/// `template` with each `«name»` written as the text `parts` gives it, in one
/// pass: what is written in is not read again, so a value that holds a mark is
/// only text. A mark no part names is left as it is (a test holds the templates
/// to naming none).
fn fill(template: &str, parts: &[(&str, &str)]) -> String {
    let mut text = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('«') {
        text.push_str(before);
        let Some((name, tail)) = after.split_once('»') else {
            text.push('«');
            rest = after;
            continue;
        };
        match parts.iter().find(|(part, _)| *part == name) {
            Some((_, value)) => text.push_str(value),
            None => {
                text.push('«');
                text.push_str(name);
                text.push('»');
            }
        }
        rest = tail;
    }
    text.push_str(rest);
    text
}

/// The script of a run, with a mark where each part of it goes. It begins and
/// ends with a line feed, as the JavaScript's template did. `Exit-Run` is the one
/// way out: the line that says how the run went, then the exit (the `finally`
/// that takes the run's record away writes nothing, so the line is the last).
///
/// - The command's output goes to the stream as the command writes it, not line by line
///   through PowerShell, so it may end in the middle of a line (`type` of a file that has
///   no final line end does): the exit line begins with a line feed, so that it is always
///   a line of its own and never glued to the command's last words.
/// - What the command prints is UTF-8, and goes back as it came.
/// - Codex's own PATH entry is a junction an SSH session may not cross ("an untrusted mount
///   point"): its release folder is named outright.
/// - The lock is opened before anything else is done in the folder, and a run that is
///   refused closes it and leaves the folder as it found it. The process in
///   [`RUNNING`] is looked at after it: a script of the form before [`LOCK`] holds only that.
const REMOTE: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
function Exit-Run([int]$code) { Write-Output "`n«exit»$code"; exit $code }
$dir = Join-Path $env:USERPROFILE '«dir»'
$tarball = Join-Path $env:USERPROFILE '«tarball»'
$running = Join-Path $dir '«running»'
$owner = Join-Path $dir '«owner»'
$held = $false
$lock = $null
try {
New-Item -ItemType Directory -Force $dir | Out-Null
try { $lock = [IO.File]::Open((Join-Path $dir '«lock»'), 'OpenOrCreate', 'ReadWrite', 'None') } catch [IO.IOException] {}
$holder = if (Test-Path $running) { Get-Content $running }
$other = if ($holder) { Get-Process -Id $holder -ErrorAction SilentlyContinue }
if (-not $lock -or $other) { Write-Output "A run is still going on this machine$(if ($other) { " (process $($other.Id))" }): one at a time."; Exit-Run 75 }
Set-Content -Path $running -Value $PID
Set-Content -Path $owner -Value '«id»'
$held = $true
tar.exe -xmzf $tarball -C $dir
if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '«sweep»'))))
if ($failed.Count) { Exit-Run 1 }
Write-Output "«tree_mark»$(Get-Content -LiteralPath (Join-Path $dir '«tree_file»') -TotalCount 1)"
$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;" + $env:Path
$codex = (Get-Item "$env:USERPROFILE\.codex\packages\standalone\current" -ErrorAction SilentlyContinue).Target
if ($codex) { $env:Path = "$(@($codex)[0])\bin;" + $env:Path }
Set-Location $dir
$ErrorActionPreference = 'Continue'
foreach ($step in @(«steps»)) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
}
«given»Set-Location (Join-Path $dir '«inside»')
$run = Join-Path $dir '«command_file»'
[IO.File]::WriteAllText($run, "@echo off«crlf»" + '«command»' + "«crlf»")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
} finally {
  Remove-Item -LiteralPath $tarball -Force -ErrorAction SilentlyContinue
  if ($held) { Remove-Item -LiteralPath $running, $owner -Force -ErrorAction SilentlyContinue }
  if ($lock) { $lock.Dispose() }
}
"##;

/// The sweep, with a mark where each part of it goes. `Clear-Stale` answers how many
/// things it left in the folder it was given, and says nothing else, so what it
/// removed and could not are kept in lists and said once it is done.
const SWEEP_TEMPLATE: &str = r##"
$now = [Collections.Generic.HashSet[string]]::new([string[]](Get-Content -Encoding UTF8 (Join-Path $dir '«synced»')), [StringComparer]::OrdinalIgnoreCase)
$kept = @(«kept»)
$removed = [Collections.Generic.List[string]]::new()
$failed = [Collections.Generic.List[string]]::new()
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
$root = Get-Item -LiteralPath $dir
$base = $root.FullName.TrimEnd('\').Length + 1
$null = Clear-Stale $root
foreach ($name in $removed) { Write-Output "== removed $name" }
if ($failed.Count) { Write-Output "What left the tree could not be removed from the copy: $($failed -join '; ')" }
$upload = '^' + [regex]::Escape($root.Name) + '(\.[0-9a-f]{8}-[0-9a-f]{8})?\.tgz$'
try { Get-ChildItem -LiteralPath $root.Parent.FullName -File | Where-Object { $_.Name -match $upload -and $_.LastWriteTime -lt (Get-Date).AddHours(-1) } | Remove-Item -Force -ErrorAction SilentlyContinue } catch {}
"##;

/// The script that ends a run, with a mark where each part of it goes. It begins
/// as the run's does: a PowerShell that has no progress to show says it had some
/// (`Preparing modules for first use`) as XML on the error stream, which the
/// JavaScript's did, and the person who stopped the run read.
const STOP: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$dir = Join-Path $env:USERPROFILE '«dir»'
$running = Join-Path $dir '«running»'
$owner = Join-Path $dir '«owner»'
if ((Test-Path $running) -and (Test-Path $owner) -and (Get-Content $owner) -eq '«id»') { taskkill /PID (Get-Content $running) /T /F | Out-Null; Remove-Item -LiteralPath $running, $owner -Force }
Remove-Item -LiteralPath (Join-Path $env:USERPROFILE '«tarball»') -Force -ErrorAction SilentlyContinue
"##;

#[cfg(test)]
mod tests;
