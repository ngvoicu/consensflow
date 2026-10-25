//! The Windows machines a run can go to, and how ssh reaches the PowerShell of each.
//!
//! - **zeewin** (`ssh zeewin`, user `ngvoi`): Windows' own OpenSSH server, whose default shell
//!   is PowerShell and whose login folder is the profile. ssh hands it the line
//!   `powershell -NoProfile -NonInteractive -EncodedCommand …`, and what scp sends to a name
//!   lands in the profile. A machine named by `--host` is reached the same way.
//! - **zeewin2** (`ssh zeewin2`, Gabriel's laptop `DESKTOP-HK83944`, user `zee`): ssh lands in
//!   Ubuntu in WSL2 on that Windows, which has no SSH server of its own. WSL's interop starts
//!   Windows' programs from Linux, so the line is a shell's there: it goes to the profile
//!   (`C:\Users\AGV`, which WSL has at `/mnt/c/Users/AGV`) and starts Windows' PowerShell by its
//!   path, which runs as the user who is signed in at the desktop. scp sends to that folder by
//!   its WSL path, so the tarball is where the bootstrap looks for it, beside the copy
//!   (`C:\Users\AGV\consensflow-build`, on the Windows disk: never WSL's own, which Windows
//!   reads through a network share). Measured there on 2026-10-25: the status PowerShell ends
//!   with comes back as ssh's (`exit 7` is 7, where zeewin's shell makes it 1; the `== exit N`
//!   line is read either way), and a PowerShell goes on when its ssh is killed, as on zeewin,
//!   so a stop ends it there the same way.
//!
//! zeewin2 is poker-lab's machine too, and its jobs hold `~/.compile-lock` (a folder, in WSL)
//! while they run. A run neither takes it nor waits on it: a run whose line finds it there ends
//! at once with 75, as the machine's own lock refuses a run (its tarball removed, nothing
//! started), and the default ([`choose`]) goes to zeewin then. A poker-lab job that starts while
//! a run goes on is not seen: it is poker-lab's lock, which a run does not hold.
//!
//! The default, where neither `--machine` nor `--host` is given, is zeewin2 while it is free
//! (it answers, poker-lab does not hold it, and no run of this runner holds its copy), and else
//! zeewin, which is slower (7 GB of memory to zeewin2's 27).

use super::script::OWNER;
use crate::context::Context;
use crate::dispatch::Console;
use crate::process::{self, Invocation};

/// The machines `--machine` names.
pub const NAMES: [&str; 2] = ["zeewin2", "zeewin"];

/// A Windows machine, as a run reaches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// What the runner calls it.
    pub name: String,
    /// The ssh host it is.
    pub host: String,
    /// How ssh reaches its PowerShell.
    pub reach: Reach,
}

/// How ssh reaches the machine's PowerShell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Windows' own SSH server, whose shell is PowerShell and whose login folder is the profile.
    OpenSsh,
    /// Linux in WSL on the machine, which starts Windows' programs (WSL's interop).
    Wsl(Wsl),
}

/// Where a machine reached through WSL keeps what a run needs, as WSL's shell names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wsl {
    /// The Windows profile (`%USERPROFILE%`), where the copy and the tarballs are.
    pub profile: String,
    /// Windows' PowerShell.
    pub powershell: String,
    /// What, under the WSL user's home, says that another project's job (poker-lab's) holds the machine.
    pub busy: String,
    /// The Node the runs use (PowerShell's words): the machine's own in Program Files is not CI's.
    pub node: String,
}

/// Node's folder on a machine whose Node is the one in Program Files.
const PROGRAM_FILES_NODE: &str = r"C:\Program Files\nodejs";

impl Machine {
    /// The machine `--machine` calls `name`, if it is one.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "zeewin" => Some(zeewin()),
            "zeewin2" => Some(zeewin2()),
            _ => None,
        }
    }

    /// The machine `--host` names: one that Windows' own SSH server is, as zeewin.
    pub fn at(host: &str) -> Self {
        Self {
            name: host.into(),
            host: host.into(),
            reach: Reach::OpenSsh,
        }
    }

    /// The line ssh hands the machine for the PowerShell script `encoded` (`-EncodedCommand`'s).
    pub fn line(&self, encoded: &str) -> String {
        match &self.reach {
            Reach::OpenSsh => {
                format!("powershell -NoProfile -NonInteractive -EncodedCommand {encoded}")
            }
            Reach::Wsl(wsl) => format!(
                "cd {} && exec {} -NoProfile -NonInteractive -EncodedCommand {encoded}",
                sh_quoted(&wsl.profile),
                sh_quoted(&wsl.powershell)
            ),
        }
    }

    /// The line that starts a run, whose tarball was sent as `upload`: [`Machine::line`], but
    /// on a machine another project may hold, a run refused (75, its tarball removed) while it does.
    pub fn run_line(&self, encoded: &str, upload: &str) -> String {
        let line = self.line(encoded);
        match &self.reach {
            Reach::OpenSsh => line,
            Reach::Wsl(wsl) => {
                let said = format!(
                    "{} is busy: poker-lab's job holds it (~/{}), and no run goes while it does.",
                    self.name, wsl.busy
                );
                format!(
                    "if [ -e \"$HOME\"/{busy} ]; then rm -f -- {tarball}; printf '%s\\n' {said} '' '== exit 75'; exit 75; fi; {line}",
                    busy = sh_quoted(&wsl.busy),
                    tarball = sh_quoted(&format!("{}/{upload}", wsl.profile)),
                    said = sh_quoted(&said),
                )
            }
        }
    }

    /// Where scp sends a file that the scripts find in the profile as `name`.
    pub fn in_profile(&self, name: &str) -> String {
        match &self.reach {
            Reach::OpenSsh => format!("{}:{name}", self.host),
            Reach::Wsl(wsl) => format!("{}:{}/{name}", self.host, wsl.profile),
        }
    }

    /// The folder of the Node a run puts first on its PATH, in PowerShell's words.
    pub fn node(&self) -> &str {
        match &self.reach {
            Reach::OpenSsh => PROGRAM_FILES_NODE,
            Reach::Wsl(wsl) => &wsl.node,
        }
    }

    /// Whether a run in the copy `dir` may go to this machine now, as far as can be told before
    /// it is sent: a machine reached through WSL is asked; one that is not answers for itself
    /// when the run asks for its lock.
    fn free(&self, context: &Context, dir: &str) -> Free {
        let Reach::Wsl(wsl) = &self.reach else {
            return Free::Yes;
        };
        let line = format!(
            "if [ -e \"$HOME\"/{} ]; then echo busy; elif [ -e {} ]; then echo run; else echo free; fi",
            sh_quoted(&wsl.busy),
            sh_quoted(&format!("{}/{dir}/{OWNER}", wsl.profile))
        );
        let asked = Invocation::new("ssh", &context.root)
            .arg(&self.host)
            .arg(line);
        match process::capture(&asked, &context.env) {
            Ok(answer) if answer.code == 0 => match answer.stdout.trim() {
                "free" => Free::Yes,
                "busy" => Free::No(format!("poker-lab's job holds it (~/{})", wsl.busy)),
                "run" => Free::No("a run goes on there".into()),
                said => Free::No(format!("it answered {said:?}")),
            },
            Ok(answer) => Free::No(format!(
                "it did not answer (ssh ended with status {})",
                answer.code
            )),
            Err(failure) => Free::No(format!("it could not be asked: {failure}")),
        }
    }
}

/// zeewin, through Windows' own SSH server.
fn zeewin() -> Machine {
    Machine::at("zeewin")
}

/// zeewin2, through WSL on it.
fn zeewin2() -> Machine {
    Machine {
        name: "zeewin2".into(),
        host: "zeewin2".into(),
        reach: Reach::Wsl(Wsl {
            profile: "/mnt/c/Users/AGV".into(),
            powershell: "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe".into(),
            busy: ".compile-lock".into(),
            node: r"$env:USERPROFILE\cf-tools\node".into(),
        }),
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
    choose_between(context, dir, zeewin2(), zeewin(), console)
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

/// `text` as a POSIX shell takes it between single quotes.
fn sh_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests;
