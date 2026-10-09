#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

use cf_base::env::Env;

fn main() -> ExitCode {
    let env = Env::from_process();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let status = xtask::run(
        &env,
        &args,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    exit_code(status)
}

/// `status` as the process's exit code: a program's on Windows is more than a byte.
fn exit_code(status: i32) -> ExitCode {
    u8::try_from(status).map_or_else(|_| std::process::exit(status), ExitCode::from)
}
