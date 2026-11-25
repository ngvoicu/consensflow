//! The commands a member's window may not run: the ones that end every program whose name
//! matches. On a machine shared by other members and the person's own apps, that ends their
//! work and ConsensFlow itself (T-2130, T-2174, T-2196; the owner's 2026-11-12 live run ended
//! 22 programs that were not its own).
//!
//! ConsensFlow puts a folder of stand-ins first on the `PATH` of every window it launches
//! (`cf-daemon`'s `member_guard`): the bundle's `cf`, linked there under the names `pkill`
//! and `killall` on a Mac, `taskkill.exe` and `tskill.exe` on Windows. `cf` started under one
//! of those names asks this module what to do ([`judge`]).
//!
//! The limit, in the owner's words: a member window that calls the real command by its full
//! path, or a PowerShell cmdlet, isn't stopped: the guard catches the habit, not a determined
//! call.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::env::Env;

/// A command the guard stands in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guarded {
    Pkill,
    Killall,
    Taskkill,
    Tskill,
}

/// Every name a stand-in is linked under, with the command it stands in for.
pub const NAMES: [(&str, Guarded); 4] = [
    ("pkill", Guarded::Pkill),
    ("killall", Guarded::Killall),
    ("taskkill", Guarded::Taskkill),
    ("tskill", Guarded::Tskill),
];

/// The names a stand-in is linked under on the platform this is built for, with the file
/// each is: `pkill` and `killall` on a Mac, `taskkill.exe` and `tskill.exe` on Windows.
pub fn file_names() -> Vec<(String, Guarded)> {
    NAMES
        .iter()
        .filter(|(_, guarded)| guarded.is_of_this_platform())
        .map(|(name, guarded)| {
            let file = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                (*name).to_owned()
            };
            (file, *guarded)
        })
        .collect()
}

impl Guarded {
    fn is_of_this_platform(self) -> bool {
        matches!(self, Self::Taskkill | Self::Tskill) == cfg!(windows)
    }

    /// The command's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pkill => "pkill",
            Self::Killall => "killall",
            Self::Taskkill => "taskkill",
            Self::Tskill => "tskill",
        }
    }

    /// The real program a call that ends one program by its number goes to, on Windows:
    /// `%SystemRoot%\System32\<name>.exe`.
    fn real(self, env: &Env) -> PathBuf {
        let root = env
            .path("SystemRoot")
            .or_else(|| env.path("windir"))
            .map_or_else(|| PathBuf::from(r"C:\Windows"), Path::to_path_buf);
        root.join("System32").join(format!("{}.exe", self.name()))
    }
}

/// The program `program` is started as: its file name without a folder or `.exe`, in lower
/// case, when it is one of the [`NAMES`].
pub fn named(program: &OsStr) -> Option<Guarded> {
    let file = Path::new(program)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    NAMES
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(_, guarded)| *guarded)
}

/// What a stand-in does with a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Print these words to standard error and exit with 1.
    Refuse(String),
    /// Run the real program with these arguments, and exit as it does.
    Run(PathBuf, Vec<OsString>),
}

/// The words a refused call is answered with.
pub fn refusal(guarded: Guarded) -> String {
    let name = guarded.name();
    let end = if matches!(guarded, Guarded::Taskkill | Guarded::Tskill) {
        "End a program you started with `taskkill /PID <number>` (`taskkill //PID <number>` in \
         Git Bash), after checking it's yours."
    } else {
        "End a program you started by its number, after checking it's yours."
    };
    let by_name = if matches!(guarded, Guarded::Taskkill | Guarded::Tskill) {
        format!("{name} by name isn't run in member windows.")
    } else {
        format!("{name} isn't run in member windows.")
    };
    format!(
        "ConsensFlow: {by_name} It ends every program whose name matches, other members' and \
         the owner's among them. {end}"
    )
}

/// What `guarded` does with `args`: `pkill` and `killall` refuse every call. `taskkill` and
/// `tskill` go by what is allowed, not by what is refused: only a call that is purely by
/// number runs the real one, in the spellings a shell hands over (`/PID` or `-PID`, any
/// case). Git Bash turns an option written `/PID` into a path (`C:/Program Files/Git/PID`)
/// before the program reads it, and such a call is refused with every other: `//PID` is
/// Git Bash's spelling.
pub fn judge(guarded: Guarded, args: &[OsString], env: &Env) -> Verdict {
    let by_number = match guarded {
        Guarded::Pkill | Guarded::Killall => false,
        Guarded::Taskkill => taskkill_by_number(args),
        Guarded::Tskill => tskill_by_number(args),
    };
    if by_number {
        Verdict::Run(guarded.real(env), args.to_vec())
    } else {
        Verdict::Refuse(refusal(guarded))
    }
}

/// The name of the switch `arg` is, in lower case, when it is one (`/PID` and `-PID` alike).
fn switch(arg: &OsStr) -> Option<String> {
    let text = arg.to_string_lossy();
    let name = text.strip_prefix('/').or_else(|| text.strip_prefix('-'))?;
    Some(name.to_lowercase())
}

fn number(arg: &OsStr) -> bool {
    let text = arg.to_string_lossy();
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// `/PID <number>`, once or more, with `/F` and `/T` or neither, and nothing else.
fn taskkill_by_number(args: &[OsString]) -> bool {
    let mut args = args.iter();
    let mut named = 0;
    while let Some(arg) = args.next() {
        match switch(arg).as_deref() {
            Some("f" | "t") => {}
            Some("pid") if args.next().is_some_and(|value| number(value)) => named += 1,
            _ => return false,
        }
    }
    named > 0
}

/// One number or more, with `/V` or not, and nothing else.
fn tskill_by_number(args: &[OsString]) -> bool {
    args.iter()
        .all(|arg| number(arg) || switch(arg).as_deref() == Some("v"))
        && args.iter().any(|arg| number(arg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    const WORDS: &str = "ConsensFlow: pkill isn't run in member windows. It ends every \
         program whose name matches, other members' and the owner's among them. End a program \
         you started by its number, after checking it's yours.";

    #[test]
    fn pkill_and_killall_refuse_every_call_in_the_owners_words() {
        let env = Env::default();
        for call in [&["-f", "x"][..], &[][..], &["-9", "node"][..]] {
            assert_eq!(
                judge(Guarded::Pkill, &args(call), &env),
                Verdict::Refuse(WORDS.to_owned())
            );
            assert_eq!(
                judge(Guarded::Killall, &args(call), &env),
                Verdict::Refuse(WORDS.replace("pkill", "killall"))
            );
        }
    }

    #[test]
    fn taskkill_by_name_or_filter_is_refused_and_by_number_goes_to_the_real_one() {
        let env = Env::from_vars([("SystemRoot", r"D:\Win")]);
        let words = "ConsensFlow: taskkill by name isn't run in member windows. It ends every \
             program whose name matches, other members' and the owner's among them. End a \
             program you started with `taskkill /PID <number>` (`taskkill //PID <number>` in \
             Git Bash), after checking it's yours.";
        for call in [
            &["/IM", "x.exe"][..],
            &["/F", "/im", "x.exe"][..],
            &["/FI", "IMAGENAME eq x.exe"][..],
            &["/pid", "5", "/Fi", "PID gt 1"][..],
            &["-IM", "x.exe"][..],
            &["C:/Program Files/Git/IM", "x.exe"][..],
            &["C:/Program Files/Git/FI", "IMAGENAME eq x.exe"][..],
            &["C:/Program Files/Git/PID", "123"][..],
            &["/PID", "123", "/IM", "x.exe"][..],
            &["/PID", "x.exe"][..],
            &["/PID"][..],
            &["/F"][..],
            &["/S", "other", "/PID", "123"][..],
            &["/?"][..],
            &[][..],
        ] {
            assert_eq!(
                judge(Guarded::Taskkill, &args(call), &env),
                Verdict::Refuse(words.to_owned()),
                "{call:?}"
            );
        }
        for call in [
            &["-pid", "123"][..],
            &["/pid", "1", "/PID", "2", "-f", "/t"][..],
        ] {
            assert!(
                matches!(
                    judge(Guarded::Taskkill, &args(call), &env),
                    Verdict::Run(..)
                ),
                "{call:?}"
            );
        }
        let real = PathBuf::from(r"D:\Win")
            .join("System32")
            .join("taskkill.exe");
        assert_eq!(
            judge(Guarded::Taskkill, &args(&["/PID", "123", "/F", "/T"]), &env),
            Verdict::Run(real, args(&["/PID", "123", "/F", "/T"]))
        );
    }

    #[test]
    fn tskill_by_name_is_refused_and_by_number_goes_to_the_real_one() {
        let env = Env::default();
        assert!(matches!(
            judge(Guarded::Tskill, &args(&["notepad"]), &env),
            Verdict::Refuse(_)
        ));
        for call in [&["12"][..], &["12", "/v"][..]] {
            assert!(matches!(
                judge(Guarded::Tskill, &args(call), &env),
                Verdict::Run(..)
            ));
        }
        for call in [
            &["12", "notepad"][..],
            &["/a", "notepad"][..],
            &[][..],
            &["/v"][..],
        ] {
            assert!(matches!(
                judge(Guarded::Tskill, &args(call), &env),
                Verdict::Refuse(_)
            ));
        }
    }

    #[test]
    fn a_program_is_known_by_its_file_name_in_any_case_with_or_without_exe() {
        for (program, guarded) in [
            ("/x/member-guard/pkill", Some(Guarded::Pkill)),
            ("killall", Some(Guarded::Killall)),
            (r"C:\x\TASKKILL.EXE", Some(Guarded::Taskkill)),
            ("tskill.exe", Some(Guarded::Tskill)),
            ("cf", None),
            ("pkill-not", None),
        ] {
            let program = if cfg!(windows) {
                program.to_owned()
            } else {
                program.replace('\\', "/")
            };
            assert_eq!(named(OsStr::new(&program)), guarded, "{program}");
        }
    }
}
