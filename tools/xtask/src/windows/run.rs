//! One run on the machine, from this checkout as it is now to the command's own
//! status: the tree sent over as a tarball (`tree`), the script that unpacks it and
//! runs the command there (`script`) handed to ssh, and what it says brought back
//! as it comes (`session`). Nothing is asked of the machine before every word of
//! the run has been read and the script it makes is known to fit.
//!
//! A run that is stopped here is ended there: ssh is ended, and a second ssh ends
//! the run's whole tree on the machine and takes its record away, which Windows'
//! SSH server does not do when the connection goes. A stop heard before ssh has
//! started ends nothing there, for nothing has started.

use super::interrupt::Interrupt;
use super::options::Options;
use super::script::{
    command_line, encode_script, remote_script, stop_script, Refusal, Script, BUILD_STEPS,
};
use super::session::{self, Ended};
use super::{tree, Error};
use crate::context::Context;
use crate::dispatch::Console;
use crate::process::{self, Invocation};

/// How a run ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The command ended there with this status, which is the run's own.
    Exited(i32),
    /// This program was stopped, and the run was ended there as well.
    Stopped,
}

/// What ssh is given to run on the machine: PowerShell, the script encoded.
struct Plan {
    /// The run itself.
    run: String,
    /// What ends it.
    stop: String,
}

impl Plan {
    fn new(options: &Options) -> Result<Self, Refusal> {
        let command = command_line(&options.command);
        let steps: &[&str] = if options.build { &BUILD_STEPS } else { &[] };
        let run = remote_script(&Script {
            dir: &options.dir,
            inside: &options.inside,
            steps,
            command: &command,
            env: &options.env,
        });
        Ok(Self {
            run: powershell(&run)?,
            stop: powershell(&stop_script(&options.dir))?,
        })
    }
}

/// The line ssh hands the machine's shell for `script`.
pub(super) fn powershell(script: &str) -> Result<String, Refusal> {
    Ok(format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        encode_script(script)?
    ))
}

/// Whether the script a run makes can be sent: the refusal that says why not, before a run starts.
pub fn check(options: &Options) -> Result<(), Refusal> {
    Plan::new(options).map(drop)
}

/// ssh to `host`, asked to run `line` there.
pub(super) fn ssh(context: &Context, host: &str, line: &str) -> Invocation {
    Invocation::new("ssh", &context.root).arg(host).arg(line)
}

/// Runs `options.command` on the machine from this checkout as it is now; what it
/// says goes to `console` as it says it.
pub fn run(
    context: &Context,
    options: &Options,
    console: &mut Console,
    interrupt: &Interrupt,
) -> Result<Outcome, Error> {
    let plan = Plan::new(options)?;
    let scratch = tempfile::tempdir().map_err(Error::Scratch)?;

    // A stop is looked for after each step, before what it says is: a Ctrl-C ends
    // the step that is running (it is in the terminal's group) with a failure that
    // is the stop's doing.
    let listed = tree::files(context);
    if stopped(interrupt, listed.is_err()) {
        return Ok(Outcome::Stopped);
    }
    let packed = tree::pack(context, &listed?, scratch.path());
    if stopped(interrupt, packed.is_err()) {
        return Ok(Outcome::Stopped);
    }
    let tarball = packed?;
    let scp = Invocation::new("scp", &context.root)
        .arg("-q")
        .arg(&tarball)
        .arg(format!("{}:{}.tgz", options.host, options.dir));
    let sent = process::capture(&scp, &context.env);
    if stopped(interrupt, !matches!(&sent, Ok(done) if done.code == 0)) {
        return Ok(Outcome::Stopped);
    }
    // scp is run quiet, so what it says is what is wrong: said as the run's own words.
    let sent = sent?;
    console
        .out
        .write_all(sent.stdout.as_bytes())
        .and_then(|()| console.err.write_all(sent.stderr.as_bytes()))
        .map_err(Error::Output)?;
    if sent.code != 0 {
        return Err(Error::step(&scp, sent.code, ""));
    }

    let run = ssh(context, &options.host, &plan.run);
    match session::stream(&run, &context.env, console, interrupt)? {
        Ended::Exited(status) => Ok(Outcome::Exited(status)),
        Ended::Stopped => {
            end_there(context, options, &plan, console)?;
            Ok(Outcome::Stopped)
        }
        Ended::Unwritten(cause) => {
            end_there(context, options, &plan, console)?;
            Err(Error::Output(cause))
        }
    }
}

/// Whether this program was stopped. After a step that failed it is waited on a
/// moment: what the terminal's Ctrl-C ended may be seen to have ended before
/// this program has heard the same Ctrl-C.
fn stopped(interrupt: &Interrupt, failed: bool) -> bool {
    if failed {
        interrupt.heard_after_failure()
    } else {
        interrupt.heard()
    }
}

/// Ends the run on the machine: its whole tree, and its record.
fn end_there(
    context: &Context,
    options: &Options,
    plan: &Plan,
    console: &mut Console,
) -> Result<(), Error> {
    let stop = ssh(context, &options.host, &plan.stop);
    match process::run(&stop, &context.env)? {
        0 => Ok(()),
        status => writeln!(
            console.err,
            "windows: the run may still be going on the machine: ssh ended with status {status} when asked to end it"
        )
        .map_err(Error::Output),
    }
}

#[cfg(test)]
mod tests;
