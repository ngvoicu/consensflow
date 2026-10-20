//! What opens an HTML file of a specs folder outside ConsensFlow: the app the
//! system keeps for the file's type, which is the default browser unless the
//! human chose another, as the app opens its download page. It is given the
//! daemon's copy of the file ([`copies`](super::copies)), whose path the
//! daemon made: one program, given one argument, that path, never a URL,
//! never anything the page wrote.
//!
//! On macOS it is `/usr/bin/open`, elsewhere on Unix `xdg-open`, and on
//! Windows `explorer.exe`, which hands the file to the running shell. Each is
//! handed the file and let go ([`hand_over`]): one that ends at once says
//! whether it failed, and one that stays while the app it started runs (an
//! `xdg-open` that starts a browser in its own place) is never ended, so the
//! browser is not either.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{hand_over, with_required, Handed, Run};

use super::Opener;

/// How long the program is waited for, for what it says when it fails at once.
const WITHIN: Duration = Duration::from_secs(5);

/// The system the opener runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum System {
    Mac,
    Windows,
    Other,
}

impl System {
    /// The one this is built for.
    fn here() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

/// The system's own opener.
pub(super) fn system() -> Opener {
    Rc::new(|env, file| Box::pin(open(env.clone(), file)))
}

/// Opens `file` with the app the system keeps for its type.
async fn open(env: Env, file: PathBuf) -> Result<(), String> {
    let system = System::here();
    let run = run_for(&file, system, &env);
    let said = |why: String| format!("{} could not be opened: {why}", file.display());
    let handed = hand_over(&run, &with_required(&env, &env), WITHIN)
        .await
        .map_err(said)?;
    judged(&run, system, handed).map_err(said)
}

/// Whether the opener did as it was asked, by what came of it: one still
/// running did (it hands the file to an app as it stays), one that ended with
/// 0 did, and so did Explorer ending with 1, which it does for every file it
/// hands over; any other code, or a signal, is a failure, said in words.
pub(super) fn judged(run: &Run, system: System, handed: Handed) -> Result<(), String> {
    let program = run.program.file_name().map_or_else(
        || run.program.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    match handed {
        Handed::Running | Handed::Ended(Some(0)) => Ok(()),
        Handed::Ended(Some(1)) if system == System::Windows => Ok(()),
        Handed::Ended(Some(code)) => Err(format!("{program} ended with {code}")),
        Handed::Ended(None) => Err(format!("{program} was ended by a signal")),
    }
}

/// The program that opens `file` on `system`, and its one argument. On Windows
/// the path is quoted on the line as one argument (a comma or a space is part
/// of it; no path holds a quote), and given as the daemon made it: ConsensFlow's
/// home, then names of its own, with none of the `\\?\` a real path has there.
pub(super) fn run_for(file: &Path, system: System, env: &Env) -> Run {
    match system {
        System::Mac => Run {
            program: PathBuf::from("/usr/bin/open"),
            args: vec![file.as_os_str().to_owned()],
            verbatim: false,
        },
        System::Windows => {
            let program = env
                .os("SystemRoot")
                .or_else(|| env.os("windir"))
                .map_or_else(
                    || PathBuf::from("explorer.exe"),
                    |root| PathBuf::from(format!("{}\\explorer.exe", root.to_string_lossy())),
                );
            Run {
                program,
                args: vec![OsString::from(format!("\"{}\"", file.to_string_lossy()))],
                verbatim: true,
            }
        }
        System::Other => Run {
            program: PathBuf::from("xdg-open"),
            args: vec![file.as_os_str().to_owned()],
            verbatim: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_system_opens_the_one_file_with_its_own_program_and_one_argument() {
        let env = Env::from_vars([("SystemRoot", r"C:\Windows")]);
        let mac = run_for(
            Path::new("/Users/me/.consensflow/tmp/specs-open/Ab12/a b/SPEC.html"),
            System::Mac,
            &env,
        );
        assert_eq!(mac.program, PathBuf::from("/usr/bin/open"));
        assert_eq!(
            mac.args,
            [OsString::from(
                "/Users/me/.consensflow/tmp/specs-open/Ab12/a b/SPEC.html"
            )]
        );
        assert!(!mac.verbatim);
        let windows = run_for(
            Path::new(r"C:\Users\a,b\.consensflow\tmp\specs-open\Ab12\x\SPEC.html"),
            System::Windows,
            &env,
        );
        assert_eq!(windows.program, PathBuf::from(r"C:\Windows\explorer.exe"));
        assert_eq!(
            windows.args,
            [OsString::from(
                r#""C:\Users\a,b\.consensflow\tmp\specs-open\Ab12\x\SPEC.html""#
            )]
        );
        assert!(windows.verbatim, "quoted once, as Explorer reads it");
        let bare = run_for(
            Path::new(r"C:\x\SPEC.html"),
            System::Windows,
            &Env::default(),
        );
        assert_eq!(bare.program, PathBuf::from("explorer.exe"));
        let other = run_for(Path::new("/w/SPEC.html"), System::Other, &env);
        assert_eq!(other.program, PathBuf::from("xdg-open"));
        assert_eq!(other.args, [OsString::from("/w/SPEC.html")]);
    }

    /// What came of the opener, as the page is told it: Explorer's 1 is its
    /// success, as is every opener still running when it is let go; another
    /// code, or a signal, names the program and what ended it.
    #[test]
    fn what_the_opener_ended_with_says_whether_it_opened_the_file() {
        let file = Path::new("/w/SPEC.html");
        let env = Env::default();
        let judge = |system, handed| judged(&run_for(file, system, &env), system, handed);
        for system in [System::Mac, System::Windows, System::Other] {
            assert_eq!(judge(system, Handed::Running), Ok(()));
            assert_eq!(judge(system, Handed::Ended(Some(0))), Ok(()));
            assert_eq!(
                judge(system, Handed::Ended(Some(2))).map_err(|_| ()),
                Err(())
            );
        }
        assert_eq!(judge(System::Windows, Handed::Ended(Some(1))), Ok(()));
        assert_eq!(
            judge(System::Mac, Handed::Ended(Some(1))),
            Err("open ended with 1".to_owned())
        );
        assert_eq!(
            judge(System::Windows, Handed::Ended(Some(5))),
            Err("explorer.exe ended with 5".to_owned())
        );
        assert_eq!(
            judge(System::Other, Handed::Ended(None)),
            Err("xdg-open was ended by a signal".to_owned())
        );
    }
}
