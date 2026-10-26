//! One run on the machine, from this checkout as it is now to the command's own
//! status: the tree sent over as a tarball (`tree`) of the run's own name, the script
//! that unpacks that tarball and runs the command there (`script`) handed to ssh, and
//! what it says brought back as it comes (`session`). Nothing is asked of the machine
//! before every word of the run has been read and the script it makes is known to fit.
//!
//! The machine is one folder and one lock, and several runs from several trees queue
//! for it: each run's tarball is named by the run ([`script::run_id`]) and the script
//! unpacks that one, so that no run unpacks what another sent, and the machine says
//! which tree it unpacked, which must be the one this run sent.
//!
//! A run that is stopped here is ended there: ssh is ended, and a second ssh ends
//! the run's whole tree on the machine and takes its record away, which Windows'
//! SSH server does not do when the connection goes. A stop heard before ssh has
//! started ends nothing there, for nothing has started.

use super::interrupt::Interrupt;
use super::machine::{Machine, Ready};
use super::options::Options;
use super::script::{
    command_line, encode_script, remote_script, run_id, run_script, stop_script, upload_name,
    Refusal, Script, BUILD_STEPS, TREE_MARK,
};
use super::session::{self, Ended};
use super::{tree, Ending, Error};
use crate::context::Context;
use crate::dispatch::Console;
use crate::process::{self, Invocation};

/// How a run ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The command ended there with this status, which is the run's own.
    Exited(i32),
    /// The machine may not take a run now, and why (the person is told): nothing was sent,
    /// and the run answers [`REFUSED`], as the machine's own lock refuses one.
    Refused(String),
    /// This program was stopped, and the run was ended there as well.
    Stopped,
}

/// What a run is made of: what ssh is given to run on the machine (PowerShell, the
/// bootstrap encoded), what goes in the tarball for it to read, and what ends it.
struct Plan {
    /// Where the tarball is sent, as scp names it.
    upload: String,
    /// The run itself: the bootstrap.
    run: String,
    /// What the run is asked for (the steps, the variables, the command): the script the
    /// bootstrap reads from the tarball.
    script: String,
    /// What ends it.
    stop: String,
}

impl Plan {
    /// The run `id` of `options` on `machine`.
    fn new(machine: &Machine, options: &Options, id: &str) -> Result<Self, Refusal> {
        let command = command_line(&options.command);
        let steps: &[&str] = if options.build { &BUILD_STEPS } else { &[] };
        let script = Script {
            id,
            dir: &options.dir,
            inside: &options.inside,
            steps,
            command: &command,
            env: &options.env,
            node: &machine.node,
        };
        let upload = upload_name(&options.dir, id);
        Ok(Self {
            run: powershell(&remote_script(&script))?,
            script: run_script(&script),
            stop: powershell(&stop_script(&options.dir, id))?,
            upload: format!("{}:{upload}", machine.host),
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

/// The status of a run the machine could not take now, as its own lock refuses one.
pub const REFUSED: i32 = 75;

/// Whether the script a run makes can be sent: the refusal that says why not, before a run
/// starts and before a machine is chosen. What goes on the line, the bootstrap and the stop, is
/// the same for every machine (the Node's folder is the run script's, in the tarball).
pub fn check(options: &Options) -> Result<(), Refusal> {
    Plan::new(&Machine::at("zeewin"), options, &run_id()).map(drop)
}

/// ssh to `host`, asked to run `line` there.
pub(super) fn ssh(context: &Context, host: &str, line: &str) -> Invocation {
    Invocation::new("ssh", &context.root).arg(host).arg(line)
}

/// Runs `options.command` on `machine` from this checkout as it is now; what it
/// says goes to `console` as it says it.
pub fn run(
    context: &Context,
    machine: &Machine,
    options: &Options,
    console: &mut Console,
    interrupt: &Interrupt,
) -> Result<Outcome, Error> {
    let id = run_id();
    let plan = Plan::new(machine, options, &id)?;
    // A machine the runner shares is asked first, and a run it may not take now is not sent. A
    // Ctrl-C ends the probes with this program (they are in the terminal's group), so a stop is
    // looked for before what they came to is read: a probe it ended is no zeelin that did not
    // answer, no disk not said, and no refusal.
    let ready = machine.ready(context, &options.dir);
    if stopped(interrupt, !matches!(ready, Ok(Ready::Go(_)))) {
        return Ok(Outcome::Stopped);
    }
    match ready.map_err(Error::Unready)? {
        Ready::Refused(why) => {
            writeln!(console.out, "{why}").map_err(Error::Output)?;
            return Ok(Outcome::Refused(why));
        }
        Ready::Go(said) => {
            for line in said {
                writeln!(console.err, "windows: {line}").map_err(Error::Output)?;
            }
        }
    }
    let scratch = tempfile::tempdir().map_err(Error::Scratch)?;

    // A stop is looked for after each step, before what it says is: a Ctrl-C ends
    // the step that is running (it is in the terminal's group) with a failure that
    // is the stop's doing.
    let listed = tree::files(context);
    if stopped(interrupt, listed.is_err()) {
        return Ok(Outcome::Stopped);
    }
    let listed = listed?;
    let identity = tree::identity(context, &listed);
    if stopped(interrupt, identity.is_err()) {
        return Ok(Outcome::Stopped);
    }
    let identity = identity?;
    let packed = tree::pack(context, &listed, &identity, &plan.script, scratch.path());
    if stopped(interrupt, packed.is_err()) {
        return Ok(Outcome::Stopped);
    }
    let tarball = packed?;
    let scp = Invocation::new("scp", &context.root)
        .arg("-q")
        .arg(&tarball)
        .arg(&plan.upload);
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

    let run = ssh(context, &machine.host, &plan.run);
    let tree_said = format!("{TREE_MARK}{identity}");
    match session::stream(&run, &context.env, console, interrupt, &tree_said)? {
        Ended::Exited(status) => Ok(Outcome::Exited(status)),
        Ended::WrongTree(said) => {
            let ended = end_there(context, machine, &plan)?;
            Err(Error::WrongTree {
                sent: tree_said,
                said,
                ended,
            })
        }
        Ended::Stopped => {
            say_if_not_ended(end_there(context, machine, &plan)?, console)?;
            Ok(Outcome::Stopped)
        }
        Ended::Unwritten(cause) => {
            say_if_not_ended(end_there(context, machine, &plan)?, console)?;
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

/// Ends the run on the machine: its whole tree, and its record. Whether the machine
/// was asked and said it was done.
fn end_there(context: &Context, machine: &Machine, plan: &Plan) -> Result<Ending, Error> {
    let stop = ssh(context, &machine.host, &plan.stop);
    Ok(match process::run(&stop, &context.env)? {
        0 => Ending::Ended,
        status => Ending::MayGoOn(status),
    })
}

/// Says on the error stream that the run may still be going on the machine.
fn say_if_not_ended(ending: Ending, console: &mut Console) -> Result<(), Error> {
    match ending {
        Ending::Ended => Ok(()),
        Ending::MayGoOn(_) => writeln!(console.err, "windows: {ending}").map_err(Error::Output),
    }
}

#[cfg(test)]
mod tests;
