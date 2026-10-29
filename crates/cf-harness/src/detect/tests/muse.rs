//! Muse Code where its installers put it: `~/.local/bin` on macOS and Linux
//! (`install.sh`), `%LOCALAPPDATA%\Programs\muse` on Windows (`install.ps1`, a
//! `muse.cmd` beside its PowerShell launcher). Windows is simulated, as other
//! detection tests do: `OS` says it is.

use super::npm_global::{text, Machine};
use super::*;

// Windows itself looks for `muse.cmd` and `muse.exe`, never a bare `muse`.
#[cfg(not(windows))]
#[test]
fn muse_is_found_where_its_installer_puts_it_on_macos_and_linux() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let env = Env::from_vars([
        ("HOME", text(&home)),
        ("PATH", text(&root.path().join("bin"))),
    ]);
    assert_eq!(harness_path(Harness::Muse, &env), None);
    assert!(missing_harnesses(&env).contains(&Harness::Muse));
    // The binaries it runs are beside the launcher: only the launcher is Muse's command.
    startable(
        &home
            .join(".local")
            .join("bin")
            .join("muse-bin-1.4.13-R6996.1"),
    );
    assert_eq!(harness_path(Harness::Muse, &env), None);
    let launcher = home.join(".local").join("bin").join("muse");
    startable(&launcher);
    assert_eq!(harness_path(Harness::Muse, &env), Some(launcher.clone()));
    assert_eq!(executable(Harness::Muse, &env), Ok(text(&launcher)));
    assert!(detect_harnesses(&env).contains(&Detected {
        id: Harness::Muse,
        command: "muse",
    }));
    // A Windows folder means nothing elsewhere.
    let elsewhere = Env::from_vars([
        ("HOME", text(&root.path().join("nobody"))),
        ("LOCALAPPDATA", text(&root.path().join("local"))),
    ]);
    startable(
        &root
            .path()
            .join("local")
            .join("Programs")
            .join("muse")
            .join("muse"),
    );
    assert_eq!(harness_path(Harness::Muse, &elsewhere), None);
}

#[test]
fn muse_is_found_on_windows_as_the_cmd_its_installer_writes_in_local_app_data() {
    let machine = Machine::new();
    let local = machine.root.path().join("AppData").join("Local");
    let env = machine.env(&[("LOCALAPPDATA", Some(&text(&local)))]);
    assert_eq!(harness_path(Harness::Muse, &env), None);
    let folder = local.join("Programs").join("muse");
    // What the installer writes beside the shim is no program Windows starts.
    startable(&folder.join(".muse-launcher.ps1"));
    assert_eq!(harness_path(Harness::Muse, &env), None);
    startable(&folder.join("muse.cmd"));
    assert_eq!(
        harness_path(Harness::Muse, &env),
        Some(folder.join("muse.cmd"))
    );
    // No other harness is looked for there.
    startable(&folder.join("pi.cmd"));
    assert_eq!(harness_path(Harness::Pi, &env), None);
    // A LOCALAPPDATA that is missing or empty names no folder.
    assert_eq!(harness_path(Harness::Muse, &machine.env(&[])), None);
    let empty = machine.env(&[("LOCALAPPDATA", Some(""))]);
    assert_eq!(harness_path(Harness::Muse, &empty), None);
    // On PATH, it is found there first.
    startable(&machine.bin().join("muse.cmd"));
    assert_eq!(
        harness_path(Harness::Muse, &env),
        Some(machine.bin().join("muse.cmd"))
    );
}
