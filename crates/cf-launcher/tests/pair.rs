//! A launcher on Windows is a pair, the launch file and the program that reads
//! it (`cf.launch` and `cf.exe`), and never a `.cmd`: cmd.exe ends a command at
//! the first line break of an argument, so a PowerShell user's many-line
//! `cf ask "…"` reached the board as its first line (`crates/cf/tests/
//! windows_shells.rs` is that, run in each shell). The form is Windows's by its
//! environment (`OS=Windows_NT`), so every case here is played on any system.
//!
//! - `cf setup` writes the pair for both names, takes away the `.cmd` of ours a
//!   build before the shim left, and leaves a command of someone else's, a
//!   `.cmd` or a program, where it is, with no launcher under its name.
//! - The app's repair at its start makes that `.cmd` a pair and keeps the program
//!   the bundle's own, and makes no pair where there was no command.

// A test's own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use cf_launcher::{install, repair, runtime, status, Repair};
use common::{
    aged, at, expected, files, launchers, old_launcher, outcomes, program_of, read, script_of,
    write, Bundle, Home, SHIM,
};

/// The two names' launch files, programs and `.cmd`s, in that folder.
fn pairs(home: &Home) -> [(PathBuf, PathBuf, PathBuf); 2] {
    files(home, true).map(|launch| (program_of(&launch), script_of(&launch), launch))
}

/// What the folder holds, by name.
fn held(home: &Home) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(launchers(home))
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A `.cmd` of a build before the shim, which names a `cf` and pins the home.
fn script_naming(cf: &Path, home: &Home) -> String {
    expected(true, cf, home.consensflow().to_str())
}

#[test]
fn setup_writes_the_launch_file_and_the_shim_under_both_names_and_no_cmd() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");

    let installed = install(&home.env(true), &bundle.cf, &at(&home))
        .unwrap()
        .unwrap();

    assert_eq!(
        held(&home),
        [
            "cf.exe",
            "cf.launch",
            "consensflow.exe",
            "consensflow.launch"
        ]
    );
    for (program, _, launch) in pairs(&home) {
        assert_eq!(
            read(&launch),
            expected(true, &bundle.cf, home.consensflow().to_str())
        );
        assert_eq!(read(&program), SHIM);
    }
    // It reports the first name's launch file, where the terminal finds the command.
    assert_eq!(installed.path, launchers(&home).join("consensflow.launch"));
    assert_eq!(
        status(&home.env(true), &at(&home)).unwrap().unwrap().path,
        installed.path
    );
}

#[test]
fn setup_takes_away_the_cmd_of_ours_that_a_build_before_the_shim_left() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    for (_, script, _) in pairs(&home) {
        write(&script, &script_naming(&old.cf, &home));
    }

    install(&home.env(true), &bundle.cf, &at(&home)).unwrap();

    assert_eq!(
        held(&home),
        [
            "cf.exe",
            "cf.launch",
            "consensflow.exe",
            "consensflow.launch"
        ]
    );
}

#[test]
fn setup_leaves_a_cmd_of_someone_else_and_makes_no_launcher_under_its_name() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    let [_, (cf_program, cf_script, cf_launch)] = pairs(&home);
    write(&cf_script, "@echo someone else's\r\n");

    install(&home.env(true), &bundle.cf, &at(&home)).unwrap();

    assert_eq!(read(&cf_script), "@echo someone else's\r\n");
    assert!(!cf_launch.exists() && !cf_program.exists());
    assert_eq!(
        held(&home),
        ["cf.cmd", "consensflow.exe", "consensflow.launch"]
    );
}

#[test]
fn setup_leaves_a_program_of_someone_else_and_makes_no_launcher_under_its_name() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    let [_, (cf_program, _, cf_launch)] = pairs(&home);
    write(&cf_program, "someone else's program");

    install(&home.env(true), &bundle.cf, &at(&home)).unwrap();

    assert_eq!(read(&cf_program), "someone else's program");
    assert!(!cf_launch.exists());
    assert_eq!(
        held(&home),
        ["cf.exe", "consensflow.exe", "consensflow.launch"]
    );
}

#[test]
fn setup_leaves_a_launch_file_of_someone_else_alone() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    let [_, (cf_program, _, cf_launch)] = pairs(&home);
    write(&cf_launch, "not ours\n");

    install(&home.env(true), &bundle.cf, &at(&home)).unwrap();

    assert_eq!(read(&cf_launch), "not ours\n");
    assert!(!cf_program.exists());
}

#[test]
fn setup_run_again_by_another_bundle_names_that_one_and_gives_it_that_ones_shim() {
    let home = Home::new();
    let first = Bundle::new(home.root(), "First");
    let second = Bundle::new(home.root(), "Second");
    write(&second.shim, "the shim of the second bundle");

    install(&home.env(true), &first.cf, &at(&home)).unwrap();
    install(&home.env(true), &second.cf, &at(&home)).unwrap();

    for (program, _, launch) in pairs(&home) {
        assert_eq!(read(&launch), script_naming(&second.cf, &home));
        assert_eq!(read(&program), "the shim of the second bundle");
    }
    assert_eq!(
        held(&home),
        [
            "cf.exe",
            "cf.launch",
            "consensflow.exe",
            "consensflow.launch"
        ]
    );
}

#[test]
fn a_bundle_with_no_shim_makes_no_launcher_and_the_sentence_says_where_it_looked() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    fs::remove_file(&bundle.shim).unwrap();

    let said = install(&home.env(true), &bundle.cf, &at(&home)).unwrap_err();

    assert!(
        said.contains("missing beside") && said.contains(&bundle.cf.display().to_string()),
        "{said}"
    );
    assert!(held(&home).is_empty(), "{:?}", held(&home));
}

#[test]
fn a_cmd_alone_is_still_the_command_a_status_and_a_doctor_find() {
    // Until the app repairs it: a machine whose terminal command was made by a build
    // before the shim, read by this one.
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    let [(_, script, _), _] = pairs(&home);
    write(&script, &script_naming(&bundle.cf, &home));
    let env = home.env(true);

    let found = status(&env, &at(&home)).unwrap().unwrap();
    assert_eq!(found.path, script);
    let wiring = runtime(&env, &bundle.cf, &at(&home)).unwrap().unwrap();
    assert!(wiring.mine && wiring.exists, "{wiring:?}");
}

#[test]
fn a_launch_file_is_found_before_a_cmd_that_a_build_before_the_shim_wrote_again() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    install(&home.env(true), &bundle.cf, &at(&home)).unwrap();
    let [(_, script, launch), _] = pairs(&home);
    write(&script, &script_naming(&old.cf, &home));

    assert_eq!(
        status(&home.env(true), &at(&home)).unwrap().unwrap().path,
        launch
    );
    let wiring = runtime(&home.env(true), &bundle.cf, &at(&home))
        .unwrap()
        .unwrap();
    assert!(wiring.mine, "{wiring:?}");
}

#[test]
fn the_repair_makes_the_cmd_of_a_build_before_the_shim_a_launch_file_and_a_shim() {
    // Each shape such a build wrote: the old (a runtime and a `cf.mjs`) and the
    // native `cf`, pinned to the home the app runs.
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    let [(_, consensflow, _), (_, cf, _)] = pairs(&home);
    write(
        &consensflow,
        &old_launcher(true, &old.node, &old.cf_mjs, home.consensflow().to_str()),
    );
    write(&cf, &script_naming(&old.cf, &home));

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Rewritten]);
    assert_eq!(
        held(&home),
        [
            "cf.exe",
            "cf.launch",
            "consensflow.exe",
            "consensflow.launch"
        ]
    );
    for (program, _, launch) in pairs(&home) {
        assert_eq!(read(&launch), script_naming(&this.cf, &home));
        assert_eq!(read(&program), SHIM);
    }
}

#[test]
fn the_repair_run_again_finds_every_pair_whole_and_writes_nothing() {
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    install(&home.env(true), &this.cf, &at(&home)).unwrap();
    let times: Vec<_> = pairs(&home)
        .iter()
        .flat_map(|(program, _, launch)| [aged(program), aged(launch)])
        .collect();

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Current, Repair::Current]);
    let after: Vec<_> = pairs(&home)
        .iter()
        .flat_map(|(program, _, launch)| [program, launch].map(|file| modified(file)))
        .collect();
    assert_eq!(after, times);
}

fn modified(file: &Path) -> std::time::SystemTime {
    fs::metadata(file).unwrap().modified().unwrap()
}

#[test]
fn the_repair_gives_a_pair_the_shim_of_the_bundle_when_its_own_is_another() {
    // An app that changed the shim: a new release, found by the bytes.
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    install(&home.env(true), &this.cf, &at(&home)).unwrap();
    write(&this.shim, "the shim of a newer release");

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Rewritten]);
    for (program, _, _) in pairs(&home) {
        assert_eq!(read(&program), "the shim of a newer release");
    }
    assert_eq!(
        held(&home),
        [
            "cf.exe",
            "cf.launch",
            "consensflow.exe",
            "consensflow.launch"
        ]
    );
}

#[test]
fn the_repair_takes_away_a_cmd_of_ours_left_beside_a_pair_that_is_whole() {
    // What an older app's `cf setup`, run after the pair was made, writes again.
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    install(&home.env(true), &this.cf, &at(&home)).unwrap();
    let [(_, consensflow, _), (_, cf, _)] = pairs(&home);
    write(&consensflow, &script_naming(&this.cf, &home));
    write(&cf, "@echo someone else's\r\n");

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Current]);
    assert!(!consensflow.exists());
    assert_eq!(read(&cf), "@echo someone else's\r\n");
}

#[test]
fn the_repair_leaves_a_cmd_that_serves_another_home_as_it_is() {
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    let [(_, script, _), _] = pairs(&home);
    let text = old_launcher(true, &old.node, &old.cf_mjs, Some(r"C:\elsewhere\home"));
    write(&script, &text);

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Elsewhere, Repair::Absent]);
    assert_eq!(read(&script), text);
    assert_eq!(held(&home), ["consensflow.cmd"]);
}

#[test]
fn the_repair_makes_no_pair_where_there_was_no_command() {
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Absent, Repair::Absent]);
    assert!(held(&home).is_empty());
}

#[test]
fn the_repair_keeps_the_cmd_that_runs_when_the_bundle_has_no_shim_to_make_a_pair_of() {
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    let [(_, script, _), _] = pairs(&home);
    write(&script, &script_naming(&old.cf, &home));
    fs::remove_file(&this.shim).unwrap();

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    let outcomes = outcomes(&repaired);
    let [Repair::Failed(said), Repair::Absent] = &outcomes[..] else {
        panic!("{repaired:?}");
    };
    assert!(said.contains("missing beside"), "{said}");
    assert_eq!(read(&script), script_naming(&old.cf, &home));
    assert_eq!(held(&home), ["consensflow.cmd"]);
}

#[test]
fn the_repair_mends_a_launch_file_all_the_same_when_the_bundle_has_no_shim_but_the_program_is_there(
) {
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let old = Bundle::new(home.root(), "Old");
    install(&home.env(true), &old.cf, &at(&home)).unwrap();
    fs::remove_file(&this.shim).unwrap();

    let repaired = repair(&home.env(true), &this.cf, &at(&home));

    assert_eq!(outcomes(&repaired), [Repair::Rewritten, Repair::Rewritten]);
    for (program, _, launch) in pairs(&home) {
        assert_eq!(read(&launch), script_naming(&this.cf, &home));
        assert_eq!(read(&program), SHIM);
    }
    // And once mended there is nothing to do, shim or none.
    let again = repair(&home.env(true), &this.cf, &at(&home));
    assert_eq!(outcomes(&again), [Repair::Current, Repair::Current]);
}
