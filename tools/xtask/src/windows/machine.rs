//! The Windows machines a run can go to. Both are reached alike: Windows' own OpenSSH server,
//! whose default shell is Windows PowerShell 5.1 and whose login folder is the profile, so ssh
//! hands each the line `powershell -NoProfile -NonInteractive -EncodedCommand …` and what scp
//! sends to a name lands in the profile, beside the copy.
//!
//! - **zeewin** (`ssh zeewin`, 10.6.0.10, user `ngvoi`): the old PC, 7 GB of memory. A machine
//!   named by `--host` is reached as it is.
//! - **zeewin2** (`ssh zeewin2`, 10.6.0.6 port 2222, user `AGV`): Gabriel's laptop
//!   (`DESKTOP-HK83944`, 16 threads, 31 GB), its copy `C:\Users\AGV\consensflow-build`. Its runs
//!   put CI's Node (`C:\Users\AGV\cf-tools\node`) first on PATH, not the laptop's own. Its ssh
//!   session has the user's full administrator token and `SeDebugPrivilege` enabled, as zeewin's
//!   has: the scripts are the same on both and rely on nothing either lacks.
//!
//! The laptop is poker-lab's too: its jobs hold `~/.compile-lock` (with an `owner` file in it)
//! in the laptop's Ubuntu, `zeelin` (10.6.0.6 port 22, in WSL2), while they run. So before a run
//! is sent there zeelin is asked, with a short timeout ([`ZEELIN_TIMEOUT_S`]), whether the owner
//! file is there: a run that finds it ends at once with 75, as the machine's own lock refuses a
//! run, having sent nothing. A zeelin that does not answer is an Ubuntu that is not running, and
//! no poker-lab job can hold a lock in it: that is said, and the run goes on. A run neither takes
//! the lock nor waits on it, and a poker-lab job that starts while a run goes on is not seen.
//!
//! The laptop's C: holds Gabriel's own files and WSL's disk: a run there is sent only while its
//! profile's disk has [`FLOOR_GIB`] free, as the laptop's PowerShell says it, and is refused with
//! 75 below it.
//!
//! The default, where neither `--machine` nor `--host` is given, is zeewin2 while it is free
//! (poker-lab does not hold it, its disk has the floor free, and no run of this runner holds its
//! copy), and else zeewin.

use super::script::{encode_script, OWNER};
use crate::context::Context;
use crate::dispatch::Console;
use crate::process::{self, Invocation};

/// The machines `--machine` names.
pub const NAMES: [&str; 2] = ["zeewin2", "zeewin"];

/// How much of zeewin2's C: must be free for a run to be sent, in GiB. It is Gabriel's laptop,
/// and a run's build is tens of GB (zeewin's build folder held 64 GB of `debug` a day after it was
/// emptied, 2026-10-25): a run that filled the disk would stop the laptop, and poker-lab's jobs,
/// whose WSL disk grows on the same C:.
const FLOOR_GIB: u64 = 50;

/// How long zeelin is waited on to answer, in seconds: it answers only while WSL runs.
pub const ZEELIN_TIMEOUT_S: u32 = 5;

/// A Windows machine, as a run reaches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// What the runner calls it.
    pub name: String,
    /// The ssh host it is.
    pub host: String,
    /// The folder of the Node a run puts first on its PATH, in PowerShell's words.
    pub node: String,
    /// What a run must find before it is sent, on a machine that is not the runner's alone.
    pub shared: Option<Shared>,
}

/// A machine another project uses too, and whose disk is its owner's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    /// The ssh host of the Linux beside it, where the other project's lock is.
    pub linux: String,
    /// The file, from that Linux user's home, that says the other project's job holds the machine.
    pub lock: String,
    /// How much of the profile's disk must be free for a run to be sent, in GiB.
    pub floor_gib: u64,
}

/// Node's folder on a machine whose Node is the one in Program Files.
const PROGRAM_FILES_NODE: &str = r"C:\Program Files\nodejs";

impl Machine {
    /// The machine `--machine` calls `name`, if it is one.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "zeewin" => Some(Self::at("zeewin")),
            "zeewin2" => Some(Self {
                name: name.into(),
                host: name.into(),
                node: r"$env:USERPROFILE\cf-tools\node".into(),
                shared: Some(Shared {
                    linux: "zeelin".into(),
                    lock: ".compile-lock/owner".into(),
                    floor_gib: FLOOR_GIB,
                }),
            }),
            _ => None,
        }
    }

    /// The machine `--host` names, reached as zeewin is.
    pub fn at(host: &str) -> Self {
        Self {
            name: host.into(),
            host: host.into(),
            node: PROGRAM_FILES_NODE.into(),
            shared: None,
        }
    }

    /// Whether a run may be sent now, for the copy `dir`: on a machine the runner shares, why
    /// not if it may not ([`Ready::Refused`], which the run ends with 75), and what was found
    /// that the person is told of if it may. A machine that could not be asked is the error.
    pub fn ready(&self, context: &Context, dir: &str) -> Result<Ready, String> {
        Ok(self.look(context, dir)?.0)
    }

    /// [`Machine::ready`], and whether a run of this runner holds the copy (none is known of a
    /// machine the runner does not share, or that is refused).
    fn look(&self, context: &Context, dir: &str) -> Result<(Ready, bool), String> {
        let Some(shared) = &self.shared else {
            return Ok((Ready::Go(Vec::new()), false));
        };
        let mut said = Vec::new();
        match lock_of(context, shared) {
            Lock::Held => {
                let why = format!(
                    "{} is busy: poker-lab's job holds it (~/{} on {}), and no run goes while it does.",
                    self.name, shared.lock, shared.linux
                );
                return Ok((Ready::Refused(why), false));
            }
            Lock::Free => {}
            Lock::NoAnswer(why) => said.push(format!(
                "{} did not answer ({why}), so Ubuntu is not running there and no poker-lab job can hold its lock: going on",
                shared.linux
            )),
        }
        let state = self.state(context, dir)?;
        if state.free_gib < shared.floor_gib {
            let why = format!(
                "{} is short of disk: a run needs {} GiB free there, and it has {}.",
                self.name, shared.floor_gib, state.free_gib
            );
            return Ok((Ready::Refused(why), false));
        }
        Ok((Ready::Go(said), state.running))
    }

    /// What the machine says of its profile's disk and of a run in the copy `dir`.
    fn state(&self, context: &Context, dir: &str) -> Result<State, String> {
        let script = state_script(dir);
        let line = format!(
            "powershell -NoProfile -NonInteractive -EncodedCommand {}",
            encode_script(&script).map_err(|refusal| refusal.to_string())?
        );
        let asked = Invocation::new("ssh", &context.root)
            .arg(&self.host)
            .arg(line);
        let answer =
            process::capture(&asked, &context.env).map_err(|failure| failure.to_string())?;
        if answer.code != 0 {
            return Err(format!(
                "{} did not say what it has free (ssh ended with status {}{})",
                self.name,
                answer.code,
                said_as(&answer.stderr)
            ));
        }
        State::read(&answer.stdout).ok_or_else(|| {
            format!(
                "{} did not say what it has free: {:?}",
                self.name, answer.stdout
            )
        })
    }

    /// Whether this machine is free for a run in the copy `dir`: [`Machine::ready`], and no run of
    /// this runner holding its copy.
    fn free(&self, context: &Context, dir: &str) -> Free {
        match self.look(context, dir) {
            Ok((Ready::Go(_), false)) => Free::Yes,
            Ok((Ready::Go(_), true)) => Free::No("a run goes on there".into()),
            Ok((Ready::Refused(why), _)) | Err(why) => Free::No(why),
        }
    }
}

/// Whether a run may be sent to a machine now.
#[derive(Debug, PartialEq, Eq)]
pub enum Ready {
    /// It may: with what is to be said of it on the error stream.
    Go(Vec<String>),
    /// It may not, and why: the run ends with 75, having sent nothing.
    Refused(String),
}

/// What the other project's lock is, as its Linux says.
#[derive(Debug, PartialEq, Eq)]
enum Lock {
    Held,
    Free,
    /// The Linux did not answer, and why: it is not running.
    NoAnswer(String),
}

/// Asks `shared`'s Linux whether its lock is held, waiting [`ZEELIN_TIMEOUT_S`] for it.
fn lock_of(context: &Context, shared: &Shared) -> Lock {
    let timeout = format!("ConnectTimeout={ZEELIN_TIMEOUT_S}");
    let asked = Invocation::new("ssh", &context.root)
        .args(["-o", &timeout, "-o", "BatchMode=yes"])
        .args(["-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=1"])
        .arg(&shared.linux)
        .arg(format!(
            "if [ -e ~/{} ]; then echo held; else echo free; fi",
            shared.lock
        ));
    match process::capture(&asked, &context.env) {
        Ok(answer) if answer.code == 0 => match answer.stdout.trim() {
            "held" => Lock::Held,
            "free" => Lock::Free,
            said => Lock::NoAnswer(format!("it said {said:?}")),
        },
        Ok(answer) => Lock::NoAnswer(format!(
            "ssh ended with status {}{}",
            answer.code,
            said_as(&answer.stderr)
        )),
        Err(failure) => Lock::NoAnswer(failure.to_string()),
    }
}

/// What a machine said of itself.
#[derive(Debug, PartialEq, Eq)]
struct State {
    /// What its profile's disk has free, in whole GiB.
    free_gib: u64,
    /// Whether a run of this runner holds the copy (its owner file is there).
    running: bool,
}

impl State {
    /// The state `text` says: a line `free N`, and a line `run` if a run holds the copy.
    fn read(text: &str) -> Option<Self> {
        let free_gib = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("free ")?.trim().parse().ok())?;
        let running = text.lines().any(|line| line.trim() == "run");
        Some(Self { free_gib, running })
    }
}

/// The script that says what the machine's profile's disk has free (`free N`, whole GiB) and, if
/// a run holds the copy `dir` (its owner file, [`OWNER`], is there), `run`.
pub fn state_script(dir: &str) -> String {
    format!(
        "$ProgressPreference = 'SilentlyContinue'\n\
         \"free $([math]::Floor((Get-Item $env:USERPROFILE).PSDrive.Free / 1GB))\"\n\
         if (Test-Path -LiteralPath (Join-Path (Join-Path $env:USERPROFILE '{dir}') '{OWNER}')) {{ 'run' }}\n"
    )
}

/// What a program said on its error stream, after a colon, or nothing.
fn said_as(stderr: &str) -> String {
    let said = stderr.trim();
    if said.is_empty() {
        String::new()
    } else {
        format!(": {said}")
    }
}

/// Whether a machine is free, and why not.
#[derive(Debug, PartialEq, Eq)]
enum Free {
    Yes,
    No(String),
}

/// The machine a run in the copy `dir` goes to when none is named: zeewin2 if it is free, else
/// zeewin, whatever it is (it refuses a run itself while one goes on). Which, and why, is said on
/// the error stream.
pub fn choose(context: &Context, dir: &str, console: &mut Console) -> std::io::Result<Machine> {
    let (Some(first), Some(other)) = (Machine::named("zeewin2"), Machine::named("zeewin")) else {
        return Err(std::io::Error::other("the machines are not named"));
    };
    choose_between(context, dir, first, other, console)
}

/// [`choose`] between `first`, if it is free, and `other`.
fn choose_between(
    context: &Context,
    dir: &str,
    first: Machine,
    other: Machine,
    console: &mut Console,
) -> std::io::Result<Machine> {
    let (machine, why) = match first.free(context, dir) {
        Free::Yes => (first, String::new()),
        Free::No(why) => (other, format!(", as {} is not free: {why}", first.name)),
    };
    writeln!(console.err, "windows: on {}{why}", machine.name)?;
    Ok(machine)
}

#[cfg(test)]
mod tests;
