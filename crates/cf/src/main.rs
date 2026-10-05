#![forbid(unsafe_code)]

use std::io::{self, ErrorKind, Write};
use std::process::ExitCode;

use cf_base::env::Env;

fn main() -> ExitCode {
    let env = Env::from_process();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    // A Codex window's supervisor reports from any of its threads: it runs
    // before this thread takes the standard streams' locks, which a panic
    // on another thread would then wait on for good.
    if let Some(code) = cf::codex_session(&env, &args) {
        return exit_code(code);
    }
    // The daemon does the same, behind the switch: it reads and writes the
    // standard streams from threads of its own.
    if let Some(code) = cf::native_ui(&env, &args) {
        return exit_code(code);
    }
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

/// `code` as the process's exit code: Windows exit codes are more than a byte.
fn exit_code(code: i32) -> ExitCode {
    u8::try_from(code).map_or_else(|_| std::process::exit(code), ExitCode::from)
}
