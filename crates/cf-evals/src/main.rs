//! The `cf-evals` command: `cargo xtask eval`, `eval summary` and `watch` hand
//! it their words ([`cf_evals::command`] says what it does with them).

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let status = cf_evals::command(&args, &mut std::io::stdout(), &mut std::io::stderr());
    ExitCode::from(u8::try_from(status).unwrap_or(1))
}
