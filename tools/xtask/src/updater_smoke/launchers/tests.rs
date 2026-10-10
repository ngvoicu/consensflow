//! The terminal command, once the app that replaced the installed one has
//! started, as the updater smoke reads it: each command of this home that serves
//! this home runs the update's own `cf` and still pins this home, the app's log says
//! so, and no command that serves another home, in this home's `bin` or in its
//! own, is changed by a byte or spoken of. The commands are scripts of `sh`, and a
//! stand-in `cf` says its version, so these run where `sh` does.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use super::*;

const MARK: &str = "# Installed by ConsensFlow. Runs the app’s own cf.";

fn launcher(home: &Path, program: &Path) -> String {
    format!(
        "#!/bin/sh\n{MARK}\nexport CONSENSFLOW_HOME=\"{}\"\nexec \"{}\" \"$@\"\n",
        home.display(),
        program.display()
    )
}

fn old_shape(home: &Path) -> String {
    format!(
        "#!/bin/sh\n{MARK}\nexport CONSENSFLOW_HOME=\"{}\"\nexec \"/old/node\" \"/old/cf.mjs\" \"$@\"\n",
        home.display()
    )
}

/// A machine as the repair leaves it, with a `cf` that says its version.
struct Machine {
    _parent: tempfile::TempDir,
    sandbox: Sandbox,
    planted: Planted,
    cf: PathBuf,
    /// What the app's log says.
    log: String,
    release: Release,
}

/// Writes `text` as the file `file`, with `mode`.
fn write(file: &Path, text: &str, mode: u32) {
    fs::write(file, text).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(mode)).unwrap();
}

impl Machine {
    /// Sets the command `name` of `home` to `text`, a program.
    fn put(&self, home: &Path, name: &str, text: &str) {
        write(&home.join("bin").join(name), text, 0o755);
    }

    fn state(&self) -> &Path {
        &self.sandbox.state
    }

    fn other(&self) -> &Path {
        &self.sandbox.other
    }

    /// What `assert_repaired` says of the machine, given `version` and `log`.
    fn check_with(&self, version: &str, log: &str) -> Result {
        assert_repaired(&Repaired {
            sandbox: &self.sandbox,
            planted: &self.planted,
            cf: &self.cf,
            app_log: log,
            version,
            release: self.release,
        })
    }

    fn check(&self) -> Result {
        self.check_with("3.0.0-alpha.81", &self.log)
    }

    /// The line the app's log holds for the command `name` of `home`, repaired.
    fn said_of(&self, home: &Path, name: &str) -> String {
        format!(
            "consensflow: the terminal command {} now runs {}\n",
            home.join("bin").join(name).display(),
            self.cf.display()
        )
    }
}

/// The machine, where `repaired` names the commands of this home that serve this
/// home: the other name, if there is one, is another home's command in this
/// home's `bin`, which stays as it was. With `flip` the installed release was the
/// flip's, whose `cf setup` wrote the commands naming its `cf`, which is the
/// update's path: there was nothing to repair, and the log says nothing. Else it
/// was the bridge's, whose named Node.
fn on_a_machine(repaired: &[&'static str], flip: bool) -> Machine {
    let parent = tempfile::tempdir().unwrap();
    let sandbox = Sandbox::make(parent.path()).unwrap();
    let app = sandbox.root.join("ConsensFlow.app");
    for dir in [&sandbox.state, &sandbox.other, &sandbox.probe, &app] {
        fs::create_dir_all(dir.join("bin")).unwrap();
    }
    let cf = app.join("cf");
    write(&cf, "#!/bin/sh\necho 3.0.0-alpha.81\n", 0o755);
    let written = |home: &Path| {
        if flip {
            launcher(home, &cf)
        } else {
            old_shape(home)
        }
    };
    let serving_this_home = |name: &str| repaired.contains(&name);
    let own: Commands = NAMES
        .iter()
        .map(|name| {
            let home = if serving_this_home(name) {
                &sandbox.state
            } else {
                &sandbox.other
            };
            (*name, written(home))
        })
        .collect();
    let other: Commands = NAMES
        .iter()
        .map(|name| (*name, written(&sandbox.other)))
        .collect();
    for name in NAMES {
        let in_this_home = if serving_this_home(name) {
            launcher(&sandbox.state, &cf)
        } else {
            own[name].clone()
        };
        write(&sandbox.state.join("bin").join(name), &in_this_home, 0o755);
        write(&sandbox.other.join("bin").join(name), &other[name], 0o755);
    }
    let machine = Machine {
        planted: Planted {
            own,
            other,
            repaired: repaired.to_vec(),
        },
        log: String::new(),
        release: if flip { Release::Flip } else { Release::Bridge },
        cf,
        sandbox,
        _parent: parent,
    };
    let log = if flip {
        String::new()
    } else {
        repaired
            .iter()
            .map(|name| machine.said_of(machine.state(), name))
            .collect()
    };
    Machine { log, ..machine }
}

fn refusal(result: Result) -> String {
    result.unwrap_err().to_string()
}

mod the_terminal_command_once_the_app_that_replaced_the_installed_one_has_started {
    use super::*;

    #[test]
    fn runs_the_updates_cf_with_the_homes_pin_and_leaves_every_other_command_as_it_was() {
        on_a_machine(&["cf"], false).check().unwrap();
    }

    #[test]
    fn has_both_names_repaired_when_both_serve_the_home() {
        on_a_machine(&NAMES, false).check().unwrap();
    }

    #[test]
    fn is_not_repaired_while_it_still_names_nodes_loses_its_pin_or_its_mark_or_is_not_a_program() {
        type Change = fn(&Machine);
        let scenarios: [(&str, Change, &str); 5] = [
            (
                "still the old shape",
                |machine| machine.put(machine.state(), "cf", &old_shape(machine.state())),
                "does not run",
            ),
            (
                "the pin lost",
                |machine| {
                    let text = format!(
                        "#!/bin/sh\n{MARK}\nexec \"{}\" \"$@\"\n",
                        machine.cf.display()
                    );
                    machine.put(machine.state(), "cf", &text);
                },
                "lost the pin",
            ),
            (
                "the mark lost",
                |machine| {
                    let text = format!(
                        "#!/bin/sh\nexport CONSENSFLOW_HOME=\"{}\"\nexec \"{}\" \"$@\"\n",
                        machine.state().display(),
                        machine.cf.display()
                    );
                    machine.put(machine.state(), "cf", &text);
                },
                "lost its mark",
            ),
            (
                "it runs the cf and names Node's as well",
                |machine| {
                    let text = format!(
                        "{}# was: exec \"/old/node\" \"/old/cf.mjs\"\n",
                        launcher(machine.state(), &machine.cf)
                    );
                    machine.put(machine.state(), "cf", &text);
                },
                "still names Node's",
            ),
            (
                "it cannot be run",
                |machine| {
                    write(
                        &machine.state().join("bin").join("cf"),
                        &launcher(machine.state(), &machine.cf),
                        0o644,
                    );
                },
                "not executable",
            ),
        ];
        for (what, change, words) in scenarios {
            let machine = on_a_machine(&["cf"], false);
            change(&machine);
            let said = refusal(machine.check());
            assert!(said.contains(words), "{what}: {said}");
        }
    }

    #[test]
    fn is_not_repaired_when_the_second_name_was_left_as_it_was_though_it_serves_the_home() {
        let machine = on_a_machine(&NAMES, false);
        machine.put(machine.state(), "consensflow", &old_shape(machine.state()));
        assert!(refusal(machine.check()).contains("does not run"));
    }

    #[test]
    fn is_no_repair_when_it_runs_another_cf_than_the_daemons() {
        let machine = on_a_machine(&["cf"], false);
        let said = refusal(machine.check_with("3.0.0-alpha.99", &machine.log));
        assert!(said.contains("another cf than the daemon"), "{said}");
    }

    #[test]
    fn is_not_own_home_only_when_a_command_of_another_home_was_rewritten_or_the_log_speaks_of_one()
    {
        type Change = fn(&Machine);
        type Log = fn(&Machine) -> String;
        let scenarios: [(&str, Change, Log, &str); 5] = [
            (
                "the command pinned to another home was repaired",
                |machine| {
                    machine.put(
                        machine.state(),
                        "consensflow",
                        &launcher(machine.other(), &machine.cf),
                    );
                },
                |machine| machine.log.clone(),
                "serves another home, changed",
            ),
            (
                "another home's own command was repaired",
                |machine| {
                    machine.put(
                        machine.other(),
                        "cf",
                        &launcher(machine.other(), &machine.cf),
                    );
                },
                |machine| machine.log.clone(),
                "another home's commands changed",
            ),
            (
                "the log does not say the command was repaired",
                |_| {},
                |_| String::new(),
                "does not say",
            ),
            (
                "the log says the command pinned to another home was repaired too",
                |_| {},
                |machine| {
                    format!(
                        "{}{}",
                        machine.log,
                        machine.said_of(machine.state(), "consensflow")
                    )
                },
                "speaks of a command that serves another home",
            ),
            (
                "the log says another home's command was repaired too",
                |_| {},
                |machine| format!("{}{}", machine.log, machine.said_of(machine.other(), "cf")),
                "speaks of a command that serves another home",
            ),
        ];
        for (what, change, log, words) in scenarios {
            let machine = on_a_machine(&["cf"], false);
            change(&machine);
            let said = refusal(machine.check_with("3.0.0-alpha.81", &log(&machine)));
            assert!(said.contains(words), "{what}: {said}");
        }
    }
}

mod the_terminal_command_of_the_flip_release_once_the_update_has_started {
    use super::*;

    #[test]
    fn is_current_it_names_the_cf_of_the_bundle_which_the_updates_is_at_and_is_as_it_was() {
        on_a_machine(&["cf"], true).check().unwrap();
        on_a_machine(&NAMES, true).check().unwrap();
    }

    #[test]
    fn is_not_current_when_the_app_rewrote_it_or_spoke_of_it() {
        type Change = fn(&Machine);
        type Log = fn(&Machine) -> String;
        let scenarios: [(&str, Change, Log, &str); 3] = [
            (
                "a byte of it changed",
                |machine| {
                    let text = format!("{}# repaired\n", launcher(machine.state(), &machine.cf));
                    machine.put(machine.state(), "cf", &text);
                },
                |machine| machine.log.clone(),
                "which was current, changed",
            ),
            (
                "the log says it was repaired",
                |_| {},
                |machine| machine.said_of(machine.state(), "cf"),
                "speaks of a command that was current",
            ),
            (
                "it names another cf than the update's",
                |machine| {
                    machine.put(
                        machine.state(),
                        "cf",
                        &launcher(machine.state(), Path::new("/elsewhere/cf")),
                    );
                },
                |machine| machine.log.clone(),
                "does not run",
            ),
        ];
        for (what, change, log, words) in scenarios {
            let machine = on_a_machine(&["cf"], true);
            change(&machine);
            let said = refusal(machine.check_with("3.0.0-alpha.81", &log(&machine)));
            assert!(said.contains(words), "{what}: {said}");
        }
    }
}

mod planted;

#[test]
fn a_version_is_three_numbers_with_dots_and_anything_after() {
    for text in ["1.2.3", "3.0.0-alpha.81", "10.20.30 and more", "0.0.0\n"] {
        assert!(is_a_version(text), "{text:?}");
    }
    for text in [
        "",
        "1",
        "1.2",
        "1.2.",
        "a.b.c",
        ".1.2.3",
        "v1.2.3",
        "1..3",
        "cf: not found",
    ] {
        assert!(!is_a_version(text), "{text:?}");
    }
}

#[test]
fn a_command_is_asked_its_version_by_the_machine_that_serves_it() {
    let machine = on_a_machine(&["cf"], true);
    assert_eq!(
        version_of(&machine.sandbox, machine.state(), "cf").unwrap(),
        "3.0.0-alpha.81"
    );
    machine.put(
        machine.state(),
        "cf",
        "#!/bin/sh\necho trouble >&2\nexit 3\n",
    );
    let said = version_of(&machine.sandbox, machine.state(), "cf")
        .unwrap_err()
        .to_string();
    assert!(said.ends_with("--version failed: trouble"), "{said}");
    assert_eq!(commands_of(machine.state()).unwrap().len(), 2);
    assert!(commands_of(&machine.sandbox.root).is_err());
}
