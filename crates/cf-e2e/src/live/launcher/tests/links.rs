// The tests for the machines that install by links: the Unix ones. The portable ones, a launcher that
// is a file as Windows installs it, are in the parent.

use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;

use super::*;
use crate::live::machine::Installed;

impl Home {
    /// An executable file at `below`.
    fn executable(&self, below: &str, text: &str) {
        self.write(below, text);
        fs::set_permissions(self.path(below), fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A link at `below` to `to`, over one that is there, as an installer makes it.
    fn link(&self, below: &str, to: &Path) {
        let at = self.path(below);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        let staged = at.with_extension("staged");
        symlink(to, &staged).unwrap();
        fs::rename(&staged, &at).unwrap();
    }

    /// Claude Code as its native installer leaves it.
    fn claude(&self) -> PathBuf {
        self.write(".local/share/claude/versions/2.1.316", "claude 2.1.316");
        self.link(
            ".local/bin/claude",
            &self.path(".local/share/claude/versions/2.1.316"),
        );
        self.path(".local/bin/claude")
    }
}

/// What the probe says, asked again where a script just written was refused as a file still open
/// for writing (a program another test started held it, for a moment, in a child).
fn version_of(install: &Installed) -> String {
    for _ in 0..20 {
        let said = install.version();
        if !said.starts_with("unknown") {
            return said;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    install.version()
}

#[test]
fn a_version_probe_is_run_with_the_switches_of_its_harness() {
    // The probe runs the owner's launcher with the owner's environment: Muse's, run with
    // `--version`, ran its hourly check (T-1082), which may rewrite itself.
    let home = Home::new();
    home.executable(
        ".local/bin/muse",
        "#!/bin/sh\necho \"muse-update-off-${MUSE_NO_AUTO_UPDATE:-unset}\"\n",
    );
    home.executable(
        ".local/bin/claude",
        "#!/bin/sh\necho \"claude-${DISABLE_AUTOUPDATER:-unset}-${DISABLE_UPDATES:-unset}\"\n",
    );
    let muse = Installed::at(Harness::Muse, home.path(".local/bin/muse"), home.0.path());
    assert_eq!(version_of(&muse), "muse-update-off-1");
    let claude = Installed::at(
        Harness::Claude,
        home.path(".local/bin/claude"),
        home.0.path(),
    );
    assert_eq!(version_of(&claude), "claude-1-1");
}

#[test]
fn a_launcher_that_is_left_alone_is_no_change_and_the_summary_names_it_and_its_hash() {
    let home = Home::new();
    let command = home.claude();
    let watch = Watch::of(Harness::Claude, &command, home.0.path());
    let seen = watch.look();
    assert!(seen.changes_to(&watch.look(), home.0.path()).is_empty());
    let summary = seen.summary(home.0.path());
    assert!(summary.starts_with("~/.local/bin/claude "), "{summary}");
    assert_eq!(
        summary.matches("~/.local/bin/claude ").count(),
        1,
        "{summary}"
    );
}

#[test]
fn a_link_taken_to_a_copy_of_the_same_binary_is_a_change_whatever_the_bytes_say() {
    let home = Home::new();
    let command = home.claude();
    let was = fs::read_link(&command).unwrap();
    let elsewhere = home.path("rig/claude/versions/2.1.316");
    fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    fs::copy(&was, &elsewhere).unwrap();
    let found = changes(
        &Watch::of(Harness::Claude, &command, home.0.path()),
        &home,
        || home.link(".local/bin/claude", &elsewhere),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("~/.local/bin/claude now points at "),
        "{found:?}"
    );
    assert!(
        found[0].contains(&format!("ln -sfn {} ~/.local/bin/claude", was.display())),
        "{found:?}"
    );
}

#[test]
fn a_binary_replaced_in_place_is_a_change_though_the_link_is_the_same() {
    let home = Home::new();
    let command = home.claude();
    let found = changes(
        &Watch::of(Harness::Claude, &command, home.0.path()),
        &home,
        || home.write(".local/share/claude/versions/2.1.316", "claude 2.1.317"),
    );
    assert_eq!(
        found,
        ["~/.local/share/claude/versions/2.1.316 is not as it was"]
    );
}

#[test]
fn a_launcher_that_dangles_that_is_gone_or_that_became_a_file_is_named_for_what_it_became() {
    let home = Home::new();
    let command = home.claude();
    let watch = Watch::of(Harness::Claude, &command, home.0.path());
    // The run's folder was removed when it ended (T-1068): the link reads what it read, and
    // ends nowhere.
    let found = changes(&watch, &home, || {
        fs::remove_file(home.path(".local/share/claude/versions/2.1.316")).unwrap();
    });
    assert!(
        found
            .iter()
            .any(|said| said.contains("now ends at nothing (it dangles)")),
        "{found:?}"
    );
    // Gone; made; a link for a file.
    let found = changes(&watch, &home, || {
        fs::remove_file(home.path(".local/bin/claude")).unwrap();
    });
    assert!(
        found
            .iter()
            .any(|said| said == "~/.local/bin/claude is gone"),
        "{found:?}"
    );
    let found = changes(&watch, &home, || {
        home.write(".local/bin/claude", "a file");
    });
    assert!(
        found
            .iter()
            .any(|said| said.contains("~/.local/bin/claude was made")),
        "{found:?}"
    );
    let watch = Watch::of(Harness::Claude, &command, home.0.path());
    let found = changes(&watch, &home, || {
        home.link(".local/bin/claude", Path::new("/nowhere"));
    });
    assert!(
        found
            .iter()
            .any(|said| said.contains("is a link to /nowhere now, and was a file")),
        "{found:?}"
    );
    // And a link that became a file says the link it was.
    let home = Home::new();
    let command = home.claude();
    let was = fs::read_link(&command).unwrap();
    let found = changes(
        &Watch::of(Harness::Claude, &command, home.0.path()),
        &home,
        || {
            fs::remove_file(home.path(".local/bin/claude")).unwrap();
            home.write(".local/bin/claude", "a file");
        },
    );
    assert!(
        found.iter().any(|said| said.contains(&format!(
            "is a file now, and was a link to {}; put the link back with `ln -sfn {} ~/.local/bin/claude`",
            was.display(),
            was.display()
        ))),
        "{found:?}"
    );
}

#[test]
fn a_version_installed_among_the_owners_is_named() {
    let home = Home::new();
    let command = home.claude();
    let found = changes(
        &Watch::of(Harness::Claude, &command, home.0.path()),
        &home,
        || home.write(".local/share/claude/versions/2.1.317", "newer"),
    );
    assert_eq!(found, ["~/.local/share/claude/versions gained 2.1.317"]);
}

#[test]
fn devin_is_watched_through_its_current_link_and_its_versions() {
    let home = Home::new();
    home.write(
        ".local/share/devin/cli/_versions/3000.11.1/bin/devin",
        "devin",
    );
    home.link(
        ".local/share/devin/cli/_versions/current",
        Path::new("3000.11.1"),
    );
    home.link(
        ".local/bin/devin",
        &home.path(".local/share/devin/cli/_versions/current/bin/devin"),
    );
    let command = home.path(".local/bin/devin");
    let found = changes(
        &Watch::of(Harness::Devin, &command, home.0.path()),
        &home,
        || {
            home.write(
                ".local/share/devin/cli/_versions/3000.11.2/bin/devin",
                "devin2",
            );
            home.link(
                ".local/share/devin/cli/_versions/current",
                Path::new("3000.11.2"),
            );
        },
    );
    assert!(
        found
            .iter()
            .any(|said| said.starts_with("~/.local/bin/devin now ends at ")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|said| said == "~/.local/share/devin/cli/_versions gained 3000.11.2"),
        "{found:?}"
    );
}

#[test]
fn muses_launcher_binaries_and_version_line_are_watched_beside_it() {
    let home = Home::new();
    home.write(".local/bin/muse", "#!/bin/sh\n# the launcher\n");
    home.write(".local/bin/muse-bin-1.4.14-R1", "binary");
    home.write(".local/bin/.muse-version", "1.4.14-R1\n");
    home.write(".local/bin/.muse-update-checked-at", "1793577896\n");
    let command = home.path(".local/bin/muse");
    let watch = Watch::of(Harness::Muse, &command, home.0.path());
    // The hourly stamp is what a launcher run without the switch writes; it is no damage
    // (the switch keeps it from being written), and an owner's own Muse writes it too.
    let found = changes(&watch, &home, || {
        home.write(".local/bin/.muse-update-checked-at", "1793581496\n");
    });
    assert!(found.is_empty(), "{found:?}");
    let found = changes(&watch, &home, || {
        home.write(".local/bin/muse-bin-1.4.15-R2", "newer binary");
        home.write(".local/bin/.muse-version", "1.4.15-R2\n");
        home.write(".local/bin/muse", "#!/bin/sh\n# the launcher, updated\n");
    });
    assert!(
        found
            .iter()
            .any(|said| said == "~/.local/bin gained muse-bin-1.4.15-R2"),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|said| said == "~/.local/bin/.muse-version is not as it was"),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|said| said == "~/.local/bin/muse is not as it was"),
        "{found:?}"
    );
}

#[test]
fn codex_is_watched_where_the_kit_runs_it_and_nowhere_in_a_home_that_is_its_own_folder() {
    let home = Home::new();
    let watch = Watch::of(
        Harness::Codex,
        Path::new("/opt/homebrew/bin/codex"),
        home.0.path(),
    );
    assert_eq!(watch.launchers, [PathBuf::from("/opt/homebrew/bin/codex")]);
    assert!(watch.folders.is_empty() && watch.files.is_empty());
    let pi = Watch::of(
        Harness::Pi,
        Path::new("/opt/homebrew/bin/pi"),
        home.0.path(),
    );
    assert_eq!(
        pi.launchers,
        [
            PathBuf::from("/opt/homebrew/bin/pi"),
            home.path(".local/bin/pi")
        ]
    );
}

#[test]
fn the_guard_fails_what_ends_with_the_way_back_and_is_silent_before_it_looked() {
    let home = Home::new();
    let command = home.claude();
    let guard = Guard::new(Harness::Claude, &command, home.0.path());
    // A guard that was never asked for its variables looked at nothing, and has nothing to say.
    guard.finish().unwrap();
    assert!(guard.vars().unwrap().is_empty());
    let was = fs::read_link(&command).unwrap();
    home.link(".local/bin/claude", Path::new("/gone/claude"));
    // The first look is the one that holds: asked again, it is not taken again.
    assert!(guard.vars().unwrap().is_empty());
    let failed = guard.finish().unwrap_err().to_string();
    assert!(
        failed.contains("THE RUN CHANGED THE OWNER'S CLAUDE LAUNCHER"),
        "{failed}"
    );
    assert!(
        failed.contains(&format!("ln -sfn {} ~/.local/bin/claude", was.display())),
        "{failed}"
    );
    // The way back is not taken by the kit.
    assert_eq!(fs::read_link(&command).unwrap(), Path::new("/gone/claude"));
}

#[test]
fn a_launcher_made_by_a_guard_that_saw_none_is_named_made() {
    let home = Home::new();
    let command = home.path(".local/bin/claude");
    let guard = Guard::new(Harness::Claude, &command, home.0.path());
    guard.vars().unwrap();
    home.claude();
    let failed = guard.finish().unwrap_err().to_string();
    assert!(failed.contains("~/.local/bin/claude was made"), "{failed}");
    assert!(
        failed.contains("~/.local/share/claude/versions was made"),
        "{failed}"
    );
}
