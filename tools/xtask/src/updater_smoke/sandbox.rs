//! A whole machine for one case of the updater smoke: its own HOME, its own
//! ConsensFlow home (`state`), its own PATH holding stand-ins for the harnesses,
//! the folder the installed app sits in, and a second ConsensFlow home that
//! belongs to nothing that runs. Nothing of the real machine's is read or
//! written: every case runs in its own.
//!
//! `SHELL` is absent on purpose, as in the packaged smoke: the app asks the
//! login shell for its PATH when it has one and would replace the box's with it.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_base::env::Env;

use super::{files, Error, Result};

/// The stand-in of a harness: it says it is alive, records its pid, and reads
/// its input for ever.
const STAND_IN: &str = "#!/bin/sh
set -eu
if [ \"${1:-}\" = \"--version\" ]; then
  printf '@VERSION@\\n'
  exit 0
fi
printf '%s\\n' \"$$\" > '@PIDS@'-$$.pid
printf 'CFUPDATER-ALIVE %s\\n' \"$$\"
while IFS= read -r _line; do
  :
done
";

/// The harnesses that have a stand-in on the box's PATH.
const HARNESSES: [&str; 4] = ["claude", "codex", "pi", "opencode"];

/// The folders of a machine, made.
#[derive(Debug, Clone)]
pub struct Sandbox {
    pub root: PathBuf,
    /// Where the installed app sits.
    pub apps: PathBuf,
    /// The installed app.
    pub copy: PathBuf,
    pub home: PathBuf,
    /// The ConsensFlow home the app and its daemon work in.
    pub state: PathBuf,
    /// A second ConsensFlow home, which no app serves.
    pub other: PathBuf,
    /// Where the page opens its projects.
    pub workspace: PathBuf,
    /// The stand-ins of the harnesses.
    pub bin: PathBuf,
    /// The pids the stand-ins write.
    pub pids: PathBuf,
    pub tls: PathBuf,
    /// Where the smoke's own programs run from.
    pub probe: PathBuf,
}

/// What the packaged self-test is told, over a terminal's environment: the feed
/// it asks, the certificate that feed's address holds, the public key its
/// archives are signed by (the run's), the version it is to find when the update
/// has been installed, and how long it has.
pub struct SelfTest<'a> {
    pub feed: &'a str,
    pub certificate: &'a Path,
    pub public_key_file: &'a Path,
    pub expected: &'a str,
    pub deadline: Duration,
}

impl Sandbox {
    /// A new machine in a folder of its own under `parent`, which is made if it is not there.
    pub fn make(parent: &Path) -> Result<Self> {
        // Tauri deliberately rejects relaunch paths with symlinked ancestors;
        // macOS /var is a symlink to /private/var.
        fs::create_dir_all(parent).map_err(files("make", parent))?;
        let made = tempfile::Builder::new()
            .prefix("cf-updater-smoke-")
            .tempdir_in(parent)
            .map_err(files("make a folder in", parent))?
            .keep();
        let root = fs::canonicalize(&made).map_err(files("find", &made))?;
        let apps = root.join("Applications");
        let workspace = root.join("workspace");
        let sandbox = Self {
            copy: apps.join("ConsensFlow.app"),
            home: root.join("home"),
            state: root.join("state"),
            other: root.join("other-state"),
            bin: root.join("bin"),
            pids: root.join("pids"),
            tls: root.join("tls"),
            probe: root.join("probe"),
            apps,
            workspace,
            root,
        };
        for path in [
            &sandbox.apps,
            &sandbox.home,
            &sandbox.state,
            &sandbox.other,
            &sandbox.workspace,
            &sandbox.workspace.join(".consensflow-updater-second"),
            &sandbox.bin,
            &sandbox.pids,
            &sandbox.tls,
            &sandbox.probe,
        ] {
            fs::create_dir_all(path).map_err(files("make", path))?;
        }
        for name in HARNESSES {
            sandbox.stand_in(name)?;
        }
        Ok(sandbox)
    }

    /// The stand-in of the harness `name`, a program on the box's PATH.
    fn stand_in(&self, name: &str) -> Result {
        let version = if name == "claude" { "2.1.266" } else { "0.0.0" };
        let script = STAND_IN
            .replace("@VERSION@", version)
            .replace("@PIDS@", &self.pids.join(name).to_string_lossy());
        let path = self.bin.join(name);
        fs::write(&path, script).map_err(files("write", &path))?;
        executable(&path)
    }

    /// What a terminal on the box has: the box's PATH and homes, and whichever
    /// ConsensFlow home the caller names.
    fn terminal_vars(&self, home: &Path) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "PATH",
                format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.bin.display()).into(),
            ),
            ("HOME", self.home.clone().into()),
            ("TMPDIR", self.root.clone().into()),
            ("CONSENSFLOW_HOME", home.into()),
            ("CLAUDE_CONFIG_DIR", self.home.join(".claude").into()),
            ("CODEX_HOME", self.home.join(".codex").into()),
            ("XDG_CONFIG_HOME", self.home.join(".config").into()),
            (
                "PI_CODING_AGENT_DIR",
                self.home.join(".pi").join("agent").into(),
            ),
        ]
    }

    /// What a terminal on the box has, in the ConsensFlow home `home`.
    pub fn terminal_env(&self, home: &Path) -> Env {
        Env::from_vars(self.terminal_vars(home))
    }

    /// What the packaged app is started with: a terminal's, and the self-test's:
    /// the folder its two projects are opened in, and what `selftest` says.
    pub fn app_env(&self, selftest: &SelfTest) -> Env {
        let mut vars = self.terminal_vars(&self.state);
        vars.extend([
            ("CONSENSFLOW_SELFTEST", "1".into()),
            ("CONSENSFLOW_SELFTEST_DIR", self.workspace.clone().into()),
            (
                "CONSENSFLOW_SELFTEST_UPDATER_EXPECTED",
                selftest.expected.into(),
            ),
            ("CONSENSFLOW_SELFTEST_UPDATER_URL", selftest.feed.into()),
            (
                "CONSENSFLOW_SELFTEST_UPDATER_CERT",
                selftest.certificate.into(),
            ),
            (
                "CONSENSFLOW_SELFTEST_UPDATER_KEY",
                selftest.public_key_file.into(),
            ),
            (
                "CONSENSFLOW_SELFTEST_DEADLINE_MS",
                selftest.deadline.as_millis().to_string().into(),
            ),
        ]);
        Env::from_vars(vars)
    }

    /// The pids the stand-in harnesses wrote: one file each.
    pub fn recorded_pids(&self) -> Result<Vec<u32>> {
        let mut pids = Vec::new();
        for entry in fs::read_dir(&self.pids).map_err(files("list", &self.pids))? {
            let path = entry.map_err(files("list", &self.pids))?.path();
            if path.extension().is_none_or(|extension| extension != "pid") {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(files("read", &path))?;
            // A file being written is empty or short: it names no pid yet.
            pids.extend(text.trim().parse::<u32>().ok().filter(|pid| *pid > 0));
        }
        Ok(pids)
    }
}

/// Makes the file at `path` a program.
fn executable(path: &Path) -> Result {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(files("make a program of", path))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// A FIFO the app reads as its input, which outlives the process Tauri restarts:
/// one of its own for each start of an app, since a case may start more than one.
#[cfg(unix)]
pub fn make_fifo(directory: &Path) -> Result<PathBuf> {
    let made = tempfile::Builder::new()
        .prefix("control-")
        .tempdir_in(directory)
        .map_err(files("make a folder in", directory))?
        .keep();
    let file = made.join("input.fifo");
    nix::unistd::mkfifo(&file, nix::sys::stat::Mode::from_bits_truncate(0o600)).map_err(
        |cause| {
            Error::new(format!(
                "could not make the FIFO {}: {cause}",
                file.display()
            ))
        },
    )?;
    Ok(file)
}

/// A FIFO is a Unix file: the app's input has none to be on elsewhere.
#[cfg(not(unix))]
pub fn make_fifo(directory: &Path) -> Result<PathBuf> {
    Err(Error::new(format!(
        "could not make a FIFO in {}: a FIFO is a Unix file",
        directory.display()
    )))
}

/// The two ends of the FIFO at `fifo` this side keeps: the one the app is started
/// with as its input, and the one this side writes to and closes to end it.
pub struct Ends {
    pub input: fs::File,
    pub control: fs::File,
}

/// Opens the FIFO at `fifo` for the app and for this side. It is opened for
/// reading and writing without blocking first, so that the open for reading has a
/// writer to meet; then only the writer this side keeps.
#[cfg(unix)]
pub fn open_fifo(fifo: &Path) -> Result<Ends> {
    use std::os::unix::fs::OpenOptionsExt;

    let hold = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(nix::fcntl::OFlag::O_NONBLOCK.bits())
        .open(fifo)
        .map_err(files("open", fifo))?;
    let input = fs::File::open(fifo).map_err(files("open", fifo))?;
    let control = fs::OpenOptions::new()
        .write(true)
        .open(fifo)
        .map_err(files("open", fifo))?;
    drop(hold);
    Ok(Ends { input, control })
}

/// A FIFO is a Unix file: there is none to open.
#[cfg(not(unix))]
pub fn open_fifo(fifo: &Path) -> Result<Ends> {
    Err(Error::new(format!(
        "could not open the FIFO {}: a FIFO is a Unix file",
        fifo.display()
    )))
}

#[cfg(test)]
mod tests;
