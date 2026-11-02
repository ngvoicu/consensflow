// The tests that hold on every machine, with a launcher that is a file as Windows installs one
// (`%USERPROFILE%\.local\bin\claude.exe` is a copy, not a link); the Unix ones, which make links
// and scripts, are in `links`.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

#[cfg(unix)]
mod links;

/// A pretend owner's home.
struct Home(tempfile::TempDir);

impl Home {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn path(&self, below: &str) -> PathBuf {
        self.0.path().join(below)
    }

    fn write(&self, below: &str, text: &str) {
        let file = self.path(below);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
    }
}

/// A harness's program as its installer names it on this machine.
fn program(harness: &str) -> String {
    if cfg!(windows) {
        format!("{harness}.exe")
    } else {
        harness.to_owned()
    }
}

/// Where Claude Code's installer puts its launcher, here.
fn claude_launcher(home: &Home) -> PathBuf {
    home.path(&format!(".local/bin/{}", program("claude")))
}

/// What the watch of `watch` says differs after `act`, said below the owner's `home`.
fn changes(watch: &Watch, home: &Home, act: impl FnOnce()) -> Vec<String> {
    let before = watch.look();
    act();
    before.changes_to(&watch.look(), home.0.path())
}

#[test]
fn the_switches_are_claude_codes_two_and_muses_one_and_every_window_has_all_three() {
    assert_eq!(
        updates_off_of(Harness::Claude),
        [("DISABLE_AUTOUPDATER", "1"), ("DISABLE_UPDATES", "1")]
    );
    assert_eq!(
        updates_off_of(Harness::Muse),
        [("MUSE_NO_AUTO_UPDATE", "1")]
    );
    for other in [Harness::Codex, Harness::Pi, Harness::Devin] {
        assert!(updates_off_of(other).is_empty(), "{other:?}");
    }
    let every: Vec<_> = updates_off().collect();
    assert_eq!(
        every,
        [
            ("DISABLE_AUTOUPDATER", "1"),
            ("DISABLE_UPDATES", "1"),
            ("MUSE_NO_AUTO_UPDATE", "1"),
        ]
    );
}

#[test]
fn a_launcher_that_is_a_file_is_hashed_and_named_in_the_summary_and_left_alone_it_is_no_change() {
    let home = Home::new();
    home.write(
        &format!(".local/bin/{}", program("claude")),
        "claude 2.1.316",
    );
    let command = claude_launcher(&home);
    let watch = Watch::of(Harness::Claude, &command, home.0.path());
    let seen = watch.look();
    assert!(seen.changes_to(&watch.look(), home.0.path()).is_empty());
    // The program the kit runs and the one the installer writes are the one file, said once.
    let summary = seen.summary(home.0.path());
    assert_eq!(summary.matches("claude").count(), 1, "{summary}");
    assert!(summary.contains(&program("claude")), "{summary}");
    assert!(
        summary
            .rsplit(' ')
            .next()
            .is_some_and(|hash| hash.len() == 12),
        "{summary}"
    );
}

#[test]
fn a_launcher_that_is_a_file_and_is_overwritten_is_named_though_no_link_moved() {
    let home = Home::new();
    home.write(
        &format!(".local/bin/{}", program("claude")),
        "claude 2.1.316",
    );
    let command = claude_launcher(&home);
    let found = changes(
        &Watch::of(Harness::Claude, &command, home.0.path()),
        &home,
        || {
            home.write(
                &format!(".local/bin/{}", program("claude")),
                "claude 2.1.317",
            )
        },
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains(&program("claude")), "{found:?}");
    assert!(found[0].ends_with("is not as it was"), "{found:?}");
}

#[test]
fn a_launcher_that_is_a_file_and_is_removed_or_made_is_named_for_it() {
    let home = Home::new();
    let name = program("claude");
    let command = claude_launcher(&home);
    let watch = Watch::of(Harness::Claude, &command, home.0.path());
    let found = changes(&watch, &home, || {
        home.write(&format!(".local/bin/{name}"), "a launcher")
    });
    assert!(
        found
            .iter()
            .any(|said| said.ends_with(&format!("{name} was made"))),
        "{found:?}"
    );
    let found = changes(&watch, &home, || fs::remove_file(&command).unwrap());
    assert!(
        found
            .iter()
            .any(|said| said.ends_with(&format!("{name} is gone"))),
        "{found:?}"
    );
}

#[test]
fn a_version_installed_beside_a_launcher_that_is_a_file_is_named() {
    let home = Home::new();
    home.write(
        &format!(".local/bin/{}", program("claude")),
        "claude 2.1.316",
    );
    home.write(".local/share/claude/versions/2.1.316", "claude 2.1.316");
    let command = claude_launcher(&home);
    let found = changes(
        &Watch::of(Harness::Claude, &command, home.0.path()),
        &home,
        || home.write(".local/share/claude/versions/2.1.317", "newer"),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].ends_with("versions gained 2.1.317"), "{found:?}");
}

#[test]
fn devins_launcher_as_a_file_and_its_folder_of_versions_are_watched_too() {
    let home = Home::new();
    home.write(&format!(".local/bin/{}", program("devin")), "devin");
    home.write(".local/share/devin/cli/_versions/3000.11.1/devin", "devin");
    let command = home.path(&format!(".local/bin/{}", program("devin")));
    let found = changes(
        &Watch::of(Harness::Devin, &command, home.0.path()),
        &home,
        || {
            home.write(".local/share/devin/cli/_versions/3000.11.2/devin", "devin2");
            home.write(&format!(".local/bin/{}", program("devin")), "devin2");
        },
    );
    assert!(
        found
            .iter()
            .any(|said| said.ends_with("_versions gained 3000.11.2")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|said| said.ends_with("is not as it was")),
        "{found:?}"
    );
}

#[test]
fn the_guard_of_a_launcher_that_is_a_file_fails_what_ends_and_names_it_and_is_silent_before_it_looked(
) {
    let home = Home::new();
    home.write(
        &format!(".local/bin/{}", program("claude")),
        "claude 2.1.316",
    );
    let command = claude_launcher(&home);
    let guard = Guard::new(Harness::Claude, &command, home.0.path());
    // A guard that was never asked for its variables looked at nothing.
    guard.finish().unwrap();
    assert!(guard.vars().unwrap().is_empty());
    guard.finish().unwrap();
    // An updater that wrote over it.
    home.write(
        &format!(".local/bin/{}", program("claude")),
        "claude 2.1.317",
    );
    let failed = guard.finish().unwrap_err().to_string();
    assert!(
        failed.contains("THE RUN CHANGED THE OWNER'S CLAUDE LAUNCHER"),
        "{failed}"
    );
    assert!(failed.contains(&program("claude")), "{failed}");
    assert!(failed.contains("is not as it was"), "{failed}");
    // A file is no link, so there is no `ln` to say; the kit does not put it back.
    assert!(!failed.contains("ln -sfn"), "{failed}");
    assert_eq!(
        fs::read_to_string(&command).unwrap(),
        "claude 2.1.317",
        "the kit never writes the owner's launcher"
    );
}

#[test]
fn the_hash_of_a_big_file_reads_it_whole_and_tells_two_files_of_one_size_apart() {
    let folder = tempfile::tempdir().unwrap();
    let a = folder.path().join("a");
    let b = folder.path().join("b");
    // Bigger than the buffer, and one byte apart at the end.
    let mut bytes = vec![7_u8; (1 << 20) * 2 + 5];
    fs::write(&a, &bytes).unwrap();
    *bytes.last_mut().unwrap() = 8;
    fs::write(&b, &bytes).unwrap();
    let (a, b) = (Bytes::of(&a).unwrap(), Bytes::of(&b).unwrap());
    assert_eq!(a.size, b.size);
    assert_ne!(a.hash, b.hash);
    assert_eq!(a.hash.len(), 64);
    assert!(Bytes::of(&folder.path().join("missing")).is_none());
}

#[test]
fn a_path_followed_through_a_link_is_said_below_the_home_the_run_was_given() {
    // macOS spells its temporary folders through `/private/var`, and Windows its canonical paths
    // with a `\\?\` prefix: the home as it was given and as the system spells it are one.
    let home = Home::new();
    home.write("a/b", "x");
    let real = fs::canonicalize(home.path("a/b")).unwrap();
    let shown = shown(&real, home.0.path());
    assert!(
        shown.starts_with("~/") || shown.starts_with("~\\"),
        "{shown}"
    );
    assert!(shown.contains('b'), "{shown}");
    assert!(
        !shown.contains(&home.0.path().display().to_string()),
        "{shown}"
    );
}
