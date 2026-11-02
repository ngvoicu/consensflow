//! The launcher run for real: a stand-in `cf` that prints what it was given,
//! started through the command the app writes, by `sh`. What the text says
//! (`terminal.rs`) is not what it does: this holds that the command forwards
//! every argument as it was given, hands on the exit code, pins the home a
//! terminal does not name, and still runs a program in a folder whose name a
//! shell would read for itself.
//!
//! Windows's command is a program, the shim, which this crate does not build:
//! the `cf` crate does, and runs it the same ways (`crates/cf/tests/shim.rs`,
//! and `windows_shells.rs` in the shells a person types in).

#![cfg(unix)]
// A test starts the command it wrote, and a failure in its own folders and
// files is the test's.
#![allow(clippy::unwrap_used, clippy::disallowed_methods)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cf_base::env::Env;
use cf_launcher::{install, repair, Places};
use common::{old_launcher, write, Bundle, Home};

/// What a run of the launcher came to.
struct Ran {
    code: Option<i32>,
    lines: Vec<String>,
}

/// A stand-in `cf` at `folder`, named `name`, that prints one `arg:` line for
/// each argument it was given, the `home:` it was told, and ends with 7.
fn stand_in(folder: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let file = folder.join(name);
    write(
        &file,
        "#!/bin/sh\nfor each in \"$@\"; do printf 'arg:%s\\n' \"$each\"; done\nprintf 'home:%s\\n' \"$CONSENSFLOW_HOME\"\nexit 7\n",
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    file
}

/// Runs `launcher` with `args`, in an environment of nothing but `vars`.
fn run(launcher: &Path, args: &[&str], vars: &[(&str, &str)]) -> Ran {
    let mut env = vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())];
    env.extend(
        vars.iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    let arguments: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
    let started = cf_process::runnable(launcher, &arguments, &Env::from_vars(env.clone())).unwrap();
    let mut command: Command = started.command();
    let output = command.env_clear().envs(env).output().unwrap();
    Ran {
        code: output.status.code(),
        lines: String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect(),
    }
}

/// The launcher `install` writes in `folder` for `cf`, under the first name.
fn installed(home: &Home, cf: &Path, folder: &Path) -> PathBuf {
    let places = Places::at(vec![folder.to_path_buf()]);
    install(&home.env(cfg!(windows)), cf, &places)
        .unwrap()
        .unwrap()
        .path
}

#[test]
fn the_launcher_runs_the_cf_it_names_with_every_argument_as_it_was_given_and_ends_as_it_ends() {
    let home = Home::new();
    let cf = stand_in(&home.root().join("bundle"), "cf");
    let launcher = installed(&home, &cf, &home.root().join("launchers"));

    let args = [
        "setup",
        "two words",
        "it's",
        "\"quoted\"",
        "$HOME",
        "*",
        "",
        "-n",
        "a\nb",
    ];
    let ran = run(&launcher, &args, &[]);

    assert_eq!(ran.code, Some(7), "the exit code of cf is the command's");
    // Each as it was given: the empty one is an `arg:` of nothing, and the one
    // with a line break is two lines of what the stand-in prints.
    assert_eq!(
        ran.lines,
        [
            "arg:setup",
            "arg:two words",
            "arg:it's",
            "arg:\"quoted\"",
            "arg:$HOME",
            "arg:*",
            "arg:",
            "arg:-n",
            "arg:a",
            "b",
            &format!("home:{}", home.consensflow().display()),
        ]
    );
}

#[test]
fn the_pin_is_the_home_a_terminal_that_names_none_gets_and_a_home_it_names_does_not_move_it() {
    let home = Home::new();
    let cf = stand_in(&home.root().join("bundle"), "cf");
    let pinned = installed(&home, &cf, &home.root().join("pinned"));
    let plain = {
        let places = Places::at(vec![home.root().join("plain")]);
        install(&home.plain_env(false), &cf, &places)
            .unwrap()
            .unwrap()
            .path
    };
    let pin = format!("home:{}", home.consensflow().display());

    for vars in [&[][..], &[("CONSENSFLOW_HOME", "/elsewhere")]] {
        assert_eq!(run(&pinned, &[], vars).lines, std::slice::from_ref(&pin));
    }
    // A command that pins none leaves the terminal's own alone.
    assert_eq!(run(&plain, &[], &[]).lines, ["home:"]);
    assert_eq!(
        run(&plain, &[], &[("CONSENSFLOW_HOME", "/elsewhere")]).lines,
        ["home:/elsewhere"]
    );
}

#[test]
fn a_program_in_a_folder_whose_name_a_shell_reads_for_itself_still_runs() {
    let home = Home::new();
    let folder = home
        .root()
        .join("we ird \"q\" $HOME `tick` \\back 'single' %PATH%");
    let cf = stand_in(&folder, "cf");
    // The pin is a folder like that too.
    let odd_home = home.root().join("home $USER \"x\" `y` \\z");
    let places = Places::at(vec![home.root().join("launchers")]);
    let env = home.env_with(false, &[("CONSENSFLOW_HOME", odd_home.to_str())]);
    let launcher = install(&env, &cf, &places).unwrap().unwrap().path;

    let ran = run(&launcher, &["hello"], &[]);

    assert_eq!(ran.code, Some(7));
    assert_eq!(
        ran.lines,
        [
            "arg:hello".to_owned(),
            format!("home:{}", odd_home.display())
        ]
    );
}

#[test]
fn an_old_command_repaired_runs_the_new_cf_where_it_ran_the_old_runtime() {
    // What an upgrade does, end to end: the command alpha.78 wrote, run; the
    // repair; the same command, run again.
    let home = Home::new();
    let this = Bundle::new(home.root(), "This");
    let cf = stand_in(this.cf.parent().unwrap(), "cf");
    let old = Bundle::new(home.root(), "Old");
    // The old runtime and its `cf.mjs`: a script that says which it is.
    write(&old.node, "#!/bin/sh\nprintf 'old runtime\\n'\nexit 3\n");
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&old.node, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let places = Places::at(vec![home.root().join("launchers")]);
    let launcher = home.root().join("launchers").join("cf");
    write(
        &launcher,
        &old_launcher(
            false,
            &old.node,
            &old.cf_mjs,
            Some(&home.consensflow().display().to_string()),
        ),
    );
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let before = run(&launcher, &["doctor"], &[]);
    assert_eq!(
        (before.code, before.lines),
        (Some(3), vec!["old runtime".to_owned()])
    );

    repair(&home.env(false), &cf, &places);

    let after = run(&launcher, &["doctor"], &[]);
    assert_eq!(after.code, Some(7));
    assert_eq!(
        after.lines,
        [
            "arg:doctor".to_owned(),
            format!("home:{}", home.consensflow().display())
        ]
    );
}
