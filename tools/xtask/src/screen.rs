//! Whether the Mac's screen is locked, which every smoke that starts the app asks
//! before it does.
//!
//! A window cannot be shown on a locked screen. About half a minute after the app
//! starts, macOS puts it and its web view in the background band, where the page
//! sleeps (0% of the CPU, at any load): the smoke's page stops, and the run ends
//! minutes later at whatever step it was on, which says nothing of the cause
//! (T-429: nine runs of the packaged smoke, 2026-10-24). So the smoke refuses at
//! once, in words, and starts nothing.
//!
//! The console says it in `ioreg`'s answer for the registry's root
//! (`ioreg -n Root -d1`, ten milliseconds): `"IOConsoleLocked" = Yes`, and
//! `"CGSSessionScreenIsLocked"=Yes` in the record of the console's user. Only a
//! `Yes` refuses. A screen that says it is not locked, an answer without either
//! key, an `ioreg` that is not there or does not answer: the run goes on as it
//! did before this was asked (a runner with no screen to lock must never be
//! refused for a key it does not have).

use std::io::{self, Write};

use crate::context::Context;
use crate::process::{self, Captured, Invocation};

/// What the smoke says where the screen is locked.
const LOCKED: &str =
    "the screen is locked: a window can't be shown; unlock the Mac and run it again";

/// The words of `ioreg` that say the console is locked, and that it is not.
const LOCKED_IS: [&str; 2] = [
    r#""IOConsoleLocked" = Yes"#,
    r#""CGSSessionScreenIsLocked"=Yes"#,
];
const NOT_LOCKED_IS: &str = r#""IOConsoleLocked" = No"#;

/// What the console's lock is, as far as `ioreg` told.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reading {
    Locked,
    NotLocked,
    /// Not told: `ioreg` is not there, did not answer, or said nothing of the lock.
    Unknown,
}

/// What `ioreg`'s answer, or its failure to start, says of the lock.
pub(crate) fn read(asked: Result<Captured, process::Failure>) -> Reading {
    match asked {
        Ok(answer) if answer.code == 0 => {
            if LOCKED_IS.iter().any(|said| answer.stdout.contains(said)) {
                Reading::Locked
            } else if answer.stdout.contains(NOT_LOCKED_IS) {
                Reading::NotLocked
            } else {
                Reading::Unknown
            }
        }
        _ => Reading::Unknown,
    }
}

/// Whether the screen is locked, asked of this machine's `ioreg`: said to `err` as
/// `who` (the command) says it, where it is. The caller starts nothing then.
pub(crate) fn locked(who: &str, context: &Context, err: &mut dyn Write) -> io::Result<bool> {
    let ask = Invocation::new("ioreg", &context.root).args(["-n", "Root", "-d1"]);
    let is_locked = read(process::capture(&ask, &context.env)) == Reading::Locked;
    if is_locked {
        writeln!(err, "{who}: {LOCKED}")?;
    }
    Ok(is_locked)
}

/// For the tests of the commands that ask: a machine whose screen is what the test says,
/// whatever this one's is.
#[cfg(test)]
pub(crate) mod testing {
    use cf_base::env::Env;

    /// What `ioreg -n Root -d1` printed on a locked Mac (T-429, 2026-10-24), cut to the
    /// properties around the lock.
    pub(crate) const LOCKED_ANSWER: &str = r#"+-o Root  <class IORegistryEntry, id 0x100000100, retain 36>
    {
      "OSKernelCPUType" = 16777228
      "IOConsoleLocked" = Yes
      "IOConsoleUsers" = ({"kCGSSessionOnConsoleKey"=Yes,"kCGSSessionSystemSafeBoot"=No,"CGSSessionScreenIsLocked"=Yes,"kCGSSessionIDKey"=257})
    }
"#;

    /// The same on a Mac that is in use: the console says it is not locked, and its user's
    /// record has no such key.
    pub(crate) const UNLOCKED_ANSWER: &str = r#"+-o Root  <class IORegistryEntry, id 0x100000100, retain 36>
    {
      "OSKernelCPUType" = 16777228
      "IOConsoleLocked" = No
      "IOConsoleUsers" = ({"kCGSSessionOnConsoleKey"=Yes,"kCGSSessionSystemSafeBoot"=No,"kCGSSessionIDKey"=257})
    }
"#;

    /// `env`, with a program `ioreg` first on its PATH that prints `printed` and nothing
    /// else (on a system with no such programs, `env` as it is). The folder holds it.
    pub(crate) fn with_ioreg(env: &Env, printed: &str) -> (tempfile::TempDir, Env) {
        let folder = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let script = folder.path().join("ioreg");
            // A shell builtin prints it: the PATH of the machine is this folder and the old one.
            std::fs::write(&script, format!("#!/bin/sh\nprintf '%s' '{printed}'\n")).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = printed;
        let path = std::env::join_paths(
            std::iter::once(folder.path().to_path_buf()).chain(
                env.iter()
                    .filter(|(name, _)| *name == "PATH")
                    .flat_map(|(_, value)| std::env::split_paths(value)),
            ),
        )
        .unwrap();
        let vars = env
            .iter()
            .filter(|(name, _)| *name != "PATH")
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([(std::ffi::OsString::from("PATH"), path)]);
        (folder, Env::from_vars(vars))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::testing::with_ioreg;
    use super::testing::{LOCKED_ANSWER, UNLOCKED_ANSWER};
    use super::*;

    #[cfg(unix)]
    use cf_base::env::Env;

    fn answer(code: i32, stdout: &str) -> Result<Captured, process::Failure> {
        Ok(Captured {
            code,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        })
    }

    #[test]
    fn a_console_that_says_it_is_locked_is_locked_by_either_of_its_two_keys() {
        assert_eq!(read(answer(0, LOCKED_ANSWER)), Reading::Locked);
        let root_only = LOCKED_ANSWER.replace(r#""CGSSessionScreenIsLocked"=Yes,"#, "");
        assert_eq!(read(answer(0, &root_only)), Reading::Locked);
        let record_only = LOCKED_ANSWER.replace(r#""IOConsoleLocked" = Yes"#, "");
        assert_eq!(read(answer(0, &record_only)), Reading::Locked);
    }

    #[test]
    fn a_console_that_says_it_is_not_locked_is_not() {
        assert_eq!(read(answer(0, UNLOCKED_ANSWER)), Reading::NotLocked);
    }

    #[test]
    fn an_answer_that_says_nothing_of_the_lock_or_cannot_be_had_is_not_told_and_never_refuses() {
        let silent = UNLOCKED_ANSWER.replace(r#""IOConsoleLocked" = No"#, "");
        for asked in [
            // The key is absent: a runner with no screen to lock.
            answer(0, &silent),
            answer(0, ""),
            // ioreg ends with an error, whatever it printed first.
            answer(1, LOCKED_ANSWER),
            // ioreg is not there, or cannot be run.
            Err(process::Failure::NotFound {
                program: "ioreg".into(),
            }),
            Err(process::Failure::Refused {
                program: "ioreg".into(),
                cause: io::Error::from(io::ErrorKind::PermissionDenied),
            }),
        ] {
            assert_eq!(read(asked), Reading::Unknown);
        }
    }

    /// A checkout whose environment has `ioreg` print `printed`, or has none; the folders
    /// it is made of, to hold as long as it is used.
    #[cfg(unix)]
    fn machine_with_ioreg(printed: Option<&str>) -> (Vec<tempfile::TempDir>, Context) {
        let root = tempfile::tempdir().unwrap();
        let mut folders = Vec::new();
        let env = match printed {
            Some(printed) => {
                // The system's own programs stay on the PATH, as on a Mac that has them.
                let system = Env::from_vars([("PATH", "/usr/bin:/bin")]);
                let (stand_in, env) = with_ioreg(&system, printed);
                folders.push(stand_in);
                env
            }
            None => Env::default(),
        };
        let context = Context {
            root: root.path().to_path_buf(),
            env,
        };
        folders.push(root);
        (folders, context)
    }

    #[cfg(unix)]
    #[test]
    fn a_smoke_is_refused_in_words_where_the_ioreg_of_the_machine_says_the_screen_is_locked() {
        let (_folder, context) = machine_with_ioreg(Some(LOCKED_ANSWER));
        let mut said = Vec::new();
        assert!(locked("smoke", &context, &mut said).unwrap());
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "smoke: the screen is locked: a window can't be shown; unlock the Mac and run it again\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_smoke_goes_on_without_a_word_where_the_screen_is_not_locked_or_nothing_says_it_is() {
        for printed in [Some(UNLOCKED_ANSWER), Some("nothing of a lock\n"), None] {
            let (_folder, context) = machine_with_ioreg(printed);
            let mut said = Vec::new();
            assert!(
                !locked("smoke", &context, &mut said).unwrap(),
                "{printed:?}"
            );
            assert!(said.is_empty(), "{printed:?}");
        }
    }
}
