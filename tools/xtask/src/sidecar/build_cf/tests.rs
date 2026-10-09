use std::ffi::OsString;
use std::fs;

use super::*;
use crate::sidecar::testing::{checkout, files_under, read, write, Fake};

fn words(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// Where the native `cf` is put (`cargo xtask build-cf`): bin/, which a clone may not
/// have (git keeps no empty folder, and nothing else is tracked in it once the
/// Node CLI is gone).
mod placing_the_built_cf_in_bin {
    use super::*;

    #[test]
    fn makes_the_folder_a_clone_does_not_have_and_puts_the_cf_in_it() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf");
        write(&built, "the new cf");
        let bin = dir.path().join("fresh").join("bin");
        assert!(!bin.exists());

        let placed = place(&built, &bin, "cf", &mut Fake::on(Platform::Other)).unwrap();

        assert_eq!(placed, bin.join("cf"));
        assert_eq!(read(&placed), "the new cf");
    }

    #[test]
    fn replaces_the_cf_there_and_takes_away_the_copies_set_aside_earlier() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf");
        write(&built, "the new cf");
        let bin = dir.path().join("used").join("bin");
        write(&bin.join("cf"), "the old cf");
        write(&bin.join("cf.old-1760000000000"), "one set aside");
        write(&bin.join("other"), "not a copy of the cf");

        let placed = place(&built, &bin, "cf", &mut Fake::on(Platform::Other)).unwrap();

        assert_eq!(read(&placed), "the new cf");
        assert_eq!(files_under(&bin), ["cf", "other"]);
    }

    #[cfg(unix)]
    #[test]
    fn puts_a_new_file_where_the_old_one_was_and_never_writes_into_it() {
        // macOS can kill the next run of a Mach-O changed in place: the copy there is
        // deleted and a file made, so what held the old cf's bytes is not written to.
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf");
        write(&built, "the new cf");
        let bin = dir.path().join("bin");
        write(&bin.join("cf"), "the old cf");
        let held_open = fs::File::open(bin.join("cf")).unwrap();

        place(&built, &bin, "cf", &mut Fake::on(Platform::Other)).unwrap();

        assert_eq!(io::read_to_string(held_open).unwrap(), "the old cf");
        assert_eq!(read(&bin.join("cf")), "the new cf");
    }

    #[test]
    fn leaves_nothing_set_aside_when_the_copy_there_could_be_deleted() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf");
        write(&built, "the new cf");
        let bin = dir.path().join("bin");
        write(&bin.join("cf.exe"), "the old cf");

        place(&built, &bin, "cf.exe", &mut Fake::on(Platform::Windows)).unwrap();

        assert_eq!(files_under(&bin), ["cf.exe"]);
    }

    #[test]
    fn needs_nothing_set_aside_where_there_is_no_cf_yet() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf");
        write(&built, "the new cf");
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        // Windows would refuse the deleting, but there is nothing to refuse.
        let mut windows = Fake::on(Platform::Windows);
        windows.undeletable = vec![bin.join("cf.exe")];

        place(&built, &bin, "cf.exe", &mut windows).unwrap();

        assert_eq!(files_under(&bin), ["cf.exe"]);
    }
}

/// A `cf.exe` that a question hook still runs cannot be deleted on Windows, and
/// can be renamed. The stand-in refuses the deleting as Windows does.
mod a_cf_exe_that_runs {
    use super::*;

    fn windows_refusing(bin: &Path, names: &[&str]) -> Fake {
        let mut windows = Fake::on(Platform::Windows);
        windows.undeletable = names.iter().map(|name| bin.join(name)).collect();
        windows
    }

    #[test]
    fn is_set_aside_and_the_new_one_put_in_its_place() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf.exe");
        write(&built, "the new cf");
        let bin = dir.path().join("bin");
        write(&bin.join("cf.exe"), "the cf that runs");
        write(&bin.join("cf.exe.old-1"), "an earlier one");
        let mut windows = windows_refusing(&bin, &["cf.exe"]);

        let placed = place(&built, &bin, "cf.exe", &mut windows).unwrap();

        assert_eq!(read(&placed), "the new cf");
        // Named for the time it was set aside, and still the cf that runs.
        assert_eq!(
            read(&bin.join("cf.exe.old-1760000000000")),
            "the cf that runs"
        );
        // The earlier one was not run any more, and is gone.
        assert_eq!(files_under(&bin), ["cf.exe", "cf.exe.old-1760000000000"]);
    }

    #[test]
    fn is_not_stopped_by_an_earlier_copy_that_still_runs() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf.exe");
        write(&built, "the new cf");
        let bin = dir.path().join("bin");
        write(&bin.join("cf.exe"), "the cf that runs");
        write(&bin.join("cf.exe.old-1"), "an earlier one that runs");
        let mut windows = windows_refusing(&bin, &["cf.exe", "cf.exe.old-1"]);

        let placed = place(&built, &bin, "cf.exe", &mut windows).unwrap();

        assert_eq!(read(&placed), "the new cf");
        assert_eq!(read(&bin.join("cf.exe.old-1")), "an earlier one that runs");
        assert_eq!(
            read(&bin.join("cf.exe.old-1760000000000")),
            "the cf that runs"
        );
    }

    #[test]
    fn is_an_error_that_names_it_when_it_cannot_be_set_aside_either() {
        let (dir, _) = checkout();
        let built = dir.path().join("built-cf.exe");
        write(&built, "the new cf");
        let bin = dir.path().join("bin");
        write(&bin.join("cf.exe"), "the cf that runs");
        // A name that cannot be taken: a folder with something in it is there.
        write(&bin.join("cf.exe.old-1760000000000").join("inside"), "");
        let mut windows = windows_refusing(&bin, &["cf.exe"]);

        let refused = place(&built, &bin, "cf.exe", &mut windows).unwrap_err();

        let said = refused.to_string();
        let expected = format!("could not set aside {}: ", bin.join("cf.exe").display());
        assert!(said.starts_with(&expected), "{said}");
        // The cf there is as it was: nothing took its place.
        assert_eq!(read(&bin.join("cf.exe")), "the cf that runs");
    }
}

#[test]
fn a_build_that_left_no_cf_is_said_before_the_cf_in_bin_is_touched() {
    let (_dir, context) = checkout();
    let cf_in_bin = context.path("bin").join("cf");
    write(&cf_in_bin, "the old cf");
    let mut system = Fake::on(Platform::Other);
    system.built = None;

    let refused = build(&context, false, &mut system).unwrap_err();

    let built = context.path("app/src-tauri/target/release").join("cf");
    assert_eq!(
        refused.to_string(),
        format!("the build left no cf at {}", built.display())
    );
    assert_eq!(read(&cf_in_bin), "the old cf");
}

#[test]
fn a_build_that_fails_ends_the_command_with_its_status_and_leaves_bin_alone() {
    let (_dir, context) = checkout();
    let mut system = Fake::on(Platform::Other);
    system.cargo_status = 101;

    let failed = build(&context, false, &mut system).unwrap_err();

    assert!(
        matches!(&failed, Error::Ended { status: 101, .. }),
        "{failed:?}"
    );
    assert_eq!(
        failed.to_string(),
        "cargo build --release --locked -p cf --bin cf ended with status 101"
    );
    assert!(!context.path("bin").exists());
    assert_eq!(system.lines().len(), 1);
}

#[test]
fn cargo_is_not_found_is_one_error_and_nothing_is_built() {
    let (_dir, context) = checkout();
    let mut system = Fake::on(Platform::Other);
    system.missing = vec!["cargo".into()];

    let failed = build(&context, false, &mut system).unwrap_err();

    assert_eq!(
        failed.to_string(),
        "`cargo` was not found: is it installed, and on the PATH?"
    );
    assert!(!context.path("bin").exists());
}

mod the_build {
    use super::*;

    /// The one cargo line `build` ran, which has to run from the checkout's root
    /// with the environment xtask has and no more.
    fn cargo_line(offline: bool) -> String {
        let (_dir, context) = checkout();
        let mut system = Fake::on(Platform::Other);
        build(&context, offline, &mut system).unwrap();
        assert_eq!(system.ran.len(), 1, "{:?}", system.lines());
        let invocation = &system.ran[0];
        assert_eq!(invocation.cwd, context.root);
        assert!(invocation.vars.is_empty());
        invocation.display()
    }

    #[test]
    fn is_cargos_release_build_of_the_cf_binary_on_the_lockfile_from_the_root() {
        assert_eq!(
            cargo_line(false),
            "cargo build --release --locked -p cf --bin cf"
        );
    }

    #[test]
    fn is_given_offline_in_the_place_cargo_takes_it_when_it_is_asked_for() {
        assert_eq!(
            cargo_line(true),
            "cargo build --release --locked --offline -p cf --bin cf"
        );
    }
}

#[test]
fn the_cf_cargo_built_is_put_in_bin_named_for_the_system() {
    for (platform, name) in [
        (Platform::Other, "cf"),
        (Platform::MacOs, "cf"),
        (Platform::Windows, "cf.exe"),
    ] {
        let (_dir, context) = checkout();
        let mut system = Fake::on(platform);

        let placed = build(&context, false, &mut system).unwrap();

        assert_eq!(placed, context.path("bin").join(name), "{platform:?}");
        // The stand-in's signing adds to the file it signs.
        let expected = if platform == Platform::MacOs {
            "the new cf signed"
        } else {
            "the new cf"
        };
        assert_eq!(read(&placed), expected, "{platform:?}");
    }
}

#[test]
fn only_on_macos_is_the_copy_signed_ad_hoc_once_it_is_in_place() {
    let (_dir, context) = checkout();
    let mut mac = Fake::on(Platform::MacOs);
    let placed = build(&context, false, &mut mac).unwrap();
    assert_eq!(
        mac.lines(),
        [
            "cargo build --release --locked -p cf --bin cf".to_string(),
            format!("codesign --force --sign - {}", placed.display()),
        ]
    );
    assert_eq!(mac.ran[1].cwd, context.root);

    for platform in [Platform::Windows, Platform::Other] {
        let (_dir, context) = checkout();
        let mut system = Fake::on(platform);
        build(&context, false, &mut system).unwrap();
        assert_eq!(
            system.lines().len(),
            1,
            "{platform:?}: {:?}",
            system.lines()
        );
    }
}

#[test]
fn a_signing_that_fails_ends_the_command_with_its_status() {
    let (_dir, context) = checkout();
    let mut mac = Fake::on(Platform::MacOs);
    mac.codesign_status = 1;

    let failed = build(&context, false, &mut mac).unwrap_err();

    assert!(
        matches!(&failed, Error::Ended { status: 1, .. }),
        "{failed:?}"
    );
    assert!(failed.to_string().starts_with("codesign --force --sign - "));
}

/// The signature that `codesign` is asked for, held up by `codesign` itself: what the
/// stand-in cannot say. Only macOS has it.
#[cfg(target_os = "macos")]
#[test]
fn a_copy_signed_here_verifies_and_replaces_the_signature_it_had() {
    let (dir, context) = checkout();
    let cf = dir.path().join("cf");
    // A program that Apple signed, so a signature is there to be replaced.
    fs::copy("/bin/echo", &cf).unwrap();
    let env = cf_base::env::Env::from_process();

    let signed = crate::process::capture(&sign(&context, &cf), &env).unwrap();
    assert_eq!(signed.code, 0, "{}", signed.stderr);

    let verify = Invocation::new("codesign", &context.root)
        .args(["--verify", "--strict"])
        .arg(&cf);
    let verified = crate::process::capture(&verify, &env).unwrap();
    assert_eq!(verified.code, 0, "{}", verified.stderr);
    let shown = Invocation::new("codesign", &context.root)
        .args(["--display", "--verbose=2"])
        .arg(&cf);
    let shown = crate::process::capture(&shown, &env).unwrap();
    assert!(shown.stderr.contains("Signature=adhoc"), "{}", shown.stderr);
}

#[test]
fn the_command_says_where_the_cf_is() {
    let (_dir, context) = checkout();
    let mut out = Vec::new();

    say(&context, false, &mut Fake::on(Platform::Other), &mut out).unwrap();

    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("cf → {}\n", context.path("bin").join("cf").display())
    );
}

#[test]
fn offline_is_the_one_word_build_cf_takes() {
    assert!(!parse(&words(&[])).unwrap());
    assert!(parse(&words(&["--offline"])).unwrap());
    for refused in [
        vec!["--ofline"],
        vec!["offline"],
        vec!["--offline", "--offline"],
        vec!["--offline", "extra"],
    ] {
        let said = parse(&words(&refused)).unwrap_err();
        assert!(matches!(said, Failure::Usage(_)), "{refused:?}");
        assert_eq!(
            said.to_string(),
            format!(
                "build-cf takes --offline or nothing, not {}",
                refused.join(" ")
            )
        );
    }
}

#[test]
fn a_word_it_does_not_take_is_refused_before_anything_is_done() {
    let (_dir, context) = checkout();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };

    let refused = run(&context, &words(&["--ofline"]), &mut console).unwrap_err();

    assert!(matches!(refused, Failure::Usage(_)));
    assert!(out.is_empty() && err.is_empty());
    assert!(!context.path("bin").exists());
}
