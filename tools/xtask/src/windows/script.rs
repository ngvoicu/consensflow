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

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// What the line that ends the script's output begins with: the number is the
/// status the command ended with (`== exit 7`), a signed decimal as PowerShell
/// writes an `[int]`. It is a line of its own, as `== env NAME=VALUE` and
/// `== <step>` are.
pub const EXIT_MARK: &str = "== exit ";

/// The list of what went over, kept beside it there, so the next run knows what left the tree.
pub const SYNCED: &str = ".synced-files";

/// The running script's process id, there, while a run goes on.
pub const RUNNING: &str = ".windows-run.pid";

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
/// 6,100 of it, and each variable a run names about 300 more.
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

/// What a run is, for the script: where its copy is, where in it the command
/// runs, what runs first, the command, and the variables it is given.
#[derive(Debug, Clone, Copy)]
pub struct Script<'a> {
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

/// The script: the tarball unpacked into `dir` under the user's profile (what
/// left the tree since the last run removed), then `steps` run there, then the
/// variables `env` given to the process and said in the output, then `command`
/// run from `inside`, a folder of the copy. The steps do not see the variables:
/// they are the command's.
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
            ("running", RUNNING),
            ("synced", SYNCED),
            ("steps", &steps.join(", ")),
            ("given", &given),
            ("inside", script.inside),
            ("command", &script.command.replace('\'', "''")),
            ("crlf", "\r\n"),
            ("exit", EXIT_MARK),
        ],
    )
}

/// What ends a run that goes on there: its whole tree ended, its record taken
/// away. Sent when the run is stopped here, which Windows' SSH server does not
/// pass on.
pub fn stop_script(dir: &str) -> String {
    fill(STOP, &[("dir", dir), ("running", RUNNING)])
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
const REMOTE: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
# What the command prints is UTF-8, and goes back as it came.
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
function Exit-Run([int]$code) { Write-Output "«exit»$code"; exit $code }
$dir = Join-Path $env:USERPROFILE '«dir»'
New-Item -ItemType Directory -Force $dir | Out-Null
$running = Join-Path $dir '«running»'
if (Test-Path $running) {
  $other = Get-Process -Id (Get-Content $running) -ErrorAction SilentlyContinue
  if ($other) { Write-Output "A run is still going on this machine (process $($other.Id)): one at a time."; Exit-Run 75 }
}
Set-Content -Path $running -Value $PID
try {
$synced = Join-Path $dir '«synced»'
$before = if (Test-Path $synced) { Get-Content $synced } else { @() }
tar.exe -xzf (Join-Path $env:USERPROFILE '«dir».tgz') -C $dir
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
foreach ($step in @(«steps»)) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { Exit-Run $LASTEXITCODE }
}
«given»Set-Location (Join-Path $dir '«inside»')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off«crlf»" + '«command»' + "«crlf»")
cmd /c "$run 2>&1"
Exit-Run $LASTEXITCODE
} finally {
  Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue
}
"##;

/// The script that ends a run, with a mark where each part of it goes. It begins
/// as the run's does: a PowerShell that has no progress to show says it had some
/// (`Preparing modules for first use`) as XML on the error stream, which the
/// JavaScript's did, and the person who stopped the run read.
const STOP: &str = r##"
$ProgressPreference = 'SilentlyContinue'
$running = Join-Path (Join-Path $env:USERPROFILE '«dir»') '«running»'
if (Test-Path $running) { taskkill /PID (Get-Content $running) /T /F | Out-Null; Remove-Item -LiteralPath $running -Force }
"##;

#[cfg(test)]
mod tests;
