//! The commands not answered here yet: the CLI's Node sources beside this
//! binary (`cf.mjs`), on the runtime the app names in `CONSENSFLOW_NODE`.

use std::ffi::OsString;
use std::io::{self, Write};

use cf_base::env::Env;

/// Runs `cf.mjs` with `args` in this process's place: an exit code only
/// when it could not be run.
pub fn run(env: &Env, args: &[OsString], err: &mut dyn Write) -> io::Result<u8> {
    let script = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name("cf.mjs"),
        Err(cause) => {
            writeln!(err, "cf: cannot tell which folder it is in: {cause}")?;
            return Ok(1);
        }
    };
    // The app names its runtime when it opens a pane. A `node` off PATH could
    // be any Node, the one an older install left first on it.
    let Some(node) = env.os("CONSENSFLOW_NODE").filter(|node| !node.is_empty()) else {
        writeln!(
            err,
            "cf: CONSENSFLOW_NODE is not set: this command runs ConsensFlow's own runtime, which \
             the app sets when it opens a pane. Open a pane from the app, or run {} with a node \
             of your choosing.",
            script.display()
        )?;
        return Ok(1);
    };
    let mut command = vec![script.into_os_string()];
    command.extend(args.iter().cloned());
    let cause = cf_process::run_in_place(node, &command);
    writeln!(err, "cf: {} did not start: {cause}", node.to_string_lossy())?;
    Ok(1)
}
