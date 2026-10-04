use std::io::{self, ErrorKind, Write};
use std::process::ExitCode;

use cf_base::env::Env;

fn main() -> ExitCode {
    let env = Env::from_process();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let ran = cf::run(
        &env,
        &args,
        &mut io::stdin().lock(),
        &mut out,
        &mut io::stderr().lock(),
    )
    .and_then(|code| out.flush().map(|()| code));
    match ran {
        Ok(code) => ExitCode::from(code),
        // `cf … | head` closes the pipe mid-stream: an error for that would be
        // a crash where a quiet exit is the whole contract of a CLI.
        Err(cause) if cause.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(cause) => {
            let _ = writeln!(io::stderr(), "cf: {cause}");
            ExitCode::FAILURE
        }
    }
}
