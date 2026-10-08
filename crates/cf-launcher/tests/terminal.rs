//! The app can put its own CLI on your PATH, as Node's terminal suite held it:
//! the eight sentences of that suite, each in the form of a launcher `sh` runs
//! and in the form cmd.exe does (`OS=Windows_NT` makes it, on any system),
//! where Node's ran the form of the system it was on.
//!
//! Where a sentence names the runtime Node's launcher ran (`process.execPath`
//! and its `cf.mjs`) the new one names the native `cf`, and says so.

// A test's own folders and files: a failure in them is the test's.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use cf_launcher::{install, runtime, status, Places, Shape};
use common::{at, called, forms, join, launchers, old_launcher, read, write, Bundle, Home};

#[test]
fn reports_nothing_installed_to_begin_with() {
    let home = Home::new();
    let bundle = Bundle::new(home.root(), "This");
    for windows in forms() {
        let env = home.env(windows);
        assert_eq!(runtime(&env, &bundle.cf, &at(&home)).unwrap(), None);
        assert_eq!(status(&env, &at(&home)).unwrap(), None);
    }
}

#[test]
fn writes_a_launcher_that_runs_this_very_copy() {
    for windows in forms() {
        let home = Home::new();
        let bundle = Bundle::new(home.root(), "This");
        let outcome = install(&home.env(windows), &bundle.cf, &at(&home)).unwrap();

        assert!(
            outcome.is_some(),
            "installed, in the form of cmd: {windows}"
        );
        let launcher = join(&launchers(&home), &called("consensflow", windows));
        assert!(launcher.exists());
        // It must point at the program running right now, so the terminal and
        // the app can never drift apart. Node's named a runtime and a
        // `cf.mjs`; this names the native `cf`, which is the whole command.
        let script = read(&launcher);
        assert!(
            script.contains(&bundle.cf.display().to_string()),
            "{script}"
        );
        assert!(!script.contains("cf.mjs"), "{script}");
        #[cfg(unix)]
        if !windows {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&launcher).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "must be executable");
        }
    }
}

#[test]
fn says_where_it_went_and_whether_that_place_is_on_path() {
    for windows in forms() {
        let home = Home::new();
        let bundle = Bundle::new(home.root(), "This");
        let on = launchers(&home).display().to_string();
        let env = home.env_with(windows, &[("PATH", Some(&on))]);
        let status = install(&env, &bundle.cf, &at(&home)).unwrap().unwrap();
        assert_eq!(
            status.path,
            join(&launchers(&home), &called("consensflow", windows))
        );
        assert_eq!(status.dir, launchers(&home));
        assert!(status.on_path);

        let env = home.env_with(windows, &[("PATH", Some("/nowhere"))]);
        let elsewhere = install(&env, &bundle.cf, &at(&home)).unwrap().unwrap();
        assert!(!elsewhere.on_path);
    }
}

#[test]
fn says_whether_the_command_runs_this_copy_or_another_consensflow() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let env = home.env(windows);
        install(&env, &this.cf, &at(&home)).unwrap();

        // Node's `ours.runtime == process.execPath`: a native command has no
        // runtime of its own, and what it runs is the `cf` it names.
        let ours = runtime(&env, &this.cf, &at(&home)).unwrap().unwrap();
        assert_eq!(ours.shape, Shape::Native);
        assert_eq!(ours.runtime, this.cf.to_string_lossy());
        assert!(ours.exists);
        assert!(ours.mine);

        // Two ConsensFlows on one machine, an app beside a repo build, and the
        // command names the other one. It exists, so every other check calls
        // this healthy while every `cf` the skill teaches runs the other
        // one's code. The other is in the old shape, as one an older build
        // wrote is, and then in the new one.
        let other = Bundle::new(home.root(), "Other");
        let launcher = join(&launchers(&home), &called("consensflow", windows));
        write(
            &launcher,
            &old_launcher(windows, &other.node, &other.cf_mjs, None),
        );
        let theirs = runtime(&env, &this.cf, &at(&home)).unwrap().unwrap();
        assert!(
            theirs.exists,
            "it is there, which is what made this invisible"
        );
        assert!(!theirs.mine);
        // And it says WHICH copy, because a developer syncing a build has to
        // write into the one the command runs, not the one they just built.
        assert_eq!(theirs.entry, other.cf_mjs.to_string_lossy());
        assert_eq!(theirs.runtime, other.node.to_string_lossy());

        install(&env, &other.cf, &at(&home)).unwrap();
        let theirs = runtime(&env, &this.cf, &at(&home)).unwrap().unwrap();
        assert!(theirs.exists && !theirs.mine);
        assert_eq!(theirs.entry, other.cf.to_string_lossy());
    }
}

#[test]
fn a_command_that_runs_the_cf_mjs_beside_this_cf_is_this_copys_though_it_is_the_old_shape() {
    // The flip release still holds `cf.mjs` beside `cf`: a command that names
    // it runs this bundle, and is waiting for the repair to say so in the new
    // shape.
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        write(
            &join(&launchers(&home), &called("consensflow", windows)),
            &old_launcher(windows, &this.node, &this.cf_mjs, None),
        );
        let found = runtime(&home.env(windows), &this.cf, &at(&home))
            .unwrap()
            .unwrap();
        assert_eq!(found.shape, Shape::Node);
        assert!(found.exists && found.mine);
    }
}

#[cfg(not(windows))]
#[test]
fn says_when_the_command_runs_the_installed_release_which_development_must_never_write_into() {
    // The release bundle is macOS-shaped; the Windows installer has its own.
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let plist = |app: &str, identifier: &str| {
        write(
            &home.root().join(format!("{app}.app/Contents/Info.plist")),
            &format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleIdentifier</key>\n\t<string>{identifier}</string>\n</dict>\n</plist>\n"),
        );
    };
    let env = home.env(false);
    let launcher = join(&launchers(&home), "consensflow");
    for old in [true, false] {
        for (app, identifier, live) in [
            ("Live", "dev.ngvoicu.consensflow", true),
            ("Candidate", "dev.ngvoicu.consensflow.candidate", false),
        ] {
            let bundle = Bundle::new(home.root(), app);
            plist(app, identifier);
            if old {
                write(
                    &launcher,
                    &old_launcher(false, &bundle.node, &bundle.cf_mjs, None),
                );
            } else {
                install(&env, &bundle.cf, &at(&home)).unwrap();
            }
            let found = runtime(&env, &this.cf, &at(&home)).unwrap().unwrap();
            assert_eq!(found.live, live, "{app}, old shape: {old}");
        }
    }
    // A checkout is not a bundle.
    let checkout = home.root().join("checkout").join("bin").join("cf");
    write(&checkout, "");
    install(&env, &checkout, &at(&home)).unwrap();
    let found = runtime(&env, &this.cf, &at(&home)).unwrap().unwrap();
    assert!(!found.live, "a checkout is not a bundle");
}

#[test]
fn keeps_a_separate_home_when_run_from_a_terminal_that_does_not_name_one() {
    // The candidate's launcher is run from an ordinary terminal, where no
    // CONSENSFLOW_HOME is set: without the pin it would fall back to the live
    // ~/.consensflow.
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let env = home.env(windows);
        install(&env, &this.cf, &at(&home)).unwrap();
        let script = read(&join(&launchers(&home), &called("cf", windows)));
        let consensflow = home.consensflow().display().to_string();
        let pin = if windows {
            format!("set \"CONSENSFLOW_HOME={consensflow}\"")
        } else {
            format!("export CONSENSFLOW_HOME=\"{consensflow}\"")
        };
        assert!(script.contains(&pin), "{script}");
        assert!(runtime(&env, &this.cf, &at(&home)).unwrap().unwrap().mine);

        let plain = home.root().join("plain-bin");
        fs::create_dir(&plain).unwrap();
        let places = Places::at(vec![plain.clone()]);
        install(&home.plain_env(windows), &this.cf, &places).unwrap();
        assert!(!read(&join(&plain, &called("cf", windows))).contains("CONSENSFLOW_HOME"));
    }
}

#[test]
fn explains_itself_when_no_candidate_directory_can_be_written() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        // A file where a directory is wanted cannot be written into, anywhere.
        let blocked = home.root().join("blocked");
        write(&blocked, "");
        let candidate = blocked.join("bin");
        let places = Places::at(vec![candidate.clone()]);
        let failed = install(&home.env(windows), &this.cf, &places).unwrap_err();
        assert_eq!(
            failed,
            format!(
                "no writable directory for the command (tried {}) — create one, or add it yourself",
                candidate.display()
            )
        );
    }
}

#[test]
fn the_first_candidate_that_can_be_written_takes_the_command_and_a_missing_one_is_made() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let blocked = home.root().join("blocked");
        write(&blocked, "");
        let candidates = vec![
            blocked.join("bin"),
            home.root().join("first").join("bin"),
            home.root().join("second"),
        ];
        let outcome = install(
            &home.env(windows),
            &this.cf,
            &Places::at(candidates.clone()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(outcome.dir, candidates[1]);
        assert!(join(&candidates[1], &called("cf", windows)).exists());
        // A user-owned folder is made wherever it is missing, as Node made each
        // before it asked which could be written; but only one takes the command.
        assert!(candidates[2].is_dir());
        assert_eq!(fs::read_dir(&candidates[2]).unwrap().count(), 0);
    }
}

#[test]
fn a_command_by_the_short_name_alone_is_not_the_command_that_is_reported() {
    // A status is asked of the first name, `consensflow`: the one the app is
    // called. Someone else's under that name is not ours to report, whatever
    // ours is under the other.
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let env = home.env(windows);
        install(&env, &this.cf, &at(&home)).unwrap();
        let consensflow = join(&launchers(&home), &called("consensflow", windows));
        write(&consensflow, "someone else's\n");

        assert_eq!(status(&env, &at(&home)).unwrap(), None);
        assert_eq!(runtime(&env, &this.cf, &at(&home)).unwrap(), None);
        assert_eq!(read(&consensflow), "someone else's\n");
    }
}

#[test]
fn keeps_default_cli_launchers_in_consensflow_home_despite_a_project_bin_override() {
    for windows in forms() {
        let home = Home::new();
        let this = Bundle::new(home.root(), "This");
        let project = home.root().join("project").join("bin");
        let named = project.display().to_string();
        let env = home.env_with(windows, &[("CONSENSFLOW_BIN_DIR", Some(&named))]);
        let outcome = install(&env, &this.cf, &Places::default())
            .unwrap()
            .unwrap();
        assert_eq!(outcome.dir, join(&home.consensflow(), "bin"));
        assert!(!project.exists());
        assert!(!home.root().join("home").join(".local").join("bin").exists());
        assert!(join(&outcome.dir, &called("cf", windows)).exists());
    }
}
