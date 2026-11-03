//! How a command of a window starts. A window's shell can find another `cf` first on its
//! PATH (Devin's login zsh on the Mac puts the user's `~/.local/bin` first, Claude Code's
//! login Git Bash on Windows `~/bin`, T-972 and T-1025, 2026-10-31; the Cloud Foundry CLI is
//! a `cf` too), so every command ConsensFlow teaches a window starts with the window's own
//! `cf` by its whole path, and no order of a PATH matters to it ([`CfWord`]).
//!
//! The shell decides how a path is written as the first word of a command ([`Shell`]): a
//! path with a space or a character a shell reads is one quoted word in a bash-like shell,
//! and in PowerShell a quoted word is a string and a command only after `&`.

use std::borrow::Cow;
use std::fmt;

/// The variable a window's environment carries its [`CfWord`] in, as the daemon made it for
/// the window's harness and platform: every `cf` the window runs (a command, a hook, the
/// broker of Codex's questions) says its own hints with it ([`CfWord::of_word`]).
pub const ENV: &str = "CONSENSFLOW_CF";

/// How the shell a window's commands are typed in runs a program by its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// bash, zsh, sh and Git Bash: `'/a b/cf' task get T-3`.
    Posix,
    /// PowerShell (Codex's on Windows): `& '/a b/cf' task get T-3`.
    PowerShell,
}

/// The first word of a command that runs a window's own `cf`: its path as the window's shell
/// takes it as one word, or `cf` where there is no path to give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfWord(String);

/// The commands of `cf` ConsensFlow's own words name in a text it types into a window
/// ([`CfWord::in_text`]): the board's, as `cf --help` lists them.
const COMMANDS: [&str; 8] = [
    "task", "ask", "inbox", "answer", "note", "tell", "staff", "history",
];

impl CfWord {
    /// No path: the word `cf`, as a shell finds it. What a text says where nobody knows where
    /// the window's `cf` is.
    pub fn bare() -> Self {
        Self("cf".to_owned())
    }

    /// `path`, the window's own `cf`, for `shell`: the path as it is where every shell takes it
    /// as one word (letters, digits and `/ . _ - : + ~`), else quoted for the shell. An empty
    /// path is none.
    pub fn new(path: &str, shell: Shell) -> Self {
        if path.is_empty() {
            return Self::bare();
        }
        let plain = path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-:+~".contains(&byte));
        if plain {
            return Self(path.to_owned());
        }
        Self(match shell {
            Shell::Posix => format!("'{}'", path.replace('\'', r"'\''")),
            Shell::PowerShell => format!("& '{}'", path.replace('\'', "''")),
        })
    }

    /// A word a window was given as it starts a command (the value of [`ENV`]), taken as it is:
    /// none for an empty one, one over a line or one with a control character, which no shell
    /// takes as the start of a command.
    pub fn of_word(word: &str) -> Option<Self> {
        let fits = !word.is_empty() && word.len() <= 1_024 && !word.chars().any(char::is_control);
        fits.then(|| Self(word.to_owned()))
    }

    /// The word, as it starts a command.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `text`, a text ConsensFlow wrote to be typed into the window, with each command it names
    /// by the word `cf` (`cf task get T-3`, in `` `…` `` or in brackets, at the start of a line
    /// or after a space) started by this word instead. A path already written (`…/cf task get`),
    /// and any other use of the letters, stay as they are.
    pub fn in_text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.0 == "cf" {
            return Cow::Borrowed(text);
        }
        let mut out = String::new();
        let mut copied = 0;
        for (at, _) in text.match_indices("cf ") {
            if !starts_a_word(text, at) || !names_a_command(&text[at + 3..]) {
                continue;
            }
            out.push_str(&text[copied..at]);
            out.push_str(&self.0);
            copied = at + 2;
        }
        if copied == 0 {
            return Cow::Borrowed(text);
        }
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }
}

impl fmt::Display for CfWord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether the `cf` at `at` of `text` begins a word of its own: it follows no letter, digit or
/// one of `_ - . / \ ' $ %`, which would make it part of a name, a path or a variable.
fn starts_a_word(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_none_or(|before| !(before.is_alphanumeric() || "_-./\\'$%".contains(before)))
}

/// Whether `rest`, the text after `cf `, begins with one of the board's commands as a word.
fn names_a_command(rest: &str) -> bool {
    COMMANDS.iter().any(|command| {
        rest.strip_prefix(command).is_some_and(|after| {
            !after.starts_with(|next: char| next.is_alphanumeric() || next == '-' || next == '_')
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posix(path: &str) -> CfWord {
        CfWord::new(path, Shell::Posix)
    }

    fn powershell(path: &str) -> CfWord {
        CfWord::new(path, Shell::PowerShell)
    }

    const MAC: &str = "/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf";

    #[test]
    fn a_path_every_shell_takes_as_one_word_is_the_word_in_every_shell() {
        for path in [
            MAC,
            "C:/Users/AGV/consensflow-build/target/release/cf.exe",
            "/a~1/b+c/cf",
        ] {
            assert_eq!(posix(path).as_str(), path);
            assert_eq!(powershell(path).as_str(), path);
        }
    }

    #[test]
    fn a_path_with_a_space_or_a_shell_character_is_one_quoted_word_in_bash_and_after_an_ampersand_in_powershell(
    ) {
        let spaced = "C:/Program Files/ConsensFlow/bin/cf.exe";
        assert_eq!(
            posix(spaced).as_str(),
            "'C:/Program Files/ConsensFlow/bin/cf.exe'"
        );
        assert_eq!(
            powershell(spaced).as_str(),
            "& 'C:/Program Files/ConsensFlow/bin/cf.exe'"
        );
        let hostile = "/Users/me/My Apps/cf `x` $HOME 'q'";
        assert_eq!(
            posix(hostile).as_str(),
            r"'/Users/me/My Apps/cf `x` $HOME '\''q'\'''"
        );
        assert_eq!(
            powershell(hostile).as_str(),
            "& '/Users/me/My Apps/cf `x` $HOME ''q'''"
        );
        for shell in [Shell::Posix, Shell::PowerShell] {
            assert_eq!(CfWord::new("", shell), CfWord::bare());
            assert_eq!(CfWord::bare().as_str(), "cf");
        }
    }

    #[test]
    fn a_text_names_the_boards_commands_by_the_word_and_by_nothing_else() {
        let word = posix(MAC);
        let said = |text: &str| word.in_text(text).into_owned();
        assert_eq!(
            said("Decide with: cf task accept T-4 · cf task reopen T-4 \"…\""),
            format!("Decide with: {MAC} task accept T-4 · {MAC} task reopen T-4 \"…\"")
        );
        assert_eq!(
            said("`cf inbox read m-12`.\n(cf answer m-3 \"…\")\ncf history --page 2"),
            format!("`{MAC} inbox read m-12`.\n({MAC} answer m-3 \"…\")\n{MAC} history --page 2")
        );
        // A path already written, a name that ends in the letters, a variable, a word that is
        // none of the board's commands, and the letters at the end of a text.
        for stays in [
            format!("{MAC} task get T-1"),
            "'/a b/cf' task get T-1".to_owned(),
            "the cfg task".to_owned(),
            "my-cf task get".to_owned(),
            "$cf task get".to_owned(),
            "cf taskmaster".to_owned(),
            "cf tasks".to_owned(),
            "cf hook claude".to_owned(),
            "the cf is here".to_owned(),
            "ends with cf".to_owned(),
        ] {
            assert_eq!(said(&stays), stays);
        }
        // The command at the very end, and the word after it ends in a mark.
        assert_eq!(said("run cf task"), format!("run {MAC} task"));
        assert_eq!(said("cf tell."), format!("{MAC} tell."));
    }

    #[test]
    fn a_word_a_window_was_given_is_taken_as_it_is_unless_no_shell_could_take_it() {
        for word in [MAC, "& 'C:/Program Files/c/cf.exe'", "'/a b/cf'", "cf"] {
            assert_eq!(CfWord::of_word(word).unwrap().as_str(), word);
        }
        let long = "x".repeat(1_025);
        for none in ["", "a\nb", "a\u{7}b", long.as_str()] {
            assert_eq!(CfWord::of_word(none), None, "{none:?}");
        }
    }

    #[test]
    fn with_no_path_a_text_is_left_as_it_is() {
        let text = "cf task get T-1 and cf ask \"…\"";
        assert!(matches!(CfWord::bare().in_text(text), Cow::Borrowed(same) if same == text));
    }

    #[test]
    fn the_word_for_powershell_starts_the_command_as_an_ampersand_call() {
        let word = powershell("C:/Program Files/c/cf.exe");
        assert_eq!(
            word.in_text("run cf task get T-1").into_owned(),
            "run & 'C:/Program Files/c/cf.exe' task get T-1"
        );
    }
}
