// A test starts the program it wrote, and a failure in its own folders and
// files is the test's.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

use super::*;
use crate::text::launcher;

const CF: &str = r"C:\Program Files\ConsensFlow\cli\bin\cf.exe";
const HOME: &str = r"C:\Users\me\.consensflow";

#[test]
fn the_launch_file_says_which_cf_to_run_and_which_home_it_is_told() {
    let text = launcher(true, CF, Some(HOME));
    assert_eq!(
        plan(&text).unwrap(),
        Plan {
            cf: CF.to_owned(),
            home: Some(HOME.to_owned())
        }
    );
    let plain = launcher(true, CF, None);
    assert_eq!(
        plan(&plain).unwrap(),
        Plan {
            cf: CF.to_owned(),
            home: None
        }
    );
}

#[test]
fn a_percent_sign_in_a_path_is_read_back_as_the_launch_file_wrote_it() {
    let text = launcher(true, r"C:\100%\cf.exe", Some(r"C:\50%\home"));
    assert_eq!(
        plan(&text).unwrap(),
        Plan {
            cf: r"C:\100%\cf.exe".to_owned(),
            home: Some(r"C:\50%\home".to_owned())
        }
    );
}

#[test]
fn the_old_shape_is_one_the_app_must_repair_and_the_words_say_so() {
    let old =
        "@echo off\r\nREM Installed by ConsensFlow.\r\n\"C:\\node.exe\" \"C:\\app\\cf.mjs\" %*\r\n";
    let said = plan(old).unwrap_err();
    assert!(
        said.contains("older ConsensFlow") && said.contains("cf setup"),
        "{said}"
    );
}

#[test]
fn a_file_that_names_no_cf_says_so() {
    for text in ["", "@echo off\r\n", "hello\n"] {
        let said = plan(text).unwrap_err();
        assert!(said.contains("names no cf"), "{text:?}: {said}");
    }
}

#[test]
fn a_launch_file_that_names_the_program_itself_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let exe = folder.path().join("cf.exe");
    std::fs::write(&exe, "").unwrap();
    assert!(names_itself(&exe.to_string_lossy(), &exe));
    assert!(!names_itself(
        &folder.path().join("other").to_string_lossy(),
        &exe
    ));
    assert!(!names_itself("missing", &exe));
}

#[test]
fn without_a_launch_file_the_shim_says_which_it_looked_for_and_what_makes_it() {
    let folder = tempfile::tempdir().unwrap();
    let exe = folder.path().join("cf.exe");
    let said = start(&exe, &[]).unwrap_err();
    assert!(
        said.contains(&folder.path().join("cf.launch").display().to_string())
            && said.contains("cf setup"),
        "{said}"
    );
}

#[cfg(unix)]
mod running {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    /// A shim at `<folder>/cf.exe`, its launch file naming a stand-in `cf` that
    /// writes each argument it was given, each ended by a NUL, to `args`, the home
    /// it was told to `home`, and ends with 7.
    fn shimmed(folder: &Path, home: Option<&Path>) -> (PathBuf, PathBuf, PathBuf) {
        let (args, told) = (folder.join("args"), folder.join("home"));
        let cf = folder.join("cf-stand-in");
        fs::write(
            &cf,
            format!(
                "#!/bin/sh\nprintf '%s\\0' \"$@\" > '{}'\nprintf '%s' \"$CONSENSFLOW_HOME\" > '{}'\nexit 7\n",
                args.display(),
                told.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&cf, fs::Permissions::from_mode(0o755)).unwrap();
        let exe = folder.join("cf.exe");
        fs::write(&exe, "").unwrap();
        let pinned = home.map(|home| home.to_string_lossy().into_owned());
        fs::write(
            folder.join("cf.launch"),
            launcher(true, &cf.to_string_lossy(), pinned.as_deref()),
        )
        .unwrap();
        (exe, args, told)
    }

    fn given(args: &Path) -> Vec<Vec<u8>> {
        let bytes = fs::read(args).unwrap();
        let mut each: Vec<Vec<u8>> = bytes.split(|byte| *byte == 0).map(<[u8]>::to_vec).collect();
        each.pop();
        each
    }

    #[test]
    fn every_argument_reaches_the_cf_as_it_was_given_and_the_code_it_ends_with_is_the_commands() {
        let folder = tempfile::tempdir().unwrap();
        let (exe, args, _) = shimmed(folder.path(), None);
        let text = [
            "setup",
            "two words",
            "it's",
            "\"quoted\"",
            "$HOME `echo`",
            "*",
            "",
            "-n",
            "line one\nline two\r\n\nPLOP-6142",
            "100% & | ^ !x! %PATH%",
            r"C:\dir\",
            "ends with a backslash\\",
        ];
        let words: Vec<OsString> = text.iter().map(OsString::from).collect();

        let code = start(&exe, &words).unwrap();

        assert_eq!(code, 7, "the exit code of the cf is the command's");
        let sent: Vec<Vec<u8>> = text.iter().map(|each| each.as_bytes().to_vec()).collect();
        assert_eq!(given(&args), sent);
    }

    #[test]
    fn the_home_a_launch_file_pins_is_the_home_the_cf_is_told() {
        let folder = tempfile::tempdir().unwrap();
        let pinned = folder.path().join("a home with 100% in it");
        let (exe, _, told) = shimmed(folder.path(), Some(&pinned));
        start(&exe, &[OsString::from("x")]).unwrap();
        assert_eq!(fs::read(&told).unwrap(), pinned.as_os_str().as_bytes());
    }

    #[test]
    fn a_cf_that_cannot_be_started_is_said_with_what_to_do() {
        let folder = tempfile::tempdir().unwrap();
        let (exe, _, _) = shimmed(folder.path(), None);
        fs::remove_file(folder.path().join("cf-stand-in")).unwrap();
        let said = start(&exe, &[]).unwrap_err();
        assert!(
            said.contains("cannot run") && said.contains("cf setup"),
            "{said}"
        );
    }

    #[test]
    fn a_launch_file_that_names_the_shim_itself_starts_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let exe = folder.path().join("cf.exe");
        fs::write(&exe, "").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            folder.path().join("cf.launch"),
            launcher(true, &exe.to_string_lossy(), None),
        )
        .unwrap();
        let said = start(&exe, &[]).unwrap_err();
        assert!(said.contains("names this program itself"), "{said}");
    }

    #[test]
    fn a_cf_ended_by_a_signal_is_not_a_success() {
        let folder = tempfile::tempdir().unwrap();
        let (exe, _, _) = shimmed(folder.path(), None);
        fs::write(folder.path().join("cf-stand-in"), "#!/bin/sh\nkill -9 $$\n").unwrap();
        assert_eq!(start(&exe, &[]).unwrap(), 1);
    }
}
