//! `cf` in a pane on Windows: ConsensFlow's CLI, the `cf.mjs` beside this
//! file, run on the runtime the app names in `CONSENSFLOW_NODE`, with its
//! arguments as they came. The `.cmd` it replaces ran through cmd.exe, which
//! ends a command at its first line break: a question or a brief of many
//! lines arrived cut to its first, and `cf` said it was sent.

use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    // The app names its runtime when it opens a pane. A `node` off PATH could
    // be any Node, the one an older install left (see bin/cf).
    let Some(node) = env::var_os("CONSENSFLOW_NODE").filter(|node| !node.is_empty()) else {
        eprintln!(
            "cf: CONSENSFLOW_NODE is not set - this launcher runs ConsensFlow's own runtime, \
             which the app sets when it opens a pane. Open a pane from the app, or run \
             bin\\cf.mjs with a node of your choosing."
        );
        return ExitCode::FAILURE;
    };
    let script = match env::current_exe() {
        Ok(exe) => exe.with_file_name("cf.mjs"),
        Err(cause) => {
            eprintln!("cf: cannot tell which folder it is in: {cause}");
            return ExitCode::FAILURE;
        }
    };
    match Command::new(&node)
        .arg(script)
        .args(env::args_os().skip(1))
        .status()
    {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(cause) => {
            eprintln!("cf: {} did not start: {cause}", node.to_string_lossy());
            ExitCode::FAILURE
        }
    }
}
