//! Pi's managed install, which Pi's own installers make (`install.ps1` on Windows,
//! `install.sh` elsewhere, both read on 2026-10-21): its launcher is `bin/pi`
//! (`pi.cmd` on Windows) in the agent folder, `~/.pi/agent` unless the human named
//! another, and on a Mac or Linux a link to it in the first folder of the PATH that
//! the installer may write in. An app started before the installer changed the PATH
//! has the old one, so the launcher is looked for where it always is. It is looked
//! for last, after every place a Pi was looked for before: where a Pi is found
//! without it, the same one is found with it. Windows is simulated, as other
//! detection tests do: `OS` says it is.

use cf_base::env::TOOL_FOLDERS;
use cf_process::pane_argv;

use super::npm_global::{text, Machine};
use super::*;

/// The `pi.cmd` Pi's Windows installer writes, byte for byte (read on zeewin,
/// 2026-10-20): `node` bare, not an npm shim's quoted program.
const MANAGED_CMD: &str = "@ECHO off\nnode \"%~dp0pi-launcher.js\" %*\r\n";

/// The agent folder's `bin`, where the managed launcher is, under `home`.
fn agent_bin(home: &Path) -> PathBuf {
    home.join(".pi").join("agent").join("bin")
}

#[test]
fn a_managed_pi_is_found_in_its_agent_folder_when_path_lacks_it_and_a_window_opens_on_it() {
    let machine = Machine::new();
    let env = machine.env(&[]);
    assert_eq!(harness_path(Harness::Pi, &env), None);
    let folder = agent_bin(&machine.home());
    let launcher = folder.join("pi-launcher.js");
    startable(&launcher);
    startable(&folder.join("pi"));
    startable(&folder.join("pi.ps1"));
    assert_eq!(
        harness_path(Harness::Pi, &env),
        None,
        "a bare name and a script are no program"
    );
    let shim = folder.join("pi.cmd");
    startable_with(&shim, MANAGED_CMD);
    assert_eq!(harness_path(Harness::Pi, &env), Some(shim.clone()));
    assert_eq!(executable(Harness::Pi, &env), Ok(text(&shim)));
    // A window opens on it as the Node it finds and the launcher.
    let node = machine.bin().join("node.exe");
    startable(&node);
    let argv = [text(&shim), "--model".to_owned(), "a b".to_owned()];
    assert_eq!(
        pane_argv(&argv, &env).unwrap(),
        [text(&node), text(&launcher), "--model".into(), "a b".into()]
    );
}

#[test]
fn it_is_looked_in_after_every_place_a_pi_was_looked_for_before_so_no_machine_finds_another() {
    let machine = Machine::new();
    let tools = machine.root.path().join("tools");
    let named = text(&tools);
    let env = machine.env(&[(TOOL_FOLDERS, Some(named.as_str()))]);
    let shim = |folder: PathBuf| folder.join("pi.cmd");
    let managed = shim(agent_bin(&machine.home()));
    startable(&managed);
    assert_eq!(harness_path(Harness::Pi, &env), Some(managed));
    // Each of the other places, with the managed one there, is still the one found.
    let home = machine.home();
    for (what, folder) in [
        ("the harness's own place", home.join(".pi").join("bin")),
        ("a common place", home.join(".volta").join("bin")),
        ("npm's folder", machine.npm()),
        ("the system's folders", tools),
        ("the PATH", machine.bin()),
    ] {
        let there = shim(folder);
        startable(&there);
        assert_eq!(
            harness_path(Harness::Pi, &env),
            Some(there.clone()),
            "{what} before the managed one"
        );
        fs::remove_file(there).unwrap();
    }
}

/// Both installers read `PI_CODING_AGENT_DIR` as the agent folder (`install.ps1` when it holds
/// something, `install.sh` as `${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}`), and put the launcher
/// in its `bin`. The folder named is looked in before the default, which an install made
/// before the variable was set may still be in; both after every other place.
#[test]
fn a_managed_pi_in_the_agent_folder_the_human_named_is_found_and_the_default_is_still_looked_in() {
    let machine = Machine::new();
    let named = machine.root.path().join("work").join("pi-agent");
    let custom = text(&named);
    let env = machine.env(&[("PI_CODING_AGENT_DIR", Some(custom.as_str()))]);
    assert_eq!(harness_path(Harness::Pi, &env), None);
    let in_named = named.join("bin").join("pi.cmd");
    startable_with(&in_named, MANAGED_CMD);
    assert_eq!(harness_path(Harness::Pi, &env), Some(in_named.clone()));
    assert_eq!(executable(Harness::Pi, &env), Ok(text(&in_named)));
    // Without the variable, or with it empty, as the installers take it, it is not looked in.
    for changes in [
        &[][..],
        &[("PI_CODING_AGENT_DIR", None)][..],
        &[("PI_CODING_AGENT_DIR", Some(""))][..],
    ] {
        assert_eq!(harness_path(Harness::Pi, &machine.env(changes)), None);
    }
    // The default is still looked in, and after the one named.
    let in_default = agent_bin(&machine.home()).join("pi.cmd");
    startable_with(&in_default, MANAGED_CMD);
    assert_eq!(harness_path(Harness::Pi, &env), Some(in_named.clone()));
    assert_eq!(
        harness_path(Harness::Pi, &machine.env(&[])),
        Some(in_default.clone())
    );
    fs::remove_file(&in_named).unwrap();
    assert_eq!(harness_path(Harness::Pi, &env), Some(in_default));
    // A trailing separator on the variable names the same folder.
    startable_with(&in_named, MANAGED_CMD);
    let slashed = format!("{custom}{}", std::path::MAIN_SEPARATOR);
    assert_eq!(
        harness_path(
            Harness::Pi,
            &machine.env(&[("PI_CODING_AGENT_DIR", Some(slashed.as_str()))])
        ),
        Some(in_named)
    );
    // It is after every other place a Pi was looked for, as the default is, and it is Pi's alone.
    let env = machine.env(&[("PI_CODING_AGENT_DIR", Some(custom.as_str()))]);
    let npm = machine.npm().join("pi.cmd");
    startable(&npm);
    assert_eq!(harness_path(Harness::Pi, &env), Some(npm));
    startable(&named.join("bin").join("claude.cmd"));
    assert_eq!(harness_path(Harness::Claude, &env), None);
}

#[test]
fn the_launcher_the_unix_installer_writes_is_found_there_too_and_is_pis_alone() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    let name = |command: &str| {
        if cfg!(windows) {
            format!("{command}.exe")
        } else {
            command.to_owned()
        }
    };
    let env = Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("PATH", bin.to_str().unwrap()),
    ]);
    assert_eq!(harness_path(Harness::Pi, &env), None);
    let launcher = agent_bin(&home).join(name("pi"));
    startable(&launcher);
    assert_eq!(harness_path(Harness::Pi, &env), Some(launcher));
    // Another harness's CLI is not looked for there.
    startable(&agent_bin(&home).join(name("claude")));
    assert_eq!(harness_path(Harness::Claude, &env), None);
    // The link the installer made in a folder of the PATH comes first.
    let linked = bin.join(name("pi"));
    startable(&linked);
    assert_eq!(harness_path(Harness::Pi, &env), Some(linked));
    // A machine with no home has no such place.
    let homeless = Env::from_vars([("PATH", "")]);
    assert_eq!(harness_path(Harness::Pi, &homeless), None);
}
