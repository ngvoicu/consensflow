//! A stand-in for the programs xtask starts (`node`, `cargo`, `npm`), for its
//! tests: it says where it was started and with what, and ends as the
//! environment tells it.
//!
//! On its standard output, one line each: `cwd: <folder>`, then `arg: <word>` for
//! every argument (as Rust writes a string, so that a space, a quote or an empty
//! word shows), then, for each variable named in `FAKE_CHILD_REPORT` (comma
//! apart), `var <NAME>: <value>` or `var <NAME>: unset`. `FAKE_CHILD_STDERR` is
//! written to its standard error, and `FAKE_CHILD_EXIT` is the code it exits
//! with (0 when it is not a number or not there). With `FAKE_CHILD_QUIET` set it
//! reports nothing: for a test that lets it write where the test's own output
//! goes.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::process::ExitCode;

use cf_base::env::Env;

fn main() -> ExitCode {
    let env = Env::from_process();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if env.text("FAKE_CHILD_QUIET").is_none() {
        let folder = std::env::current_dir().map_or_else(
            |cause| format!("unknown ({cause})"),
            |dir| dir.display().to_string(),
        );
        let _ = writeln!(out, "cwd: {folder}");
        for arg in std::env::args_os().skip(1) {
            let _ = writeln!(out, "arg: {arg:?}");
        }
        let asked = env.text("FAKE_CHILD_REPORT").unwrap_or_default();
        for name in asked.split(',').filter(|name| !name.is_empty()) {
            match env.os(name) {
                Some(value) => {
                    let _ = writeln!(out, "var {name}: {value:?}");
                }
                None => {
                    let _ = writeln!(out, "var {name}: unset");
                }
            }
        }
    }
    let _ = out.flush();
    if let Some(text) = env.text("FAKE_CHILD_STDERR") {
        let _ = writeln!(io::stderr().lock(), "{text}");
    }
    match env
        .text("FAKE_CHILD_EXIT")
        .and_then(|code| code.parse::<i32>().ok())
    {
        Some(code) => u8::try_from(code).map_or_else(|_| std::process::exit(code), ExitCode::from),
        None => ExitCode::SUCCESS,
    }
}
