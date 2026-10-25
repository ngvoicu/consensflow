//! What `cargo xtask windows` has the Windows machine run, as text: nothing here
//! reaches a machine, so a test can read it. There are two scripts, both PowerShell.
//! The bootstrap ([`remote_script`]) is handed to ssh encoded, on a command line that
//! holds 8,000 characters of it: it takes the lock, unpacks the run's tarball, sweeps,
//! says the tree and reads the second script. That is the run script ([`run_script`]),
//! which rides in the tarball as the sweep does and holds what a run is asked for (the
//! steps, the variables, the command), so that none of it counts against the line; the
//! command goes from a file there, so that nothing on its way (ssh, PowerShell) reads it
//! again.
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

/// The id of the run ([`run_id`]) that holds the lock and the process it is, there, beside
/// it (two lines: the id, the process, and a third, [`IN_JOB`], once the run has put itself
/// in its job), so that a run that is ended there ends its own and no other, and that
/// process and no other.
pub const OWNER: &str = ".windows-run.id";

/// The third line of the owner file ([`OWNER`]): the run is in its job ([`RUN_TEMPLATE`]), so
/// that ending its process ends every program it started, and a stop needs to end no other.
pub const IN_JOB: &str = "job";

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

/// The run script ([`run_script`]), which goes in the tarball beside the sweep, and which
/// the bootstrap reads and runs in its own scope once the tree is unpacked and said.
pub const RUN: &str = ".windows-run.ps1";

/// What of the copy there is no file of the tree and stays from one run to the next,
/// from its root, with `/` (a `*` stands for any characters, as PowerShell's `-like`
/// reads it; a folder is kept whole and not looked into). Every other file there that
/// the list of what went over ([`SYNCED`]) does not name is removed after the unpack,
/// wherever it came from: a run of another tree left 87 files there no sync listed,
/// which a test of the `cli` suite named (zeewin, 2026-10-16).
pub const KEPT: [&str; 18] = [
    // The run's own: the list of what went over, the tree's identity, the sweep and the run
    // script (the tarball holds them beside the files, and the list names the files only),
    // the lock taken before the unpack, the records beside it, the command's batch file.
    SYNCED,
    TREE,
    SWEEP,
    RUN,
    RUNNING,
    OWNER,
    LOCK,
    COMMAND_FILE,
    // The copy's own checkout of git's, whose one commit is what went over (the sweep
    // makes it so): the plants' runner asks git what a run changed in the copy.
    ".git",
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
    // the next run's sweep took a long run's from the copy (zeewin, 2026-10-20). What is
    // older than [`REPORTS_KEPT_DAYS`] is removed there, by its own pass of the sweep.
    "evals/reports",
];

/// How long what a run left in `evals/reports` stays there, in days: a file or a folder last
/// written longer ago than this is removed by the next run's sweep. The folder is
/// [`KEPT`], so the sweep does not judge its files as the tree's, and with no bound it
/// held every matrix run's logs and every long run's report and ledger from then on
/// (T-34, P3-6). The rule is age and not a count of runs: a run's files (a long run's
/// `.md`, `.json` and ledger, a matrix run's folder) are written within hours of each
/// other and age out together, and no file written within the limit is ever touched, a
/// run going on among them; none does while the sweep runs, which is under the lock,
/// before the run's script does anything else.
pub const REPORTS_KEPT_DAYS: u32 = 14;

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
/// -EncodedCommand `) and a margin. Only the bootstrap is on the line, and it is the
/// same for every run but for the folder's name and the run's: 6,300 of it in the usual
/// folder (5,748 before Windows' own tar was named by its path), which leaves 1,700, so
/// that what a run is asked for (steps, variables, command)
/// has no limit but the shell's own on the program it starts.
pub const ENCODED_LIMIT: usize = 8000;

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
    #[error("the script for the machine is {length} characters encoded, and its shell reads {ENCODED_LIMIT} at most: use a shorter --dir")]
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

/// What a run is, for the scripts: which run it is, where its copy is, where in it
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

/// The bootstrap: the run's own tarball ([`upload_name`]) unpacked into `dir` under the
/// user's profile (what left the tree since the last run removed), the tree it held
/// said (`== tree <identity>`), then the run script ([`run_script`]) that was in it.
/// Nothing of what the run is asked for is in it: its size does not depend on the steps,
/// the variables or the command.
///
/// The run's tarball is removed whichever way the script ends, a refusal by the lock
/// too: the one thing it takes away is its own, and the lock's records only if they are
/// its (the `finally`). A tarball a run left there when it never got so far (this program
/// killed, the connection lost, an upload cut short) is taken away by the sweep of the
/// next run that holds the lock.
///
/// The unpack is Windows' own tar (bsdtar), `System32\tar.exe`, named by its path, and a
/// machine without it ends the run saying so: a bare `tar.exe` is the first on the PATH,
/// and a PATH with Git's `usr\bin` before System32 (CI's Git Bash puts it there) gives
/// Git's GNU tar, which takes `C:\…` for a host to reach and ends 2 before anything is
/// unpacked (CI run 39896583469, six cases of the lock). The other programs the scripts
/// call by name, `cmd`, `git` and `taskkill`, are none of Git's `usr\bin` (Git 2.55 on
/// zeewin, 2026-10-23: it holds `tar`, `find`, `sort`, `timeout`, `hostname`, `whoami`,
/// `ssh`, `scp` and `gzip`), and `git` is wanted wherever it is found.
///
/// The unpack is `-xmzf`: each file is given the time of the unpack (`-m`,
/// which GNU tar and bsdtar both take), not the one the tarball keeps of it, which
/// is the time it was changed on the machine that packed it. Cargo builds what is
/// newer than its last build, so a checkout switched to a branch whose file was
/// changed before that build, there, would leave the file as it was built: a test
/// that called a function the switch added did not compile against the old crate.
pub fn remote_script(script: &Script) -> String {
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
            ("run", RUN),
            ("exit", EXIT_MARK),
        ],
    )
}

/// The run script ([`RUN`]), which the bootstrap runs in its own scope once the tree is
/// unpacked and said: the run put in a job, then `steps` run in the copy, then the
/// variables `env` given to the process and said in the output, then `command` run from
/// `inside`, a folder of the copy. The steps do not see the variables: they are the
/// command's. It ends the run (`Exit-Run`) with the command's status.
pub fn run_script(script: &Script) -> String {
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
        RUN_TEMPLATE,
        &[
            ("job", JOB_SOURCE),
            ("in_job", IN_JOB),
            ("command_file", COMMAND_FILE),
            ("steps", &steps.join(", ")),
            ("given", &given),
            ("inside", script.inside),
            ("command", &script.command.replace('\'', "''")),
            ("crlf", "\r\n"),
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
/// - Then `evals/reports`, which is kept, is pruned by age ([`REPORTS_KEPT_DAYS`]): a file
///   last written before the cutoff goes, and a folder that was old itself and has nothing
///   left in it. Every folder is walked, whatever its own time: a folder's time is when an
///   entry was last added to it, so a `long` folder that a recent run wrote into would
///   otherwise keep every old report in it for ever. A link is not walked, nor is the reports
///   folder itself when it is one. What cannot be removed there is said and does not end
///   the run: it is no file of the tree. (The matrix's `windows/` folders are written on the
///   Mac, not here.)
/// - Then the files unpacked are held to the tree that was sent: a SHA-256 over the files
///   the list names (in its order, each as the length of its name, its name, its kind and,
///   for a file, its bytes; a link or a folder is `other`, with no bytes), its first 16 hex
///   digits against the identity in [`TREE`] that the tarball held. A file missing, changed
///   or added by another unpack at the same time makes them differ: said, and the run ends
///   before it says a tree, and before the copy is made a checkout (the next bullet is of a
///   tree that matched). (This is the same hash [`crate::windows::tree::identity`] makes.)
/// - Then the copy is made a checkout of git's whose one commit is what went over, so
///   that what a run changes in it can be told (the plants' runner asks git, and a copy
///   git says nothing of told it nothing: every plant was UNSTABLE on zeewin, 2026-10-17):
///   its repository made the first time and kept ([`KEPT`]), with git's line ends left as
///   they are (`core.autocrlf false`, kept in it, so that what git says later of a file
///   is of its bytes as they were added); its index emptied and given the files the list
///   names, and no other file (a build's outputs are none of them), an ignored one too;
///   committed. What [`KEPT`] names is left out of what git says of the copy
///   (`.git/info/exclude`): the run's own files and the build's, which a run writes. A
///   git that is not there, or fails, is said in `$failed`, which ends the run.
/// - Last, the tarballs runs left beside the copy: this folder's own (`<folder>.<id>.tgz`,
///   [`upload_name`], and the `<folder>.tgz` the runs sent before each had its own) that
///   were written to more than an hour ago, and those of folders that are gone
///   (`<gone>.<id>.tgz`, no folder of that name beside it) written to more than a day
///   ago. A run removes its own, so these are of runs that never got so far (this
///   program killed, the connection lost, an upload cut short). The hour is safe for a
///   folder that is in use: a run's tarball is written seconds before its script asks
///   for the lock. It never fails the run, and it runs with the lock held, which no run
///   is waiting on.
pub fn sweep_script() -> String {
    let kept: Vec<String> = KEPT.iter().map(|name| quoted(name)).collect();
    let excluded: Vec<String> = KEPT
        .iter()
        .map(|name| quoted(&format!("/{name}")))
        .collect();
    fill(
        SWEEP_TEMPLATE,
        &[
            ("synced", SYNCED),
            ("tree_file", TREE),
            ("kept", &kept.join(", ")),
            ("excluded", &excluded.join(", ")),
            ("days", &REPORTS_KEPT_DAYS.to_string()),
        ],
    )
}

/// What ends the run `id` that goes on there: its process ended, and with it everything it
/// started, its record taken away, and its tarball. Sent when the run is stopped here,
/// which Windows' SSH server does not pass on. Only the run the lock's owner file
/// ([`OWNER`]) names is ended, and the process it names (not the one in the older file,
/// which another script may have written over): a run that was stopped before its script
/// had the lock (or that the lock refused) must not end the one that holds it. The stop
/// says nothing, and ends with 0 whether or not there was a run, a process or a tarball
/// left to end: it ended with the status of its last command, so that a stop of a run
/// whose tarball was gone already (the run had ended) ended with 1, which the Mac took
/// for a run that may still be going on the machine.
///
/// - The process is ended only if it is still the run's: a PowerShell that was started
///   before the owner file was last written. A process id is given again to the next
///   program that starts once its program has ended, so one that is not (the record of a
///   run that had ended, which a stop that landed as it ended leaves) names whatever
///   program has it now, and ending it ends that.
/// - Its records are taken away without a word if they cannot be: the run that ends by
///   itself takes them away too, and one that lands between the stop's reading of the owner
///   file and its removal made PowerShell say "Cannot find path" (the stop's contract is that
///   it says nothing).
/// - A run that is in its job ([`IN_JOB`] in the owner file) is ended by its process alone:
///   the system ends the job's programs, every one of them, with it. The tree is not read
///   and walked as `taskkill /T` does (leaves first, about 7 ms a program, so that the
///   walk of a run that has started hundreds goes on for seconds): a program of the
///   walk's list that has ended since has its process id given to a program that has
///   not, and the walk ends that one, whose it is (CI, 2026-10-24: two PowerShells of
///   other cases of the Windows runner's tests were ended, in their first seconds, by
///   the stop of a case that had a run starting programs; each ended with 1 and what a
///   PowerShell has written by then, `#< CLIXML`). A run that is not in its job yet, or
///   cannot be put in one, is ended with its tree, as before.
pub fn stop_script(dir: &str, id: &str) -> String {
    fill(
        STOP,
        &[
            ("dir", dir),
            ("id", id),
            ("tarball", &upload_name(dir, id)),
            ("running", RUNNING),
            ("owner", OWNER),
            ("in_job", IN_JOB),
        ],
    )
}

/// What lists the reports a run left in `evals/reports` on the machine, to be brought back
/// ([`super::fetch`]): each file under it as `<path from it, with />` and a tab and its size, but
/// for what is under `windows/`, which the matrix writes here and not there. A folder that is
/// not there lists nothing, nor does one that is itself a link (the prune refuses it too: it
/// would be walked to where it goes).
pub fn reports_script(dir: &str) -> String {
    fill(REPORTS, &[("dir", dir)])
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
/// - The process in [`RUNNING`] is looked at after it, as a script of the form before
///   [`LOCK`] holds only that: it counts only if the process is a PowerShell that was
///   started before the file was written, so that a process id a later program has taken
///   is no run, and the lock decides. The file is read as nothing if it is gone or is
///   being written.
/// - The job the run script puts the run in ([`RUN_TEMPLATE`]) is released here, in the
///   `finally`, which every way the run ends by itself goes through (`Exit-Run`'s `exit`
///   too) and the end of a run by a stop does not: so a program a command leaves running
///   stays running when the run ends, and is ended with the run when the run is killed.
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
$job = $null
try {
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
  Remove-Item -LiteralPath $tarball -Force -ErrorAction SilentlyContinue
  if ($held) {
    Remove-Item -LiteralPath $owner -Force -ErrorAction SilentlyContinue
    if ((Get-Content $running -ErrorAction SilentlyContinue) -eq $PID) { Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue }
  }
  if ($lock) { $lock.Dispose() }
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
/// - Once it is in the job it says so in the owner file ([`OWNER`], a third line,
///   [`IN_JOB`]), which is how a stop knows that ending the run's process ends all it
///   started and walks no tree ([`stop_script`]). A run that cannot say it is ended with its
///   tree, as one that is not in the job.
/// - Codex's own PATH entry is a junction an SSH session may not cross ("an untrusted
///   mount point"): its release folder is named outright.
const RUN_TEMPLATE: &str = r##"
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
"##;

/// The C# that `Add-Type` makes of the job in the run script: a job that ends every
/// process in it when its last handle closes (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
/// 0x2000), the calling process put in it (so are the programs it starts from then on,
/// and theirs), and `Release`, which takes the end-on-close away again at a normal end.
/// The structures are the system's (`JOBOBJECT_EXTENDED_LIMIT_INFORMATION`, 144 bytes
/// where a pointer is 8) and `9` is `JobObjectExtendedLimitInformation`. `Why` says what
/// failed, with the system's error number, where `Enter` answers zero.
const JOB_SOURCE: &str = r#"using System;
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
foreach ($name in $removed) { Write-Output "== removed $name" }
if ($failed.Count) { Write-Output "What left the tree could not be removed from the copy: $($failed -join '; ')" }
if ($unpruned.Count) { Write-Output "What the reports held past their age could not be removed: $($unpruned -join '; ')" }
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
$beside = $root.Parent.FullName
$own = '^' + [regex]::Escape($root.Name) + '(\.[0-9a-f]{8}-[0-9a-f]{8})?\.tgz$'
$left = '^(?<folder>.+)\.[0-9a-f]{8}-[0-9a-f]{8}\.tgz$'
try { Get-ChildItem -LiteralPath $beside -File | Where-Object { $_.Name -match $own -and $_.LastWriteTime -lt (Get-Date).AddHours(-1) } | Remove-Item -Force -ErrorAction SilentlyContinue } catch {}
try { Get-ChildItem -LiteralPath $beside -File | Where-Object { $_.Name -match $left -and -not (Test-Path -LiteralPath (Join-Path $beside $Matches.folder)) -and $_.LastWriteTime -lt (Get-Date).AddDays(-1) } | Remove-Item -Force -ErrorAction SilentlyContinue } catch {}
"##;

/// The script that lists the reports, with a mark where the copy's folder goes.
const REPORTS: &str = r##"
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
const STOP: &str = r##"
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

#[cfg(test)]
mod tests;
