//! What `clippy-windows` runs, and the archiver it has cc-rs run, on a checkout of
//! a test's own and no cargo of anyone's. The command as a process, and cc-rs's
//! way of running the archiver from what the command sets, are
//! `tests/drivers.rs`'s.

use super::*;

use cf_base::env::Env;

fn context() -> Context {
    Context {
        root: PathBuf::from("checkout"),
        env: Env::default(),
    }
}

fn words(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// The value of the variable `name` that `invocation` sets.
fn set(invocation: &Invocation, name: &str) -> Option<OsString> {
    invocation
        .vars
        .iter()
        .rev()
        .find(|(var, _)| var == name)
        .and_then(|(_, value)| value.clone())
}

fn console<'a>(out: &'a mut Vec<u8>, err: &'a mut Vec<u8>) -> Console<'a> {
    Console { out, err }
}

#[test]
fn the_workspace_but_the_app_is_linted_for_the_target_from_the_root_when_no_crate_is_named() {
    let lint = lint(&context(), &[], Path::new("xtask"));
    assert_eq!(
        lint.display(),
        "cargo clippy --offline --target x86_64-pc-windows-msvc --all-targets \
         --workspace --exclude app -- -D warnings"
    );
    assert_eq!(lint.cwd, PathBuf::from("checkout"));
}

#[test]
fn each_crate_named_is_one_package_and_the_workspace_is_not_linted_whole() {
    let lint = lint(
        &context(),
        &words(&["cf-daemon", "cf-process"]),
        Path::new("xtask"),
    );
    assert_eq!(
        lint.display(),
        "cargo clippy --offline --target x86_64-pc-windows-msvc --all-targets \
         -p cf-daemon -p cf-process -- -D warnings"
    );
}

#[test]
fn the_c_is_not_compiled_and_the_archive_is_made_by_xtask_run_as_the_archiver() {
    let archiver = Path::new("checkout")
        .join("target")
        .join("debug")
        .join("xtask");
    let lint = lint(&context(), &[], &archiver);
    assert_eq!(set(&lint, "CC_x86_64_pc_windows_msvc"), Some("true".into()));
    assert_eq!(
        set(&lint, "AR_x86_64_pc_windows_msvc"),
        Some(OsString::from(format!(
            "{} --as-archiver",
            archiver.display()
        )))
    );
    // Nothing of the app's, nor any other variable, when the app is not linted.
    assert_eq!(lint.vars.len(), 2);
}

#[test]
fn the_app_is_linted_with_its_resources_left_out_when_it_is_named_among_the_crates() {
    let cases: [&[&str]; 3] = [&["app"], &["cf-daemon", "app"], &["app", "cf-daemon"]];
    for crates in cases {
        let lint = lint(&context(), &words(crates), Path::new("xtask"));
        assert_eq!(
            set(&lint, "TAURI_CONFIG"),
            Some(r#"{"bundle":{"resources":null}}"#.into()),
            "{crates:?}"
        );
        assert!(names_the_app(&words(crates)), "{crates:?}");
    }
    // A crate that only has `app` in its name is not the app.
    let other = words(&["cf-app", "application"]);
    assert!(!names_the_app(&other));
    assert_eq!(
        set(
            &lint(&context(), &other, Path::new("xtask")),
            "TAURI_CONFIG"
        ),
        None
    );
}

#[test]
fn an_archiver_line_names_its_archive_as_ar_does_or_as_lib_does() {
    let named = |line: &[&str]| archive_named(&words(line));
    // `ar cq ARCHIVE OBJECT ...`, and the deterministic mode cc-rs tries first.
    assert_eq!(
        named(&["cq", "libsqlite3.a", "a.o"]),
        Some("libsqlite3.a".into())
    );
    assert_eq!(
        named(&["cqD", "libsqlite3.a", "a.o", "b.o"]),
        Some("libsqlite3.a".into())
    );
    // `lib -out:ARCHIVE OBJECT ...`, in either of the ways Windows writes an option, in any case.
    assert_eq!(
        named(&["-out:sqlite3.lib", "-nologo", "a.o"]),
        Some("sqlite3.lib".into())
    );
    assert_eq!(named(&["/out:sqlite3.lib"]), Some("sqlite3.lib".into()));
    assert_eq!(named(&["-OUT:sqlite3.lib"]), Some("sqlite3.lib".into()));
    assert_eq!(
        named(&["/Out:sqlite3.lib", "x.lib", "a.o"]),
        Some("sqlite3.lib".into())
    );
    // The option is looked for in the whole line, and what follows it is all of the name.
    assert_eq!(
        named(&["-nologo", "-out:a b/c:d.lib"]),
        Some("a b/c:d.lib".into())
    );
    // Whatever else is out-like is not the option.
    assert_eq!(named(&["-outx:no", "cq"]), Some("cq".into()));
    assert_eq!(named(&["-ou:no", "second"]), Some("second".into()));
}

#[test]
fn a_line_that_names_no_archive_names_none() {
    assert_eq!(archive_named(&[]), None);
    assert_eq!(archive_named(&words(&["cq"])), None);
}

#[test]
fn the_archive_is_made_empty_when_there_is_none_and_left_as_it_is_when_there_is() {
    let dir = tempfile::tempdir().unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let sqlite = dir.path().join("libsqlite3.a");
    let args = vec![OsString::from("cq"), sqlite.clone().into(), "a.o".into()];

    let status = archive(&context(), &args, &mut console(&mut out, &mut err)).unwrap();
    assert_eq!(status, 0);
    assert_eq!(fs::read(&sqlite).unwrap(), b"");

    // What is in it stays: cc-rs appends the objects of another batch to it.
    fs::write(&sqlite, b"!<arch>\n").unwrap();
    archive(&context(), &args, &mut console(&mut out, &mut err)).unwrap();
    assert_eq!(fs::read(&sqlite).unwrap(), b"!<arch>\n");

    let lib = dir.path().join("sqlite3.lib");
    let args = vec![
        OsString::from("-nologo"),
        format!("-out:{}", lib.display()).into(),
    ];
    archive(&context(), &args, &mut console(&mut out, &mut err)).unwrap();
    assert_eq!(fs::read(&lib).unwrap(), b"");

    // It says nothing, on either stream.
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn a_line_that_names_no_archive_makes_nothing_and_ends_with_0() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = archive(&context(), &[], &mut console(&mut out, &mut err)).unwrap();
    assert_eq!(status, 0);
}

#[test]
fn an_archive_that_cannot_be_made_is_said_with_what_was_being_made() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-folder").join("lib.a");
    let args = vec![OsString::from("cq"), missing.clone().into()];
    let (mut out, mut err) = (Vec::new(), Vec::new());

    let failed = archive(&context(), &args, &mut console(&mut out, &mut err)).unwrap_err();

    let said = failed.to_string();
    assert!(
        said.starts_with(&format!(
            "could not make the archive {}: ",
            missing.display()
        )),
        "{said}"
    );
    assert!(matches!(failed, Failure::Io(cause) if cause.kind() == io::ErrorKind::NotFound));
}

#[test]
fn the_two_commands_run_in_rust_and_the_archiver_is_the_word_cc_rs_is_told() {
    let lines: Vec<_> = COMMANDS
        .iter()
        .map(|command| command.words.join(" "))
        .collect();
    assert_eq!(lines, ["clippy-windows", "--as-archiver"]);
    assert!(COMMANDS
        .iter()
        .all(|command| matches!(command.run, Run::Native(_))));
}

#[test]
fn xtasks_own_program_is_the_archiver_it_is_told_to_run_as() {
    assert_eq!(archiver().unwrap(), std::env::current_exe().unwrap());
}

/// A checkout of a test's own, with the PATH it is given.
fn checkout_with_path(path: impl Into<OsString>) -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        root: dir.path().to_path_buf(),
        env: Env::from_vars([("PATH", path)]),
    };
    (dir, context)
}

#[test]
fn the_stand_in_is_the_first_place_the_path_names_and_the_others_follow_in_order() {
    let (_dir, context) = checkout_with_path(std::env::join_paths(["one", "two"]).unwrap());
    let path = path_with_stand_in(&context).unwrap();
    let folder = context.path("app/src-tauri/target/clippy-windows");
    let expected = std::env::join_paths([folder.clone(), "one".into(), "two".into()]).unwrap();
    assert_eq!(path, expected);
    assert!(folder.join("llvm-rc").is_file());
}

#[test]
fn a_path_there_is_none_of_is_the_stand_in_alone() {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        root: dir.path().to_path_buf(),
        env: Env::default(),
    };
    let folder = context.path("app/src-tauri/target/clippy-windows");
    assert_eq!(
        path_with_stand_in(&context).unwrap(),
        OsString::from(folder.as_os_str())
    );
}

#[test]
#[cfg(unix)]
fn the_stand_in_is_a_program_that_compiles_nothing_and_ends_with_0() {
    let (_dir, context) = checkout_with_path("");
    path_with_stand_in(&context).unwrap();
    let stand_in = context.path("app/src-tauri/target/clippy-windows/llvm-rc");
    let env = Env::default();
    // As the app's build script runs it: by its name, with options it has no use for.
    let run = Invocation::new(&stand_in, &context.root).args(["/fo", "out.res", "in.rc"]);
    assert_eq!(process::run(&run, &env).unwrap(), 0);
    assert_eq!(
        fs::read_to_string(&stand_in).unwrap(),
        "#!/bin/sh\nexit 0\n"
    );
}

#[test]
fn the_stand_in_is_written_again_each_time_and_a_folder_that_is_there_is_no_hindrance() {
    let (_dir, context) = checkout_with_path("p");
    let folder = context.path("app/src-tauri/target/clippy-windows");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("llvm-rc"), "something else").unwrap();
    path_with_stand_in(&context).unwrap();
    path_with_stand_in(&context).unwrap();
    assert_eq!(
        fs::read_to_string(folder.join("llvm-rc")).unwrap(),
        "#!/bin/sh\nexit 0\n"
    );
}

#[test]
#[cfg(unix)]
fn a_folder_the_path_cannot_hold_is_said() {
    // `:` is a character of a name on unix, and the separator of the PATH.
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        root: dir.path().join("a:b"),
        env: Env::default(),
    };
    let failed = path_with_stand_in(&context).unwrap_err();
    let said = failed.to_string();
    assert!(
        said.starts_with("could not put ") && said.contains("on the PATH"),
        "{said}"
    );
    assert!(matches!(failed, Failure::Io(_)));
}

#[test]
fn what_is_being_done_is_said_with_the_kind_of_its_failure_kept() {
    let failed = doing("write the file")(io::Error::from(io::ErrorKind::PermissionDenied));
    assert_eq!(
        failed.to_string(),
        "could not write the file: permission denied"
    );
    assert!(
        matches!(failed, Failure::Io(cause) if cause.kind() == io::ErrorKind::PermissionDenied)
    );
}
