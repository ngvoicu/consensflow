//! Which commands an app repairs: only those that serve its own home, which is
//! the home they pin, or, where they pin none, the default one (`~/.consensflow`).
//! The owner runs the Candidate (a home of its own, `~/.consensflow-candidate`) and
//! the installed app on one machine, and a terminal's `cf` is one of them: if
//! each rewrote every command of ours that it found, the `cf` of the terminal
//! would be whichever app started last, running one app's binary against the
//! other's home. A command that serves another home is left byte for byte.

// A test's own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::PathBuf;

use cf_base::env::Env;
use cf_launcher::{repair, Places, Repair};
use common::{
    aged, called, expected, forms, join, old_launcher, outcomes, read, write, Bundle, Home,
};

/// The two apps of one machine, and a command of each in the folders both
/// look in: the live app's, which pins the default home, and the Candidate's,
/// which pins its own. Both name an older build's runtime.
struct Machine {
    home: Home,
    live: Bundle,
    candidate: Bundle,
    old: Bundle,
    candidate_home: String,
    places: Places,
    live_file: PathBuf,
    candidate_file: PathBuf,
}

impl Machine {
    fn new(windows: bool) -> Self {
        let home = Home::new();
        let live = Bundle::new(home.root(), "Live");
        let candidate = Bundle::new(home.root(), "Candidate");
        let old = Bundle::new(home.root(), "Old");
        let candidate_home = home
            .root()
            .join(".consensflow-candidate")
            .display()
            .to_string();
        let (first, second) = (home.root().join("first"), home.root().join("second"));
        let live_file = join(&first, &called("cf", windows));
        let candidate_file = join(&second, &called("cf", windows));
        let live_home = home.default_home().display().to_string();
        write(
            &live_file,
            &old_launcher(windows, &old.node, &old.cf_mjs, Some(&live_home)),
        );
        write(
            &candidate_file,
            &old_launcher(windows, &old.node, &old.cf_mjs, Some(&candidate_home)),
        );
        let places = Places::at(vec![first, second]);
        Self {
            home,
            live,
            candidate,
            old,
            candidate_home,
            places,
            live_file,
            candidate_file,
        }
    }

    /// The live app's environment: no `CONSENSFLOW_HOME`, so the default home.
    fn live_env(&self, windows: bool) -> Env {
        self.home.plain_env(windows)
    }

    /// The Candidate's: `CONSENSFLOW_HOME` is its own, as `isolated_home` sets it.
    fn candidate_env(&self, windows: bool) -> Env {
        self.home.env_with(
            windows,
            &[("CONSENSFLOW_HOME", Some(self.candidate_home.as_str()))],
        )
    }

    /// What a command of each pins and runs, as it was written.
    fn old(&self, windows: bool, pinned: &str) -> String {
        old_launcher(windows, &self.old.node, &self.old.cf_mjs, Some(pinned))
    }
}

#[test]
fn the_live_app_repairs_its_command_and_leaves_the_candidates_byte_for_byte() {
    for windows in forms() {
        let machine = Machine::new(windows);
        let theirs = read(&machine.candidate_file);
        let time = aged(&machine.candidate_file);

        let repaired = repair(
            &machine.live_env(windows),
            &machine.live.cf,
            &machine.places,
        );

        assert_eq!(
            outcomes(&repaired),
            [
                Repair::Absent,
                Repair::Rewritten,
                Repair::Absent,
                Repair::Elsewhere
            ]
        );
        assert_eq!(
            read(&machine.live_file),
            expected(
                windows,
                &machine.live.cf,
                Some(&machine.home.default_home().display().to_string())
            )
        );
        assert_eq!(read(&machine.candidate_file), theirs);
        assert_eq!(
            fs::metadata(&machine.candidate_file)
                .unwrap()
                .modified()
                .unwrap(),
            time,
            "not written"
        );
    }
}

#[test]
fn the_candidate_repairs_its_command_and_leaves_the_live_apps_byte_for_byte() {
    for windows in forms() {
        let machine = Machine::new(windows);
        let theirs = read(&machine.live_file);
        let time = aged(&machine.live_file);

        let repaired = repair(
            &machine.candidate_env(windows),
            &machine.candidate.cf,
            &machine.places,
        );

        assert_eq!(
            outcomes(&repaired),
            [
                Repair::Absent,
                Repair::Elsewhere,
                Repair::Absent,
                Repair::Rewritten
            ]
        );
        assert_eq!(
            read(&machine.candidate_file),
            expected(
                windows,
                &machine.candidate.cf,
                Some(&machine.candidate_home)
            )
        );
        assert_eq!(read(&machine.live_file), theirs);
        assert_eq!(
            fs::metadata(&machine.live_file)
                .unwrap()
                .modified()
                .unwrap(),
            time,
            "not written"
        );
    }
}

#[test]
fn whichever_app_starts_last_each_command_runs_the_cf_of_its_own_apps() {
    for windows in forms() {
        let results: Vec<_> = [true, false]
            .into_iter()
            .map(|live_first| {
                let machine = Machine::new(windows);
                let starts: [(Env, &Bundle); 2] = [
                    (machine.live_env(windows), &machine.live),
                    (machine.candidate_env(windows), &machine.candidate),
                ];
                let order: Vec<_> = if live_first {
                    starts.iter().collect()
                } else {
                    starts.iter().rev().collect()
                };
                for (env, app) in order {
                    repair(env, &app.cf, &machine.places);
                }
                // The paths differ between machines, so what is held is which app's.
                (
                    read(&machine.live_file).contains(&machine.live.cf.display().to_string()),
                    read(&machine.candidate_file)
                        .contains(&machine.candidate.cf.display().to_string()),
                )
            })
            .collect();
        assert_eq!(results, [(true, true), (true, true)]);
    }
}

#[test]
fn a_command_that_pins_no_home_is_the_default_homes_and_the_candidate_leaves_it() {
    for windows in forms() {
        let machine = Machine::new(windows);
        let unpinned = old_launcher(windows, &machine.old.node, &machine.old.cf_mjs, None);
        write(&machine.live_file, &unpinned);
        let time = aged(&machine.live_file);

        let repaired = repair(
            &machine.candidate_env(windows),
            &machine.candidate.cf,
            &machine.places,
        );

        assert_eq!(outcomes(&repaired)[1], Repair::Elsewhere);
        assert_eq!(read(&machine.live_file), unpinned);
        assert_eq!(
            fs::metadata(&machine.live_file)
                .unwrap()
                .modified()
                .unwrap(),
            time
        );
        // And the live app repairs it, still pinning none.
        repair(
            &machine.live_env(windows),
            &machine.live.cf,
            &machine.places,
        );
        assert_eq!(
            read(&machine.live_file),
            expected(windows, &machine.live.cf, None)
        );
    }
}

#[test]
fn a_pin_is_the_home_as_it_was_written_and_one_spelled_otherwise_is_another() {
    // The repair's mistake would be to touch a home that is not its own: a pin
    // that is not the app's home as the app spells it is left, where a Windows
    // path that differs in case only is the same one.
    for windows in forms() {
        let machine = Machine::new(windows);
        let live_home = machine.home.default_home().display().to_string();
        let slash = if windows { '\\' } else { '/' };
        for (pin, served) in [
            (format!("{live_home}{slash}"), false),
            (live_home.to_uppercase(), windows),
        ] {
            write(&machine.live_file, &machine.old(windows, &pin));
            let before = read(&machine.live_file);

            let repaired = repair(
                &machine.live_env(windows),
                &machine.live.cf,
                &machine.places,
            );

            let outcome = if served {
                Repair::Rewritten
            } else {
                Repair::Elsewhere
            };
            assert_eq!(
                outcomes(&repaired)[1],
                outcome,
                "{pin} (windows: {windows})"
            );
            if !served {
                assert_eq!(read(&machine.live_file), before, "{pin}");
            }
        }
    }
}

#[test]
fn an_app_with_no_home_serves_no_command_and_touches_none() {
    for windows in forms() {
        let machine = Machine::new(windows);
        let before = (read(&machine.live_file), read(&machine.candidate_file));
        // An environment with no home in it, saying only whose it is.
        let env = if windows {
            Env::from_vars([("OS", "Windows_NT")])
        } else {
            Env::default()
        };

        let repaired = repair(&env, &machine.live.cf, &machine.places);

        assert_eq!(
            outcomes(&repaired),
            [
                Repair::Absent,
                Repair::Elsewhere,
                Repair::Absent,
                Repair::Elsewhere
            ]
        );
        assert_eq!(
            (read(&machine.live_file), read(&machine.candidate_file)),
            before
        );
    }
}
