//! A program reached through a shell (`/bin/sh -c "codex --version"`, `cmd /c`, `powershell
//! -Command`), judged by the program the shell is told to run (K3, T-2219).
//!
//! The door knows a harness's program by the file it resolves to. A shell is a plain program, so
//! `sh -c "claude …"` would start Claude Code on no door's judgement: no update switches, no refusal
//! of the Keychain's utility or cargo. So a start whose program is a shell and whose words give it a
//! command is judged by that command's first word, in the shell's own way of reading words, and
//! where the command cannot be read as one simple command (an operator, a substitution, a
//! variable, a glob, an assignment in front, a quote that does not end), the start is refused: the
//! door never guesses what a shell will run.
//!
//! A shell given no command (an interactive one, or one that runs a script file) starts a plain
//! program: the file is whatever the run made or the machine has, and is judged as any program.

/// The kinds of shell, each reading its words and its command flag its own way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// `sh`, `bash`, `zsh`, `dash`, `ksh`, `fish` and their kin: `-c` (alone or in a cluster of
    /// letters such as `-lc`, `-ic`) takes the command string.
    Posix,
    /// `cmd`: `/c` and `/k` take the rest of the line.
    Cmd,
    /// `powershell` and `pwsh`: `-Command` (or an abbreviation, or `-c`) takes the command.
    Powershell,
}

/// The shell a program's file name is, where it is one. `name` is the file's name without its
/// folder, in any case, with or without an extension.
pub fn shell_named(name: &str) -> Option<Shell> {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let lower = name.to_lowercase();
    let stem = lower.split('.').next().unwrap_or_default();
    match stem {
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "ash" | "fish" | "csh" | "tcsh" => {
            Some(Shell::Posix)
        }
        "cmd" => Some(Shell::Cmd),
        "powershell" | "pwsh" => Some(Shell::Powershell),
        _ => None,
    }
}

/// What a shell's words tell it to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Told {
    /// No command: an interactive shell, or one running a script file.
    Nothing,
    /// One simple command, as its words.
    Command(Vec<String>),
}

/// What `shell`, started with `args` (its words after the program), is told to run: no command, one
/// command read as words, or why the command cannot be read.
pub fn told(shell: Shell, args: &[String]) -> Result<Told, String> {
    match shell {
        Shell::Posix => posix_told(args),
        Shell::Cmd => cmd_told(args),
        Shell::Powershell => powershell_told(args),
    }
}

fn posix_told(args: &[String]) -> Result<Told, String> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            return Ok(Told::Nothing);
        }
        let Some(flags) = word.strip_prefix('-') else {
            // The first word that is no option is a script file, run as the program it is.
            return Ok(Told::Nothing);
        };
        if flags.starts_with('-') || flags.is_empty() {
            continue;
        }
        if flags.chars().all(char::is_alphabetic) && flags.contains('c') {
            let command = words.next().ok_or_else(|| {
                format!("the shell is given {word} and no command to run")
            })?;
            return posix_words(command).map(Told::Command);
        }
        // `-o name`, `-O name` and `--rcfile file` take a word of their own.
        if matches!(flags, "o" | "O") {
            words.next();
        }
    }
    Ok(Told::Nothing)
}

/// The words of one simple POSIX command, or why it is no such thing.
fn posix_words(text: &str) -> Result<Vec<String>, String> {
    let unread = |why: &str| {
        Err(format!(
            "the shell's command ({text}) cannot be read as one program with words: {why}"
        ))
    };
    let mut words: Vec<String> = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = text.chars().peekable();
    while let Some(next) = chars.next() {
        match next {
            ' ' | '\t' => {
                if let Some(done) = word.take() {
                    words.push(done);
                }
            }
            '\'' => {
                let held = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inside) => held.push(inside),
                        None => return unread("a quote does not end"),
                    }
                }
            }
            '"' => {
                let held = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('$' | '`') => return unread("a substitution inside quotes"),
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '\\')) => held.push(escaped),
                            _ => return unread("an escape inside quotes"),
                        },
                        Some(inside) => held.push(inside),
                        None => return unread("a quote does not end"),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(escaped) if escaped != '\n' && escaped != '\r' => {
                    word.get_or_insert_with(String::new).push(escaped);
                }
                _ => return unread("an escape that ends the command or a line"),
            },
            ';' | '&' | '|' | '<' | '>' | '(' | ')' | '{' | '}' | '$' | '`' | '\n' | '\r'
            | '*' | '?' | '[' | ']' | '~' | '#' | '!' | '=' => {
                if next == '=' && word.is_some() && !words.is_empty() {
                    // `a=b` after the program is an argument, not an assignment.
                    word.get_or_insert_with(String::new).push(next);
                } else {
                    return unread(&format!("`{next}` is the shell's own"));
                }
            }
            other => word.get_or_insert_with(String::new).push(other),
        }
    }
    if let Some(done) = word {
        words.push(done);
    }
    // `exec program …` runs the program itself.
    if words.first().is_some_and(|first| first == "exec") {
        words.remove(0);
    }
    if words.is_empty() {
        return unread("no program");
    }
    Ok(words)
}

fn cmd_told(args: &[String]) -> Result<Told, String> {
    let at = args
        .iter()
        .position(|word| matches!(word.to_lowercase().as_str(), "/c" | "/k" | "/r"));
    let Some(at) = at else {
        return Ok(Told::Nothing);
    };
    let line = args[at + 1..].join(" ");
    windows_words(&line, &['&', '|', '<', '>', '^', '%', '(', ')', '!'], "cmd's own").map(Told::Command)
}

fn powershell_told(args: &[String]) -> Result<Told, String> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let lower = word.to_lowercase();
        let flag = lower.trim_start_matches(['-', '/']);
        if flag == lower {
            // A word that is no flag: a script file's name, run as the program it is.
            return Ok(Told::Nothing);
        }
        if flag == "file" || flag == "f" {
            return Ok(Told::Nothing);
        }
        if flag == "c" || (flag.len() > 1 && "command".starts_with(flag)) {
            let rest: Vec<&str> = words.map(String::as_str).collect();
            if rest.is_empty() {
                return Err("PowerShell is given -Command and no command to run".to_owned());
            }
            return windows_words(
                &rest.join(" "),
                &['&', '|', '<', '>', ';', '$', '`', '(', ')', '{', '}', '@', '%', '\n', '\r'],
                "PowerShell's own",
            )
            .map(Told::Command);
        }
        if flag.len() > 1 && ("encodedcommand".starts_with(flag) || flag == "ec") {
            return Err("PowerShell is given an encoded command, which cannot be read".to_owned());
        }
        // `-ExecutionPolicy x`, `-WindowStyle x`, `-Version x`, `-ConfigurationName x` take a word.
        if ["executionpolicy", "ep", "windowstyle", "w", "version", "v", "configurationname"]
            .contains(&flag)
        {
            words.next();
        }
    }
    Ok(Told::Nothing)
}

/// The words of one simple Windows command line, double-quoted words allowed, or why not.
fn windows_words(line: &str, own: &[char], whose: &str) -> Result<Vec<String>, String> {
    let mut words: Vec<String> = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    for next in line.chars() {
        if quoted {
            if next == '"' {
                quoted = false;
            } else {
                word.get_or_insert_with(String::new).push(next);
            }
            continue;
        }
        match next {
            '"' => {
                quoted = true;
                word.get_or_insert_with(String::new);
            }
            ' ' | '\t' => {
                if let Some(done) = word.take() {
                    words.push(done);
                }
            }
            own_char if own.contains(&own_char) => {
                return Err(format!(
                    "the shell's command ({line}) cannot be read as one program with words: `{own_char}` is {whose}"
                ));
            }
            other => word.get_or_insert_with(String::new).push(other),
        }
    }
    if quoted {
        return Err(format!(
            "the shell's command ({line}) cannot be read: a quote does not end"
        ));
    }
    if let Some(done) = word {
        words.push(done);
    }
    if words.is_empty() {
        return Err("the shell is given no program to run".to_owned());
    }
    Ok(words)
}

/// The harnesses' programs, the Keychain's utility and cargo: the names a command the door cannot
/// read is held off by, where the start is a kit fixture's plain one (a lexical net: it finds an
/// honest mention, and cannot find a name that is built by the shell).
const DOOR_PROGRAMS: &[&str] = &[
    "claude", "codex", "pi", "devin", "muse", "agy", "gemini", "security", "cargo",
];

/// Whether any word of `args` names a program that has a door of its own, in any case and with or
/// without an extension or a folder.
pub fn names_a_door_program(args: &[String]) -> Option<&'static str> {
    args.iter()
        .flat_map(|arg| {
            arg.split(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.')))
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .find_map(|word| {
            let stem = word.split('.').next().unwrap_or_default().to_owned();
            DOOR_PROGRAMS.iter().find(|name| **name == stem).copied()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(shell: Shell, words: &[&str]) -> Result<Told, String> {
        told(
            shell,
            &words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>(),
        )
    }

    fn command(words: &[&str]) -> Told {
        Told::Command(words.iter().map(|word| (*word).to_owned()).collect())
    }

    #[test]
    fn a_shell_is_known_by_its_name_in_any_case_with_or_without_an_extension() {
        for name in ["sh", "bash", "zsh", "dash", "SH.EXE", "Bash"] {
            assert_eq!(shell_named(name), Some(Shell::Posix), "{name}");
        }
        assert_eq!(shell_named("cmd.exe"), Some(Shell::Cmd));
        assert_eq!(shell_named("PowerShell.exe"), Some(Shell::Powershell));
        assert_eq!(shell_named("pwsh"), Some(Shell::Powershell));
        assert_eq!(
            shell_named("C:\\Windows\\System32\\cmd.exe"),
            Some(Shell::Cmd)
        );
        assert_eq!(shell_named("/usr/bin/zsh"), Some(Shell::Posix));
        for name in ["claude", "shasum", "node", "bashful", ""] {
            assert_eq!(shell_named(name), None, "{name}");
        }
    }

    #[test]
    fn a_posix_shell_is_told_the_command_after_its_c_flag_in_any_cluster() {
        assert_eq!(
            said(Shell::Posix, &["-c", "codex --version"]),
            Ok(command(&["codex", "--version"]))
        );
        assert_eq!(
            said(Shell::Posix, &["-lc", "claude"]),
            Ok(command(&["claude"]))
        );
        assert_eq!(
            said(Shell::Posix, &["-l", "-i", "-c", "exec pi 'a b' \"c d\" e\\ f"]),
            Ok(command(&["pi", "a b", "c d", "e f"]))
        );
        assert_eq!(
            said(Shell::Posix, &["-c", "echo a=b"]),
            Ok(command(&["echo", "a=b"]))
        );
    }

    #[test]
    fn a_posix_shell_with_no_command_runs_no_program_of_its_words() {
        assert_eq!(said(Shell::Posix, &[]), Ok(Told::Nothing));
        assert_eq!(said(Shell::Posix, &["-i"]), Ok(Told::Nothing));
        assert_eq!(said(Shell::Posix, &["./setup.sh", "-c", "claude"]), Ok(Told::Nothing));
        assert_eq!(said(Shell::Posix, &["--", "-c"]), Ok(Told::Nothing));
    }

    #[test]
    fn a_posix_command_that_is_not_one_program_with_words_is_refused() {
        for text in [
            "echo hi && claude",
            "echo hi; codex",
            "claude | cat",
            "claude > out",
            "$HOME/bin/claude",
            "echo $(claude)",
            "echo `claude`",
            "ls *",
            "FOO=1 claude",
            "(claude)",
            "claude &",
            "claude\nfoo",
            "'claude",
            "\"echo $HOME\"",
            "",
            "exec",
        ] {
            assert!(
                said(Shell::Posix, &["-c", text]).is_err(),
                "{text:?} was read as a program"
            );
        }
        assert!(said(Shell::Posix, &["-c"]).is_err(), "a -c with no command");
    }

    #[test]
    fn an_unreadable_command_that_names_a_program_with_a_door_is_found_by_its_words() {
        let words = |text: &[&str]| text.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        assert_eq!(names_a_door_program(&words(&["-c", "echo a; Claude --x"])), Some("claude"));
        assert_eq!(names_a_door_program(&words(&["-c", "/usr/bin/security x | cat"])), Some("security"));
        assert_eq!(names_a_door_program(&words(&["/c", "cargo.exe build & dir"])), Some("cargo"));
        assert_eq!(names_a_door_program(&words(&["-c", "sleep 4; printf 'a' > /dev/null"])), None);
        assert_eq!(names_a_door_program(&words(&["-c", "pipe pizza opium"])), None);
    }

    #[test]
    fn cmd_and_powershell_are_told_one_program_and_refuse_what_they_would_read_themselves() {
        assert_eq!(
            said(Shell::Cmd, &["/d", "/c", "codex", "--version"]),
            Ok(command(&["codex", "--version"]))
        );
        assert_eq!(
            said(Shell::Cmd, &["/c", "\"C:\\Program Files\\x\\pi.exe\" --version"]),
            Ok(command(&["C:\\Program Files\\x\\pi.exe", "--version"]))
        );
        assert_eq!(said(Shell::Cmd, &[]), Ok(Told::Nothing));
        assert!(said(Shell::Cmd, &["/c", "echo a & claude"]).is_err());
        assert!(said(Shell::Cmd, &["/c", "claude %X%"]).is_err());
        assert_eq!(
            said(Shell::Powershell, &["-NoProfile", "-Command", "node", "./a.cjs"]),
            Ok(command(&["node", "./a.cjs"]))
        );
        assert_eq!(
            said(Shell::Powershell, &["-NoProfile", "-c", "\"pi --version\""]),
            Ok(command(&["pi --version"]))
        );
        assert_eq!(
            said(Shell::Powershell, &["-NoProfile", "-File", "x.ps1", "-Command", "claude"]),
            Ok(Told::Nothing)
        );
        assert!(said(Shell::Powershell, &["-Command", "claude; pi"]).is_err());
        assert!(said(Shell::Powershell, &["-Command", "& claude"]).is_err());
        assert!(said(Shell::Powershell, &["-Command", "$x = 1"]).is_err());
        assert!(said(Shell::Powershell, &["-EncodedCommand", "AAAA"]).is_err());
        assert!(said(Shell::Powershell, &["-Command"]).is_err());
    }
}
