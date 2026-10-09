//! A stand-in for a daemon that is not the one it was asked to be: it writes the
//! start line of Node's daemon in the log and prints a handle line, the two
//! things a rig that was asked for the native daemon looks at, and then waits to
//! be ended (by its input ending). `tests/rig/daemon_seam.rs` has the rig refuse
//! it.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::process::ExitCode;

use cf_e2e::process::own_var;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failed) => {
            let _ = writeln!(io::stderr(), "liar-daemon: {failed}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let home = own_var("CONSENSFLOW_HOME")
        .ok_or_else(|| io::Error::other("CONSENSFLOW_HOME is not set"))?;
    fs::create_dir_all(&home)?;
    let at = jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ");
    let mut log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(std::path::Path::new(&home).join("daemon.log"))?;
    writeln!(
        log,
        "{at} info start pid {} node v0.0.0 home {home}",
        std::process::id()
    )?;
    let mut out = io::stdout().lock();
    writeln!(out, r#"{{"url":"http://127.0.0.1:1/","token":"none"}}"#)?;
    out.flush()?;
    // Waits to be ended: its input ends when the rig ends it.
    io::copy(&mut io::stdin().lock(), &mut io::sink())?;
    Ok(())
}
