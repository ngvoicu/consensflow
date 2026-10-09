#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

use cf_base::env::Env;

fn main() -> ExitCode {
    let env = Env::from_process();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let status = cf_release::run(
        &env,
        &args,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(status)
}
