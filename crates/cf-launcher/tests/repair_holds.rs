//! What the repair leaves as it is, and says of what it could not do: an
//! unmarked command, a missing one (none is made, nor the folder it would be
//! in), one that already runs this bundle, a program that merely holds the
//! mark, an app run from a place macOS takes away, a repair run twice, and the
//! commands that cannot be read or written. What it rewrites is `repair.rs`'s.

// A test's own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use cf_launcher::{install, repair, Places, Repair};
use common::{
    aged, at, expected, files, forms, launchers, old_launcher, outcomes, read, write, Bundle, Home,
};

#[test]
fn an_unmarked_command_is_left_alone_byte_for_byte_and_so_is_its_time() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let theirs = if windows {
            "@echo off\r\nREM someone else's cf\r\n"
        } else {
            "#!/bin/sh\n# someone else's cf\nexec echo hello\n"
        };
        let found = files(&home, windows);
        for file in &found {
            write(file, theirs);
        }
        let times = found.each_ref().map(|file| aged(file));

        let repaired = repair(&home.env(windows), &this.cf, &at(&home));

        assert_eq!(outcomes(&repaired), [Repair::Unmarked, Repair::Unmarked]);
        for (file, time) in found.iter().zip(times) {
            assert_eq!(read(file), theirs);
            assert_eq!(fs::metadata(file).unwrap().modified().unwrap(), time);
        }
    }
}

#[cfg(unix)]
#[test]
fn a_link_to_the_cf_itself_is_no_command_of_ours_and_the_cf_is_never_written() {
    // The plainest way to put a command on PATH is a link to the program, and
    // the `cf` of this build holds the mark in its bytes, as the program that
    // writes launchers must. A repair writes through a link: it must not find
    // the program ours.
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let program = b"\x7fELF\0\0\0Installed by ConsensFlow\0\0\0 the program itself";
    fs::write(&this.cf, program).unwrap();
    let [_, cf_link] = files(&home, false);
    fs::create_dir_all(launchers(&home)).unwrap();
    std::os::unix::fs::symlink(&this.cf, &cf_link).unwrap();
    let env = home.env(false);

    let repaired = repair(&env, &this.cf, &at(&home));
    assert_eq!(outcomes(&repaired), [Repair::Absent, Repair::Unmarked]);
    install(&env, &this.cf, &at(&home)).unwrap();

    assert_eq!(fs::read(&this.cf).unwrap(), program, "the cf is as it was");
    assert!(fs::symlink_metadata(&cf_link).unwrap().is_symlink());
    assert_eq!(
        cf_launcher::status(&env, &at(&home)).unwrap().unwrap().path,
        common::join(&launchers(&home), "consensflow"),
        "the command that is made is the other name"
    );
}

#[test]
fn of_two_names_the_one_that_is_ours_is_rewritten_and_the_one_that_is_not_is_not() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let [ours, theirs] = files(&home, windows);
        write(&ours, &old_launcher(windows, &old.node, &old.cf_mjs, None));
        write(&theirs, "someone else's\n");

        let repaired = repair(&home.env(windows), &this.cf, &at(&home));

        assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Unmarked]);
        assert_eq!(read(&ours), expected(windows, &this.cf, None));
        assert_eq!(read(&theirs), "someone else's\n");
    }
}

#[test]
fn a_missing_command_is_never_made_and_nor_is_the_folder_it_would_be_in() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let env = home.env(windows);

        // No folder: none is made.
        let repaired = repair(&env, &this.cf, &at(&home));
        assert_eq!(outcomes(&repaired), [Repair::Absent, Repair::Absent]);
        assert!(!launchers(&home).exists());

        // A folder with nothing in it: nothing is put in it.
        fs::create_dir_all(launchers(&home)).unwrap();
        repair(&env, &this.cf, &at(&home));
        assert_eq!(fs::read_dir(launchers(&home)).unwrap().count(), 0);

        // The home's own, by default: a machine that never ran `cf setup`.
        repair(&env, &this.cf, &Places::default());
        assert!(!home.consensflow().exists());
    }
}

#[test]
fn a_command_that_is_there_under_one_name_is_not_made_under_the_other() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let [consensflow, cf] = files(&home, windows);
        write(&cf, &old_launcher(windows, &old.node, &old.cf_mjs, None));

        let repaired = repair(&home.env(windows), &this.cf, &at(&home));

        assert_eq!(outcomes(&repaired), [Repair::Absent, Repair::Rewritten]);
        assert!(!consensflow.exists());
    }
}

#[test]
fn a_repair_run_twice_writes_nothing_the_second_time() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let candidate = home
            .root()
            .join(".consensflow-candidate")
            .display()
            .to_string();
        let found = files(&home, windows);
        write(
            &found[0],
            &old_launcher(windows, &old.node, &old.cf_mjs, Some(&candidate)),
        );
        write(
            &found[1],
            &old_launcher(windows, &old.node, &old.cf_mjs, None),
        );
        let env = home.env(windows);

        let first = repair(&env, &this.cf, &at(&home));
        assert_eq!(outcomes(&first), [Repair::Rewritten, Repair::Rewritten]);
        let times = found.each_ref().map(|file| aged(file));
        let texts = found.each_ref().map(|file| read(file));

        let second = repair(&env, &this.cf, &at(&home));

        assert_eq!(outcomes(&second), [Repair::Current, Repair::Current]);
        assert_eq!(found.each_ref().map(|file| read(file)), texts);
        for (file, time) in found.iter().zip(times) {
            assert_eq!(
                fs::metadata(file).unwrap().modified().unwrap(),
                time,
                "not written"
            );
        }
        // And again: the same result however many times.
        assert_eq!(
            outcomes(&repair(&env, &this.cf, &at(&home))),
            outcomes(&second)
        );
    }
}

#[test]
fn a_command_that_already_runs_this_cf_is_left_as_it_is_whatever_else_it_says() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let [file, _] = files(&home, windows);
        install(&home.env(windows), &this.cf, &at(&home)).unwrap();
        // A user added a line to a command that still runs this bundle.
        let edited = format!(
            "{}{}",
            read(&file),
            if windows { "REM mine\r\n" } else { "# mine\n" }
        );
        write(&file, &edited);
        let time = aged(&file);

        let repaired = repair(&home.env(windows), &this.cf, &at(&home));

        assert_eq!(outcomes(&repaired), [Repair::Current, Repair::Current]);
        assert_eq!(read(&file), edited);
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), time);
    }
}

#[test]
fn an_app_opened_from_where_it_was_downloaded_leaves_the_command_as_it_is() {
    // Opened from Downloads, an app runs from a copy macOS makes for the run
    // and takes away: a command that named it would work until the app closed.
    // The app that is installed repairs the command at its own start.
    for windows in forms() {
        let home = Home::new();
        let old = Bundle::new(home.root(), "Old");
        let translocated = home
            .root()
            .join("private/var/folders/xy/T/AppTranslocation/4F2A-90C1/d/ConsensFlow.app")
            .join("Contents/Resources/cli/bin/cf");
        let env = home.env(windows);
        let [file, other] = files(&home, windows);
        for name in [&file, &other] {
            write(name, &old_launcher(windows, &old.node, &old.cf_mjs, None));
        }
        let before = (read(&file), read(&other));

        let repaired = repair(&env, &translocated, &at(&home));

        assert_eq!(outcomes(&repaired), [Repair::Transient, Repair::Transient]);
        assert_eq!((read(&file), read(&other)), before);

        // `cf setup` is the user's own act and names what it is run from. A
        // command that names that very place is the place's, and one of
        // someone else's is no concern of the guard's.
        install(&env, &translocated, &at(&home)).unwrap();
        write(&other, "someone else's\n");
        let repaired = repair(&env, &translocated, &at(&home));
        assert_eq!(outcomes(&repaired), [Repair::Current, Repair::Unmarked]);
    }
}

#[test]
fn a_command_that_cannot_be_read_is_said_and_the_other_name_is_repaired_all_the_same() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let [consensflow, cf] = files(&home, windows);
        // A folder where the command should be.
        fs::create_dir_all(&consensflow).unwrap();
        write(&cf, &old_launcher(windows, &old.node, &old.cf_mjs, None));

        let repaired = repair(&home.env(windows), &this.cf, &at(&home));

        assert_eq!(
            repaired[0].outcome,
            Repair::Failed("EISDIR: illegal operation on a directory, read".to_owned())
        );
        assert_eq!(repaired[1].outcome, Repair::Rewritten);
        assert!(consensflow.is_dir());
    }
}

#[cfg(unix)]
#[test]
fn a_command_that_cannot_be_written_is_said_with_the_call_that_failed() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    let [file, other] = files(&home, false);
    write(&file, &old_launcher(false, &old.node, &old.cf_mjs, None));
    write(&other, &old_launcher(false, &old.node, &old.cf_mjs, None));
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(&file).is_ok() {
        return; // Root may write any file.
    }

    let repaired = repair(&home.env(false), &this.cf, &at(&home));

    assert_eq!(
        repaired[0].outcome,
        Repair::Failed(format!(
            "EACCES: permission denied, open '{}'",
            file.display()
        ))
    );
    assert_eq!(
        repaired[1].outcome,
        Repair::Rewritten,
        "the other is repaired all the same"
    );
}

#[test]
fn where_there_is_no_home_to_look_in_there_is_nothing_to_repair() {
    let this = Bundle::new(tempfile::tempdir().unwrap().path(), "This");
    assert_eq!(
        repair(&cf_base::env::Env::default(), &this.cf, &Places::default()),
        []
    );
}
