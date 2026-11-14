//! `cf`, ConsensFlow's command. Inside a window the daemon opened (its
//! participant's token in `CONSENSFLOW_TOKEN`) it is the board's commands,
//! answered here against the daemon's API; anywhere else it is the CLI's
//! standalone commands, which `standalone` answers, and `ui`, the daemon's,
//! which `main` runs before any of this (`native_ui`).
//! In a window `cf ui` is not the daemon's: it says so, and why.
//! `cf hook <harness>` is what a harness's hooks run, in a window or
//! not; it says only what its harness reads, and never fails.
//! `cf codex-session <codex> <args…>` is what a Codex window runs in Codex's place.
//! `cf ui` is the app's daemon, the one of `cf-daemon`.
//!
//! Every verb the usage names, the board's and the standalone ones, and every
//! sub-verb, answers `--help` and `-h` with its usage and exit code 0, and asks
//! the board nothing and reads nothing of the home. `hook` and `codex-session`
//! are run by harnesses, never typed, and have no usage to answer with.
//!
//! Nothing else runs behind it: the bundle ships no Node and no sources, and a
//! `use-node` file left in a home by the flip release, which sent the commands
//! of that home to them, is read by nothing.

#![forbid(unsafe_code)]

mod board;
mod hook;
mod standalone;

#[cfg(test)]
mod memories_tests;

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;

use cf_base::env::Env;
use cf_board::Board;

/// Runs `cf codex-session <codex> <args…>` when `args` ask for it: its exit
/// code. Matched on the first argument as it came, before a hook, a token or
/// a `--json` is looked for, and its arguments are passed on as the system
/// gave them. It holds the terminal for as long as the window lives and may
/// report from any thread, so the caller runs it before taking the standard
/// streams, never with them held.
pub fn codex_session(env: &Env, args: &[OsString]) -> Option<i32> {
    let (first, rest) = args.split_first()?;
    (first == "codex-session").then(|| cf_codex_session::run(env, rest))
}

/// Runs `cf ui [--json] [--no-open]` as the daemon when `args` ask for it: its
/// exit code. It is matched on the first argument as it came, and tokenless (a
/// window has its participant's token, and there `cf` is the board). `cf ui
/// --help` is not asking for the daemon: it is the usage of the verb, which
/// [`run`] answers.
///
/// The daemon reads its input and writes its output from other threads, which
/// would wait for the standard streams' locks for good if the caller held
/// them, so the caller runs this before taking them, as it does
/// [`codex_session`].
pub fn native_ui(env: &Env, args: &[OsString]) -> Option<i32> {
    let (first, rest) = args.split_first()?;
    let asked = first == "ui" && env.text("CONSENSFLOW_TOKEN").is_none();
    let words: Vec<String> = rest
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    (asked && !standalone::asks_for_help(&words)).then(|| cf_daemon::ui(env, rest))
}

/// What `cf ui` says where a member's token is set, which is a window of a board: it starts
/// ConsensFlow's own daemon, outside one. It names the variables to take away
/// ([`WINDOW_VARIABLES`], which a test holds the words to) and never their values; an app
/// opened from a window had a daemon that never started (T-868) until it took them away itself.
const UI_IN_A_WINDOW: &str = "`cf ui` starts ConsensFlow's own daemon, and this is a window of \
    a ConsensFlow board (CONSENSFLOW_TOKEN is set), where cf is that board's command and has no \
    `ui`. Run it in a terminal outside the window, or unset CONSENSFLOW_URL, CONSENSFLOW_TOKEN, \
    CONSENSFLOW_PARTICIPANT and CONSENSFLOW_PROJECT first.";

/// Runs the command in `args`: its exit code. Only a failure to write is an error. Its working
/// folder is the process's ([`run_in`]).
pub fn run(
    env: &Env,
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    run_in(
        std::env::current_dir().ok().as_deref(),
        env,
        args,
        input,
        out,
        err,
    )
}

/// [`run`] as it would run with `folder` as its working folder (none where the system gave it
/// none): a board command that changes the board is refused where the folder is Codex's
/// memories folder, or inside it. What reads the board, `cf hook`, and the usage of a command
/// (`--help`) are not changed by it.
pub(crate) fn run_in(
    folder: Option<&Path>,
    env: &Env,
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    // `--json` anywhere asks for the API's JSON; the words are the rest.
    let json = args.iter().any(|arg| arg == "--json");
    let given: Vec<OsString> = args
        .iter()
        .filter(|arg| *arg != "--json")
        .cloned()
        .collect();
    if given.first().is_some_and(|first| first == "hook") {
        let harness = given.get(1).map(|arg| arg.to_string_lossy());
        let rest = given.get(2..).unwrap_or_default();
        return hook::run(harness.as_deref(), rest, env, input, out);
    }
    // The daemon's `cf ui` is no board command, and `main` left it to this when a token is
    // set (`native_ui`): said in words, where the board would call it an unknown command.
    if given.first().is_some_and(|first| first == "ui") && env.text("CONSENSFLOW_TOKEN").is_some() {
        writeln!(err, "cf: {UI_IN_A_WINDOW}")?;
        return Ok(1);
    }
    match Board::from_env(env) {
        Some(board) => {
            // A command run where a harness does its background work with a window's config
            // (Codex's memory job) is no window's: it changes nothing on the board.
            let board = match folder.and_then(|folder| cf_harness::background::refusal(env, folder))
            {
                Some(why) => board.refusing_writes(why),
                None => board,
            };
            board::run(&given, json, &board, input, out, err)
        }
        // Tokenless: the human's own `cf`. The words as they came: a `--json`
        // among them is the verb's own, and `ui`, which `main` ran before this
        // (`native_ui`), is no verb here.
        None => standalone::run(env, args, out, err),
    }
}
