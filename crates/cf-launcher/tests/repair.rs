//! What the repair rewrites: the repair the app runs at its start, in the flip
//! release and in the deletion release. A command of ours that does not name
//! this bundle's `cf` is rewritten in the new shape, with the home it pinned
//! and with no other. The cases are the ones an upgrade meets: an old command,
//! an old command of another home, one that names a `cf` that is gone, and what
//! a user who skipped the flip release has. What it leaves as it is, and says,
//! is `repair_holds.rs`'s.

// A test's own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use cf_launcher::{install, repair, runtime, Places, Repair};
use common::{
    at, called, expected, files, forms, join, old_launcher, outcomes, read, write, Bundle, Home,
};

#[test]
fn an_old_command_is_rewritten_in_the_new_shape_by_both_its_names() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let env = home.env(windows);
        for file in files(&home, windows) {
            write(&file, &old_launcher(windows, &old.node, &old.cf_mjs, None));
        }

        let repaired = repair(&env, &this.cf, &at(&home));

        assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Rewritten]);
        assert_eq!(
            repaired
                .iter()
                .map(|each| each.path.clone())
                .collect::<Vec<_>>(),
            files(&home, windows)
        );
        for file in files(&home, windows) {
            assert_eq!(read(&file), expected(windows, &this.cf, None));
        }
    }
}

#[cfg(unix)]
#[test]
fn a_rewritten_command_is_a_program_the_system_runs() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    let [file, _] = files(&home, false);
    write(&file, &old_launcher(false, &old.node, &old.cf_mjs, None));
    // The mode an old file may have lost: the repair gives it back.
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();

    repair(&home.env(false), &this.cf, &at(&home));

    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn a_cf_named_in_the_verbatim_spelling_is_written_and_known_in_the_plain_one() {
    // Tauri may answer its folders as `\\?\C:\…`, which cmd.exe starts no
    // program through: the command names the plain spelling, and the same
    // `cf` asked for in either is the command's.
    let home = Home::new();
    let env = home.env(true);
    let [file, _] = files(&home, true);
    for (verbatim, plain) in [
        (r"\\?\C:\App\cli\bin\cf.exe", r"C:\App\cli\bin\cf.exe"),
        (
            r"\\?\UNC\server\share\cli\bin\cf.exe",
            r"\\server\share\cli\bin\cf.exe",
        ),
    ] {
        let cf = Path::new(verbatim);
        install(&env, cf, &at(&home)).unwrap();
        assert_eq!(
            read(&file),
            expected(true, Path::new(plain), home.consensflow().to_str())
        );
        assert!(runtime(&env, cf, &at(&home)).unwrap().unwrap().mine);
        let repaired = repair(&env, cf, &at(&home));
        assert_eq!(outcomes(&repaired), [Repair::Current, Repair::Current]);
        // A command that named the verbatim one, as an app that did not strip it wrote.
        let old = old_launcher(
            true,
            Path::new(r"C:\n.exe"),
            Path::new(r"C:\x\cf.mjs"),
            None,
        );
        write(&file, &old);
        repair(&env, cf, &at(&home));
        assert_eq!(read(&file), expected(true, Path::new(plain), None));
    }
}

#[test]
fn an_old_command_keeps_the_home_it_pinned() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let candidate = home
            .root()
            .join(".consensflow-candidate")
            .display()
            .to_string();
        for file in files(&home, windows) {
            write(
                &file,
                &old_launcher(windows, &old.node, &old.cf_mjs, Some(&candidate)),
            );
        }

        repair(&home.env(windows), &this.cf, &at(&home));

        for file in files(&home, windows) {
            assert_eq!(read(&file), expected(windows, &this.cf, Some(&candidate)));
        }
    }
}

#[test]
fn an_old_command_of_another_home_keeps_that_home_and_not_the_one_that_repairs_it() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        // The environment names a home of its own, which the command does not.
        let other = home.root().join("another-home").display().to_string();
        let [file, _] = files(&home, windows);
        write(
            &file,
            &old_launcher(windows, &old.node, &old.cf_mjs, Some(&other)),
        );
        let env = home.env(windows);
        assert_ne!(env.text("CONSENSFLOW_HOME"), Some(other.as_str()));

        repair(&env, &this.cf, &at(&home));

        assert_eq!(read(&file), expected(windows, &this.cf, Some(&other)));
    }
}

#[test]
fn an_old_command_that_pins_no_home_pins_none_where_the_environment_names_one() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let [file, _] = files(&home, windows);
        write(&file, &old_launcher(windows, &old.node, &old.cf_mjs, None));
        // The environment has a CONSENSFLOW_HOME, as the Candidate's has.
        let env = home.env(windows);
        assert!(env.text("CONSENSFLOW_HOME").is_some());

        repair(&env, &this.cf, &at(&home));

        assert_eq!(
            read(&file),
            expected(windows, &this.cf, None),
            "the repair changes what runs, never which home a command talks to"
        );
    }
}

#[test]
fn a_command_that_runs_another_bundles_cf_is_rewritten_to_this_one() {
    // A portable runtime's folder is named by its version: the next version
    // leaves the command naming a `cf` that is gone, and an app moved to
    // another folder leaves one that is not.
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let other = Bundle::new(home.root(), "Other");
        let gone = home
            .root()
            .join("runtime-0.78")
            .join("cli")
            .join("bin")
            .join("cf");
        let env = home.env(windows);
        let [file, _] = files(&home, windows);
        for named in [other.cf.as_path(), gone.as_path()] {
            install(&env, named, &at(&home)).unwrap();
            let repaired = repair(&env, &this.cf, &at(&home));
            assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Rewritten]);
            assert_eq!(
                read(&file),
                expected(windows, &this.cf, home.consensflow().to_str())
            );
        }
    }
}

#[test]
fn a_command_of_ours_that_says_nothing_either_shape_does_is_rewritten_with_the_pin_it_has() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let [pinned, plain] = files(&home, windows);
        let strange = |pin: &str| {
            if windows {
                format!("@echo off\r\nREM Installed by ConsensFlow.\r\n{pin}echo hello\r\n")
            } else {
                format!("#!/bin/sh\n# Installed by ConsensFlow.\n{pin}echo hello\n")
            }
        };
        let pin = if windows {
            "set \"CONSENSFLOW_HOME=C:\\kept\"\r\n"
        } else {
            "export CONSENSFLOW_HOME=\"/kept\"\n"
        };
        write(&pinned, &strange(pin));
        write(&plain, &strange(""));

        repair(&home.env(windows), &this.cf, &at(&home));

        let kept = if windows { "C:\\kept" } else { "/kept" };
        assert_eq!(read(&pinned), expected(windows, &this.cf, Some(kept)));
        assert_eq!(read(&plain), expected(windows, &this.cf, None));
    }
}

#[test]
fn every_place_given_is_looked_in_in_order() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let old = Bundle::new(home.root(), "Old");
        let (first, second) = (home.root().join("first"), home.root().join("second"));
        for folder in [&first, &second] {
            write(
                &join(folder, &called("cf", windows)),
                &old_launcher(windows, &old.node, &old.cf_mjs, None),
            );
        }
        let places = Places::at(vec![first.clone(), second.clone()]);

        let repaired = repair(&home.env(windows), &this.cf, &places);

        assert_eq!(
            outcomes(&repaired),
            [
                Repair::Absent,
                Repair::Rewritten,
                Repair::Absent,
                Repair::Rewritten
            ]
        );
        for folder in [first, second] {
            assert_eq!(
                read(&join(&folder, &called("cf", windows))),
                expected(windows, &this.cf, None)
            );
        }
    }
}
