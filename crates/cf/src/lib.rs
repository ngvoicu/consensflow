//! `cf`, ConsensFlow's command. Inside a window the daemon opened (its
//! participant's token in `CONSENSFLOW_TOKEN`) it is the board's commands,
//! answered here against the daemon's API; anywhere else it is the CLI's
//! standalone commands, which its Node sources beside this binary still
//! answer, but for the ones `standalone` answers once the switch is on: all of
//! them but `ui`, which is the daemon's.
//! `cf hook <harness>` is what a harness's hooks run, in a window or
//! not; it says only what its harness reads, and never fails.
//! `cf codex-session <codex> <args…>` is what a Codex window runs in Codex's place.
//! `cf ui` is the app's daemon: the Node one while the switch is off, the one
//! of `cf-daemon` once `CONSENSFLOW_DAEMON=native` is set.

#![forbid(unsafe_code)]

mod board;
mod hook;
mod node;
mod standalone;

use std::ffi::OsString;
use std::io::{self, Read, Write};

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

/// Runs `cf ui [--json] [--no-open]` as the native daemon when `args` ask for
/// it and the switch is on: its exit code. It is matched on the first argument
/// as it came, tokenless (a window has its participant's token, and there `cf`
/// is the board), and only with `CONSENSFLOW_DAEMON=native`: without the
/// switch `ui` goes to the CLI's Node sources as it always did, until the
/// default flips (step 4).
///
/// The daemon reads its input and writes its output from other threads, which
/// would wait for the standard streams' locks for good if the caller held
/// them, so the caller runs this before taking them, as it does
/// [`codex_session`].
pub fn native_ui(env: &Env, args: &[OsString]) -> Option<i32> {
    let (first, rest) = args.split_first()?;
    let asked = first == "ui"
        && env.text("CONSENSFLOW_DAEMON") == Some("native")
        && env.text("CONSENSFLOW_TOKEN").is_none();
    asked.then(|| cf_daemon::ui(env, rest))
}

/// Runs the command in `args`: its exit code. Only a failure to write is an error.
pub fn run(
    env: &Env,
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    // `--json` anywhere asks for the API's JSON; the words are the rest.
    let json = args.iter().any(|arg| arg == "--json");
    let words: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--json")
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if words.first().map(String::as_str) == Some("hook") {
        return hook::run(words.get(1).map(String::as_str), env, input, out);
    }
    match Board::from_env(env) {
        Some(board) => board::run(&words, json, &board, input, out, err),
        // The words as they came: a `--json` among them is the verb's own.
        None => match standalone::run(env, args, out, err)? {
            Some(code) => Ok(code),
            None => node::run(env, args, err),
        },
    }
}
