//! The stand-in for the `cf` a member's window runs, where the real one would
//! have no daemon to reach: it writes down the words it was given in the log
//! beside its own file, and says it is a stand-in (`cf_e2e::live::calls` has what
//! it writes and what it says). A check that asks a window to run `cf ask` reads
//! the log, which a call that ran left and one that was refused did not.
//!
//! A program of the suites' own, and no script, put first on the windows' `PATH` under
//! the name `cf`: every shell a window's agent may run its commands in finds a
//! program (Devin's on Windows is Git Bash), where a `.cmd` is found by some of them
//! only.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::process::ExitCode;

use cf_e2e::live::calls;

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    match note(&words) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failed) => {
            let _ = writeln!(io::stderr(), "stand-in cf: {failed}");
            ExitCode::FAILURE
        }
    }
}

/// Writes the call down in the log beside this program, and answers it.
fn note(words: &[String]) -> io::Result<()> {
    let log = std::env::current_exe()?.with_file_name(calls::LOG);
    let at = jiff::Timestamp::now().as_millisecond();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?
        .write_all(calls::line(at, words).as_bytes())?;
    if let Some(said) = calls::answer(words) {
        writeln!(io::stdout(), "{said}")?;
    }
    Ok(())
}
