//! `cargo xtask live <name>`: the live tests of `crates/cf-e2e`, one command for
//! all of them. A live test is a file `tests/live_<name>.rs` whose tests are all
//! ignored (a run of the workspace's tests runs none): it runs the machine's own
//! harnesses on their own logins, which spends real quota, so it is no part of
//! `check`, and a person asks for each by name. `<name>` is the file's, with its
//! dashes or its underscores (`input-box`, `input_box`), and the tests there are
//! found by listing the folder: a new live test needs no word here.
//!
//! A Claude Code window of a run never runs on the owner's `~/.claude`: it runs on a
//! config folder of the run's own, with a copy of the Keychain's login (the first
//! run on a machine asks the owner once to allow the Keychain read; on Linux and on
//! Windows, of `~/.claude/.credentials.json`), which an acting run removes when it is over,
//! `--keep` or not (`--keep` copies the conversations and sessions it kept into the kept
//! home first, never the login). A run refuses to start when the login's
//! access token expires within the case's planned length (ten minutes unless the case says
//! more: `cf_e2e::live::Log::lasting`) and ten minutes more (Claude Code refreshes a token only when five
//! minutes of it are left, so on a machine the Windows runner started the command on, `cargo xtask
//! windows` gives it `CONSENSFLOW_TEST_MACHINE` and the kit runs the harness once as the owner to
//! renew a login that has run short, T-1067 (an acting run only: a say-only run renews nothing, and
//! a login that would need it ends the run, with the rig that needed it not started, after the builds and what the kit asked of the installed harnesses); elsewhere run again after the owner's own Claude
//! Code has), and fails if the owner's `settings.json`
//! changed or `~/.claude.json`, `history.jsonl` or `projects` gained anything of its
//! folders (`cf_e2e::live::login`).
//!
//! **The programs of a run are built before it.** `cargo xtask live` builds what the cases can need
//! (`cf`, the pane host `consensflow-bridge`, and the test binaries `harness-ask` and `engine-ask`)
//! first, each by its own `cargo build --release` in the checkout's own build folder, with the
//! caller's `--offline`, `--locked`, `--frozen` and `-j`, and names the folder to the run
//! (`CF_E2E_BUILT`, `CF_E2E_HELPERS`) and that no case builds (`CF_E2E_NO_BUILD`): a case that finds
//! a program missing stops and names it, and starts no `cargo`. A build is the checkout's own act
//! before the run, a say-only run's too; it is no act of the run's cases, which could cut a program
//! out from under another run of the same checkout.
//!
//! What the test is asked to do is said in words after its name, which this turns
//! into the variables it reads (`CONSENSFLOW_LIVE_<FLAG>`, which `cf_e2e::live`
//! has by name: [`variable`]):
//!
//! - `--flag value` sets `CONSENSFLOW_LIVE_FLAG` to the value; a flag given more
//!   than once has all its values, with commas between them, in the order given;
//! - `--keep` takes no value (it leaves what the run made where it is, for a
//!   person to look at) and sets `CONSENSFLOW_LIVE_KEEP` to `1`: a test's own
//!   flag is never called `keep` (the stops that kept a folder of transcripts by
//!   `--keep <folder>` call it `--transcripts`);
//! - a word that is no flag and no flag's value names a harness, which is the
//!   same as `--harness word`: `cargo xtask live approval claude pi`;
//! - `--models pi=openai/gpt-6-luna,codex=gpt-6.1-sol:high` is a flag as any
//!   other, which sets `CONSENSFLOW_LIVE_MODELS`: what it means (each harness
//!   named on that model, and that effort, the others on their cheap one) is the
//!   suites' to read (`cf_e2e::live::agents`), and a value they cannot ask a
//!   window for is refused there, before anything is spent.
//!
//! The words end at a `--`, or at the first of cargo's own flags that a run is
//! likely to want (`--offline`, `--locked`, `--frozen`): they and what follows
//! are cargo's. After a `--` the rest goes to the test program, after what this
//! always gives it, `--ignored --nocapture --test-threads=1` (what a live run says as it
//! goes is on the screen, the ignored tests are the ones it runs, and they run one at a
//! time: two live windows of Claude Code at once break the one-session rule; a word of
//! those that the caller gave as well is not given twice, which the test program
//! refuses, and a `--test-threads` of the caller's, as `=N` or as two words, replaces
//! the one).
//!
//! A `--help` or `-h` among the words that are this command's own is its help,
//! and starts nothing: the test's name comes first, so xtask's own rule (a help
//! that is the first word after the command's) does not reach it, and a flag of a
//! test is never called that. Beyond a `--` or a flag of cargo's it is the test
//! program's, as every word there is.
//!
//! `cargo xtask live --set <set>` runs a named set of live runs ([`SETS`]), one after another,
//! each as `cargo xtask live <its words>` would, and says what each came to: the set fails
//! when any run did, after all of them ran. `windows` is the regular live run on a Windows
//! machine (the release's step 2 names it): the checks only a Windows machine holds, the
//! typed marks of `live parity` among them (T-961, row 32; T-972).
//!
//! `cargo xtask live sweep [--in <folder>] [--home <folder>] [--older-than <days>]` lists the
//! folders earlier runs left (rig folders, login copies and harness homes in the temp folder,
//! the project folders of live cases below the home's `.consensflow-candidate/live`), each with
//! what it is, how old, and why a person might keep it (`live-sweep`). It takes none, and has
//! no word that does: a person removes what they choose. No run's start takes one either.
//!
//! With `CF_LIVE_AUDIT=1` a run is say-only: the kit's door ends no process, closes no window,
//! signals none and takes no file away, and writes down instead what it would have, each with its
//! ground, in a file of the run's own: `CF_LIVE_AUDIT_FILE` (one in the temp folder that this
//! names, unless given), made by the run as a new file (a file that is there is never added to, and
//! a file that is there already stops the run before its builds, and one that cannot be made or written stops it when the run first uses its door, after the builds and the compile of the suite); the programs the run
//! starts say into the same file. It says what it still does, as its first lines do:
//! - before the run's audit file is made, `cargo xtask live` builds in the checkout, in a say-only run as in an acting one: four release builds (`cf`, the pane host `consensflow-bridge`, and the test programs `harness-ask` and `engine-ask`; on Windows `cf-shim` as well) into `app/src-tauri/target/release`, and then `cargo test` compiles the live suite and the stand-in programs into `app/src-tauri/target/debug`; they write and replace files in the checkout's build folder, which every run of the checkout shares
//! - the product's own shutdown at the rig's close still runs: the daemon ends, the pane host ends, and every pane of the rig and what descends from it (a Codex session's children among them) is ended by the product's own code, not the kit's door
//! - the product's own operations while the case runs go on, each by the product's own code and not the kit's door: the daemon closes a window at a task's end, ends a pane it restarts or kills, and moves what it is asked to
//! - the product's own clean-up of files goes on: a Codex session removes the private socket folder it made, and the daemon takes the files it wrote
//! - keys the run types into the rig's own panes (an Escape, a Ctrl-C, /exit, a dialog's answer, a paste) are typed
//! - a terminal the run opened itself (a pane of its own test, Muse's window) is hung up by the system when the run's process ends, with what runs in it
//! - the programs the run starts (a harness's window and what it runs, its tools, git, a stand-in) run, and what they do or end is theirs, not the kit's door; among them the machine's own program of each harness the run seats, asked for its version, and on a Mac `security`, which reads the Keychain item of Claude Code's login (the first read asks the person once to allow it) and lists the names of its items
//! - the files and folders the run writes below its own folders (its rig's folder in the temp folder, a project folder of its own in the live folder, the config folder of its login copy, its wrappers, what it exports into a new folder of its own in a folder a caller names) are written; besides them the run makes new files of its own outside its folders and never writes over one that is there (the audit file, a fixture or a report file a caller names, a failure report), and makes a folder that is missing on the way to them
//! - an acting run on a test machine (CONSENSFLOW_TEST_MACHINE, which the Windows runner sets) may renew that machine's own Claude Code or Pi login by running the harness itself, which writes that login; a say-only run never starts that renewal: where a login would need it, the rig that needed it does not start and the run ends with one line that says so, which is in the audit file too; what was done by then stays done: the builds, the questions to the installed harnesses for their versions, the reads of the logins, and the run's first folders (the folder of its shims in the temp folder)
//! - a say-only run uses the real model's quota, and keeps what it made for reading until a person removes it: its folders, its transcripts, the whole command lines in the audit file, and its copies of the logins
//! - what a run's copy of a login holds, harness by harness, and a say-only run removes none of them: Claude Code's (in a config folder of the run's own) the access token and no refresh token, and on a Mac its window may move that into a Keychain item named for the folder, which outlives the run and which the kit never deletes; Codex's the whole `auth.json` as the machine holds it, so a refresh token that is in it is copied; Pi's the entries of the providers the run's model needs, each with its access token (a key entry, its key) and no refresh token, with the files of a proxy provider the model names, copied whole; Devin's `credentials.toml`, which holds an API key, and its `config.json`; Muse's `auth.json` with every key that names a refresh token taken out (on a Mac that file holds metadata and the secret stays in the Keychain, which the run's Muse reads)
//!
//! Such a run is never a pass: this names the file and leaves with a code that is not 0, whatever
//! the run came to; so does `--set` (each run of the set with a file of its own, `<name>-<n>`) and
//! `sweep`.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;

use cf_base::env::Env;

use crate::context::Context;
use crate::dispatch::{self, Command, Console, Failure, Run};
use crate::process::{self, Invocation};

pub const COMMANDS: &[Command] = &[Command {
    words: &["live"],
    about: "Run a live test of cf-e2e (the tests/live_<name>.rs files): it runs real harnesses on the machine's own logins and spends real quota, so it is ignored unless asked for and is no part of check",
    usage: "<name> [harness ...] | sweep [--in <folder>] [--home <folder>] [--older-than <days>] (lists what earlier runs left; takes none) | --set <set> (each run of the set, one after another: windows) [--keep] [--models harness=model[:effort],...] [--<flag> <value> ...] [--offline] [-- libtest's arguments] (each flag sets CONSENSFLOW_LIVE_<FLAG>, a repeated flag its values with commas between them; a folder or file a flag names to be read (--claude-config) is read and never written, and one it names for the run to write (--fixture, --transcripts, CF_EVIDENCE) is made new, or a new child of it, and refused where it is there; a run that came to a pass takes the project folders it made in the live folder away when it is done, and one that did not, or --keep, leaves them marked kept; --keep leaves the run's home and project where they are (the Claude Code config folder goes in an acting run, for it holds a copy of a login: its conversations and sessions are copied into the kept home; a say-only run removes nothing, so its config folder and the copies of the logins in it stay); --models runs each harness it names on that model, and that effort where one is given, the others on their cheap one (--claude-model M is --models claude=M, and --models wins where both name Claude); words that are no flags name harnesses; run it with no name to list the tests) | it builds the programs its cases need first (cf, the pane host and the two test binaries the cases ask), in the checkout's own build folder, before any case starts, and a case builds nothing: a build is the checkout's own act before the run, in a say-only run too, and a program a case finds missing stops it by name | with CF_LIVE_AUDIT=1 the run is say-only: the kit's door ends no process, closes no window and takes no file away, and writes down instead what it would have, in CF_LIVE_AUDIT_FILE (a new file of the run's own, whole command lines; one that is there already stops the run before its builds, and one that cannot be made or written stops it when the run first uses its door, after the builds and the compile of the suite), and the run is no pass (exit is never 0; --set gives each run a file of its own; sweep exits 1). What it still does (the first lines of the audit file say the same, word for word): before the run's audit file is made, `cargo xtask live` builds in the checkout, in a say-only run as in an acting one: four release builds (`cf`, the pane host `consensflow-bridge`, and the test programs `harness-ask` and `engine-ask`; on Windows `cf-shim` as well) into `app/src-tauri/target/release`, and then `cargo test` compiles the live suite and the stand-in programs into `app/src-tauri/target/debug`; they write and replace files in the checkout's build folder, which every run of the checkout shares; the product's own shutdown at the rig's close still runs: the daemon ends, the pane host ends, and every pane of the rig and what descends from it (a Codex session's children among them) is ended by the product's own code, not the kit's door; the product's own operations while the case runs go on, each by the product's own code and not the kit's door: the daemon closes a window at a task's end, ends a pane it restarts or kills, and moves what it is asked to; the product's own clean-up of files goes on: a Codex session removes the private socket folder it made, and the daemon takes the files it wrote; keys the run types into the rig's own panes (an Escape, a Ctrl-C, /exit, a dialog's answer, a paste) are typed; a terminal the run opened itself (a pane of its own test, Muse's window) is hung up by the system when the run's process ends, with what runs in it; the programs the run starts (a harness's window and what it runs, its tools, git, a stand-in) run, and what they do or end is theirs, not the kit's door; among them the machine's own program of each harness the run seats, asked for its version, and on a Mac `security`, which reads the Keychain item of Claude Code's login (the first read asks the person once to allow it) and lists the names of its items; the files and folders the run writes below its own folders (its rig's folder in the temp folder, a project folder of its own in the live folder, the config folder of its login copy, its wrappers, what it exports into a new folder of its own in a folder a caller names) are written; besides them the run makes new files of its own outside its folders and never writes over one that is there (the audit file, a fixture or a report file a caller names, a failure report), and makes a folder that is missing on the way to them; an acting run on a test machine (CONSENSFLOW_TEST_MACHINE, which the Windows runner sets) may renew that machine's own Claude Code or Pi login by running the harness itself, which writes that login; a say-only run never starts that renewal: where a login would need it, the rig that needed it does not start and the run ends with one line that says so, which is in the audit file too; what was done by then stays done: the builds, the questions to the installed harnesses for their versions, the reads of the logins, and the run's first folders (the folder of its shims in the temp folder); a say-only run uses the real model's quota, and keeps what it made for reading until a person removes it: its folders, its transcripts, the whole command lines in the audit file, and its copies of the logins; what a run's copy of a login holds, harness by harness, and a say-only run removes none of them: Claude Code's (in a config folder of the run's own) the access token and no refresh token, and on a Mac its window may move that into a Keychain item named for the folder, which outlives the run and which the kit never deletes; Codex's the whole `auth.json` as the machine holds it, so a refresh token that is in it is copied; Pi's the entries of the providers the run's model needs, each with its access token (a key entry, its key) and no refresh token, with the files of a proxy provider the model names, copied whole; Devin's `credentials.toml`, which holds an API key, and its `config.json`; Muse's `auth.json` with every key that names a refresh token taken out (on a Mac that file holds metadata and the secret stays in the Keychain, which the run's Muse reads)",
    run: Run::Native(live),
}];

/// The named sets of live runs, each a list of the words of `cargo xtask live` that run one.
/// `windows`: the typed marks and the interrupt after them in a Codex and a Devin member's box
/// (`parity`, T-548), the member's shell finding its window's own `cf` (`own-cf`), and a
/// human's paste into a Codex window sent by its Enter (`paste-enter`). Devin's and Muse's
/// windows on Windows run on a home of the run's own, as the others' do (T-1084, T-1136:
/// `seat` seats all five in one rig on zeewin2); `seat` is no part of the set, which it would
/// make spend the allowance of every harness each time.
pub const SETS: &[(&str, &[&str])] = &[(
    "windows",
    &[
        "parity codex devin",
        "own-cf claude codex pi",
        "paste-enter codex",
    ],
)];

/// The flag that names a set.
const SET: &str = "--set";

/// What every variable a live test reads its options by begins with
/// (`cf_e2e::live::PREFIX`, which a test here holds this to).
const PREFIX: &str = "CONSENSFLOW_LIVE_";

/// The flag that takes no value.
const KEEP: &str = "keep";

/// The flag that bare words are the values of: the harnesses a test is to run.
const HARNESS: &str = "harness";

/// Cargo's own flags, which end the words of the command and are cargo's with
/// what follows them.
const CARGO_FLAGS: [&str; 3] = ["--offline", "--locked", "--frozen"];

/// The variable the flag `flag` sets: `CONSENSFLOW_LIVE_`, then the flag in
/// capitals with its dashes as underscores.
fn variable(flag: &str) -> String {
    format!("{PREFIX}{}", flag.to_ascii_uppercase().replace('-', "_"))
}

/// What the command line of `live` says.
#[derive(Debug, PartialEq, Eq)]
struct Asked {
    /// The test: `tests/live_<test>.rs`.
    test: String,
    /// The variables it is run with, in the order their flags were first given.
    vars: Vec<(String, String)>,
    /// The words that are cargo's, then a `--` and the test program's, as they came.
    rest: Vec<OsString>,
}

fn live(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if asks_for_help(args) {
        write!(console.out, "{}", dispatch::command_help(&COMMANDS[0]))?;
        return Ok(0);
    }
    let names = tests(&context.root)?;
    if args.first().is_some_and(|word| word == SET) {
        return run_set(context, args, &names, console);
    }
    if args.first().is_some_and(|word| word == SWEEP) && !names.iter().any(|name| name == SWEEP) {
        // The kit's own program lists and takes nothing in any mode, and under `CF_LIVE_AUDIT`
        // leaves with 1, which a say-only run does whatever it came to.
        return Ok(process::run(&sweep(context, &args[1..]), &context.env)?);
    }
    let asked = parse(args, &names)?;
    let audit = audit_file(&context.env);
    if let Some(file) = &audit {
        refuse_old_audit_file(file)?;
    }
    if let Some(code) = prepare(context, &asked, console)? {
        return Ok(code);
    }
    let Some(file) = audit else {
        return Ok(process::run(&started(context, &asked), &context.env)?);
    };
    let code = process::run(
        &started(context, &asked).var(AUDIT_FILE, &file),
        &context.env,
    )?;
    writeln!(console.err, "{}", audit_report(&file))?;
    Ok(if code == 0 { 1 } else { code })
}

/// Stops before anything starts where `file` is there: a say-only run makes its file as a new one
/// of its own and never adds to an older run's, and what an older file says is not what this run
/// would have done.
fn refuse_old_audit_file(file: &Path) -> Result<(), Failure> {
    if fs::symlink_metadata(file).is_ok() {
        return Err(Failure::Usage(format!(
            "the audit file {} is there already: a say-only run ({AUDIT}) makes its file as a new one of its own and never adds to an older run's; name another in {AUDIT_FILE}, or take it away if it is nobody's",
            file.display()
        )));
    }
    Ok(())
}

/// What a say-only run left, as the person is told: the acts its file says, or that the run
/// made no file, and in both that a say-only run is no pass. The file is made by the run (a new
/// one of its own), so one that is not there says nothing of what the run did.
fn audit_report(file: &Path) -> String {
    match fs::read_to_string(file) {
        Ok(text) => {
            let said = said_acts(&text);
            format!(
                "say only ({AUDIT}): {said} ending{} and removal{} the run would have made are in {}, after the first lines that say what it still did; a say-only run is no pass",
                if said == 1 { "" } else { "s" },
                if said == 1 { "" } else { "s" },
                file.display()
            )
        }
        // A file that is not there says nothing of what the run did before: it may have stopped
        // before it made one, or the file was taken away; any other failure is a file the run
        // made that this cannot read. In both, what the run did is not known.
        Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => format!(
            "say only ({AUDIT}): the run made no audit file {}; what it did before is not known (a file that is there is never added to; name another in {AUDIT_FILE}); a say-only run is no pass",
            file.display()
        ),
        Err(failed) => format!(
            "say only ({AUDIT}): the audit could not be read; what the run did is not known ({}: {failed}); a say-only run is no pass",
            file.display()
        ),
    }
}

/// How many acts an audit file says: its lines but the first lines of the run's own (`# …`).
fn said_acts(text: &str) -> usize {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count()
}

/// The audit file of run number `at` (from 1) of a set, from the file the set's runs are named
/// by: `<stem>-<at>.<extension>` beside it, as each run makes a new file of its own.
fn audit_file_of(base: &Path, at: usize) -> std::path::PathBuf {
    let stem = base.file_stem().map_or_else(
        || "audit".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let name = match base.extension() {
        Some(extension) => format!("{stem}-{at}.{}", extension.to_string_lossy()),
        None => format!("{stem}-{at}"),
    };
    base.with_file_name(name)
}

/// The word that sweeps the rig folders older runs left.
const SWEEP: &str = "sweep";

/// The variable that makes a run say-only, and the one that names where it says
/// (`cf_e2e::door::AUDIT`, `cf_e2e::door::AUDIT_FILE`).
const AUDIT: &str = "CF_LIVE_AUDIT";
const AUDIT_FILE: &str = "CF_LIVE_AUDIT_FILE";
/// The variable the run hands the folders it made to a program it starts by (`cf_e2e::door::MADE_ROOTS`).
const MADE_ROOTS: &str = "CF_LIVE_MADE_ROOTS";

/// Where a say-only run says what it would have done: the file `CF_LIVE_AUDIT_FILE` names, or
/// one of this run's own in the temp folder. None where the run is not say-only.
fn audit_file(env: &Env) -> Option<std::path::PathBuf> {
    let audit = env.text(AUDIT).unwrap_or_default();
    if audit.is_empty() || audit == "0" {
        return None;
    }
    Some(
        env.path(AUDIT_FILE)
            .filter(|file| !file.as_os_str().is_empty())
            .map_or_else(
                || {
                    std::env::temp_dir()
                        .join(format!("consensflow-live-audit-{}.txt", std::process::id()))
                },
                Path::to_path_buf,
            ),
    )
}

/// `cargo xtask live sweep <args>`: the kit's own program for it.
fn sweep(context: &Context, args: &[OsString]) -> Invocation {
    Invocation::new("cargo", &context.root)
        .args(["run", "-q", "-p", "cf-e2e", "--bin", "live-sweep", "--"])
        .args(args)
}

/// The runs of a set, each the line of [`SETS`] it is and what it asks.
type SetRuns = Vec<(&'static str, Asked)>;

/// The runs of the set `--set <name>` names, each as its words ask, every one of them read
/// before the first starts: a word of the set that names no test is refused, and nothing runs.
fn set_runs(args: &[OsString], names: &[String]) -> Result<(String, SetRuns), Failure> {
    let listed = || {
        SETS.iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let name = match args {
        [_, name] => name.to_string_lossy().into_owned(),
        _ => {
            return Err(Failure::Usage(format!(
                "--set takes the name of a set, and nothing after it: {}",
                listed()
            )))
        }
    };
    let (_, lines) = SETS
        .iter()
        .find(|(set, _)| *set == name)
        .ok_or_else(|| Failure::Usage(format!("no set is called {name}: {}", listed())))?;
    let runs = lines
        .iter()
        .map(|line| {
            let words: Vec<OsString> = line.split_whitespace().map(OsString::from).collect();
            parse(&words, names).map(|asked| (*line, asked))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((name, runs))
}

/// Runs the set `--set <name>` names, one run after another, and says what each came to: 0
/// when every run passed.
fn run_set(
    context: &Context,
    args: &[OsString],
    names: &[String],
    console: &mut Console,
) -> Result<i32, Failure> {
    let (name, runs) = set_runs(args, names)?;
    let audit = audit_file(&context.env);
    if let Some(base) = &audit {
        // Before the first run starts, so that no run of the set is begun on an older file.
        for at in 1..=runs.len() {
            refuse_old_audit_file(&audit_file_of(base, at))?;
        }
    }
    for (_, asked) in &runs {
        if let Some(code) = prepare(context, asked, console)? {
            return Ok(code);
        }
    }
    let mut came = Vec::new();
    for (at, (line, asked)) in runs.iter().enumerate() {
        writeln!(console.out, "== live {line}")?;
        // A say-only run makes its audit file as a new one of its own, so each run of a set has one.
        let file = audit.as_ref().map(|base| audit_file_of(base, at + 1));
        let mut invoked = started(context, asked);
        if let Some(file) = &file {
            invoked = invoked.var(AUDIT_FILE, file);
        }
        let status = process::run(&invoked, &context.env)?;
        came.push(set_line(status, line, file.is_some()));
        if let Some(file) = &file {
            writeln!(console.err, "{}", audit_report(file))?;
        }
    }
    writeln!(console.out, "\nthe set {name}:\n{}", came.join("\n"))?;
    if audit.is_some() {
        writeln!(
            console.err,
            "say only ({AUDIT}): the set {name} is no pass, whatever each run came to"
        )?;
        return Ok(1);
    }
    Ok(i32::from(
        came.iter().any(|line| line.starts_with("FAILED")),
    ))
}

/// The line of a run in the end of a set: `ok` or `FAILED` by what the run left with; a say-only
/// run (its exit code is 1 whatever it came to) is `said`, which reads one way only: it is no
/// pass, so no line of a say-only set says `ok`.
fn set_line(status: i32, line: &str, say_only: bool) -> String {
    let word = match (say_only, status) {
        (true, _) => "said  ",
        (false, 0) => "ok    ",
        (false, _) => "FAILED",
    };
    format!("{word} live {line}")
}

/// Whether the words ask for help: `--help` or `-h` among the words that are live's
/// own, as it is for a command that has a first word (`cargo xtask live designer
/// --help` is no flag of the test, which has none of the name). What follows a `--`,
/// or a flag of cargo's, is not live's: `-- --help` is the test program's.
fn asks_for_help(args: &[OsString]) -> bool {
    args.iter()
        .take_while(|word| *word != "--" && !CARGO_FLAGS.iter().any(|flag| *word == flag))
        .any(|word| word == "--help" || word == "-h")
}

/// The live tests there are: the files `tests/live_*.rs` of `crates/cf-e2e`,
/// without the `live_` and the `.rs`, by name.
fn tests(root: &Path) -> Result<Vec<String>, Failure> {
    let mut names: Vec<String> = fs::read_dir(root.join("crates").join("cf-e2e").join("tests"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = file.strip_prefix("live_")?.strip_suffix(".rs")?;
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect();
    names.sort();
    Ok(names)
}

/// Reads the words of `live`, against the tests there are (`names`).
fn parse(args: &[OsString], names: &[String]) -> Result<Asked, Failure> {
    let listed = || names.join(", ");
    let text = |word: &OsStr| word.to_string_lossy().into_owned();
    let Some((first, mut rest)) = args.split_first() else {
        return Err(Failure::Usage(format!(
            "live takes the name of a live test: {}",
            listed()
        )));
    };
    let test = text(first).replace('-', "_");
    if !names.contains(&test) {
        return Err(Failure::Usage(format!(
            "no live test is called {}: {}",
            text(first),
            listed()
        )));
    }
    // The values of each flag, in the order the flags were first given.
    let mut flags: Vec<(String, Vec<String>)> = Vec::new();
    let mut give = |flag: &str, value: String| match flags.iter_mut().find(|(name, _)| name == flag)
    {
        Some((_, values)) => values.push(value),
        None => flags.push((flag.to_owned(), vec![value])),
    };
    while let Some((word, after)) = rest.split_first() {
        let word_text = text(word);
        if word_text == "--" || CARGO_FLAGS.contains(&word_text.as_str()) {
            break;
        }
        if let Some(flag) = word_text.strip_prefix("--").filter(|flag| !flag.is_empty()) {
            if flag == KEEP {
                give(KEEP, "1".to_owned());
                rest = after;
                continue;
            }
            let Some((value, after_value)) = after
                .split_first()
                .filter(|(value, _)| !text(value).starts_with('-'))
            else {
                return Err(Failure::Usage(format!(
                    "--{flag} takes a value (a flag that takes none is --{KEEP})"
                )));
            };
            give(flag, text(value));
            rest = after_value;
            continue;
        }
        if word_text.starts_with('-') {
            return Err(Failure::Usage(format!(
                "unknown option: {word_text} (a flag is --name, and a word without a dash names a harness)"
            )));
        }
        give(HARNESS, word_text);
        rest = after;
    }
    Ok(Asked {
        test,
        vars: flags
            .into_iter()
            .map(|(flag, values)| (variable(&flag), values.join(",")))
            .collect(),
        rest: rest.to_vec(),
    })
}

/// What every run of a live test is given to run: the tests that are ignored, what they
/// say as they go, on the screen, and one test thread: the tests of one file run on parallel
/// threads otherwise, and a file with more than one (`live_finish` has three) seats two windows
/// of Claude Code at once, which the one live Claude Code session at a time rule forbids. A word
/// the caller gave too is not given twice: the test program refuses a flag it is given more than
/// once; a `--test-threads` of the caller's, in either spelling (`--test-threads=N`, or the
/// flag and its number as two words), replaces the one given here ([`gives_threads`]).
const ALWAYS: [&str; 3] = ["--ignored", "--nocapture", ONE_THREAD];

/// The flag that gives the test program one test thread.
const ONE_THREAD: &str = "--test-threads=1";

/// Whether `given`, the words the caller gave the test program, say how many test threads it has:
/// `--test-threads=N`, or `--test-threads` and the number after it.
fn gives_threads(given: &[OsString]) -> bool {
    given
        .iter()
        .filter_map(|word| word.to_str())
        .any(|word| word == "--test-threads" || word.starts_with("--test-threads="))
}

/// The `cargo test` of the live test `asked` names: its ignored tests, with what
/// it is asked for in the variables it reads.
fn invocation(context: &Context, asked: &Asked) -> Invocation {
    let at = asked.rest.iter().position(|word| word == "--");
    let (cargo, test_program) = match at {
        Some(at) => (&asked.rest[..at], &asked.rest[at + 1..]),
        None => (&asked.rest[..], &[][..]),
    };
    let always = ALWAYS.into_iter().filter(|word| {
        if *word == ONE_THREAD {
            !gives_threads(test_program)
        } else {
            !test_program.iter().any(|given| given == word)
        }
    });
    let mut invocation = Invocation::new("cargo", &context.root)
        .args(["test", "-p", "cf-e2e", "--test"])
        .arg(format!("live_{}", asked.test))
        .args(cargo)
        .arg("--")
        .args(always)
        .args(test_program);
    for (name, value) in &asked.vars {
        invocation = invocation.var(name, value);
    }
    // The folders a run may write below are the ones it makes: the variable that hands them to a
    // program the run starts is never a person's to set (`cf_e2e::door::MADE_ROOTS`).
    invocation.without(MADE_ROOTS)
}

/// The variable that names the folder `cf` and the pane host were built in before the run
/// (`cf_e2e::cf::BUILT`), the one that names where the test binaries the cases ask are
/// (`cf_e2e::cf::HELPERS`), and the one that says a case builds nothing
/// (`cf_e2e::cf::NO_BUILD`): a program that is missing is an error that names it, never a
/// `cargo` the case starts.
const BUILT: &str = "CF_E2E_BUILT";
const HELPERS: &str = "CF_E2E_HELPERS";
const NO_BUILD: &str = "CF_E2E_NO_BUILD";

/// The programs a live case can need, each as `cf_e2e::cf` asks for it: its package, its binary
/// and the cargo words after the rest. Built apart, one `cargo build` each, as the builder of
/// the tests builds them (a build of two programs in one gives `cf` the features of both, and
/// builds it again each time the other way follows). The shim is the terminal's command where the
/// terminal's command is a program, which is Windows.
fn programs() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    let shim: &[&str] = if cfg!(windows) {
        &["--bin", "cf-shim"]
    } else {
        &[]
    };
    vec![
        ("cf", "cf", shim.to_vec()),
        ("cf-panes", "consensflow-bridge", Vec::new()),
        (
            "cf-harness",
            "harness-ask",
            vec!["--features", "test-support"],
        ),
        (
            "cf-engine",
            "engine-ask",
            vec!["--features", "test-support"],
        ),
    ]
}

/// The folder the release builds of the checkout at `root` are in (`.cargo/config.toml` puts the
/// build folder at `app/src-tauri/target`).
fn built_in(root: &Path) -> std::path::PathBuf {
    root.join("app")
        .join("src-tauri")
        .join("target")
        .join("release")
}

/// The words of the caller that a build takes as they stand: cargo's own switches of the command
/// (`--offline`, `--locked`, `--frozen`) and the number of jobs (`-j N`, `-jN`, `--jobs N`,
/// `--jobs=N`). What else is cargo's for the test (a feature, a filter) is not the build's.
fn build_words(cargo: &[OsString]) -> Vec<OsString> {
    let mut words = Vec::new();
    let mut at = cargo.iter();
    while let Some(word) = at.next() {
        let text = word.to_string_lossy();
        if CARGO_FLAGS.contains(&text.as_ref()) {
            if text != "--locked" {
                words.push(word.clone());
            }
        } else if text == "-j" || text == "--jobs" {
            words.push(word.clone());
            if let Some(count) = at.next() {
                words.push(count.clone());
            }
        } else if (text.starts_with("-j") && text.len() > 2) || text.starts_with("--jobs=") {
            words.push(word.clone());
        }
    }
    words
}

/// The builds of the programs a live case can need ([`programs`]), each in the checkout's own
/// build folder as `cargo build` there makes them, with the caller's switches ([`build_words`]).
fn builds(context: &Context, asked: &Asked) -> Vec<Invocation> {
    let at = asked.rest.iter().position(|word| word == "--");
    let cargo = &asked.rest[..at.unwrap_or(asked.rest.len())];
    programs()
        .into_iter()
        .map(|(package, bin, more)| {
            Invocation::new("cargo", &context.root)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "-p",
                    package,
                    "--bin",
                    bin,
                ])
                .args(more)
                .args(build_words(cargo))
        })
        .collect()
}

/// Builds the programs the run's cases can need, before any case starts: a build is the
/// checkout's own act, made here in its own build folder, where a case that builds could cut a
/// program out from under another run of the same checkout; a case that finds a program missing
/// stops and names it ([`BUILT`]). Where a build does not succeed, or leaves a program
/// missing, its code (1 where there is none) and nothing is started.
fn prepare(
    context: &Context,
    asked: &Asked,
    console: &mut Console,
) -> Result<Option<i32>, Failure> {
    for build in builds(context, asked) {
        writeln!(
            console.err,
            "live: preparing the programs of the run: {}",
            build.display()
        )?;
        let code = process::run(&build, &context.env)?;
        if code != 0 {
            writeln!(
                console.err,
                "live: a program the run needs was not built (exit {code}); nothing is started"
            )?;
            return Ok(Some(code));
        }
    }
    let folder = built_in(&context.root);
    for (_, bin, _) in programs() {
        let program = folder.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
        if !program.is_file() {
            writeln!(
                console.err,
                "live: {} was built, and there is none at {}: is the build folder somewhere else than .cargo/config.toml says? nothing is started",
                bin,
                program.display()
            )?;
            return Ok(Some(1));
        }
    }
    Ok(None)
}

/// [`invocation`] of a run whose programs are built ([`prepare`]): told where they are, and that
/// a case builds none.
fn started(context: &Context, asked: &Asked) -> Invocation {
    let folder = built_in(&context.root);
    invocation(context, asked)
        .var(BUILT, &folder)
        .var(HELPERS, &folder)
        .var(NO_BUILD, "1")
}

#[cfg(test)]
mod tests;
