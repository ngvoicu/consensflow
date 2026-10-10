//! A whole machine for the app to live in: its own HOME, its own ConsensFlow
//! home (the state root), its own PATH with one harness on it, a folder for the
//! project the page opens and one to run the bundle's own programs from.
//! Nothing of the real machine's is read or written.
//!
//! `SHELL` is deliberately absent. The app asks the login shell for a PATH when
//! it has one and REPLACES the child's with the answer, which would hand it the
//! real machine's harnesses. With no `SHELL` that lookup returns nothing and the
//! PATH built here is the one the app uses: the isolation is the absence, so do
//! not add `SHELL` back.
//!
//! A machine is a folder, kept: a failed smoke leaves it behind on purpose (the
//! harness, its pid file, the state root and the app's own launchers are the
//! evidence, and building again to look at them costs minutes), and a passing
//! one tidies up ([`Machine::cleanup`]).

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::process::Run;
use cf_e2e::{files, Error, Result};

/// The stand-in harness, with where its flood's size goes marked.
const HARNESS: &str = include_str!("../../fixtures/smoke-harness.sh");

/// How many lines the stand-in's flood is, and how wide each is.
pub const FLOOD_LINES: usize = 4096;
pub const FLOOD_WIDTH: usize = 384;

/// How long a program of the bundle's that is run to its end is given: a
/// daemon that started would stop at once on the input it is given, and one
/// still going after this is ended, which says it never stopped.
const PROBE_LIMIT: Duration = Duration::from_secs(30);

/// The system's own folders of programs, after the harness's.
const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// The machine the app runs in.
#[derive(Debug)]
pub struct Machine {
    /// The folder everything of the machine is under.
    pub root: PathBuf,
    /// `HOME`.
    pub home: PathBuf,
    /// ConsensFlow's home, `CONSENSFLOW_HOME`: the daemon's log, the roster, the app's log.
    pub state: PathBuf,
    /// Where the page opens its project.
    pub workspace: PathBuf,
    /// The one folder of the PATH that is the machine's own: the stand-in harness is in it.
    pub bin: PathBuf,
    /// Where the smoke's own programs run from, outside the checkout.
    pub probe: PathBuf,
    /// Where each window of the stand-in harness writes its pid.
    pub pid_file: PathBuf,
    /// What the run is told apart by: the harness says it, the page types it.
    pub tag: String,
    paste_reader: PathBuf,
}

impl Machine {
    /// A new machine in a folder of its own, with the stand-in harness (as
    /// `claude`) on its PATH. `paste_reader` is the program the harness runs to
    /// read a large paste.
    pub fn new(paste_reader: &Path) -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("cf-smoke-")
            .tempdir()
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?
            .keep();
        let suffix = root
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("cf-smoke-"))
            .unwrap_or_default();
        let machine = Self {
            home: root.join("home"),
            state: root.join("state"),
            workspace: root.join("workspace"),
            bin: root.join("bin"),
            probe: root.join("probe"),
            pid_file: root.join("harness.pid"),
            tag: format!("smoke-{}-{suffix}", std::process::id()),
            paste_reader: paste_reader.to_path_buf(),
            root,
        };
        for folder in [
            &machine.home,
            &machine.state,
            &machine.workspace,
            &machine.bin,
            &machine.probe,
        ] {
            files::make_dir(folder)?;
        }
        files::write_executable(&machine.bin.join("claude"), harness())?;
        Ok(machine)
    }

    /// The PATH of the machine: the folder of the harness, then the system's own.
    pub fn path(&self) -> String {
        format!("{}:{SYSTEM_PATH}", self.bin.display())
    }

    /// The environment the app is started with, and every program of the
    /// bundle's that the smoke runs: these variables and no others.
    pub fn vars(&self) -> Vec<(&'static str, OsString)> {
        let own = |path: &Path| path.as_os_str().to_owned();
        vec![
            ("PATH", self.path().into()),
            ("HOME", own(&self.home)),
            ("TMPDIR", own(&self.root)),
            ("CONSENSFLOW_HOME", own(&self.state)),
            ("CLAUDE_CONFIG_DIR", own(&self.home.join(".claude"))),
            ("CODEX_HOME", own(&self.home.join(".codex"))),
            ("XDG_CONFIG_HOME", own(&self.home.join(".config"))),
            // The self-test: the page reports on its own stdout, to the folder and under the tag.
            ("CONSENSFLOW_SELFTEST", "1".into()),
            ("CONSENSFLOW_SELFTEST_DIR", own(&self.workspace)),
            ("CONSENSFLOW_SELFTEST_TAG", self.tag.clone().into()),
            // What the stand-in harness is told.
            ("CFSMOKE_PIDFILE", own(&self.pid_file)),
            ("CFSMOKE_PASTE_READER", own(&self.paste_reader)),
            ("CFSMOKE_TAG", self.tag.clone().into()),
        ]
    }

    /// `program` to run to its end, for a program of the bundle's: from the
    /// probe folder, which is outside this checkout, on this machine's
    /// environment (a variable the caller adds comes after, and wins), with its
    /// input closed. One that outlasts [`PROBE_LIMIT`] is ended, and says so.
    /// The caller gives it its words and runs it.
    pub fn command(&self, program: &Path) -> Run {
        Run::new(program)
            .vars(self.vars())
            .cwd(&self.probe)
            .limit(PROBE_LIMIT)
    }

    /// Takes the machine away, once the run it was for has passed.
    pub fn cleanup(self) -> Result {
        std::fs::remove_dir_all(&self.root).map_err(|source| Error::File {
            action: "remove",
            path: self.root.clone(),
            source,
        })
    }

    /// Says where the machine is kept to `out`, if it is to be: the run is
    /// failing (or it was asked to keep what it passed on).
    pub fn say_where_kept(&self, out: &mut impl Write, kept: bool) {
        if kept {
            let _ = writeln!(
                out,
                "the smoke's machine is kept at {}",
                self.root.display()
            );
        }
    }
}

impl Drop for Machine {
    /// A run that fails with its machine in hand says where it is kept, whichever
    /// check it was that failed. (Said to the error stream itself: the test's own
    /// output is held back from a run that passes, and this is not said by one.)
    fn drop(&mut self) {
        self.say_where_kept(&mut std::io::stderr(), std::thread::panicking());
    }
}

/// The stand-in harness, its flood sized.
fn harness() -> String {
    HARNESS
        .replace("@FLOOD_LINES@", &FLOOD_LINES.to_string())
        .replace("@FLOOD_WIDTH@", &FLOOD_WIDTH.to_string())
}

/// The pids a file written by the stand-in harness holds, one to a line; none
/// if a line is not a process id.
pub fn parse_pids(text: &str) -> Option<Vec<u32>> {
    text.trim()
        .lines()
        .map(|line| line.trim().parse::<u32>().ok().filter(|pid| *pid > 0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use cf_e2e::process::is_alive;

    fn machine() -> Machine {
        Machine::new(Path::new("/the/paste-reader")).unwrap()
    }

    fn text(value: &OsString) -> String {
        value.to_string_lossy().into_owned()
    }

    #[test]
    fn the_environment_is_these_variables_and_has_no_shell_to_ask_for_a_path() {
        let machine = machine();
        let vars = machine.vars();
        let names: Vec<_> = vars.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "PATH",
                "HOME",
                "TMPDIR",
                "CONSENSFLOW_HOME",
                "CLAUDE_CONFIG_DIR",
                "CODEX_HOME",
                "XDG_CONFIG_HOME",
                "CONSENSFLOW_SELFTEST",
                "CONSENSFLOW_SELFTEST_DIR",
                "CONSENSFLOW_SELFTEST_TAG",
                "CFSMOKE_PIDFILE",
                "CFSMOKE_PASTE_READER",
                "CFSMOKE_TAG",
            ]
        );
        assert!(!names.contains(&"SHELL"));
        let value = |name: &str| {
            vars.iter()
                .find(|(given, _)| *given == name)
                .map(|(_, value)| text(value))
                .unwrap()
        };
        let root = machine.root.display().to_string();
        // Everything of the machine's is under its folder, but the system's own programs.
        for name in [
            "HOME",
            "TMPDIR",
            "CONSENSFLOW_HOME",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "CONSENSFLOW_SELFTEST_DIR",
            "CFSMOKE_PIDFILE",
        ] {
            assert!(value(name).starts_with(&root), "{name}: {}", value(name));
        }
        assert_eq!(
            value("PATH"),
            format!("{}/bin:/usr/bin:/bin:/usr/sbin:/sbin", root)
        );
        assert_eq!(value("CONSENSFLOW_SELFTEST"), "1");
        assert_eq!(value("CFSMOKE_PASTE_READER"), "/the/paste-reader");
        // The page types the tag and the harness says it: it is the one value.
        assert_eq!(value("CFSMOKE_TAG"), machine.tag);
        assert_eq!(value("CONSENSFLOW_SELFTEST_TAG"), machine.tag);
        machine.cleanup().unwrap();
    }

    #[test]
    fn a_machine_is_a_folder_of_its_own_with_the_harness_on_its_path_as_claude() {
        let (first, second) = (machine(), machine());
        assert_ne!(first.root, second.root);
        assert_ne!(first.tag, second.tag);
        assert!(first
            .tag
            .starts_with(&format!("smoke-{}-", std::process::id())));
        for folder in [
            &first.home,
            &first.state,
            &first.workspace,
            &first.bin,
            &first.probe,
        ] {
            assert!(folder.is_dir(), "{}", folder.display());
        }
        let harness = first.bin.join("claude");
        assert!(harness.is_file());
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&harness).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
        first.cleanup().unwrap();
        second.cleanup().unwrap();
    }

    #[test]
    fn the_harness_has_its_flood_sized_and_is_a_script_the_shell_reads_whole() {
        let script = harness();
        assert!(!script.contains('@'), "a placeholder was left in it");
        assert!(script.contains("while [ $n -lt 384 ]; do"));
        assert!(script.contains("while [ $n -le 4096 ]; do"));
        // The shell reads it to its end without a complaint.
        let machine = machine();
        let ran = Run::new("/bin/sh")
            .arg("-n")
            .arg(machine.bin.join("claude"))
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "{ran}");
        machine.cleanup().unwrap();
    }

    #[test]
    fn a_command_runs_from_the_probe_folder_on_the_machines_environment_with_what_is_added_over_it()
    {
        let machine = machine();
        let ran = machine
            .command(Path::new("/usr/bin/env"))
            .var("CONSENSFLOW_HOME", "/elsewhere")
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "{ran}");
        let lines: Vec<&str> = ran.stdout.lines().collect();
        assert!(lines.contains(&"CONSENSFLOW_HOME=/elsewhere"), "{lines:?}");
        assert!(lines.contains(&format!("HOME={}", machine.home.display()).as_str()));
        assert!(lines.contains(&format!("CFSMOKE_TAG={}", machine.tag).as_str()));
        // Nothing of the test's own comes along, a shell least of all.
        assert!(!lines.iter().any(|line| line.starts_with("SHELL=")));
        assert!(!lines.iter().any(|line| line.starts_with("CARGO")));

        let ran = machine.command(Path::new("/bin/pwd")).run().unwrap();
        let ran_in = std::fs::canonicalize(ran.stdout.trim()).unwrap();
        assert_eq!(ran_in, std::fs::canonicalize(&machine.probe).unwrap());
        machine.cleanup().unwrap();
    }

    #[test]
    fn a_machine_that_is_cleaned_up_is_gone_and_one_that_is_not_stays() {
        let kept = machine();
        let root = kept.root.clone();
        assert!(root.is_dir());
        // A failed run drops its machine without cleaning it up: the evidence is there.
        drop(kept);
        assert!(root.is_dir());
        std::fs::remove_dir_all(&root).unwrap();

        let tidied = machine();
        let root = tidied.root.clone();
        tidied.cleanup().unwrap();
        assert!(!root.exists());
    }

    #[test]
    fn a_run_that_fails_says_where_its_machine_is_kept_and_one_that_passes_says_nothing() {
        let machine = machine();
        let mut said = Vec::new();
        machine.say_where_kept(&mut said, false);
        assert!(said.is_empty());
        machine.say_where_kept(&mut said, true);
        assert_eq!(
            String::from_utf8(said).unwrap(),
            format!(
                "the smoke's machine is kept at {}\n",
                machine.root.display()
            )
        );
        machine.cleanup().unwrap();
    }

    #[test]
    fn the_pids_the_harness_wrote_are_one_to_a_line_and_nothing_else_is_taken_for_one() {
        assert_eq!(parse_pids("123\n456\n"), Some(vec![123, 456]));
        assert_eq!(parse_pids(" 7 \n"), Some(vec![7]));
        assert_eq!(parse_pids("123\nabc\n"), None);
        assert_eq!(parse_pids("0\n"), None);
        assert_eq!(parse_pids("-1\n"), None);
        // No line at all: no pid, which the smoke holds to be too few.
        assert_eq!(parse_pids(""), Some(vec![]));
        // This process is one: the pid of what runs is a pid.
        let own = std::process::id();
        assert!(is_alive(own));
        assert_eq!(parse_pids(&format!("{own}\n")), Some(vec![own]));
    }
}
