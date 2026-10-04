//! `cf`, ConsensFlow's command. Inside a window the daemon opened (its
//! participant's token in `CONSENSFLOW_TOKEN`) it is the board's commands,
//! answered here against the daemon's API; anywhere else it is the CLI's
//! standalone commands, which its Node sources beside this binary still
//! answer. `cf hook <harness>` is what a harness's hooks run, in a window or
//! not; it says only what its harness reads, and never fails.

mod board;
mod hook;
mod node;

use std::ffi::OsString;
use std::io::{self, Read, Write};

use cf_base::env::Env;
use cf_board::Board;

/// Runs the command in `args`: its exit code. Only a failure to write is an error.
pub fn run(
    env: &Env,
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let words: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let mut plain = words
        .iter()
        .map(String::as_str)
        .filter(|word| *word != "--json");
    if plain.next() == Some("hook") {
        return hook::run(plain.next(), env, input, out);
    }
    match env.text("CONSENSFLOW_TOKEN") {
        Some(token) => {
            let board = Board::new(env.text("CONSENSFLOW_URL"), token);
            board::run(&words, &board, input, out, err)
        }
        None => node::run(env, args, err),
    }
}
