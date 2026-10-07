//! npm's global folder on Windows (`NPM_GLOBAL`, `src/harnesses.js`): a CLI
//! found there when PATH lacks it, and what is found started as one on PATH
//! is. Windows is simulated, as other detection tests do: `OS` says it is.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use cf_process::{pane_argv, runnable, Run};

use super::*;

/// What npm writes as the `.cmd` of a global package: its last line runs node
/// on the package's script, `%dp0%` being the shim's own folder.
const NPM_SHIM: &str = "@ECHO off\r\nSETLOCAL\r\nCALL :find_dp0\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@earendil-works\\pi-coding-agent\\dist\\cli.js\" %*\r\n";

/// A path as text.
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A Windows machine as a test lays one out, on any system: a home, the
/// roaming folder `APPDATA` names, and a folder PATH names that holds nothing.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    /// The folder PATH names.
    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    /// `%APPDATA%`, where Windows keeps what an application keeps for its user.
    fn roaming(&self) -> PathBuf {
        self.home().join("AppData").join("Roaming")
    }

    /// npm's global folder, `%APPDATA%\npm`.
    fn npm(&self) -> PathBuf {
        self.roaming().join("npm")
    }

    /// Its environment, with `changes` made to it: a variable given none is
    /// taken away.
    fn env(&self, changes: &[(&str, Option<&str>)]) -> Env {
        let mut vars: BTreeMap<String, String> = [
            ("OS", "Windows_NT".to_owned()),
            (
                "PATHEXT",
                ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL".to_owned(),
            ),
            ("HOME", text(&self.home())),
            ("APPDATA", text(&self.roaming())),
            ("PATH", text(&self.bin())),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        for (name, value) in changes {
            match value {
                Some(value) => vars.insert((*name).to_owned(), (*value).to_owned()),
                None => vars.remove(*name),
            };
        }
        Env::from_vars(vars)
    }
}

/// Pi as `npm install -g` leaves it in `folder`: its `.cmd` shim, and the
/// script that shim runs, which prints the arguments it was given. Returns the
/// shim and the script.
fn install_pi(folder: &Path) -> (PathBuf, PathBuf) {
    let script = ["node_modules", "@earendil-works", "pi-coding-agent", "dist"]
        .iter()
        .fold(folder.to_path_buf(), |path, part| path.join(part))
        .join("cli.js");
    startable_with(&script, "printf 'pi %s\\n' \"$*\"\n");
    let shim = folder.join("pi.cmd");
    startable_with(&shim, NPM_SHIM);
    (shim, script)
}

#[test]
fn a_cli_is_found_in_npm_s_folder_when_path_lacks_it_as_the_cmd_windows_starts() {
    let machine = Machine::new();
    let env = machine.env(&[]);
    assert_eq!(harness_path(Harness::Pi, &env), None);
    // What npm writes for a global package is three files, one of which starts.
    let folder = machine.npm();
    startable(&folder.join("pi"));
    startable(&folder.join("pi.ps1"));
    assert_eq!(
        harness_path(Harness::Pi, &env),
        None,
        "a bare name and a script are no program"
    );
    startable(&folder.join("pi.cmd"));
    assert_eq!(harness_path(Harness::Pi, &env), Some(folder.join("pi.cmd")));
    // A window opens on it, where it is refused for a harness that is not there.
    assert_eq!(
        executable(Harness::Pi, &env),
        Ok(text(&folder.join("pi.cmd")))
    );
    assert_eq!(
        executable(Harness::Claude, &env),
        Err("claude is not installed on this machine".to_owned())
    );
    // The pickers and the Harnesses page ask for what is installed.
    let detected = detect_harnesses(&env);
    assert_eq!(
        detected.iter().map(|each| each.id).collect::<Vec<_>>(),
        [Harness::Pi]
    );
    assert_eq!(
        missing_harnesses(&env),
        [
            Harness::Devin,
            Harness::Claude,
            Harness::Codex,
            Harness::Opencode
        ]
    );
    // Another harness's shim is not this one's.
    startable(&folder.join("codex.cmd"));
    assert_eq!(harness_path(Harness::Claude, &env), None);
}

#[test]
fn it_comes_after_path_the_harness_s_own_places_and_the_other_common_ones() {
    let machine = Machine::new();
    let env = machine.env(&[]);
    let shim = |folder: PathBuf| folder.join("pi.cmd");
    let npm = shim(machine.npm());
    startable(&npm);
    assert_eq!(harness_path(Harness::Pi, &env), Some(npm));
    for common in [".bun", ".npm-global", ".volta"] {
        let there = shim(machine.home().join(common).join("bin"));
        startable(&there);
        assert_eq!(
            harness_path(Harness::Pi, &env),
            Some(there.clone()),
            "{common} before npm's folder"
        );
        fs::remove_file(there).unwrap();
    }
    let volta = shim(machine.home().join(".volta").join("bin"));
    startable(&volta);
    let own = shim(machine.home().join(".pi").join("bin"));
    startable(&own);
    assert_eq!(
        harness_path(Harness::Pi, &env),
        Some(own),
        "the harness's own place before the common ones"
    );
    let on_path = shim(machine.bin());
    startable(&on_path);
    assert_eq!(harness_path(Harness::Pi, &env), Some(on_path));
}

#[test]
fn an_appdata_that_is_missing_or_empty_adds_nothing() {
    let machine = Machine::new();
    // Where Windows keeps roaming data by default: not where a missing APPDATA points.
    startable(&machine.npm().join("pi.cmd"));
    assert_eq!(npm_global(&machine.env(&[])), Some(machine.npm()));
    for appdata in [None, Some("")] {
        let env = machine.env(&[("APPDATA", appdata)]);
        assert_eq!(npm_global(&env), None, "{appdata:?}");
        assert_eq!(harness_path(Harness::Pi, &env), None, "{appdata:?}");
        assert_eq!(
            executable(Harness::Pi, &env),
            Err("pi is not installed on this machine".to_owned()),
            "{appdata:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn nothing_changes_off_windows() {
    let machine = Machine::new();
    // A bare name is the program there, and this one is startable.
    startable(&machine.npm().join("pi"));
    startable(&machine.npm().join("pi.cmd"));
    for os in [None, Some("Linux"), Some("Darwin")] {
        let env = machine.env(&[("OS", os)]);
        assert_eq!(npm_global(&env), None, "{os:?}");
        assert_eq!(harness_path(Harness::Pi, &env), None, "{os:?}");
    }
    // The same files, found where the environment says it is Windows's.
    assert_eq!(
        harness_path(Harness::Pi, &machine.env(&[])),
        Some(machine.npm().join("pi.cmd"))
    );
}

#[test]
fn it_needs_no_home() {
    let machine = Machine::new();
    startable(&machine.npm().join("pi.cmd"));
    let env = machine.env(&[("HOME", None)]);
    assert_eq!(
        harness_path(Harness::Pi, &env),
        Some(machine.npm().join("pi.cmd"))
    );
}

/// A shim found there is started as one on PATH is: nothing of how it was
/// found reaches how it starts, so what a shim needs, node, is what it needs
/// anywhere.
mod started {
    use super::*;

    /// A shim found in npm's folder, and its script, with `changes` made to
    /// the environment of a machine that has no node anywhere but the app's.
    fn found(machine: &Machine, changes: &[(&str, Option<&str>)]) -> (Env, PathBuf, PathBuf) {
        let (shim, script) = install_pi(&machine.npm());
        let mut vars = vec![("CONSENSFLOW_NODE", Some("/the/app/node"))];
        vars.extend_from_slice(changes);
        let env = machine.env(&vars);
        assert_eq!(harness_path(Harness::Pi, &env), Some(shim.clone()));
        (env, shim, script)
    }

    #[test]
    fn with_the_node_beside_it_else_the_one_on_path_else_the_apps_own() {
        let machine = Machine::new();
        let (env, shim, script) = found(&machine, &[]);
        let args = [OsString::from("--version")];
        let started = |program: PathBuf| Run {
            program,
            args: vec![script.clone().into_os_string(), "--version".into()],
            verbatim: false,
        };
        // No node on PATH and none beside the shim: the app's own, as for a PATH shim.
        assert_eq!(
            runnable(&shim, &args, &env),
            started("/the/app/node".into())
        );
        startable(&machine.bin().join("node.exe"));
        assert_eq!(
            runnable(&shim, &args, &env),
            started(machine.bin().join("node.exe"))
        );
        // The same shim, in a folder PATH names: started the same way.
        let (on_path, _) = install_pi(&machine.bin());
        assert_eq!(
            runnable(&on_path, &args, &env).program,
            runnable(&shim, &args, &env).program
        );
        // And the node beside it first, as npm itself would.
        startable(&machine.npm().join("node.exe"));
        assert_eq!(
            runnable(&shim, &args, &env),
            started(machine.npm().join("node.exe"))
        );
    }

    #[test]
    fn a_window_opens_on_it_as_its_node_and_script() {
        let machine = Machine::new();
        let (env, shim, script) = found(&machine, &[]);
        let node = machine.npm().join("node.exe");
        startable(&node);
        let argv = [text(&shim), "--model".to_owned(), "a b".to_owned()];
        assert_eq!(
            pane_argv(&argv, &env).unwrap(),
            [text(&node), text(&script), "--model".into(), "a b".into()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn and_the_harness_runs() {
        let machine = Machine::new();
        // The app's own node is a shell here, which reads the script as it is.
        let (env, shim, script) = found(&machine, &[("CONSENSFLOW_NODE", Some("/bin/sh"))]);
        let run = runnable(&shim, &[OsString::from("--version")], &env);
        assert_eq!(run.program, Path::new("/bin/sh"));
        assert_eq!(run.args[0], script.into_os_string());
        let output = run.command().output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "pi --version\n");
    }
}
