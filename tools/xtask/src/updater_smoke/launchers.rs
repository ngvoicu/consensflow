//! The terminal's command (`cf` and `consensflow` in the `bin` of a ConsensFlow
//! home), as an installed app's `cf setup` writes it and as the app that
//! replaces it repairs it at its start. The installed app is the bridge, whose
//! `cf setup` is Node's and writes a command that names the bundled Node and
//! `cf.mjs`; or the flip release, whose `cf setup` is the native `cf`'s and
//! writes a command that names the `cf` of its bundle, the very path the update's
//! `cf` is at. What the repair does of them, and what it does not, is the
//! contract (`cf_launcher::repair`, app/src-tauri/src/launcher.rs):
//!
//! - a command that serves the app's own home and does not run the bundle's
//!   `cf` is rewritten to run it, keeping the home it pins: both names of it;
//! - one that already runs it is left as it is, and is not spoken of;
//! - one that serves another home is left as it is, byte for byte, whether it sits
//!   in this home's `bin` or in the other home's own.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::build::Release;
use super::bundle::{under, BundleInfo};
use super::evidence::{tail, Kind};
use super::sandbox::Sandbox;
use super::{files, Result};
use crate::process::{self, Invocation};

const MARKER: &str = "Installed by ConsensFlow";
const NAMES: [&str; 2] = ["cf", "consensflow"];

fn bin_of(home: &Path) -> PathBuf {
    home.join("bin")
}

fn file_of(home: &Path, name: &str) -> PathBuf {
    bin_of(home).join(name)
}

/// The text of each command of a home, by name.
pub type Commands = BTreeMap<&'static str, String>;

/// What a run of a program in the machine's `probe` folder, as a terminal on `home`, answers.
fn terminal(
    sandbox: &Sandbox,
    home: &Path,
    program: &Path,
    args: &[&str],
) -> Result<process::Captured> {
    let invocation = Invocation::new(program, &sandbox.probe).args(args.iter().copied());
    Ok(process::capture(&invocation, &sandbox.terminal_env(home))?)
}

/// Runs the installed app's own `cf setup` (Node's in the bridge, the native `cf`'s in the flip) as a terminal does, on `home`.
fn set_up(installed: &BundleInfo, sandbox: &Sandbox, home: &Path, release: Release) -> Result {
    let ran = match release.setup() {
        Kind::Native => terminal(sandbox, home, &installed.cf, &["setup"])?,
        Kind::Node => {
            let node = under(&installed.app, &["Contents", "MacOS", "node"]);
            let entry = under(
                &installed.app,
                &["Contents", "Resources", "cli", "bin", "cf.mjs"],
            );
            let invocation = Invocation::new(&node, &sandbox.probe)
                .arg(&entry)
                .arg("setup");
            process::capture(&invocation, &sandbox.terminal_env(home))?
        }
    };
    ensure!(
        ran.code == 0,
        "cf setup of the {} release failed on {}: {}",
        release.name(),
        home.display(),
        ran.stderr.trim()
    );
    Ok(())
}

/// What a terminal's command `name` of `home` says when it is asked for its version.
pub fn version_of(sandbox: &Sandbox, home: &Path, name: &str) -> Result<String> {
    let ran = terminal(sandbox, home, &file_of(home, name), &["--version"])?;
    ensure!(
        ran.code == 0,
        "{} --version failed: {}",
        file_of(home, name).display(),
        ran.stderr.trim()
    );
    Ok(ran.stdout.trim().to_string())
}

/// The text of each command of `home`, by name.
pub fn commands_of(home: &Path) -> Result<Commands> {
    NAMES
        .iter()
        .map(|name| {
            let file = file_of(home, name);
            fs::read_to_string(&file)
                .map(|text| (*name, text))
                .map_err(files("read", &file))
        })
        .collect()
}

/// What `plant_commands` left on a machine.
#[derive(Debug, Clone)]
pub struct Planted {
    /// The commands of this home.
    pub own: Commands,
    /// The commands of the other home.
    pub other: Commands,
    /// The commands of this home that serve this home, which the app that replaces this one is to look at.
    pub repaired: Vec<&'static str>,
}

/// Whether `text` begins like a version, `1.2.3`, which may be followed by
/// anything (`3.0.0-alpha.81`).
fn is_a_version(text: &str) -> bool {
    let mut rest = text;
    for at in 0..3 {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return false;
        }
        rest = &rest[digits..];
        if at < 2 {
            match rest.strip_prefix('.') {
                Some(after) => rest = after,
                None => return false,
            }
        }
    }
    true
}

/// The commands the app that is installed leaves on a machine: this home's, and
/// another home's (set up as the user of a second copy would). With `elsewhere`,
/// one name of this home's is replaced by the other home's command, which serves
/// another home from where this home's app looks; `repaired` names the commands
/// that serve this home, which the app that replaces this one is to look at. Each
/// is what the installed release's `cf setup` wrote: it runs the bundled Node and
/// its `cf.mjs` (the bridge's), or the bundle's native `cf` (the flip's).
pub fn plant_commands(
    installed: &BundleInfo,
    sandbox: &Sandbox,
    elsewhere: bool,
    release: Release,
) -> Result<Planted> {
    set_up(installed, sandbox, &sandbox.state, release)?;
    set_up(installed, sandbox, &sandbox.other, release)?;
    if elsewhere {
        let from = file_of(&sandbox.other, "consensflow");
        let to = file_of(&sandbox.state, "consensflow");
        fs::copy(&from, &to).map_err(files("copy the command to", &to))?;
    }
    let planted = Planted {
        own: commands_of(&sandbox.state)?,
        other: commands_of(&sandbox.other)?,
        repaired: if elsewhere {
            vec!["cf"]
        } else {
            NAMES.to_vec()
        },
    };
    let node = under(&installed.app, &["Contents", "MacOS", "node"]);
    let entry = under(
        &installed.app,
        &["Contents", "Resources", "cli", "bin", "cf.mjs"],
    );
    let (line, what) = match release.setup() {
        Kind::Native => (
            format!("exec \"{}\" \"$@\"", installed.cf.display()),
            "the installed app's cf",
        ),
        Kind::Node => (
            format!("exec \"{}\" \"{}\" \"$@\"", node.display(), entry.display()),
            "the installed app's Node and cf.mjs",
        ),
    };
    let served = planted
        .repaired
        .iter()
        .map(|name| (&sandbox.state, *name, &planted.own[name]))
        .chain(
            NAMES
                .iter()
                .map(|name| (&sandbox.other, *name, &planted.other[name])),
        );
    for (home, name, text) in served {
        let file = file_of(home, name).display().to_string();
        ensure!(text.contains(MARKER), "{file} is not ours");
        ensure!(text.contains(&line), "{file} does not run {what}:\n{text}");
        ensure!(
            text.contains(&format!("export CONSENSFLOW_HOME=\"{}\"", home.display())),
            "{file} pins no {}",
            home.display()
        );
        ensure!(
            is_a_version(&version_of(sandbox, home, name)?),
            "{file} does not run"
        );
    }
    // What this home's bin holds that serves the other home: pinned to it.
    for name in NAMES.iter().filter(|name| !planted.repaired.contains(name)) {
        ensure!(
            planted.own[name].contains(&format!(
                "export CONSENSFLOW_HOME=\"{}\"",
                sandbox.other.display()
            )),
            "{name} pins no other home"
        );
    }
    Ok(planted)
}

/// What the repair is held to, once the app that replaced the installed one has started.
#[derive(Clone, Copy)]
pub struct Repaired<'a> {
    pub sandbox: &'a Sandbox,
    pub planted: &'a Planted,
    /// The `cf` of the bundle in place.
    pub cf: &'a Path,
    pub app_log: &'a str,
    /// The version the daemon says it is.
    pub version: &'a str,
    pub release: Release,
}

/// The commands once the app that replaced the installed one has started: each
/// that serves this home runs the bundle's `cf` and still pins this home, and it
/// runs and says the version of the `cf` the daemon is. A command the bridge's
/// Node `cf setup` wrote was rewritten, and the app's log says so. One the flip's
/// `cf setup` wrote named the `cf` of the installed bundle, which is the path the
/// update's is at: it was current, and is as it was, byte for byte, and the log
/// says nothing of it. Nothing that serves another home is changed by a byte, or
/// is spoken of.
pub fn assert_repaired(repaired: &Repaired) -> Result {
    let Repaired {
        sandbox,
        planted,
        cf,
        app_log,
        version,
        release,
    } = *repaired;
    let box_state = &sandbox.state;
    let now = commands_of(box_state)?;
    for name in &planted.repaired {
        let file = file_of(box_state, name);
        let shown = file.display();
        let text = &now[name];
        ensure!(text.contains(MARKER), "{shown} lost its mark");
        ensure!(
            text.contains(&format!("exec \"{}\" \"$@\"", cf.display())),
            "{shown} does not run {}:\n{text}",
            cf.display()
        );
        ensure!(
            text.contains(&format!(
                "export CONSENSFLOW_HOME=\"{}\"",
                box_state.display()
            )),
            "{shown} lost the pin"
        );
        ensure!(
            !(text.contains("cf.mjs") || text.contains("MacOS/node")),
            "{shown} still names Node's:\n{text}"
        );
        ensure!(is_executable(&file)?, "{shown} is not executable");
        if release.setup() == Kind::Native {
            ensure!(
                *text == planted.own[name],
                "{shown}, which was current, changed"
            );
            ensure!(
                !app_log.contains(&shown.to_string()),
                "app.log speaks of a command that was current: {shown}"
            );
        } else {
            let said = format!("{shown} now runs {}", cf.display());
            ensure!(
                app_log.split('\n').any(|line| line.ends_with(&said)),
                "app.log does not say {shown} was repaired:\n{}",
                tail(app_log, 2000)
            );
        }
        ensure!(
            version_of(sandbox, box_state, name)? == version,
            "{shown} runs another cf than the daemon"
        );
    }
    for name in NAMES.iter().filter(|name| !planted.repaired.contains(name)) {
        let file = file_of(box_state, name);
        ensure!(
            now[name] == planted.own[name],
            "{}, which serves another home, changed",
            file.display()
        );
        ensure!(
            !app_log.contains(&file.display().to_string()),
            "app.log speaks of a command that serves another home: {}",
            file.display()
        );
    }
    ensure!(
        commands_of(&sandbox.other)? == planted.other,
        "another home's commands changed"
    );
    ensure!(
        !app_log.contains(&bin_of(&sandbox.other).display().to_string()),
        "app.log speaks of a command that serves another home: {}",
        bin_of(&sandbox.other).display()
    );
    Ok(())
}

/// Whether anyone may run the file: a system with no such bits runs what it can.
fn is_executable(file: &Path) -> Result<bool> {
    let metadata = fs::metadata(file).map_err(files("look at", file))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
