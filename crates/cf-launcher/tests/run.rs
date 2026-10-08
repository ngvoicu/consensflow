//! The launcher run for real: a stand-in `cf` that prints what it was given,
//! started through the command the app writes, by `sh` or by cmd.exe as the
//! system runs it. What the text says (`terminal.rs`) is not what it does:
//! this holds that the command forwards every argument as it was given, hands
//! on the exit code, pins the home a terminal does not name, and still runs a
//! program in a folder whose name a shell would read for itself.
//!
//! The `.cmd` runs on Windows only: cmd.exe is not elsewhere. Its three tests
//! were written from another system and type-checked there (`npm run
//! clippy:windows`); none has been run. What `%*` holds in them (each argument
//! in the quotes `cf_process::runnable` puts round it, which the stand-in
//! takes off) is read from that code, not seen: the first run on Windows is the
//! proof, and a difference in it is the test's to correct, not yet the launcher's.

// A test starts the command it wrote, and a failure in its own folders and
// files is the test's.
#![allow(clippy::unwrap_used, clippy::disallowed_methods)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use cf_base::env::Env;
use cf_launcher::{install, Places};
use common::{write, Home};
// The tests that make a runtime of their own, and mend a command, are Unix's.
#[cfg(unix)]
use {
    cf_launcher::repair,
    common::{old_launcher, Bundle},
    std::fs,
};

/// What a run of the launcher came to.
struct Ran {
    code: Option<i32>,
    lines: Vec<String>,
}

/// A stand-in `cf` at `folder`, named `name`, that prints one `arg:` line for
/// each argument it was given, the `home:` it was told, and ends with 7.
#[cfg(unix)]
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

/// The same for cmd.exe, a `.cmd`: `%*` is every argument as it was handed
/// over, each in the quotes the starter put round it (`cf_process::runnable`
/// quotes them for the script), which `%%~a` takes off, one line each.
#[cfg(windows)]
fn stand_in(folder: &Path, name: &str) -> PathBuf {
    let file = folder.join(format!("{name}.cmd"));
    write(
        &file,
        "@echo off\r\nfor %%a in (%*) do echo arg:%%~a\r\necho home:%CONSENSFLOW_HOME%\r\nexit /b 7\r\n",
    );
    file
}

/// Runs `launcher` with `args`, in an environment of nothing but `vars`, and
/// what Windows will not start a program without.
fn run(launcher: &Path, args: &[&str], vars: &[(&str, &str)]) -> Ran {
    let process = Env::from_process();
    let mut env = vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())];
    for name in ["SystemRoot", "ComSpec"] {
        if let Some(value) = process.os(name) {
            env.push((name.to_owned(), value.to_string_lossy().into_owned()));
        }
    }
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(windows)]
#[test]
fn the_cmd_runs_the_cf_it_names_with_every_argument_and_ends_as_it_ends() {
    let home = Home::new();
    let cf = stand_in(&home.root().join("bundle"), "cf");
    let launcher = installed(&home, &cf, &home.root().join("launchers"));

    let ran = run(&launcher, &["setup", "two", "words"], &[]);

    assert_eq!(ran.code, Some(7), "the exit code of cf is the command's");
    assert_eq!(
        ran.lines,
        [
            "arg:setup".to_owned(),
            "arg:two".to_owned(),
            "arg:words".to_owned(),
            format!("home:{}", home.consensflow().display())
        ]
    );
}

#[cfg(windows)]
#[test]
fn the_cmd_pins_the_home_a_terminal_that_names_none_gets_and_a_home_it_names_does_not_move_it() {
    let home = Home::new();
    let cf = stand_in(&home.root().join("bundle"), "cf");
    let pinned = installed(&home, &cf, &home.root().join("pinned"));
    let plain = {
        let places = Places::at(vec![home.root().join("plain")]);
        install(&home.plain_env(true), &cf, &places)
            .unwrap()
            .unwrap()
            .path
    };
    let pin = format!("home:{}", home.consensflow().display());

    for vars in [&[][..], &[("CONSENSFLOW_HOME", r"C:\elsewhere")]] {
        assert_eq!(run(&pinned, &["x"], vars).lines, ["arg:x", pin.as_str()]);
    }
    // A command that pins none leaves the terminal's own alone.
    assert_eq!(
        run(&plain, &["x"], &[("CONSENSFLOW_HOME", r"C:\elsewhere")]).lines,
        ["arg:x", r"home:C:\elsewhere"]
    );
}

#[cfg(windows)]
#[test]
fn a_cmd_runs_a_program_in_a_folder_with_a_space_and_a_percent_sign_in_its_name() {
    let home = Home::new();
    let folder = home.root().join("we ird 100%");
    let cf = stand_in(&folder, "cf");
    let odd_home = home.root().join("home 100%");
    let places = Places::at(vec![home.root().join("launchers")]);
    let env = home.env_with(true, &[("CONSENSFLOW_HOME", odd_home.to_str())]);
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
